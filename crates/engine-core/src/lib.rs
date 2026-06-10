//! # engine-core
//!
//! The rules engine for **длинные нарды** (Long Nardy) as standardised by the
//! Russian sports nardy federation (ФСНР).
//!
//! This crate is intentionally dependency-free and deterministic so it compiles
//! unchanged to native (server, training, CLI) and to `wasm32` (browser). It
//! provides the board model, fully rule-correct legal move generation, scoring,
//! a baseline heuristic evaluator and a greedy baseline AI. The self-play neural
//! net, rollouts and endgame databases layer on top of this core.
//!
//! ## Quick start
//! ```
//! use engine_core::{GameState, Rules, best_turn};
//!
//! let mut game = GameState::new(Rules::default());
//! game.set_dice(3, 1);
//! let turn = best_turn(&game).expect("a legal turn");
//! game.apply_turn(&turn);
//! assert_eq!(game.turn_number, 1);
//! ```

pub mod ai;
pub mod analysis;
pub mod bearoff;
pub mod board;
pub mod cube;
pub mod encoding;
pub mod encoding2;
pub mod eval;
pub mod game;
pub mod matchplay;
pub mod moves;
pub mod net;
pub mod net2;
pub mod phase;
pub mod player;
pub mod search;

pub use ai::{best_turn, best_turn_for};
pub use analysis::{
    analyze_cube, rank_turns, win_probabilities, CubeAction, CubeAnalysis, CubeContext,
    Probabilities, RankedTurn,
};
pub use board::{
    phys, pos_of_phys, Board, HEAD_POS, HOME_HIGH, N_CHECKERS, N_POINTS, START_PIP,
};
pub use bearoff::BearoffTable;
pub use cube::{Cube, CubeError, MAX_CUBE};
pub use encoding::{encode, INPUT_SIZE};
pub use eval::{evaluate, WIN_SCORE};
pub use game::{outcome, GameState, Outcome, Rules, Variant};
pub use matchplay::{MatchState, MoneyGame, STANDARD_MATCH_LENGTHS};
pub use moves::{
    creates_illegal_prime, generate_turns, generate_turns_cfg, legal_sequences, CheckerMove, Turn,
};
pub use encoding2::{CONTACT_INPUTS, RACE_INPUTS};
pub use net::{Net, OUTPUTS};
pub use net2::{NetV2, PhaseNets, OUTPUTS_V2};
pub use phase::{contact_for, has_contact};
pub use player::Player;
pub use search::{
    best_turn_search, best_turn_search_budget, best_turn_search_width, position_equity,
    position_equity_width, Composite, Evaluator, Heuristic, ROOT_WIDTH, SEARCH_WIDTH,
};
