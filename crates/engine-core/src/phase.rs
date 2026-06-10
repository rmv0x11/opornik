//! Game-phase detection: is blocking interaction still possible?
//!
//! In long nardy there is no hitting, so the only inter-player dynamics is
//! *blocking*: an occupied cell cannot be landed on. Once every checker of both
//! sides has passed every opposing checker (the paths are disengaged), the game
//! is a pure race and a dedicated race evaluator applies. This module hosts the
//! contact test in the rules core so both the training tooling and the phase-net
//! dispatcher share one definition.

use crate::board::Board;
use crate::player::Player;

/// True while an opponent checker stands on a cell some checker of `player`
/// has yet to cross (i.e. blocking interaction is still possible for them).
pub fn contact_for(board: &Board, player: Player) -> bool {
    let highest = board.highest_occupied(player);
    (1..highest).any(|q| board.opp_at(player, q) > 0)
}

/// Whether any blocking interaction remains possible for either side.
pub fn has_contact(board: &Board) -> bool {
    contact_for(board, Player::White) || contact_for(board, Player::Black)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_has_contact_and_disengaged_race_does_not() {
        assert!(has_contact(&Board::starting()));

        let mut race = Board::empty();
        race.place(Player::White, 8, 15);
        race.place(Player::Black, 8, 15);
        assert!(!has_contact(&race));
    }
}
