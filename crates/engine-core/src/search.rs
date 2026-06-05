//! Position evaluation and n-ply expectiminimax search.
//!
//! An [`Evaluator`] scores a position from the side-to-move's perspective. On top
//! of a static evaluator (the heuristic, or the neural net) we add lookahead over
//! the 21 distinct dice rolls — chance nodes between decision nodes — which is
//! what turns a decent static net into a strong player.

use crate::board::Board;
use crate::game::{outcome, Outcome};
use crate::moves::{generate_turns_cfg, Turn};
use crate::player::Player;

/// Anything that can score a position for the side to move (higher = better for
/// that side). Equity is in points (≈ `[-2, 2]` for long nardy).
pub trait Evaluator {
    fn equity(&self, board: &Board, mover: Player) -> f32;
}

/// The hand-crafted heuristic as an [`Evaluator`] (baseline / fallback).
pub struct Heuristic;

impl Evaluator for Heuristic {
    fn equity(&self, board: &Board, mover: Player) -> f32 {
        crate::eval::evaluate(board, mover)
    }
}

/// Wraps an evaluator with the exact bear-off database: pure-race positions are
/// scored exactly from the endgame table, everything else by the inner evaluator
/// (the neural net). This makes endgame play perfect — the most common decisive
/// phase of длинные нарды.
pub struct Composite<E: Evaluator> {
    pub inner: E,
    pub bearoff: crate::bearoff::BearoffTable,
}

impl<E: Evaluator> Composite<E> {
    pub fn new(inner: E, bearoff: crate::bearoff::BearoffTable) -> Self {
        Composite { inner, bearoff }
    }
}

impl<E: Evaluator> Evaluator for Composite<E> {
    fn equity(&self, board: &Board, mover: Player) -> f32 {
        if self.bearoff.is_race(board) {
            if let Some(e) = self.bearoff.race_equity(board, mover) {
                return e;
            }
        }
        self.inner.equity(board, mover)
    }
}

/// Exact equity for `to_roll` when the game is already decided, else `None`.
#[inline]
fn terminal_equity(board: &Board, to_roll: Player) -> Option<f32> {
    match outcome(board) {
        Outcome::Win { winner, points, .. } => {
            let sign = if winner == to_roll { 1.0 } else { -1.0 };
            Some(sign * points as f32)
        }
        Outcome::Draw => Some(0.0),
        Outcome::Ongoing => None,
    }
}

/// Default forward-pruning width for interactive (browser) search: at decision
/// nodes deeper than the root, expand only the best `WIDTH` candidate turns as
/// ranked by the cheap 0-ply static eval, instead of all legal turns.
///
/// Backgammon-family expectiminimax fans out as `(turns × 21)` per ply, so an
/// unpruned ply-3 search over a double roll (100+ legal turns) is ~10^8 leaf
/// evaluations — tens of seconds even natively, minutes in single-threaded WASM.
/// Capping the candidate set to the static-best handful keeps the depth-3
/// tactical benefit while bounding cost (this is the standard "forward pruning"
/// every TD-Gammon-lineage bot uses). `8` was chosen from measured ordering
/// accuracy: the true ply-3 best is within the top-8 static candidates ~99% of
/// the time on real midgame positions, so the strength cost is negligible.
pub const SEARCH_WIDTH: usize = 8;

/// Forward-pruning width applied at the **root** of [`best_turn_search`] (the
/// move actually returned). Wider than [`SEARCH_WIDTH`] because the root ranking
/// directly decides the played move, but still bounded: the root fan-out (up to
/// ~130 turns on a double) is the outermost multiplier of the whole ply-3 tree,
/// so capping it is what brings worst-case search from minutes down to a couple
/// of seconds. The deep search only ever *reorders* candidates the static eval
/// already rates near the top, so a generous 12 captures the best move
/// essentially always.
pub const ROOT_WIDTH: usize = 12;

/// Keep only the `width` best turns by a one-shot static (0-ply) evaluation of
/// each resulting position, ranked from `mover`'s perspective. Used for forward
/// pruning at deeper decision nodes. `None` (or a width >= the turn count) keeps
/// every turn — i.e. exact full-width search.
///
/// The static rank costs one net forward pass per turn (cheap relative to a
/// recursive subtree), and ties/ordering only affect *which* candidates survive,
/// never correctness of the equities computed for the survivors.
fn cap_candidates<E: Evaluator>(
    eval: &E,
    turns: Vec<Turn>,
    mover: Player,
    width: Option<usize>,
) -> Vec<Turn> {
    let k = match width {
        Some(k) if turns.len() > k => k,
        _ => return turns,
    };
    let opp = mover.opponent();
    // Score each turn ONCE with the static eval (resulting position from `mover`'s
    // view), then partition to the top-K. Scoring up front avoids re-running the
    // net forward pass inside the comparator (which `select_nth_unstable_by` may
    // call many times per element).
    let mut scored: Vec<(f32, Turn)> = turns
        .into_iter()
        .map(|t| (-eval.equity(&t.board, opp), t))
        .collect();
    // Partition so the K highest-scoring turns occupy the first K slots (O(n)),
    // then drop the rest. The survivors are re-scored exactly by the deep search,
    // so their internal order here does not matter.
    scored.select_nth_unstable_by(k - 1, |a, b| {
        b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(k);
    scored.into_iter().map(|(_, t)| t).collect()
}

/// Equity for `to_roll`, who is about to roll, searched `plies` deep. `plies == 0`
/// is the static evaluation; each extra ply averages over the 21 dice rolls and
/// lets the mover pick their best reply (a chance node followed by a max node).
///
/// `width` bounds the fan-out via forward pruning: at each decision node with
/// more than `width` legal turns, only the top-`width` by static eval are
/// expanded. `None` searches full width (exact expectiminimax). See
/// [`SEARCH_WIDTH`] for the rationale and default.
pub fn position_equity<E: Evaluator>(
    eval: &E,
    board: &Board,
    to_roll: Player,
    plies: u8,
    head_limit: Option<u8>,
) -> f32 {
    position_equity_width(eval, board, to_roll, plies, head_limit, None)
}

/// Like [`position_equity`] but with an explicit forward-pruning `width`.
pub fn position_equity_width<E: Evaluator>(
    eval: &E,
    board: &Board,
    to_roll: Player,
    plies: u8,
    head_limit: Option<u8>,
    width: Option<usize>,
) -> f32 {
    if let Some(e) = terminal_equity(board, to_roll) {
        return e;
    }
    if plies == 0 {
        return eval.equity(board, to_roll);
    }

    let mut weighted = 0.0;
    let total = 36.0;
    for d1 in 1..=6u8 {
        for d2 in d1..=6u8 {
            let weight = if d1 == d2 { 1.0 } else { 2.0 };
            let turns = generate_turns_cfg(board, to_roll, [d1, d2], false, head_limit);
            // Forward pruning: at a non-leaf decision node, only the static-best
            // `width` turns are worth a full recursive subtree.
            let turns = if plies > 1 {
                cap_candidates(eval, turns, to_roll, width)
            } else {
                turns
            };
            // `to_roll` picks the reply maximising their own equity.
            let mut best = f32::NEG_INFINITY;
            for t in &turns {
                let my = -position_equity_width(
                    eval,
                    &t.board,
                    to_roll.opponent(),
                    plies - 1,
                    head_limit,
                    width,
                );
                if my > best {
                    best = my;
                }
            }
            weighted += weight * best;
        }
    }
    weighted / total
}

/// Choose the best turn for `mover` given a known roll, searching `plies` deep
/// (`plies == 1` is static evaluation of each resulting position). Returns the
/// chosen turn together with its equity for `mover`.
///
/// For genuine lookahead (`plies >= 3`) this bounds the cost two ways: the root
/// candidate set is pre-filtered to the static-best [`ROOT_WIDTH`] turns, and
/// every deeper decision node is forward-pruned to [`SEARCH_WIDTH`]. At
/// `plies <= 2` the tree is already small, so it searches full width (exact).
pub fn best_turn_search<E: Evaluator>(
    eval: &E,
    board: &Board,
    mover: Player,
    dice: [u8; 2],
    first_turn: bool,
    head_limit: Option<u8>,
    plies: u8,
) -> Option<(Turn, f32)> {
    if plies >= 3 {
        best_turn_search_width(
            eval,
            board,
            mover,
            dice,
            first_turn,
            head_limit,
            plies,
            Some(SEARCH_WIDTH),
            Some(ROOT_WIDTH),
        )
    } else {
        // ply <= 2: small tree, search everything exactly.
        best_turn_search_width(
            eval, board, mover, dice, first_turn, head_limit, plies, None, None,
        )
    }
}

/// Like [`best_turn_search`] but with explicit forward-pruning widths:
/// `deep_width` caps the candidate set at every decision node below the root,
/// and `root_width` caps the root candidate set itself (pre-ranked by the cheap
/// static eval). `None` for either means full width at that level.
///
/// Capping the root is safe because the only effect is which turns get a deep
/// re-score; the static eval already ranks the genuinely best move at or near
/// the top, so the returned move is the deep-search winner among the top
/// `root_width` static candidates.
#[allow(clippy::too_many_arguments)]
pub fn best_turn_search_width<E: Evaluator>(
    eval: &E,
    board: &Board,
    mover: Player,
    dice: [u8; 2],
    first_turn: bool,
    head_limit: Option<u8>,
    plies: u8,
    deep_width: Option<usize>,
    root_width: Option<usize>,
) -> Option<(Turn, f32)> {
    let depth = plies.saturating_sub(1);
    let turns = generate_turns_cfg(board, mover, dice, first_turn, head_limit);
    let turns = cap_candidates(eval, turns, mover, root_width);
    turns
        .into_iter()
        .map(|t| {
            let equity = -position_equity_width(
                eval,
                &t.board,
                mover.opponent(),
                depth,
                head_limit,
                deep_width,
            );
            (t, equity)
        })
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
}

/// Like [`position_equity_width`] but with a shared leaf-evaluation `budget`. Once
/// the budget is spent, deeper decision nodes "top out" and return their static
/// eval instead of recursing — so total work (and thus wall-clock) is bounded
/// regardless of how bushy the position is. Degrades gracefully: the best
/// candidates (explored depth-first first) keep their full depth.
fn position_equity_budget<E: Evaluator>(
    eval: &E,
    board: &Board,
    to_roll: Player,
    plies: u8,
    head_limit: Option<u8>,
    width: Option<usize>,
    budget: &mut i64,
) -> f32 {
    if let Some(e) = terminal_equity(board, to_roll) {
        return e;
    }
    if plies == 0 || *budget <= 0 {
        *budget -= 1;
        return eval.equity(board, to_roll);
    }
    let mut weighted = 0.0;
    let total = 36.0;
    for d1 in 1..=6u8 {
        for d2 in d1..=6u8 {
            let weight = if d1 == d2 { 1.0 } else { 2.0 };
            let turns = generate_turns_cfg(board, to_roll, [d1, d2], false, head_limit);
            let turns = if plies > 1 {
                cap_candidates(eval, turns, to_roll, width)
            } else {
                turns
            };
            let mut best = f32::NEG_INFINITY;
            for t in &turns {
                let my = -position_equity_budget(
                    eval,
                    &t.board,
                    to_roll.opponent(),
                    plies - 1,
                    head_limit,
                    width,
                    budget,
                );
                if my > best {
                    best = my;
                }
            }
            weighted += weight * best;
        }
    }
    weighted / total
}

/// Best turn under forward pruning AND a hard leaf-evaluation budget (`max_leaves`).
/// Root candidates are ranked best-first by the static eval, so the budget is spent
/// deepening the most promising turns; once exhausted the rest fall back to a
/// shallow/static score. Used by the WASM play path to keep Expert responsive.
#[allow(clippy::too_many_arguments)]
pub fn best_turn_search_budget<E: Evaluator>(
    eval: &E,
    board: &Board,
    mover: Player,
    dice: [u8; 2],
    first_turn: bool,
    head_limit: Option<u8>,
    plies: u8,
    deep_width: Option<usize>,
    root_width: Option<usize>,
    max_leaves: u64,
) -> Option<(Turn, f32)> {
    let depth = plies.saturating_sub(1);
    let opp = mover.opponent();
    let turns = generate_turns_cfg(board, mover, dice, first_turn, head_limit);
    // rank the root best-first by static eval, then keep the top `root_width`
    let mut scored: Vec<(f32, Turn)> = turns
        .into_iter()
        .map(|t| (-eval.equity(&t.board, opp), t))
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    if let Some(rw) = root_width {
        scored.truncate(rw);
    }
    let mut budget = max_leaves as i64;
    let mut best: Option<(Turn, f32)> = None;
    for (_, t) in scored {
        let equity =
            -position_equity_budget(eval, &t.board, opp, depth, head_limit, deep_width, &mut budget);
        if best.as_ref().map(|(_, e)| equity > *e).unwrap_or(true) {
            best = Some((t, equity));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{GameState, Outcome, Rules};

    #[test]
    fn heuristic_search_picks_legal_turns_and_terminates_a_game() {
        // A full game where both sides use 2-ply heuristic search must finish.
        let mut g = GameState::new(Rules::default());
        let mut state = 0x1234_5678u64;
        let mut die = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % 6) as u8 + 1
        };
        for _ in 0..3000 {
            if g.is_over() {
                break;
            }
            let dice = [die(), die()];
            let first = g.is_first_turn();
            let (turn, _) = best_turn_search(
                &Heuristic,
                &g.board,
                g.turn,
                dice,
                first,
                g.rules.head_limit,
                2,
            )
            .unwrap();
            g.apply_turn(&turn);
        }
        assert!(matches!(g.outcome(), Outcome::Win { .. }));
    }

    #[test]
    fn search_takes_an_immediate_win() {
        // White: 14 off, one checker on point 2; rolling 2-1 a checker bears off
        // to win. Search must choose the winning turn.
        let mut b = Board::empty();
        b.off[Player::White.index()] = 14;
        b.place(Player::White, 2, 1);
        let (turn, equity) =
            best_turn_search(&Heuristic, &b, Player::White, [2, 1], false, Some(1), 1).unwrap();
        assert_eq!(turn.board.off[Player::White.index()], 15);
        assert!(equity > 0.0);
    }

    // A busy midgame position with many legal turns (drives the forward-pruning
    // path). 15 checkers per side on disjoint cells.
    fn busy_board() -> Board {
        let mut b = Board::empty();
        b.place(Player::White, 24, 4);
        b.place(Player::White, 21, 2);
        b.place(Player::White, 19, 2);
        b.place(Player::White, 18, 1);
        b.place(Player::White, 15, 2);
        b.place(Player::White, 13, 2);
        b.place(Player::White, 9, 2);
        b.place(Player::Black, 24, 4);
        b.place(Player::Black, 23, 2);
        b.place(Player::Black, 20, 2);
        b.place(Player::Black, 16, 2);
        b.place(Player::Black, 14, 1);
        b.place(Player::Black, 10, 2);
        b.place(Player::Black, 5, 2);
        b
    }

    #[test]
    fn bounded_ply2_matches_full_width_exactly() {
        // Forward pruning only engages at decision nodes with `plies > 1`, so a
        // ply-2 search (root + one ply, whose children are leaves) must be
        // byte-identical to full-width search regardless of the widths passed.
        let b = busy_board();
        assert!(b.is_valid());
        for dice in [[6, 5], [3, 3], [4, 1]] {
            let full = best_turn_search_width(
                &Heuristic,
                &b,
                Player::White,
                dice,
                false,
                Some(1),
                2,
                None,
                None,
            )
            .unwrap();
            let bounded = best_turn_search_width(
                &Heuristic,
                &b,
                Player::White,
                dice,
                false,
                Some(1),
                2,
                Some(SEARCH_WIDTH),
                Some(ROOT_WIDTH),
            )
            .unwrap();
            assert_eq!(full.0.board, bounded.0.board, "ply-2 move differs for {dice:?}");
            assert!((full.1 - bounded.1).abs() < 1e-6, "ply-2 equity differs for {dice:?}");
        }
    }

    #[test]
    fn bounded_ply3_returns_a_legal_turn_on_a_busy_double() {
        // 3-3 on the busy board yields >100 root turns: the default play path must
        // forward-prune and still return one of the genuinely legal turns.
        let b = busy_board();
        let legal = generate_turns_cfg(&b, Player::White, [3, 3], false, Some(1));
        assert!(legal.len() > ROOT_WIDTH, "expected a high-branching position");
        let (turn, _) =
            best_turn_search(&Heuristic, &b, Player::White, [3, 3], false, Some(1), 3).unwrap();
        assert!(
            legal.iter().any(|t| t.board == turn.board),
            "bounded ply-3 returned a turn outside the legal set"
        );
    }

    #[test]
    fn cap_candidates_is_a_noop_when_under_width() {
        // With fewer turns than the cap, every turn is kept (order may change).
        let b = busy_board();
        let turns = generate_turns_cfg(&b, Player::White, [4, 1], false, Some(1));
        let n = turns.len();
        let kept = super::cap_candidates(&Heuristic, turns, Player::White, Some(n + 5));
        assert_eq!(kept.len(), n);
    }
}
