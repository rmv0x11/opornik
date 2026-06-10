//! Position encoding **v2** for the phase neural nets (engine v2).
//!
//! Two encoders, one per phase net:
//!
//! * [`encode_contact`] — raw occupancy (with the `==3` quirk of v1 fixed to
//!   `>=3`) **plus** expert long-nardy features: block (prime) length and
//!   position, opposing checkers trapped behind it and how deep, head counts,
//!   spares, stacking, home occupancy and timing pips. Literature (TD-Gammon
//!   2002, GnuBG inputs, the Fevga prime-blindness result) says a small net
//!   only learns blocking play when these concepts are inputs.
//! * [`encode_race`] — the raw block only: a disengaged race is a pip-
//!   efficiency problem, blocking features carry no signal there.
//!
//! Everything is encoded from the **side-to-move's** perspective (the same
//! weights evaluate both sides — flip the board, not the net).
//!
//! # Block geometry
//!
//! Blocking is relative to the *enemy's* path: a block is a run of cells, at
//! consecutive positions **in the enemy's frame**, each holding at least one of
//! my checkers (any occupied cell is unplayable for them — no hitting). The
//! enemy travels from their position 24 down to 1, so a run `[lo..hi]` (their
//! coords) obstructs exactly the enemy checkers standing at positions `> hi`.
//! Runs are linear, not cyclic: their frame's 24 and 1 are the two *ends* of
//! their path, so my checkers there never form one obstacle.

use crate::board::{Board, HEAD_POS, HOME_HIGH, N_CHECKERS, START_PIP};
use crate::player::Player;

/// Features per point: `>=1`, `>=2`, `>=3`, `(n-3)/2` overflow.
pub const FEATURES_PER_POINT: usize = 4;
/// 24 points × 4 features × 2 players.
pub const POINT_FEATURES: usize = 24 * FEATURES_PER_POINT * 2;
/// Borne-off (×2) and pip (×2), as in v1.
pub const EXTRA_FEATURES: usize = 4;
/// Expert features: 9 per side + 3 global.
pub const EXPERT_FEATURES: usize = 9 * 2 + 3;
/// Contact-net input size (192 raw + 4 extras + 21 expert = 217).
pub const CONTACT_INPUTS: usize = POINT_FEATURES + EXTRA_FEATURES + EXPERT_FEATURES;
/// Race-net input size (196).
pub const RACE_INPUTS: usize = POINT_FEATURES + EXTRA_FEATURES;

#[inline]
fn point_features(count: u8, out: &mut [f32]) {
    out[0] = (count >= 1) as u8 as f32;
    out[1] = (count >= 2) as u8 as f32;
    out[2] = (count >= 3) as u8 as f32; // v1 had `== 3`: 4+ stacks lost the bit
    out[3] = if count > 3 {
        (count as f32 - 3.0) / 2.0
    } else {
        0.0
    };
}

/// Raw occupancy + off/pip extras (shared prefix of both encoders).
fn encode_raw(board: &Board, mover: Player, x: &mut [f32]) -> usize {
    let opp = mover.opponent();
    let mut idx = 0;
    for &side in &[mover, opp] {
        for pos in 1..=HEAD_POS {
            point_features(board.own_at(side, pos), &mut x[idx..idx + FEATURES_PER_POINT]);
            idx += FEATURES_PER_POINT;
        }
    }
    x[idx] = board.off[mover.index()] as f32 / N_CHECKERS as f32;
    x[idx + 1] = board.off[opp.index()] as f32 / N_CHECKERS as f32;
    x[idx + 2] = board.pip(mover) as f32 / START_PIP as f32;
    x[idx + 3] = board.pip(opp) as f32 / START_PIP as f32;
    idx + 4
}

/// `player`'s strongest block, measured in the ENEMY's coordinate frame.
/// Returns `(len, hi)`: run length and the front edge (highest enemy-frame
/// position of the run). `(0, 0)` if the player occupies no cells; a lone
/// occupied cell is a length-1 "block" (still unplayable for the enemy).
/// Of equally long runs the one further forward (higher `hi`) wins — it
/// obstructs more of the enemy's remaining path.
fn max_block(board: &Board, player: Player) -> (u8, u8) {
    let enemy = player.opponent();
    let (mut best_len, mut best_hi) = (0u8, 0u8);
    let mut run = 0u8;
    for f in 1..=HEAD_POS {
        // my checkers seen at the enemy's path position `f`
        if board.opp_at(enemy, f) > 0 {
            run += 1;
            if run >= best_len {
                best_len = run;
                best_hi = f;
            }
        } else {
            run = 0;
        }
    }
    (best_len, best_hi)
}

/// Enemy checkers standing behind `player`'s strongest block (enemy-frame
/// positions `> hi`): `(count, mean distance to the block edge)`.
fn trapped_behind(board: &Board, player: Player, hi: u8) -> (u8, f32) {
    if hi == 0 {
        return (0, 0.0);
    }
    let enemy = player.opponent();
    let (mut count, mut depth) = (0u32, 0u32);
    for e in (hi + 1)..=HEAD_POS {
        let c = board.own_at(enemy, e) as u32;
        count += c;
        depth += c * (e - hi) as u32;
    }
    if count == 0 {
        (0, 0.0)
    } else {
        (count as u8, depth as f32 / count as f32)
    }
}

/// The 9 per-side expert features, written into `out[0..9]`.
fn side_features(board: &Board, side: Player, out: &mut [f32]) {
    let (block_len, block_hi) = max_block(board, side);
    // A lone occupied cell is easily passed — block-derived features (front
    // edge, trapped count/depth) only fire for genuine runs of 2+.
    let (trapped, depth) = if block_len >= 2 {
        trapped_behind(board, side, block_hi)
    } else {
        (0, 0.0)
    };

    // Spares: checkers neither on the head nor part of the strongest block —
    // the UNION of the two sets: the head cell (own pos 24 = enemy frame 12)
    // is mid-path for the enemy and regularly sits INSIDE the strongest run
    // (chaining with own 23 = f11 and own 1 = f13), so subtracting `on_head`
    // and the block sum separately would count the head stack twice.
    // Block cells in MY frame: enemy-frame f maps back to my pos (f+12) mod 24.
    let mut in_block = 0u8;
    let mut head_in_block = false;
    if block_len >= 2 {
        for f in (block_hi + 1 - block_len)..=block_hi {
            let my_pos = (f + 12 - 1) % 24 + 1;
            if my_pos == HEAD_POS {
                head_in_block = true;
            }
            in_block += board.own_at(side, my_pos);
        }
    }
    let on_head = board.own_at(side, HEAD_POS);
    let on_board = board.checkers_on_board(side);
    let excluded = in_block + if head_in_block { 0 } else { on_head };

    let mut stacked = 0u8;
    let mut home_points = 0u8;
    let mut in_home = 0u8;
    for pos in 1..=HEAD_POS {
        let c = board.own_at(side, pos);
        if c >= 4 {
            stacked += 1;
        }
        if pos <= HOME_HIGH && c > 0 {
            home_points += 1;
            in_home += c;
        }
    }

    out[0] = (block_len.min(6)) as f32 / 6.0;
    out[1] = if block_len >= 2 { block_hi as f32 / 24.0 } else { 0.0 };
    out[2] = trapped as f32 / N_CHECKERS as f32;
    // hi >= 2 for any 2+ run, so depth <= 22 and the feature stays in [0, 1].
    out[3] = depth / 23.0;
    out[4] = on_head as f32 / N_CHECKERS as f32;
    out[5] = on_board.saturating_sub(excluded) as f32 / N_CHECKERS as f32;
    out[6] = stacked as f32 / 6.0;
    out[7] = home_points as f32 / HOME_HIGH as f32;
    out[8] = in_home as f32 / N_CHECKERS as f32;
}

/// Timing: pips of `side`'s checkers not committed to holding its strongest
/// block — how long the side can keep moving without being forced to break it.
/// Only ONE checker per block cell is committed; extra checkers stacked on a
/// block point move freely, so their pips still count as timing.
fn free_pips(board: &Board, side: Player) -> f32 {
    let (block_len, block_hi) = max_block(board, side);
    let mut pips = board.pip(side) as i32;
    if block_len >= 2 {
        for f in (block_hi + 1 - block_len)..=block_hi {
            let my_pos = (f + 12 - 1) % 24 + 1;
            pips -= my_pos as i32; // one holder per cell (block cells hold >= 1)
        }
    }
    pips.max(0) as f32 / START_PIP as f32
}

/// Encode for the CONTACT net ([`CONTACT_INPUTS`] floats).
pub fn encode_contact(board: &Board, mover: Player) -> Vec<f32> {
    let mut x = vec![0.0f32; CONTACT_INPUTS];
    encode_contact_into(board, mover, &mut x);
    x
}

/// [`encode_contact`] into a pre-allocated buffer.
pub fn encode_contact_into(board: &Board, mover: Player, x: &mut [f32]) {
    debug_assert_eq!(x.len(), CONTACT_INPUTS);
    let opp = mover.opponent();
    let mut idx = encode_raw(board, mover, x);
    side_features(board, mover, &mut x[idx..idx + 9]);
    idx += 9;
    side_features(board, opp, &mut x[idx..idx + 9]);
    idx += 9;
    let lead = (board.pip(opp) as f32 - board.pip(mover) as f32) / START_PIP as f32;
    x[idx] = lead.clamp(-1.0, 1.0);
    x[idx + 1] = free_pips(board, mover);
    x[idx + 2] = free_pips(board, opp);
}

/// Encode for the RACE net ([`RACE_INPUTS`] floats).
pub fn encode_race(board: &Board, mover: Player) -> Vec<f32> {
    let mut x = vec![0.0f32; RACE_INPUTS];
    encode_race_into(board, mover, &mut x);
    x
}

/// [`encode_race`] into a pre-allocated buffer.
pub fn encode_race_into(board: &Board, mover: Player, x: &mut [f32]) {
    debug_assert_eq!(x.len(), RACE_INPUTS);
    encode_raw(board, mover, x);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_perspective_symmetry() {
        let b = Board::starting();
        let w = encode_contact(&b, Player::White);
        let bl = encode_contact(&b, Player::Black);
        assert_eq!(w.len(), CONTACT_INPUTS);
        // The starting position is symmetric, so both perspectives encode identically.
        assert_eq!(w, bl);
        assert_eq!(encode_race(&b, Player::White).len(), RACE_INPUTS);
    }

    #[test]
    fn point_feature_three_plus_fix() {
        let mut f = [0.0; 4];
        point_features(4, &mut f);
        assert_eq!(f, [1.0, 1.0, 1.0, 0.5]); // v1 dropped the third bit for 4+
        point_features(3, &mut f);
        assert_eq!(f, [1.0, 1.0, 1.0, 0.0]);
    }

    #[test]
    fn block_and_trapped_geometry() {
        // White 6-prime on own 1..=6 → Black-frame 13..=18 (hi 18, len 6);
        // 3 spare White checkers on own 10 (Black frame 22, a separate run).
        // Black: 10 still on the head (own 24, phys 12 — free) and 5 on own 20
        // (phys 16 — free; White home cells are phys 18..23). All 15+15 on
        // board, no cell shared — the fixture must keep the prime intact.
        let mut b = Board::empty();
        for pos in 1..=6u8 {
            b.place(Player::White, pos, 2);
        }
        b.place(Player::White, 10, 3);
        b.place(Player::Black, 24, 10);
        b.place(Player::Black, 20, 5);
        assert!(b.is_valid(), "fixture must be a legal 15+15 position");

        let (len, hi) = max_block(&b, Player::White);
        assert_eq!((len, hi), (6, 18));
        // Every Black checker stands at Black-frame positions > 18 → trapped.
        let (trapped, depth) = trapped_behind(&b, Player::White, hi);
        assert_eq!(trapped, 15);
        // depth = (5×(20−18) + 10×(24−18)) / 15
        assert!((depth - 70.0 / 15.0).abs() < 1e-6);

        // Black's own strongest block (head cell, length 1) traps nothing —
        // block-derived features need a run of 2+.
        let (blen, _) = max_block(&b, Player::Black);
        assert_eq!(blen, 1);
    }

    #[test]
    fn spares_exclude_head_and_block_without_double_counting() {
        // White: 5 on the head (own 24 = enemy frame 12), 2 on own 1 (frame
        // 13) — together the strongest block (len 2, hi 13) CONTAINING the
        // head — plus 8 on own 18: the true spares. The buggy formula
        // on_board − on_head − in_block subtracted the head stack twice and
        // reported 3.
        let mut b = Board::empty();
        b.place(Player::White, 24, 5);
        b.place(Player::White, 1, 2);
        b.place(Player::White, 18, 8);
        b.place(Player::Black, 24, 15);
        assert!(b.is_valid());
        let (len, hi) = max_block(&b, Player::White);
        assert_eq!((len, hi), (2, 13));

        let mut f = [0.0f32; 9];
        side_features(&b, Player::White, &mut f);
        assert_eq!(f[5] * 15.0, 8.0, "spares must not double-subtract the head stack");

        // Control: head NOT in the block — both exclusions apply separately.
        let mut c = Board::empty();
        c.place(Player::White, 24, 5); // head alone (frame 12; neighbours empty)
        c.place(Player::White, 5, 2); // frame 17
        c.place(Player::White, 6, 2); // frame 18 → block (2, 18)
        c.place(Player::White, 18, 6); // spares
        c.place(Player::Black, 24, 15);
        assert!(c.is_valid());
        let mut g = [0.0f32; 9];
        side_features(&c, Player::White, &mut g);
        assert_eq!(g[5] * 15.0, 6.0);
    }

    #[test]
    fn lone_cell_gates_block_features_and_depth_stays_normalized() {
        // A lone occupied cell (len-1 run) must not report front edge,
        // trapped or depth.
        let mut b = Board::empty();
        b.place(Player::White, 13, 15); // enemy frame 1, a length-1 run
        b.place(Player::Black, 24, 15);
        let mut f = [0.0f32; 9];
        side_features(&b, Player::White, &mut f);
        assert_eq!((f[1], f[2], f[3]), (0.0, 0.0, 0.0));

        // Deepest possible trap under the len>=2 gate: block at enemy frame
        // 1..2 (own 13..14), enemy on their head (depth 22) → feature < 1.
        let mut d = Board::empty();
        d.place(Player::White, 13, 2);
        d.place(Player::White, 14, 2);
        d.place(Player::White, 20, 11);
        d.place(Player::Black, 24, 15);
        assert!(d.is_valid());
        let mut g = [0.0f32; 9];
        side_features(&d, Player::White, &mut g);
        assert!(g[3] > 0.9 && g[3] <= 1.0, "depth must stay in [0,1], got {}", g[3]);
    }

    #[test]
    fn free_pips_commits_one_checker_per_block_cell() {
        // Head (5 checkers) + own 1 form the block; only ONE checker per cell
        // is committed, so timing = total pips − (24 + 1).
        let mut b = Board::empty();
        b.place(Player::White, 24, 5);
        b.place(Player::White, 1, 2);
        b.place(Player::White, 18, 8);
        b.place(Player::Black, 24, 15);
        let total = (24 * 5 + 2 + 18 * 8) as f32;
        let got = free_pips(&b, Player::White) * START_PIP as f32;
        assert!((got - (total - 25.0)).abs() < 1e-3, "got {got}");
    }

    #[test]
    fn head_run_is_not_one_cyclic_block() {
        // White on own 1, 2 and 24 (head): in Black's frame those are 13, 14
        // and 12 — positions 12..14 ARE consecutive integers, so they DO form
        // one linear run of 3 in the enemy frame. Sanity-check the mapping.
        let mut b = Board::empty();
        b.place(Player::White, 1, 1);
        b.place(Player::White, 2, 1);
        b.place(Player::White, 24, 1);
        let (len, hi) = max_block(&b, Player::White);
        assert_eq!((len, hi), (3, 14));

        // But a run "around the end" of Black's path cannot exist: White on
        // own 13 and 14 sits at Black-frame 1 and 2; White on own 11, 12 at
        // Black-frame 23, 24. Together: runs of 2 and 2, never 4.
        let mut b2 = Board::empty();
        for pos in [11u8, 12, 13, 14] {
            b2.place(Player::White, pos, 1);
        }
        let (len2, _) = max_block(&b2, Player::White);
        assert_eq!(len2, 2);
    }

    #[test]
    fn contact_features_see_a_prime_race_encoder_does_not() {
        // Same position: White 6-prime trapping Black runners.
        let mut b = Board::empty();
        for pos in 1..=6u8 {
            b.place(Player::White, pos, 2);
        }
        b.place(Player::White, 24, 3);
        b.place(Player::Black, 20, 15);
        let x = encode_contact(&b, Player::White);
        let expert = &x[POINT_FEATURES + EXTRA_FEATURES..];
        assert_eq!(expert[0], 1.0, "6-prime → block feature saturated");
        assert!(expert[2] > 0.9, "all 15 Black checkers trapped");
        let r = encode_race(&b, Player::White);
        assert_eq!(r.len(), RACE_INPUTS); // race encoding simply has no such inputs
    }
}
