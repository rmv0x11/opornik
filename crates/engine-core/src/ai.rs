//! Baseline AI: greedy 1-ply move selection over the heuristic evaluator.
//!
//! This is the placeholder opponent until the self-play neural net lands. It
//! enumerates every legal turn and picks the resulting position with the best
//! heuristic score for the side to move. Deterministic given the inputs (ties
//! break toward the first-generated turn).

use crate::eval::evaluate;
use crate::game::GameState;
use crate::moves::{generate_turns, Turn};
use crate::player::Player;

/// Choose the best turn for `player` given a roll. Returns `None` only when there
/// are no legal turns at all (which never happens — generation always yields at
/// least a pass), but the signature keeps callers honest.
pub fn best_turn_for(
    board: &crate::board::Board,
    player: Player,
    dice: [u8; 2],
    first_turn: bool,
) -> Option<Turn> {
    let turns = generate_turns(board, player, dice, first_turn);
    pick_best(turns, player)
}

/// Best turn for the side to move in `state` (uses `state.dice`).
pub fn best_turn(state: &GameState) -> Option<Turn> {
    pick_best(state.legal_turns(), state.turn)
}

fn pick_best(turns: Vec<Turn>, player: Player) -> Option<Turn> {
    turns.into_iter().max_by(|a, b| {
        let ea = evaluate(&a.board, player);
        let eb = evaluate(&b.board, player);
        ea.partial_cmp(&eb).unwrap_or(std::cmp::Ordering::Equal)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;

    #[test]
    fn picks_a_legal_turn_from_the_start() {
        let b = Board::starting();
        let t = best_turn_for(&b, Player::White, [6, 5], true).unwrap();
        // A legal opening play exists and conserves checkers.
        let total = t.board.checkers_on_board(Player::White) + t.board.off[Player::White.index()];
        assert_eq!(total, crate::board::N_CHECKERS);
    }

    #[test]
    fn prefers_bearing_off_when_possible() {
        // All home on pos 6; rolling 6-6 should bear off four checkers.
        let mut b = Board::empty();
        b.place(Player::White, 6, 15);
        let t = best_turn_for(&b, Player::White, [6, 6], false).unwrap();
        assert_eq!(t.board.off[Player::White.index()], 4);
    }
}
