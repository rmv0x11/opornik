//! Plain data-transfer structs crossing the JS↔WASM boundary as JSON, plus
//! conversions to/from engine-core types. The UI works in per-player path
//! coordinates (1..=24, head = 24, home 1..=6, bear-off destination = 0); it never
//! sees the internal physical cells.

use engine_core::{
    analysis::{CubeAction, Probabilities},
    Board, CheckerMove, Cube, GameState, Outcome, Player, Turn, Variant, HEAD_POS,
};
use serde::{Deserialize, Serialize};

pub fn player_str(p: Player) -> String {
    match p {
        Player::White => "white",
        Player::Black => "black",
    }
    .to_string()
}

pub fn parse_player(s: &str) -> Result<Player, String> {
    match s {
        "white" => Ok(Player::White),
        "black" => Ok(Player::Black),
        other => Err(format!("unknown player: {other}")),
    }
}

pub fn variant_str(v: Variant) -> String {
    match v {
        Variant::Traditional => "traditional",
        Variant::Nardegammon => "nardegammon",
        Variant::Classic => "classic",
        Variant::Hachapuri => "hachapuri",
    }
    .to_string()
}

pub fn parse_variant(s: &str) -> Result<Variant, String> {
    match s {
        "traditional" => Ok(Variant::Traditional),
        "nardegammon" => Ok(Variant::Nardegammon),
        "classic" => Ok(Variant::Classic),
        "hachapuri" => Ok(Variant::Hachapuri),
        other => Err(format!("unknown variant: {other}")),
    }
}

pub fn cube_action_str(a: CubeAction) -> String {
    match a {
        CubeAction::NoDouble => "no_double",
        CubeAction::DoubleTake => "double_take",
        CubeAction::DoublePass => "double_pass",
        CubeAction::TooGood => "too_good",
    }
    .to_string()
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PointDto {
    pub pos: u8,
    pub player: String,
    pub count: u8,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CubeDto {
    pub value: u16,
    pub owner: Option<String>,
    pub turned: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct OutcomeDto {
    pub kind: String,
    pub winner: Option<String>,
    pub mars: Option<bool>,
    pub points: Option<u8>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PositionDto {
    pub variant: String,
    pub turn: String,
    pub dice: Option<[u8; 2]>,
    pub white: Vec<PointDto>,
    pub black: Vec<PointDto>,
    pub off: [u8; 2],
    pub pip: [u16; 2],
    pub first_turn_done: [bool; 2],
    pub turn_number: u32,
    pub cube: CubeDto,
    pub crawford: bool,
    pub outcome: OutcomeDto,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CheckerMoveDto {
    pub from: u8,
    pub die: u8,
    pub to: u8,
    pub bear_off: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct TurnDto {
    pub id: u32,
    pub is_pass: bool,
    pub moves: Vec<CheckerMoveDto>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ProbabilitiesDto {
    pub win: f32,
    pub win_mars: f32,
    pub lose: f32,
    pub lose_mars: f32,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RankedTurnDto {
    pub turn: TurnDto,
    pub equity: f32,
    pub win: f32,
    pub equity_loss: f32,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct BestMoveDto {
    pub turn: TurnDto,
    pub equity: f32,
    pub probs: ProbabilitiesDto,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CubeDecisionDto {
    pub action: String,
    pub equity: f32,
    pub opponent_should_take: bool,
    pub recommend_beaver: bool,
    pub note: String,
    pub probs: ProbabilitiesDto,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SetupDto {
    pub variant: String,
    pub turn: String,
    pub white: Vec<PointDto>,
    pub black: Vec<PointDto>,
    pub off: [u8; 2],
    pub dice: Option<[u8; 2]>,
    pub cube: Option<CubeDto>,
    pub crawford: Option<bool>,
}

// ---- conversions ----------------------------------------------------------

pub fn board_points(board: &Board, player: Player) -> Vec<PointDto> {
    let mut v = Vec::new();
    for pos in 1..=HEAD_POS {
        let c = board.own_at(player, pos);
        if c > 0 {
            v.push(PointDto {
                pos,
                player: player_str(player),
                count: c,
            });
        }
    }
    v
}

pub fn cube_dto(c: &Cube) -> CubeDto {
    CubeDto {
        value: c.value,
        owner: c.owner.map(player_str),
        turned: c.turned,
    }
}

pub fn outcome_dto(board: &Board) -> OutcomeDto {
    match engine_core::outcome(board) {
        Outcome::Ongoing => OutcomeDto {
            kind: "ongoing".into(),
            winner: None,
            mars: None,
            points: None,
        },
        Outcome::Win {
            winner,
            mars,
            points,
        } => OutcomeDto {
            kind: "win".into(),
            winner: Some(player_str(winner)),
            mars: Some(mars),
            points: Some(points),
        },
    }
}

pub fn probs_dto(p: &Probabilities) -> ProbabilitiesDto {
    ProbabilitiesDto {
        win: p.win,
        win_mars: p.win_mars,
        lose: p.lose(),
        lose_mars: p.lose_mars,
    }
}

pub fn position_dto(game: &GameState) -> PositionDto {
    let b = &game.board;
    PositionDto {
        variant: variant_str(game.rules.variant),
        turn: player_str(game.turn),
        dice: game.dice,
        white: board_points(b, Player::White),
        black: board_points(b, Player::Black),
        off: b.off,
        pip: [b.pip(Player::White), b.pip(Player::Black)],
        first_turn_done: game.first_turn_done,
        turn_number: game.turn_number,
        cube: cube_dto(&game.cube),
        crawford: game.crawford,
        outcome: outcome_dto(b),
    }
}

pub fn move_dto(m: &CheckerMove) -> CheckerMoveDto {
    CheckerMoveDto {
        from: m.from,
        die: m.die,
        to: m.to,
        bear_off: m.bear_off,
    }
}

pub fn turn_dto(id: u32, t: &Turn) -> TurnDto {
    TurnDto {
        id,
        is_pass: t.moves.is_empty(),
        moves: t.moves.iter().map(move_dto).collect(),
    }
}

/// Find the index (id) of `chosen` within a regenerated legal-turns list, matching
/// by resulting board (robust to move-order differences).
pub fn turn_id(turns: &[Turn], chosen: &Turn) -> u32 {
    turns
        .iter()
        .position(|t| t.board == chosen.board)
        .unwrap_or(0) as u32
}
