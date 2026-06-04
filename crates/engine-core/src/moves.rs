//! Legal move generation for длинные нарды.
//!
//! A *turn* is the full play for one dice roll: two sub-moves for a normal roll,
//! four for a double. Generation enforces every FSNR rule:
//!
//! * **Head rule** — at most one checker leaves the head (pos 24) per turn,
//!   except a player's *first* turn rolling 6-6 / 4-4 / 3-3, which allows two.
//! * **No hitting** — a checker may only land on an empty point or one of its own;
//!   a single opposing checker blocks a point.
//! * **No full prime** — a move may not create a wall of six consecutive points
//!   that traps *all* of the opponent's checkers (none ahead of the wall), and
//!   this is forbidden even transiently between the two dice.
//! * **Bearing off** — only once all 15 checkers are home; exact rolls bear a
//!   checker off, an over-roll bears off only from the highest occupied point.
//! * **Maximal use** — a player must use as many dice as legally possible; when
//!   only one of two distinct dice can be played, it must be the higher one.
//!
//! Generation returns the distinct *resulting positions* (board states), which
//! automatically merges sub-move orderings that transpose to the same position.

use crate::board::{Board, HEAD_POS};
use crate::player::Player;
use std::collections::HashSet;

/// A single checker move within a turn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CheckerMove {
    /// Source path position (`1..=24`).
    pub from: u8,
    /// Die value used (`1..=6`).
    pub die: u8,
    /// Destination path position (`1..=24`), or `0` when the checker bears off.
    pub to: u8,
    /// Whether this sub-move bears the checker off the board.
    pub bear_off: bool,
}

/// One complete, legal turn: the ordered sub-moves and the resulting board.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Turn {
    /// Sub-moves in the order they are applied (empty = forced pass).
    pub moves: Vec<CheckerMove>,
    /// Board after the whole turn has been applied.
    pub board: Board,
}

impl Turn {
    /// A turn that plays no checkers (no legal move was available).
    pub fn is_pass(&self) -> bool {
        self.moves.is_empty()
    }
}

/// Doubles that trigger the first-turn "two off the head" exception.
const HEAD_EXCEPTION_DOUBLES: [u8; 3] = [3, 4, 6];

/// Whether `mover` currently has an illegal "full prime": six consecutive points
/// (in the opponent's path order) all owned by `mover`, with no opponent checker
/// ahead of the wall (and none borne off).
///
/// Exposed for UI/analysis (highlighting why a move is illegal).
pub fn creates_illegal_prime(board: &Board, mover: Player) -> bool {
    let opp = mover.opponent();

    // owned[q] == true  ⇔  mover owns the cell at the opponent's path position q.
    let mut owned = [false; (HEAD_POS as usize) + 1];
    for q in 1..=HEAD_POS {
        owned[q as usize] = board.owner(crate::board::phys(opp, q)) == Some(mover);
    }

    // A wall occupies opponent positions [q ..= q+5]; its home-most cell is q.
    for q in 1..=(HEAD_POS - 5) {
        let full = (q..q + 6).all(|x| owned[x as usize]);
        if !full {
            continue;
        }
        // Is at least one opponent checker ahead of the wall (closer to home),
        // i.e. on a position below q, or already borne off?
        let ahead = board.off[opp.index()] > 0 || (1..q).any(|p| board.own_at(opp, p) > 0);
        if !ahead {
            return true;
        }
    }
    false
}

/// Attempt one sub-move of `die` from path position `from` for `player`.
///
/// Returns the resulting board and the move on success, or `None` if the move is
/// illegal (no checker there, head budget exhausted, blocked landing, illegal
/// bear-off, or it would create a full prime).
fn try_submove(
    board: &Board,
    player: Player,
    from: u8,
    die: u8,
    head_used: u8,
    head_budget: u8,
) -> Option<(Board, CheckerMove)> {
    if board.own_at(player, from) == 0 {
        return None;
    }
    if from == HEAD_POS && head_used >= head_budget {
        return None;
    }

    let target = from as i16 - die as i16;

    if target >= 1 {
        // Normal move within the board.
        let to = target as u8;
        if !board.is_open_for(player, to) {
            return None;
        }
        let mut nb = board.clone();
        nb.remove(player, from);
        nb.add(player, to);
        if creates_illegal_prime(&nb, player) {
            return None;
        }
        Some((
            nb,
            CheckerMove {
                from,
                die,
                to,
                bear_off: false,
            },
        ))
    } else {
        // Bear-off (target <= 0): requires every checker home.
        if !board.all_home(player) {
            return None;
        }
        let exact = target == 0;
        if !exact {
            // Over-roll: only allowed from the highest occupied point.
            if board.highest_occupied(player) != from {
                return None;
            }
        }
        // Removing a checker can never create a new prime, so no prime check.
        let mut nb = board.clone();
        nb.remove(player, from);
        nb.off[player.index()] += 1;
        Some((
            nb,
            CheckerMove {
                from,
                die,
                to: 0,
                bear_off: true,
            },
        ))
    }
}

/// Depth-first enumeration of every maximal sub-move sequence. Records a result
/// only at leaves (positions from which no further sub-move is legal), so the
/// "use as many dice as possible" rule reduces to keeping the deepest leaves.
fn explore(
    board: &Board,
    player: Player,
    counts: &mut [u8; 7],
    head_used: u8,
    head_budget: u8,
    moves: &mut Vec<CheckerMove>,
    out: &mut Vec<(Vec<CheckerMove>, Board)>,
) {
    let mut extended = false;

    for die in 1..=6u8 {
        if counts[die as usize] == 0 {
            continue;
        }
        for from in 1..=HEAD_POS {
            if board.own_at(player, from) == 0 {
                continue;
            }
            if let Some((nb, mv)) = try_submove(board, player, from, die, head_used, head_budget) {
                extended = true;
                let nhead = head_used + u8::from(from == HEAD_POS);
                counts[die as usize] -= 1;
                moves.push(mv);
                explore(&nb, player, counts, nhead, head_budget, moves, out);
                moves.pop();
                counts[die as usize] += 1;
            }
        }
    }

    if !extended {
        out.push((moves.clone(), board.clone()));
    }
}

/// Enumerate every maximal sub-move sequence under a fixed head budget, returning
/// the leaves and the maximum depth reached.
fn enumerate(
    board: &Board,
    player: Player,
    dice: [u8; 2],
    head_budget: u8,
) -> (Vec<(Vec<CheckerMove>, Board)>, usize) {
    let mut counts = [0u8; 7];
    if dice[0] == dice[1] {
        counts[dice[0] as usize] = 4;
    } else {
        counts[dice[0] as usize] += 1;
        counts[dice[1] as usize] += 1;
    }

    let mut out: Vec<(Vec<CheckerMove>, Board)> = Vec::new();
    let mut moves: Vec<CheckerMove> = Vec::new();
    explore(board, player, &mut counts, 0, head_budget, &mut moves, &mut out);

    let max_depth = out.iter().map(|(m, _)| m.len()).max().unwrap_or(0);
    (out, max_depth)
}

/// Filter leaves to the maximal-usage set, apply the larger-die rule and collapse
/// to distinct resulting positions.
fn finalize(
    leaves: Vec<(Vec<CheckerMove>, Board)>,
    dice: [u8; 2],
    max_depth: usize,
) -> Vec<Turn> {
    let mut kept: Vec<(Vec<CheckerMove>, Board)> =
        leaves.into_iter().filter(|(m, _)| m.len() == max_depth).collect();

    // Larger-die rule: if exactly one of two distinct dice can be played, it must
    // be the higher one (when a higher-die play exists at all).
    if max_depth == 1 && dice[0] != dice[1] {
        let larger = dice[0].max(dice[1]);
        if kept.iter().any(|(m, _)| m[0].die == larger) {
            kept.retain(|(m, _)| m[0].die == larger);
        }
    }

    // Distinct resulting positions only (merges transposing orderings).
    let mut seen: HashSet<Board> = HashSet::new();
    let mut turns = Vec::with_capacity(kept.len());
    for (m, b) in kept {
        if seen.insert(b.clone()) {
            turns.push(Turn { moves: m, board: b });
        }
    }
    turns
}

/// All legal turns for `player` under standard FSNR rules (head limit 1), given a
/// roll. `first_turn` enables the 6-6/4-4/3-3 two-off-the-head exception. Never
/// empty: a forced pass yields a single [`Turn`] with no moves and an unchanged
/// board.
pub fn generate_turns(board: &Board, player: Player, dice: [u8; 2], first_turn: bool) -> Vec<Turn> {
    generate_turns_cfg(board, player, dice, first_turn, Some(1))
}

/// Like [`generate_turns`] but with a configurable head limit:
/// `Some(n)` allows at most `n` checkers off the head per turn (1 = standard);
/// `None` removes the head rule entirely (the хачапури variant).
///
/// The first-turn doubles exception (only meaningful at head limit 1) lets a
/// *second* checker leave the head on a 6-6/4-4/3-3 first roll — but **only when
/// doing so lets more dice be played** (i.e. when a single checker cannot use all
/// four pips, as happens from the standard start where the opponent's head blocks
/// the chain). When one checker could already use every die, the head rule keeps
/// departures to one. This matches the spec's "forced two off the head" framing
/// rather than an unconditional cap.
pub fn generate_turns_cfg(
    board: &Board,
    player: Player,
    dice: [u8; 2],
    first_turn: bool,
    head_limit: Option<u8>,
) -> Vec<Turn> {
    let base_budget = head_limit.unwrap_or(u8::MAX);
    let (base_leaves, base_depth) = enumerate(board, player, dice, base_budget);

    // First-turn exception: only at head limit 1, only for 3-3/4-4/6-6, and only
    // if a second head checker strictly increases the number of dice played.
    if head_limit == Some(1)
        && first_turn
        && dice[0] == dice[1]
        && HEAD_EXCEPTION_DOUBLES.contains(&dice[0])
    {
        let (ex_leaves, ex_depth) = enumerate(board, player, dice, 2);
        if ex_depth > base_depth {
            return finalize(ex_leaves, dice, ex_depth);
        }
    }

    finalize(base_leaves, dice, base_depth)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::{phys, N_CHECKERS};

    fn turns(board: &Board, p: Player, dice: [u8; 2], first: bool) -> Vec<Turn> {
        generate_turns(board, p, dice, first)
    }

    #[test]
    fn head_rule_limits_to_one_checker_off_head() {
        // From the start with 3-1 (not a first-turn exception double), only one
        // checker may leave the head; both dice chain onto it, landing on pos 20.
        let b = Board::starting();
        let ts = turns(&b, Player::White, [3, 1], false);
        assert_eq!(ts.len(), 1, "exactly one distinct legal play");
        let t = &ts[0];
        assert_eq!(t.board.own_at(Player::White, HEAD_POS), N_CHECKERS - 1);
        assert_eq!(t.board.own_at(Player::White, 20), 1);
        // both dice used, both sub-moves are the same head checker
        assert_eq!(t.moves.len(), 2);
        assert_eq!(t.moves[0].from, HEAD_POS);
        assert_ne!(t.moves[1].from, HEAD_POS); // second move is the already-moved checker
    }

    #[test]
    fn first_turn_double_six_takes_two_off_head() {
        let b = Board::starting();
        let ts = turns(&b, Player::White, [6, 6], true);
        assert_eq!(ts.len(), 1);
        let t = &ts[0];
        // Two checkers to pos 18; the other two sixes are blocked (18 -> 12 is the
        // Black head), so the turn uses only two dice.
        assert_eq!(t.board.own_at(Player::White, 18), 2);
        assert_eq!(t.board.own_at(Player::White, HEAD_POS), N_CHECKERS - 2);
        assert_eq!(t.moves.len(), 2);
    }

    #[test]
    fn first_turn_double_three_forces_two_off_head_all_four_played() {
        // 3-3 on the first turn forces two checkers off the head and all four
        // threes to be played, but their distribution is the player's choice:
        // both to pos 18, or one to 15 and one to 21. So there are two legal plays.
        let b = Board::starting();
        let ts = turns(&b, Player::White, [3, 3], true);
        assert_eq!(ts.len(), 2);
        for t in &ts {
            assert_eq!(t.moves.len(), 4, "all four threes must be used");
            assert_eq!(
                t.board.own_at(Player::White, HEAD_POS),
                N_CHECKERS - 2,
                "exactly two checkers leave the head"
            );
        }
        // the symmetric play (both checkers on 18) is one of the options
        assert!(ts.iter().any(|t| t.board.own_at(Player::White, 18) == 2));
    }

    #[test]
    fn first_turn_double_four_lands_two_on_16() {
        let b = Board::starting();
        let ts = turns(&b, Player::White, [4, 4], true);
        assert_eq!(ts.len(), 1);
        assert_eq!(ts[0].board.own_at(Player::White, 16), 2);
        assert_eq!(ts[0].moves.len(), 4);
    }

    #[test]
    fn non_first_turn_double_six_keeps_head_rule() {
        // Same 6-6 but NOT the first turn: only one checker may leave the head.
        let b = Board::starting();
        let ts = turns(&b, Player::White, [6, 6], false);
        assert_eq!(ts.len(), 1);
        let t = &ts[0];
        assert_eq!(t.board.own_at(Player::White, 18), 1);
        assert_eq!(t.board.own_at(Player::White, HEAD_POS), N_CHECKERS - 1);
        assert_eq!(t.moves.len(), 1);
    }

    #[test]
    fn no_hitting_blocks_landing_and_can_force_a_pass() {
        // White has only its head checker; Black sits on White's pos 21. With 3-3
        // the only move 24->21 is blocked, so White must pass.
        let mut b = Board::empty();
        b.place(Player::White, HEAD_POS, 1);
        b.place_phys(Player::Black, phys(Player::White, 21), 1);
        let ts = turns(&b, Player::White, [3, 3], false);
        assert_eq!(ts.len(), 1);
        assert!(ts[0].is_pass());
        assert_eq!(ts[0].board.own_at(Player::White, HEAD_POS), 1);
    }

    #[test]
    fn larger_die_must_be_played_when_only_one_fits() {
        // A lone White checker on pos 13; Black blocks White's pos 6. With 6-1:
        //  - 13->7 (six) then 7->6 is blocked;
        //  - 13->12 (one) then 12->6 is blocked.
        // Neither die can follow the other, so only one die is playable and it
        // must be the larger (6): the checker ends on pos 7.
        let mut b = Board::empty();
        b.place(Player::White, 13, 1);
        b.place_phys(Player::Black, phys(Player::White, 6), 1);
        let ts = turns(&b, Player::White, [6, 1], false);
        assert_eq!(ts.len(), 1);
        let t = &ts[0];
        assert_eq!(t.moves.len(), 1);
        assert_eq!(t.moves[0].die, 6);
        assert_eq!(t.board.own_at(Player::White, 7), 1);
        assert_eq!(t.board.own_at(Player::White, 13), 0);
    }

    #[test]
    fn bear_off_exact_and_move_within() {
        // 15 White on pos 6, all home, roll 6-5. The 6 bears one off; the 5 has no
        // exact target and 6 is the highest point, so it must move 6 -> 1.
        let mut b = Board::empty();
        b.place(Player::White, 6, 15);
        let ts = turns(&b, Player::White, [6, 5], false);
        assert_eq!(ts.len(), 1);
        let t = &ts[0];
        assert_eq!(t.board.off[Player::White.index()], 1);
        assert_eq!(t.board.own_at(Player::White, 6), 13);
        assert_eq!(t.board.own_at(Player::White, 1), 1);
    }

    #[test]
    fn bear_off_overshoot_from_highest_only() {
        // Two White checkers on pos 3 (highest occupied), roll 5-5. Each five
        // over-rolls and bears a checker off (3 < 5, no higher point occupied).
        let mut b = Board::empty();
        b.place(Player::White, 3, 2);
        let ts = turns(&b, Player::White, [5, 5], false);
        assert_eq!(ts.len(), 1);
        assert_eq!(ts[0].board.off[Player::White.index()], 2);
        assert_eq!(ts[0].board.checkers_on_board(Player::White), 0);
    }

    #[test]
    fn double_forces_move_within_then_bears_off() {
        // Two White on pos 6, roll 3-3. Can't over-roll (6 is highest, 3 < 6), so
        // both move 6 -> 3; then the remaining two threes bear them off exactly.
        let mut b = Board::empty();
        b.place(Player::White, 6, 2);
        let ts = turns(&b, Player::White, [3, 3], false);
        assert_eq!(ts.len(), 1);
        assert_eq!(ts[0].board.off[Player::White.index()], 2);
        assert_eq!(ts[0].board.checkers_on_board(Player::White), 0);
        assert_eq!(ts[0].moves.len(), 4);
    }

    #[test]
    fn cannot_bear_off_while_a_checker_is_stuck_outside_home() {
        // A checker on pos 20 can't reach home with 3-1 (min landing 16 > 6), so
        // the side is never all-home this turn and may not bear off.
        let mut b = Board::empty();
        b.place(Player::White, 6, 14);
        b.place(Player::White, 20, 1);
        let ts = turns(&b, Player::White, [3, 1], false);
        assert!(!ts.is_empty());
        for t in &ts {
            assert_eq!(t.board.off[Player::White.index()], 0);
        }
    }

    #[test]
    fn bringing_last_checker_home_enables_bear_off_same_turn() {
        // 14 on pos 6, one on pos 8, roll 6-2: play 8->6 (all home) then bear a
        // checker off with the six — legal within the same turn.
        let mut b = Board::empty();
        b.place(Player::White, 6, 14);
        b.place(Player::White, 8, 1);
        let ts = turns(&b, Player::White, [6, 2], false);
        assert!(ts.iter().any(|t| t.board.off[Player::White.index()] == 1));
    }

    #[test]
    fn full_prime_is_illegal_unless_opponent_is_ahead() {
        // White owns a six-wall on Black's positions 7..=12; all Black checkers
        // sit on Black's head (pos 24), behind the wall -> illegal full prime.
        let mut b = Board::empty();
        for q in 7..=12u8 {
            b.place_phys(Player::White, phys(Player::Black, q), 1);
        }
        b.place(Player::Black, HEAD_POS, 15);
        assert!(creates_illegal_prime(&b, Player::White));

        // Move one Black checker ahead of the wall (pos 5) -> now legal.
        b.place(Player::Black, HEAD_POS, 14);
        b.place(Player::Black, 5, 1);
        assert!(!creates_illegal_prime(&b, Player::White));
    }

    #[test]
    fn checkers_are_conserved_across_a_turn() {
        let b = Board::starting();
        for t in turns(&b, Player::White, [5, 2], false) {
            let total = t.board.checkers_on_board(Player::White) + t.board.off[Player::White.index()];
            assert_eq!(total, N_CHECKERS);
        }
    }

    #[test]
    fn first_turn_exception_does_not_force_second_head_checker_when_unneeded() {
        // Audit regression: two checkers on the head, board otherwise empty, so a
        // single checker can chain all four 3s (24->21->18->15->12, pos 12 open).
        // The exception must NOT fire — only one checker may leave the head.
        let mut b = Board::empty();
        b.place(Player::White, HEAD_POS, 2);
        let ts = turns(&b, Player::White, [3, 3], true);
        assert_eq!(ts.len(), 1, "only the single-head-checker line is legal");
        let t = &ts[0];
        assert_eq!(t.board.own_at(Player::White, HEAD_POS), 1, "one checker stays on head");
        assert_eq!(t.board.own_at(Player::White, 12), 1, "the other chained to pos 12");
        assert_eq!(t.moves.len(), 4);
    }

    #[test]
    fn first_turn_exception_skipped_for_black_4_4_when_unneeded() {
        // Symmetric audit regression for Black with 4-4 on an open board.
        let mut b = Board::empty();
        b.place(Player::Black, HEAD_POS, 2);
        let ts = turns(&b, Player::Black, [4, 4], true);
        assert_eq!(ts.len(), 1);
        assert_eq!(ts[0].board.own_at(Player::Black, HEAD_POS), 1);
        assert_eq!(ts[0].board.own_at(Player::Black, 8), 1);
    }

    #[test]
    fn unlimited_head_lets_many_checkers_leave_the_head() {
        // хачапури: head rule disabled. From the standard start with 1-1, all four
        // ones should be playable as four separate head checkers (24->23 x4).
        let b = Board::starting();
        let ts = generate_turns_cfg(&b, Player::White, [1, 1], false, None);
        assert!(ts.iter().any(|t| t.board.own_at(Player::White, 23) == 4));
        // ...which is impossible under the standard head limit of 1.
        let std = generate_turns_cfg(&b, Player::White, [1, 1], false, Some(1));
        assert!(std.iter().all(|t| t.board.own_at(Player::White, HEAD_POS) >= N_CHECKERS - 1));
    }
}
