//! `engine-wasm` — a thin `wasm-bindgen` wrapper exposing the long-nardy engine to
//! the browser. A single stateful [`Engine`] owns the game, the trained net and a
//! lazily-built bear-off table; structured values cross the boundary as JSON
//! strings. The heavy methods (`best_move`, `analyze`, `equity`) are meant to run
//! inside a Web Worker so the UI thread never blocks.

mod dto;

use engine_core::analysis::{analyze_cube, CubeContext};
use engine_core::{
    best_turn_search, best_turn_search_budget, position_equity, position_equity_width, BearoffTable,
    Board, Evaluator, GameState, Net, Player, Rules, Variant, ROOT_WIDTH, SEARCH_WIDTH,
};

/// Hard leaf-evaluation budget for the 3-ply play search, so Expert stays
/// responsive (~1.5s) even in bushy doubles positions where an exact search
/// would take many seconds. Tuned for single-threaded WASM.
const PLAY_LEAF_BUDGET: u64 = 90_000;
use wasm_bindgen::prelude::*;

use dto::*;

/// The trained net, bundled at compile time (≈64 KB).
const NET_BYTES: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../models/nardy-net.bin"));

/// Evaluator that uses the exact bear-off table for pure races and the net
/// otherwise — borrowing both (no clones), mirroring `engine_core::Composite`.
struct RaceOrNet<'a> {
    net: &'a Net,
    bearoff: &'a BearoffTable,
}

impl Evaluator for RaceOrNet<'_> {
    fn equity(&self, b: &Board, m: Player) -> f32 {
        if self.bearoff.is_race(b) {
            if let Some(e) = self.bearoff.race_equity(b, m) {
                return e;
            }
        }
        self.net.equity(b, m)
    }
}

fn js_err<E: std::fmt::Debug>(e: E) -> JsError {
    JsError::new(&format!("{e:?}"))
}

fn validate_die(d: u8) -> Result<(), JsError> {
    if !(1..=6).contains(&d) {
        return Err(JsError::new("die out of range 1..=6"));
    }
    Ok(())
}

fn to_json<T: serde::Serialize>(v: &T) -> Result<String, JsError> {
    serde_json::to_string(v).map_err(|e| JsError::new(&e.to_string()))
}

#[wasm_bindgen]
pub struct Engine {
    game: GameState,
    net: Net,
    bearoff: Option<BearoffTable>,
}

#[wasm_bindgen]
impl Engine {
    /// Create a fresh game for a variant ("traditional"|"nardegammon"|"classic"|"hachapuri")
    /// using the bundled trained net.
    #[wasm_bindgen(constructor)]
    pub fn new(variant: &str) -> Result<Engine, JsError> {
        #[cfg(feature = "panic-hook")]
        console_error_panic_hook::set_once();
        Engine::with_net_bytes(variant, NET_BYTES)
    }

    /// Create with caller-supplied net bytes (hot-swappable / fetched weights).
    #[wasm_bindgen(js_name = withNetBytes)]
    pub fn with_net_bytes(variant: &str, bytes: &[u8]) -> Result<Engine, JsError> {
        let net = Net::from_bytes(bytes).ok_or_else(|| JsError::new("net weights malformed"))?;
        if net.input != engine_core::INPUT_SIZE || net.output != engine_core::OUTPUTS {
            return Err(JsError::new("net shape mismatch (expected 196 inputs, 3 outputs)"));
        }
        let rules = Rules::for_variant(parse_variant(variant).map_err(js_err)?);
        Ok(Engine {
            game: GameState::new(rules),
            net,
            bearoff: None,
        })
    }

    /// Current position as JSON `PositionDto`.
    #[wasm_bindgen(js_name = getPosition)]
    pub fn get_position(&self) -> Result<String, JsError> {
        to_json(&position_dto(&self.game))
    }

    /// Set the dice for the side to move (UI rolls and passes them in).
    /// Dice must be in 1..=6 (an out-of-range value would index past the move
    /// generator's roll table and panic — which aborts the WASM module).
    #[wasm_bindgen(js_name = setDice)]
    pub fn set_dice(&mut self, d1: u8, d2: u8) -> Result<(), JsError> {
        validate_die(d1)?;
        validate_die(d2)?;
        self.game.set_dice(d1, d2);
        Ok(())
    }

    /// Legal turns for the current dice, as JSON `TurnDto[]` (id = stable index).
    #[wasm_bindgen(js_name = legalTurns)]
    pub fn legal_turns(&self) -> Result<String, JsError> {
        let turns = self.game.legal_turns();
        let dtos: Vec<TurnDto> = turns
            .iter()
            .enumerate()
            .map(|(i, t)| turn_dto(i as u32, t))
            .collect();
        to_json(&dtos)
    }

    /// Every legal *ordered* sub-move sequence for the current dice, as JSON
    /// `SequenceDto[]` (`{ moves, turn_id }`). The UI drives click/drag
    /// move-building by prefix-matching the player's chosen sub-moves against
    /// these sequences — each is legal at every intermediate step — then commits
    /// via `applyTurn(turn_id)`. This replaces multiset-matching against the
    /// deduped turn list, which silently forbade some legal move orderings.
    #[wasm_bindgen(js_name = legalSequences)]
    pub fn legal_sequences(&self) -> Result<String, JsError> {
        let turns = self.game.legal_turns();
        let seqs = self.game.legal_sequences();
        to_json(&sequence_dtos(&turns, &seqs))
    }

    /// Apply the turn with the given id; returns the new position JSON.
    #[wasm_bindgen(js_name = applyTurn)]
    pub fn apply_turn(&mut self, id: u32) -> Result<String, JsError> {
        let turns = self.game.legal_turns();
        let turn = turns
            .get(id as usize)
            .ok_or_else(|| JsError::new("illegal turn id"))?;
        self.game.apply_turn(turn);
        self.get_position()
    }

    /// The engine's best move at the current dice, searched `ply` deep. Returns
    /// JSON `BestMoveDto` (chosen turn + equity + mover-perspective probabilities).
    #[wasm_bindgen(js_name = bestMove)]
    pub fn best_move(&mut self, ply: u8) -> Result<String, JsError> {
        let dice = self.game.dice.ok_or_else(|| JsError::new("no dice set"))?;
        let first = self.game.is_first_turn() && self.game.rules.head_doubles_exception;
        let mover = self.game.turn;
        let head_limit = self.game.rules.head_limit;

        // Play path uses the net directly: the exact bear-off table costs seconds
        // to build and (per the ER measurement) does not change play strength.
        // 3-ply (Expert) runs under a leaf budget so it stays fast in bushy
        // positions; 1–2 ply is small and searched exactly.
        let (turn, equity) = if ply >= 3 {
            best_turn_search_budget(
                &self.net,
                &self.game.board,
                mover,
                dice,
                first,
                head_limit,
                ply,
                Some(SEARCH_WIDTH),
                Some(ROOT_WIDTH),
                PLAY_LEAF_BUDGET,
            )
        } else {
            best_turn_search(&self.net, &self.game.board, mover, dice, first, head_limit, ply.max(1))
        }
        .ok_or_else(|| JsError::new("no legal turn"))?;

        // id within the canonical legal-turns list
        let turns = self.game.legal_turns();
        let id = turn_id(&turns, &turn);

        // mover-perspective probabilities of the resulting position
        let op = self.net.evaluate_board(&turn.board, mover.opponent());
        let probs = ProbabilitiesDto {
            win: 1.0 - op.win,
            win_mars: op.lose_mars,
            lose: op.win,
            lose_mars: op.win_mars,
        };
        to_json(&BestMoveDto {
            turn: turn_dto(id, &turn),
            equity,
            probs,
        })
    }

    /// Ranked legal turns (best-first) with equity, win% and equity-loss vs best.
    /// `ply==1` uses the net's static eval; `ply>=2` re-scores with search.
    #[wasm_bindgen(js_name = analyze)]
    pub fn analyze(&mut self, ply: u8) -> Result<String, JsError> {
        let mover = self.game.turn;
        let opp = mover.opponent();
        let head_limit = self.game.rules.head_limit;
        let turns = self.game.legal_turns();
        if turns.is_empty() {
            return to_json::<Vec<RankedTurnDto>>(&Vec::new());
        }

        let deep = ply >= 2;
        if deep && self.bearoff.is_none() {
            self.bearoff = Some(BearoffTable::build());
        }

        let mut ranked: Vec<RankedTurnDto> = turns
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let op = self.net.evaluate_board(&t.board, opp);
                let equity = if deep {
                    let eval = RaceOrNet {
                        net: &self.net,
                        bearoff: self.bearoff.as_ref().unwrap(),
                    };
                    // Forward-prune deeper nodes for ply>=3 (same bound as the
                    // play path) so analysis stays responsive; ply==2 is exact.
                    let width = if ply >= 3 { Some(SEARCH_WIDTH) } else { None };
                    -position_equity_width(&eval, &t.board, opp, ply - 1, head_limit, width)
                } else {
                    -op.cubeless_equity()
                };
                RankedTurnDto {
                    turn: turn_dto(i as u32, t),
                    equity,
                    win: 1.0 - op.win,
                    equity_loss: 0.0,
                }
            })
            .collect();

        ranked.sort_by(|a, b| {
            b.equity
                .partial_cmp(&a.equity)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        if let Some(best) = ranked.first().map(|r| r.equity) {
            for r in &mut ranked {
                r.equity_loss = best - r.equity;
            }
        }
        to_json(&ranked)
    }

    /// Win/mars probabilities for the side to move at the current board (net).
    #[wasm_bindgen(js_name = winProbabilities)]
    pub fn win_probabilities(&self) -> Result<String, JsError> {
        let p = self.net.evaluate_board(&self.game.board, self.game.turn);
        to_json(&probs_dto(&p))
    }

    /// Equity (mover's view) of the current position searched `ply` deep.
    #[wasm_bindgen(js_name = equity)]
    pub fn equity(&mut self, ply: u8) -> f32 {
        let mover = self.game.turn;
        let head_limit = self.game.rules.head_limit;
        if self.bearoff.is_none() {
            self.bearoff = Some(BearoffTable::build());
        }
        let eval = RaceOrNet {
            net: &self.net,
            bearoff: self.bearoff.as_ref().unwrap(),
        };
        position_equity(&eval, &self.game.board, mover, ply, head_limit)
    }

    /// Cube recommendation for the side to move. Returns JSON `CubeDecisionDto`.
    #[wasm_bindgen(js_name = cubeDecision)]
    pub fn cube_decision(&self) -> Result<String, JsError> {
        let probs = self.net.evaluate_board(&self.game.board, self.game.turn);
        let ctx = CubeContext {
            money: self.game.rules.variant == Variant::Hachapuri,
            jacoby: self.game.rules.jacoby,
            beaver: self.game.rules.beaver,
            crawford: self.game.crawford,
            cube: self.game.cube,
            on_roll: self.game.turn,
        };
        let a = analyze_cube(&probs, &ctx);
        to_json(&CubeDecisionDto {
            action: cube_action_str(a.action),
            equity: a.equity,
            opponent_should_take: a.opponent_should_take,
            recommend_beaver: a.recommend_beaver,
            note: a.note.to_string(),
            probs: probs_dto(&probs),
        })
    }

    /// Side to move offers (and opponent accepts) a double.
    #[wasm_bindgen(js_name = offerDouble)]
    pub fn offer_double(&mut self) -> Result<String, JsError> {
        let p = self.game.turn;
        self.game.double(p).map_err(js_err)?;
        self.get_position()
    }

    /// Beaver in response to a double (money game).
    #[wasm_bindgen(js_name = beaver)]
    pub fn beaver(&mut self) -> Result<String, JsError> {
        let p = self.game.turn;
        self.game.beaver(p).map_err(js_err)?;
        self.get_position()
    }

    /// Replace the game state from a JSON `SetupDto` (editor). Validates 15+15.
    #[wasm_bindgen(js_name = setPosition)]
    pub fn set_position(&mut self, setup_json: &str) -> Result<String, JsError> {
        let setup: SetupDto = serde_json::from_str(setup_json).map_err(|e| JsError::new(&e.to_string()))?;
        let variant = parse_variant(&setup.variant).map_err(js_err)?;
        let turn = parse_player(&setup.turn).map_err(js_err)?;

        for pt in setup.white.iter().chain(setup.black.iter()) {
            if !(1..=24).contains(&pt.pos) {
                return Err(JsError::new("point position out of range 1..=24"));
            }
            if pt.count > 15 {
                return Err(JsError::new("point count out of range 0..=15"));
            }
        }
        if let Some([d1, d2]) = setup.dice {
            validate_die(d1)?;
            validate_die(d2)?;
        }

        let mut board = Board::empty();
        for pt in &setup.white {
            board.place(Player::White, pt.pos, pt.count);
        }
        for pt in &setup.black {
            board.place(Player::Black, pt.pos, pt.count);
        }
        board.off = setup.off;
        if !board.is_valid() {
            return Err(JsError::new("position must have exactly 15 checkers per side"));
        }

        let mut game = GameState::new(Rules::for_variant(variant));
        game.board = board;
        game.turn = turn;
        game.dice = setup.dice;
        game.first_turn_done = [true, true]; // editor positions are not first turns
        game.crawford = setup.crawford.unwrap_or(false);
        if let Some(n) = setup.turn_number {
            game.turn_number = n;
        }
        if let Some(c) = setup.cube {
            game.cube = engine_core::Cube {
                value: c.value,
                owner: match c.owner.as_deref() {
                    Some("white") => Some(Player::White),
                    Some("black") => Some(Player::Black),
                    _ => None,
                },
                turned: c.turned,
            };
        }
        self.game = game;
        self.get_position()
    }

    /// Reset to the variant's starting position, White to move.
    #[wasm_bindgen(js_name = reset)]
    pub fn reset(&mut self) -> Result<String, JsError> {
        self.game = GameState::new(self.game.rules);
        self.get_position()
    }

    /// Set which player is on roll (e.g. the winner of the opening-roll for the
    /// first move). Unlike `setPosition` this preserves the per-player first-turn
    /// flags, so the head-doubles exception still applies on that player's first move.
    #[wasm_bindgen(js_name = setTurn)]
    pub fn set_turn(&mut self, color: &str) -> Result<String, JsError> {
        self.game.turn = parse_player(color).map_err(js_err)?;
        self.get_position()
    }

    /// Mark the current game as the Crawford game (no doubling) or not. Set by the
    /// match layer after reset(), before play.
    #[wasm_bindgen(js_name = setCrawford)]
    pub fn set_crawford(&mut self, on: bool) -> Result<String, JsError> {
        self.game.crawford = on;
        self.get_position()
    }
}
