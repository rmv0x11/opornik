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
        Outcome::Ongoing => None,
    }
}

/// Equity for `to_roll`, who is about to roll, searched `plies` deep. `plies == 0`
/// is the static evaluation; each extra ply averages over the 21 dice rolls and
/// lets the mover pick their best reply (a chance node followed by a max node).
pub fn position_equity<E: Evaluator>(
    eval: &E,
    board: &Board,
    to_roll: Player,
    plies: u8,
    head_limit: Option<u8>,
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
            // `to_roll` picks the reply maximising their own equity.
            let mut best = f32::NEG_INFINITY;
            for t in &turns {
                let my = -position_equity(eval, &t.board, to_roll.opponent(), plies - 1, head_limit);
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
pub fn best_turn_search<E: Evaluator>(
    eval: &E,
    board: &Board,
    mover: Player,
    dice: [u8; 2],
    first_turn: bool,
    head_limit: Option<u8>,
    plies: u8,
) -> Option<(Turn, f32)> {
    let depth = plies.saturating_sub(1);
    generate_turns_cfg(board, mover, dice, first_turn, head_limit)
        .into_iter()
        .map(|t| {
            let equity = -position_equity(eval, &t.board, mover.opponent(), depth, head_limit);
            (t, equity)
        })
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
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
}
