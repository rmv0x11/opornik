//! Baseline heuristic evaluation.
//!
//! This is a deliberately simple, hand-tuned evaluator to bootstrap a playable
//! opponent and to serve as a sanity baseline against the future self-play neural
//! net. It scores a board from `player`'s perspective; higher is better.
//!
//! Long nardy is a race + blocking game (no hitting), so the heuristic combines:
//!   * the pip race (how far ahead `player` is),
//!   * bear-off progress,
//!   * a blocking term rewarding contiguous owned points in front of the
//!     opponent's rearmost checker.
//!
//! These weights are placeholders; the strength will come from the trained net.

use crate::board::{phys, Board};
use crate::game::{outcome, Outcome};
use crate::player::Player;

/// Magnitude returned for a decided game (well above any heuristic score).
pub const WIN_SCORE: f32 = 1_000_000.0;

const W_RACE: f32 = 1.0;
const W_OFF: f32 = 5.0;
const W_IMPEDE: f32 = 0.5;
const W_RUN: f32 = 1.5;

/// Reward for `mover`'s points that lie in front of the opponent's rearmost
/// checker (and thus actually impede the opponent), plus a bonus for the longest
/// contiguous run of such points (encouraging primes/blocks).
fn block_term(board: &Board, mover: Player) -> f32 {
    let opp = mover.opponent();
    let rear = board.highest_occupied(opp); // opponent's farthest-from-home checker
    if rear <= 1 {
        return 0.0;
    }
    let mut impede = 0u32;
    let mut run = 0u32;
    let mut best_run = 0u32;
    // Walk the opponent's path positions in front of its rearmost checker.
    for q in (1..rear).rev() {
        if board.owner(phys(opp, q)) == Some(mover) {
            impede += 1;
            run += 1;
            best_run = best_run.max(run);
        } else {
            run = 0;
        }
    }
    impede as f32 * W_IMPEDE + best_run as f32 * W_RUN
}

/// Evaluate `board` from `player`'s perspective. Higher is better for `player`.
pub fn evaluate(board: &Board, player: Player) -> f32 {
    match outcome(board) {
        Outcome::Win { winner, points, .. } => {
            let sign = if winner == player { 1.0 } else { -1.0 };
            return sign * (WIN_SCORE + points as f32);
        }
        // board-only `outcome` never yields Draw (needs game state), but the match
        // must stay exhaustive; treat as neutral.
        Outcome::Draw => return 0.0,
        Outcome::Ongoing => {}
    }

    let opp = player.opponent();
    let my_pip = board.pip(player) as f32;
    let op_pip = board.pip(opp) as f32;

    let race = (op_pip - my_pip) * W_RACE;
    let off = (board.off[player.index()] as f32 - board.off[opp.index()] as f32) * W_OFF;
    let block = block_term(board, player) - block_term(board, opp);

    race + off + block
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::HEAD_POS;

    #[test]
    fn winning_position_dominates() {
        let mut b = Board::empty();
        b.off[Player::White.index()] = 15;
        assert!(evaluate(&b, Player::White) > WIN_SCORE);
        assert!(evaluate(&b, Player::Black) < -WIN_SCORE);
    }

    #[test]
    fn being_ahead_in_the_race_scores_higher() {
        // White has advanced a checker; Black is untouched on the head.
        let mut b = Board::starting();
        // move one White checker far forward (lower pip = better)
        b.place(Player::White, HEAD_POS, 14);
        b.place(Player::White, 2, 1);
        assert!(evaluate(&b, Player::White) > 0.0);
    }
}
