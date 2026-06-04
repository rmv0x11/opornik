//! Exact bear-off / race endgame database.
//!
//! Длинные нарды is a no-hitting race, and the two home boards sit on disjoint
//! physical cells — so once **both** sides have all 15 checkers home, the game is
//! a pure independent race that is *exactly* solvable. This module precomputes,
//! for every one-sided home configuration (checkers on points 1..=6, ≤15 total),
//! the probability distribution over the number of rolls to bear everything off
//! under rolls-minimising play. Convolving the two sides' distributions gives the
//! exact win probability for a race, which beats any neural-net approximation in
//! the most common decisive phase of the game.
//!
//! The state space is `C(21,6) = 54_264` configurations. The table reuses the
//! tested move generator for successors, so bear-off rules (exact / over-roll /
//! must-move-within) are guaranteed consistent with the rest of the engine.

use crate::board::Board;
use crate::moves::generate_turns_cfg;
use crate::player::Player;
use std::collections::HashMap;

/// Home points are 1..=6.
const HOME: usize = 6;
/// Cap on the rolls-to-clear distribution length (tail beyond is ~0).
pub const MAX_DIST: usize = 64;

/// Checker counts on points 1..=6.
pub type State = [u8; HOME];

/// Pip count of a one-sided home state.
#[inline]
fn pip(s: &State) -> u16 {
    (0..HOME).map(|i| (i as u16 + 1) * s[i] as u16).sum()
}

/// The home configuration of `player` (counts on points 1..=6).
pub fn state_of(board: &Board, player: Player) -> State {
    let mut s = [0u8; HOME];
    for (i, slot) in s.iter_mut().enumerate() {
        *slot = board.own_at(player, i as u8 + 1);
    }
    s
}

fn board_of(state: &State) -> Board {
    let mut b = Board::empty();
    for (i, &c) in state.iter().enumerate() {
        if c > 0 {
            b.place(Player::White, i as u8 + 1, c);
        }
    }
    b
}

fn enumerate(idx: usize, remaining: u8, cur: &mut State, out: &mut Vec<State>) {
    if idx == HOME {
        out.push(*cur);
        return;
    }
    for c in 0..=remaining {
        cur[idx] = c;
        enumerate(idx + 1, remaining - c, cur, out);
    }
    cur[idx] = 0;
}

/// Exact one-sided bear-off database.
pub struct BearoffTable {
    /// Expected rolls to clear each state under optimal play.
    e: HashMap<State, f32>,
    /// Distribution over number of rolls to clear (index = rolls).
    dist: HashMap<State, Vec<f32>>,
}

impl BearoffTable {
    /// Build the full database (all states up to 15 checkers).
    pub fn build() -> Self {
        Self::build_capped(15)
    }

    /// Build a database covering states with at most `max_checkers` (used for
    /// fast tests; production uses 15).
    pub fn build_capped(max_checkers: u8) -> Self {
        let mut states = Vec::new();
        enumerate(0, max_checkers, &mut [0u8; HOME], &mut states);
        // Process in increasing pip so a state's successors (strictly lower pip)
        // are always already computed.
        states.sort_by_key(pip);

        let mut e: HashMap<State, f32> = HashMap::with_capacity(states.len());
        let mut dist: HashMap<State, Vec<f32>> = HashMap::with_capacity(states.len());

        for s in states {
            if s.iter().all(|&c| c == 0) {
                e.insert(s, 0.0);
                let mut d = vec![0.0; MAX_DIST];
                d[0] = 1.0;
                dist.insert(s, d);
                continue;
            }

            let board = board_of(&s);
            let mut e_sum = 0.0f32;
            let mut d = vec![0.0f32; MAX_DIST];

            for d1 in 1..=6u8 {
                for d2 in d1..=6u8 {
                    let p = if d1 == d2 { 1.0 } else { 2.0 } / 36.0;
                    let plays = generate_turns_cfg(&board, Player::White, [d1, d2], false, Some(1));

                    // Choose the play minimising expected rolls (optimal bear-off).
                    let mut best_e = f32::INFINITY;
                    let mut best_key = s;
                    for t in &plays {
                        let key = state_of(&t.board, Player::White);
                        let ke = *e.get(&key).expect("successor has lower pip → computed");
                        if ke < best_e {
                            best_e = ke;
                            best_key = key;
                        }
                    }

                    e_sum += p * best_e;
                    let bd = &dist[&best_key];
                    for t in 0..MAX_DIST - 1 {
                        d[t + 1] += p * bd[t];
                    }
                }
            }

            e.insert(s, 1.0 + e_sum);
            dist.insert(s, d);
        }

        BearoffTable { e, dist }
    }

    /// Expected rolls to bear off `state`, if present.
    pub fn expected_rolls(&self, state: &State) -> Option<f32> {
        self.e.get(state).copied()
    }

    /// Whether the position is a pure race: both sides all home and both still
    /// have checkers on the board (i.e. not already finished).
    pub fn is_race(&self, board: &Board) -> bool {
        board.all_home(Player::White)
            && board.all_home(Player::Black)
            && board.checkers_on_board(Player::White) > 0
            && board.checkers_on_board(Player::Black) > 0
    }

    /// Exact cubeless equity for `to_move` in a pure race (the player on roll
    /// moves first, so ties go to them). Returns `None` if either home state is
    /// outside the table.
    pub fn race_equity(&self, board: &Board, to_move: Player) -> Option<f32> {
        let sp = state_of(board, to_move);
        let so = state_of(board, to_move.opponent());
        let dp = self.dist.get(&sp)?;
        let d_opp = self.dist.get(&so)?;

        // tail[a] = P(opponent needs >= a rolls)
        let mut tail = vec![0.0f32; MAX_DIST + 1];
        for a in (0..MAX_DIST).rev() {
            tail[a] = tail[a + 1] + d_opp[a];
        }
        // P(to_move wins) = P(t_me <= t_opp), moving first.
        let mut win = 0.0f32;
        for a in 0..MAX_DIST {
            win += dp[a] * tail[a];
        }
        Some(2.0 * win - 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trivial_states_clear_in_one_roll() {
        let t = BearoffTable::build_capped(3);
        // 1 or 2 checkers on point 1 → always off in a single roll.
        assert!((t.expected_rolls(&[1, 0, 0, 0, 0, 0]).unwrap() - 1.0).abs() < 1e-5);
        assert!((t.expected_rolls(&[2, 0, 0, 0, 0, 0]).unwrap() - 1.0).abs() < 1e-5);
        // A lone checker on point 6 needs more than one roll on average (needs a 6).
        assert!(t.expected_rolls(&[0, 0, 0, 0, 0, 1]).unwrap() > 1.0);
    }

    #[test]
    fn more_checkers_take_more_rolls() {
        let t = BearoffTable::build_capped(6);
        let one = t.expected_rolls(&[1, 0, 0, 0, 0, 0]).unwrap();
        let six = t.expected_rolls(&[6, 0, 0, 0, 0, 0]).unwrap();
        assert!(six > one);
    }

    #[test]
    fn player_on_roll_with_one_checker_always_wins_the_race() {
        let t = BearoffTable::build_capped(4);
        let mut b = Board::empty();
        b.place(Player::White, 1, 1); // White: 1 on point 1
        b.place(Player::Black, 1, 3); // Black: 3 on point 1
        // White moves first and clears in one roll → wins outright.
        let eq = t.race_equity(&b, Player::White).unwrap();
        assert!((eq - 1.0).abs() < 1e-5, "expected +1, got {eq}");
    }

    #[test]
    fn being_far_behind_in_the_race_is_negative() {
        let t = BearoffTable::build_capped(3);
        let mut b = Board::empty();
        b.place(Player::White, 6, 3); // White: 3 on point 6 (slow)
        b.place(Player::Black, 1, 3); // Black: 3 on point 1 (fast)
        let eq = t.race_equity(&b, Player::White).unwrap();
        assert!(eq < 0.0, "White is far behind, expected negative equity, got {eq}");
    }

    #[test]
    fn equal_race_favours_the_player_on_roll() {
        let t = BearoffTable::build_capped(6);
        let mut b = Board::empty();
        b.place(Player::White, 3, 4);
        b.place(Player::Black, 3, 4);
        let eq = t.race_equity(&b, Player::White).unwrap();
        assert!(eq > 0.0, "moving first in an equal race should be +EV, got {eq}");
    }
}
