//! Match play and money game.
//!
//! A session is either a **money game** (no target score; the cube, the Jacoby
//! rule and beavers are in play) or a **match** to a target number of points
//! (7/9/11/13/15/17 are the standard lengths) governed by the **Crawford rule**.

use crate::player::Player;

/// Standard competitive match lengths.
pub const STANDARD_MATCH_LENGTHS: [u16; 6] = [7, 9, 11, 13, 15, 17];

/// Money-game settings.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MoneyGame {
    /// Jacoby rule: gammons/mars count only after the cube has been turned.
    pub jacoby: bool,
    /// Beavers (and raccoons) are allowed.
    pub beaver: bool,
}

impl Default for MoneyGame {
    fn default() -> Self {
        MoneyGame {
            jacoby: true,
            beaver: true,
        }
    }
}

/// Match state: target length, running score and whether the Crawford game has
/// been played.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MatchState {
    pub length: u16,
    /// Points scored, indexed by [`Player::index`].
    pub score: [u16; 2],
    /// Whether the Crawford game has already been completed.
    pub crawford_played: bool,
}

impl MatchState {
    /// A new match to `length` points, 0–0.
    pub fn new(length: u16) -> Self {
        MatchState {
            length,
            score: [0, 0],
            crawford_played: false,
        }
    }

    /// Points `player` still needs to win the match.
    pub fn away(&self, player: Player) -> u16 {
        self.length.saturating_sub(self.score[player.index()])
    }

    /// Whether the match has been decided.
    pub fn is_over(&self) -> bool {
        self.score[0] >= self.length || self.score[1] >= self.length
    }

    /// Whether either player is exactly one point from winning.
    pub fn someone_is_one_away(&self) -> bool {
        Player::ALL.iter().any(|&p| self.away(p) == 1)
    }

    /// Whether the upcoming game is the Crawford game (a player just reached
    /// 1-away and the Crawford game has not yet been played). No doubling is
    /// permitted in the Crawford game.
    pub fn is_crawford_game(&self) -> bool {
        !self.is_over() && !self.crawford_played && self.someone_is_one_away()
    }

    /// Whether the cube may be used in the upcoming game (always, except the
    /// Crawford game).
    pub fn cube_allowed(&self) -> bool {
        !self.is_crawford_game()
    }

    /// Record a finished game's result, advancing the score and the Crawford flag.
    pub fn record(&mut self, winner: Player, points: u16) {
        if self.is_crawford_game() {
            self.crawford_played = true;
        }
        self.score[winner.index()] += points;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn away_and_over() {
        let mut m = MatchState::new(7);
        assert_eq!(m.away(Player::White), 7);
        m.score[Player::White.index()] = 7;
        assert!(m.is_over());
    }

    #[test]
    fn crawford_game_disables_the_cube_then_post_crawford_re_enables_it() {
        let mut m = MatchState::new(7);
        // White reaches 6 (1-away) → next game is Crawford, no cube.
        m.record(Player::White, 6);
        assert_eq!(m.away(Player::White), 1);
        assert!(m.is_crawford_game());
        assert!(!m.cube_allowed());

        // Play the Crawford game (Black wins 2): Crawford is now done.
        m.record(Player::Black, 2);
        assert!(m.crawford_played);
        // White still 1-away, but post-Crawford doubling is allowed again.
        assert!(!m.is_crawford_game());
        assert!(m.cube_allowed());
    }

    #[test]
    fn no_crawford_until_someone_is_one_away() {
        let mut m = MatchState::new(11);
        m.record(Player::White, 2);
        assert!(!m.is_crawford_game());
        assert!(m.cube_allowed());
    }
}
