//! Analysis backbone: outcome probabilities, move ranking and cube decisions.
//!
//! The probability model here is an explicit **placeholder** based on the pip
//! race — enough to wire up the analysis UI (best move + win% + cube advice) and
//! to unit-test the decision logic. It will be replaced by the self-play neural
//! net and rollouts in Phase 1; the equity/cube formulas downstream stay the same.

use crate::board::{Board, N_CHECKERS};
use crate::cube::Cube;
use crate::moves::{generate_turns_cfg, Turn};
use crate::player::Player;

/// Outcome probabilities from the perspective of one player.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Probabilities {
    /// Probability of winning (any result).
    pub win: f32,
    /// Probability of winning a mars (subset of `win`).
    pub win_mars: f32,
    /// Probability of losing a mars (subset of `lose`).
    pub lose_mars: f32,
}

impl Probabilities {
    pub fn lose(&self) -> f32 {
        1.0 - self.win
    }

    /// Cubeless equity in points (cube value 1), counting mars as 2 points.
    pub fn cubeless_equity(&self) -> f32 {
        self.win + self.win_mars - self.lose() - self.lose_mars
    }

    /// Cubeless equity ignoring mars (single-point only) — used under the Jacoby
    /// rule before the cube has been turned.
    pub fn cubeless_equity_single(&self) -> f32 {
        2.0 * self.win - 1.0
    }
}

#[inline]
fn logistic(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

/// Placeholder mars likelihood for a prospective loser at `pip` pips: zero deep
/// in the bear-off, rising toward the opening. (Replaced by the net.)
fn mars_factor(loser_pip: f32) -> f32 {
    ((loser_pip - 150.0) / 420.0).clamp(0.0, 0.5)
}

/// Estimate outcome probabilities for `player`. **Placeholder** pip-race model.
pub fn win_probabilities(board: &Board, player: Player) -> Probabilities {
    let opp = player.opponent();

    if board.off[player.index()] == N_CHECKERS {
        return Probabilities {
            win: 1.0,
            win_mars: if board.off[opp.index()] == 0 { 1.0 } else { 0.0 },
            lose_mars: 0.0,
        };
    }
    if board.off[opp.index()] == N_CHECKERS {
        return Probabilities {
            win: 0.0,
            win_mars: 0.0,
            lose_mars: if board.off[player.index()] == 0 { 1.0 } else { 0.0 },
        };
    }

    let my = board.pip(player) as f32;
    let op = board.pip(opp) as f32;
    let win = logistic((op - my) * 0.06);

    // Mars is only possible if the prospective loser has borne off nothing yet.
    let win_mars = if board.off[opp.index()] == 0 {
        win * mars_factor(op)
    } else {
        0.0
    };
    let lose_mars = if board.off[player.index()] == 0 {
        (1.0 - win) * mars_factor(my)
    } else {
        0.0
    };

    Probabilities {
        win,
        win_mars,
        lose_mars,
    }
}

/// A legal turn scored for analysis.
#[derive(Clone, Debug)]
pub struct RankedTurn {
    pub turn: Turn,
    /// Cubeless equity of the resulting position, from the mover's perspective.
    pub equity: f32,
    /// Win probability of the resulting position, from the mover's perspective.
    pub win: f32,
}

/// Rank every legal turn best-first by cubeless equity (the "best move" list with
/// win% for each candidate). `head_limit` selects the variant's head rule.
pub fn rank_turns(
    board: &Board,
    player: Player,
    dice: [u8; 2],
    first_turn: bool,
    head_limit: Option<u8>,
) -> Vec<RankedTurn> {
    let mut ranked: Vec<RankedTurn> = generate_turns_cfg(board, player, dice, first_turn, head_limit)
        .into_iter()
        .map(|turn| {
            let p = win_probabilities(&turn.board, player);
            RankedTurn {
                equity: p.cubeless_equity(),
                win: p.win,
                turn,
            }
        })
        .collect();
    ranked.sort_by(|a, b| {
        b.equity
            .partial_cmp(&a.equity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    ranked
}

/// Recommended cube action for the player on roll.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CubeAction {
    /// Not good enough to double — play on.
    NoDouble,
    /// Double; the opponent has a correct take (тайк).
    DoubleTake,
    /// Double; the opponent must pass / drop (пас) — i.e. cash the point.
    DoublePass,
    /// Too good to double (тугуд) — play on for the mars instead of cashing.
    TooGood,
}

/// Context for a cube decision.
#[derive(Clone, Copy, Debug)]
pub struct CubeContext {
    /// Money game (vs match play).
    pub money: bool,
    /// Jacoby rule active (money game).
    pub jacoby: bool,
    /// Beavers allowed (money game).
    pub beaver: bool,
    /// The upcoming game is the Crawford game (no doubling).
    pub crawford: bool,
    pub cube: Cube,
    pub on_roll: Player,
}

/// Result of a cube decision.
#[derive(Clone, Copy, Debug)]
pub struct CubeAnalysis {
    pub action: CubeAction,
    /// Equity used for the decision (mars-aware, or single-only under Jacoby).
    pub equity: f32,
    /// If doubled here, whether the opponent has a correct take.
    pub opponent_should_take: bool,
    /// Whether the opponent should beaver (money game; signals a bad double).
    pub recommend_beaver: bool,
    /// Short human-readable note (Russian).
    pub note: &'static str,
}

// Money-game decision thresholds on cubeless equity. Baseline values; the
// live-cube (Janowski) and match-equity (MWC) refinements arrive with the net.
const DOUBLE_POINT: f32 = 0.30;
const PASS_POINT: f32 = 0.50;
const TOO_GOOD_POINT: f32 = 1.0;

/// Recommend a cube action for the player on roll, given outcome probabilities.
pub fn analyze_cube(probs: &Probabilities, ctx: &CubeContext) -> CubeAnalysis {
    let equity = if ctx.money && ctx.jacoby && !ctx.cube.turned {
        probs.cubeless_equity_single()
    } else {
        probs.cubeless_equity()
    };

    if ctx.crawford {
        return CubeAnalysis {
            action: CubeAction::NoDouble,
            equity,
            opponent_should_take: false,
            recommend_beaver: false,
            note: "Кроуфорд: удвоение запрещено",
        };
    }
    if !ctx.cube.may_double(ctx.on_roll) {
        return CubeAnalysis {
            action: CubeAction::NoDouble,
            equity,
            opponent_should_take: false,
            recommend_beaver: false,
            note: "Куб недоступен (у соперника)",
        };
    }

    let action = if equity <= DOUBLE_POINT {
        CubeAction::NoDouble
    } else if equity <= PASS_POINT {
        CubeAction::DoubleTake
    } else if equity <= TOO_GOOD_POINT {
        CubeAction::DoublePass
    } else {
        CubeAction::TooGood
    };

    let opponent_should_take = equity <= PASS_POINT;
    // A double while behind (equity < 0) is a blunder the taker should beaver.
    let recommend_beaver = ctx.money && ctx.beaver && equity < 0.0;

    let note = match action {
        CubeAction::NoDouble if recommend_beaver => "Не удваивать (иначе бивер)",
        CubeAction::NoDouble => "Не удваивать",
        CubeAction::DoubleTake => "Удвоение, соперник берёт (тайк)",
        CubeAction::DoublePass => "Удвоение, соперник пасует (пас)",
        CubeAction::TooGood => "Слишком хорошо — играть на марс (тугуд)",
    };

    CubeAnalysis {
        action,
        equity,
        opponent_should_take,
        recommend_beaver,
        note,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn money_ctx() -> CubeContext {
        CubeContext {
            money: true,
            jacoby: false,
            beaver: true,
            crawford: false,
            cube: Cube::centered(),
            on_roll: Player::White,
        }
    }

    #[test]
    fn even_position_is_no_double() {
        let p = Probabilities { win: 0.5, win_mars: 0.0, lose_mars: 0.0 };
        assert_eq!(analyze_cube(&p, &money_ctx()).action, CubeAction::NoDouble);
    }

    #[test]
    fn strong_lead_is_a_double_and_take() {
        // equity ~0.4 → in the doubling window, opponent takes.
        let p = Probabilities { win: 0.70, win_mars: 0.0, lose_mars: 0.0 };
        let a = analyze_cube(&p, &money_ctx());
        assert_eq!(a.action, CubeAction::DoubleTake);
        assert!(a.opponent_should_take);
    }

    #[test]
    fn big_lead_is_double_pass_cash() {
        // equity ~0.6 → opponent must pass.
        let p = Probabilities { win: 0.80, win_mars: 0.0, lose_mars: 0.0 };
        let a = analyze_cube(&p, &money_ctx());
        assert_eq!(a.action, CubeAction::DoublePass);
        assert!(!a.opponent_should_take);
    }

    #[test]
    fn huge_mars_chances_are_too_good() {
        // equity > 1 thanks to heavy mars → play on (тугуд).
        let p = Probabilities { win: 0.95, win_mars: 0.7, lose_mars: 0.0 };
        assert_eq!(analyze_cube(&p, &money_ctx()).action, CubeAction::TooGood);
    }

    #[test]
    fn crawford_forbids_doubling() {
        let mut ctx = money_ctx();
        ctx.money = false;
        ctx.crawford = true;
        let p = Probabilities { win: 0.9, win_mars: 0.0, lose_mars: 0.0 };
        assert_eq!(analyze_cube(&p, &ctx).action, CubeAction::NoDouble);
    }

    #[test]
    fn jacoby_ignores_mars_before_the_cube_is_turned() {
        // 60% win, big mars share. With Jacoby (cube not turned) only the single
        // equity (2*0.6-1 = 0.2) counts → NoDouble. Without Jacoby the mars push
        // the equity into the doubling window.
        let p = Probabilities { win: 0.60, win_mars: 0.4, lose_mars: 0.0 };
        let mut ctx = money_ctx();
        ctx.jacoby = true;
        assert_eq!(analyze_cube(&p, &ctx).action, CubeAction::NoDouble);
        ctx.jacoby = false;
        assert_ne!(analyze_cube(&p, &ctx).action, CubeAction::NoDouble);
    }

    #[test]
    fn behind_doubler_should_be_beavered() {
        let p = Probabilities { win: 0.4, win_mars: 0.0, lose_mars: 0.0 };
        let a = analyze_cube(&p, &money_ctx());
        assert!(a.recommend_beaver);
    }

    #[test]
    fn ranking_prefers_lower_pip() {
        let b = Board::starting();
        let ranked = rank_turns(&b, Player::White, [6, 5], true, Some(1));
        assert!(!ranked.is_empty());
        // sorted descending by equity
        for w in ranked.windows(2) {
            assert!(w[0].equity >= w[1].equity);
        }
    }
}
