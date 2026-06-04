//! Position encoding for the neural net.
//!
//! TD-Gammon-style feature vector adapted to длинные нарды: there is no bar and
//! no re-entry, and the only "gammon" tier is mars. The board is encoded from the
//! **side-to-move's** perspective so the network is symmetric (the same weights
//! evaluate both sides — you flip the board rather than train two nets).
//!
//! Per player, per path position (1..=24), four features encode the checker count
//! (the classic Tesauro unary-plus-overflow encoding):
//!   * `>= 1`, `>= 2`, `== 3`, and `(n-3)/2` for `n > 3`.
//!
//! Then a few normalized extras: borne-off counts and pip counts for both sides.

use crate::board::{Board, HEAD_POS, N_CHECKERS, START_PIP};
use crate::player::Player;

/// Features encoding one point's checker count.
pub const FEATURES_PER_POINT: usize = 4;
/// 24 points × 4 features × 2 players.
pub const POINT_FEATURES: usize = 24 * FEATURES_PER_POINT * 2;
/// Borne-off (×2) and pip (×2).
pub const EXTRA_FEATURES: usize = 4;
/// Total input vector length (196).
pub const INPUT_SIZE: usize = POINT_FEATURES + EXTRA_FEATURES;

#[inline]
fn point_features(count: u8, out: &mut [f32]) {
    out[0] = (count >= 1) as u8 as f32;
    out[1] = (count >= 2) as u8 as f32;
    out[2] = (count == 3) as u8 as f32;
    out[3] = if count > 3 {
        (count as f32 - 3.0) / 2.0
    } else {
        0.0
    };
}

/// Encode `board` from `mover`'s perspective into a feature vector of
/// [`INPUT_SIZE`] floats.
pub fn encode(board: &Board, mover: Player) -> Vec<f32> {
    let mut x = vec![0.0f32; INPUT_SIZE];
    encode_into(board, mover, &mut x);
    x
}

/// Encode into a pre-allocated buffer (length must be [`INPUT_SIZE`]).
pub fn encode_into(board: &Board, mover: Player, x: &mut [f32]) {
    debug_assert_eq!(x.len(), INPUT_SIZE);
    let opp = mover.opponent();
    let mut idx = 0;
    for &side in &[mover, opp] {
        for pos in 1..=HEAD_POS {
            point_features(board.own_at(side, pos), &mut x[idx..idx + FEATURES_PER_POINT]);
            idx += FEATURES_PER_POINT;
        }
    }
    x[idx] = board.off[mover.index()] as f32 / N_CHECKERS as f32;
    x[idx + 1] = board.off[opp.index()] as f32 / N_CHECKERS as f32;
    x[idx + 2] = board.pip(mover) as f32 / START_PIP as f32;
    x[idx + 3] = board.pip(opp) as f32 / START_PIP as f32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_has_expected_length_and_is_perspective_symmetric() {
        let b = Board::starting();
        let w = encode(&b, Player::White);
        let bl = encode(&b, Player::Black);
        assert_eq!(w.len(), INPUT_SIZE);
        // The starting position is symmetric, so both perspectives encode identically.
        assert_eq!(w, bl);
    }

    #[test]
    fn point_feature_overflow_encoding() {
        let mut f = [0.0; 4];
        point_features(15, &mut f);
        assert_eq!(f, [1.0, 1.0, 0.0, 6.0]); // (15-3)/2 = 6
        point_features(3, &mut f);
        assert_eq!(f, [1.0, 1.0, 1.0, 0.0]);
        point_features(0, &mut f);
        assert_eq!(f, [0.0, 0.0, 0.0, 0.0]);
    }
}
