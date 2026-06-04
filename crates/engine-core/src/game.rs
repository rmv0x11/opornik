//! Game state, rule variants and outcomes.

use crate::board::Board;
use crate::board::N_CHECKERS;
use crate::cube::{Cube, CubeError};
use crate::moves::{generate_turns_cfg, Turn};
use crate::player::Player;

/// Long-nardy rule variant.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Variant {
    /// Традиционные — match to N points, no cube.
    Traditional,
    /// Nardegammon — match play with a doubling cube + Crawford.
    Nardegammon,
    /// Classic — the second player gets a last roll, so a game can be drawn.
    Classic,
    /// Хачапури — 4 on the head, 11 on point 6, head rule disabled (money game).
    Hachapuri,
}

/// Tunable rule flags. Build with [`Rules::for_variant`] or the named
/// constructors, then override individual fields as needed.
#[derive(Clone, Copy, Debug)]
pub struct Rules {
    pub variant: Variant,
    /// Max checkers that may leave the head per turn. `Some(1)` = standard;
    /// `None` = no head rule (хачапури).
    pub head_limit: Option<u8>,
    /// First-turn 6-6/4-4/3-3 may take two off the head (only at head limit 1).
    pub head_doubles_exception: bool,
    /// Doubling cube in play (money game / Nardegammon).
    pub cube_enabled: bool,
    /// Jacoby rule (money game): mars counts only after the cube is turned.
    pub jacoby: bool,
    /// Beavers allowed (money game).
    pub beaver: bool,
    /// Classic/modern-tournament last-roll equalisation can produce a draw.
    pub allow_final_draw: bool,
}

impl Rules {
    /// Default rules for a variant.
    pub fn for_variant(variant: Variant) -> Self {
        match variant {
            Variant::Traditional => Rules {
                variant,
                head_limit: Some(1),
                head_doubles_exception: true,
                cube_enabled: false,
                jacoby: false,
                beaver: false,
                allow_final_draw: false,
            },
            Variant::Nardegammon => Rules {
                variant,
                head_limit: Some(1),
                head_doubles_exception: true,
                cube_enabled: true,
                jacoby: false,
                beaver: false,
                allow_final_draw: false,
            },
            Variant::Classic => Rules {
                variant,
                head_limit: Some(1),
                head_doubles_exception: true,
                cube_enabled: false,
                jacoby: false,
                beaver: false,
                allow_final_draw: true,
            },
            Variant::Hachapuri => Rules {
                variant,
                head_limit: None,
                head_doubles_exception: false,
                cube_enabled: true,
                jacoby: true,
                beaver: true,
                allow_final_draw: false,
            },
        }
    }

    /// Money-game rules (cube + Jacoby + beavers) over the Traditional ruleset.
    pub fn money_game() -> Self {
        Rules {
            cube_enabled: true,
            jacoby: true,
            beaver: true,
            ..Rules::for_variant(Variant::Traditional)
        }
    }

    /// The starting board for these rules.
    pub fn starting_board(&self) -> Board {
        match self.variant {
            Variant::Hachapuri => Board::starting_hachapuri(),
            _ => Board::starting(),
        }
    }
}

impl Default for Rules {
    fn default() -> Self {
        Rules::for_variant(Variant::Traditional)
    }
}

/// Result of a finished (or ongoing) game.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    Ongoing,
    /// Someone has borne off all 15 checkers.
    Win {
        winner: Player,
        /// `true` if the loser bore off zero checkers (марс).
        mars: bool,
        /// Points awarded: 1 for оин, 2 for марс.
        points: u8,
    },
}

/// Static scoring of a board, independent of whose turn it is.
pub fn outcome(board: &Board) -> Outcome {
    for &p in &Player::ALL {
        if board.off[p.index()] == N_CHECKERS {
            let opp = p.opponent();
            let mars = board.off[opp.index()] == 0;
            return Outcome::Win {
                winner: p,
                mars,
                points: if mars { 2 } else { 1 },
            };
        }
    }
    Outcome::Ongoing
}

/// Full game state. Dice are set externally (the core stays RNG-free and
/// deterministic so it is trivially testable and reproducible).
#[derive(Clone, Debug)]
pub struct GameState {
    pub board: Board,
    pub turn: Player,
    pub dice: Option<[u8; 2]>,
    /// Whether each player has already completed their first turn.
    pub first_turn_done: [bool; 2],
    /// Number of completed half-moves (turns) so far.
    pub turn_number: u32,
    pub rules: Rules,
    /// The doubling cube (value 1, centered for a fresh game).
    pub cube: Cube,
    /// Whether the current game is the Crawford game (no doubling). Set by the
    /// match layer; the engine just honours it.
    pub crawford: bool,
}

impl GameState {
    /// A fresh game in the variant's starting position, White to move.
    pub fn new(rules: Rules) -> Self {
        GameState {
            board: rules.starting_board(),
            turn: Player::White,
            dice: None,
            first_turn_done: [false, false],
            turn_number: 0,
            rules,
            cube: Cube::centered(),
            crawford: false,
        }
    }

    /// Whether `player` may offer a double now (cube enabled, not Crawford, and
    /// the cube is centered or owned by `player`).
    pub fn can_double(&self, player: Player) -> bool {
        self.rules.cube_enabled && !self.crawford && self.cube.may_double(player)
    }

    /// `player` offers (and the opponent accepts) a double.
    pub fn double(&mut self, player: Player) -> Result<(), CubeError> {
        if !self.rules.cube_enabled || self.crawford {
            return Err(CubeError::NotAvailable);
        }
        self.cube.double(player)
    }

    /// `player` beavers in response to a double (money game).
    pub fn beaver(&mut self, player: Player) -> Result<(), CubeError> {
        if !self.rules.cube_enabled || self.crawford {
            return Err(CubeError::NotAvailable);
        }
        self.cube.beaver(player)
    }

    /// Whether the side to move is taking its first turn (for the head exception).
    pub fn is_first_turn(&self) -> bool {
        !self.first_turn_done[self.turn.index()]
    }

    /// Set the dice for the upcoming turn.
    pub fn set_dice(&mut self, d1: u8, d2: u8) {
        self.dice = Some([d1, d2]);
    }

    /// Legal turns for the side to move with the currently set dice.
    /// Returns an empty vector if no dice are set.
    pub fn legal_turns(&self) -> Vec<Turn> {
        match self.dice {
            Some(dice) => {
                let first = self.is_first_turn() && self.rules.head_doubles_exception;
                generate_turns_cfg(&self.board, self.turn, dice, first, self.rules.head_limit)
            }
            None => Vec::new(),
        }
    }

    /// Apply a chosen turn: advances the board, marks the first turn done, clears
    /// dice and passes the move to the opponent.
    pub fn apply_turn(&mut self, turn: &Turn) {
        self.board = turn.board.clone();
        self.first_turn_done[self.turn.index()] = true;
        self.turn_number += 1;
        self.dice = None;
        self.turn = self.turn.opponent();
    }

    /// Current outcome.
    pub fn outcome(&self) -> Outcome {
        outcome(&self.board)
    }

    /// Whether the game is over.
    pub fn is_over(&self) -> bool {
        !matches!(self.outcome(), Outcome::Ongoing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ongoing_at_start() {
        let g = GameState::new(Rules::default());
        assert_eq!(g.outcome(), Outcome::Ongoing);
        assert!(g.is_first_turn());
    }

    #[test]
    fn single_win_scores_one() {
        let mut b = Board::empty();
        b.off[Player::White.index()] = N_CHECKERS;
        b.off[Player::Black.index()] = 5;
        assert_eq!(
            outcome(&b),
            Outcome::Win {
                winner: Player::White,
                mars: false,
                points: 1
            }
        );
    }

    #[test]
    fn mars_scores_two() {
        let mut b = Board::empty();
        b.off[Player::White.index()] = N_CHECKERS;
        b.off[Player::Black.index()] = 0;
        assert_eq!(
            outcome(&b),
            Outcome::Win {
                winner: Player::White,
                mars: true,
                points: 2
            }
        );
    }

    #[test]
    fn applying_a_turn_flips_side_and_marks_first_turn() {
        let mut g = GameState::new(Rules::default());
        g.set_dice(3, 1);
        let turns = g.legal_turns();
        assert!(!turns.is_empty());
        g.apply_turn(&turns[0]);
        assert_eq!(g.turn, Player::Black);
        assert!(g.first_turn_done[Player::White.index()]);
        assert_eq!(g.dice, None);
        assert_eq!(g.turn_number, 1);
    }

    #[test]
    fn hachapuri_game_starts_valid_with_unlimited_head() {
        let g = GameState::new(Rules::for_variant(Variant::Hachapuri));
        assert!(g.board.is_valid());
        assert_eq!(g.board.own_at(Player::White, N_CHECKERS - 11), 0); // sanity
        assert_eq!(g.board.own_at(Player::White, 24), 4);
        assert_eq!(g.board.own_at(Player::White, 6), 11);
        assert_eq!(g.rules.head_limit, None);
    }

    #[test]
    fn cube_is_gated_by_rules_and_crawford() {
        // Traditional: no cube.
        let mut t = GameState::new(Rules::default());
        assert!(!t.can_double(Player::White));
        assert!(t.double(Player::White).is_err());

        // Money game: cube available, doubling transfers ownership.
        let mut m = GameState::new(Rules::money_game());
        assert!(m.can_double(Player::White));
        m.double(Player::White).unwrap();
        assert_eq!(m.cube.value, 2);
        assert_eq!(m.cube.owner, Some(Player::Black));
        assert!(!m.can_double(Player::White)); // White no longer owns it

        // Crawford freezes the cube.
        let mut c = GameState::new(Rules::money_game());
        c.crawford = true;
        assert!(!c.can_double(Player::White));
    }
}
