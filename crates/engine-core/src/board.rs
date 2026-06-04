//! Board model and coordinate system.
//!
//! # Coordinates
//!
//! The physical board is a loop of 24 cells indexed `0..24`. Each player has
//! their *own* path numbering the same 24 cells as positions `1..=24`, where:
//!
//! * position **24** is that player's **head** (голова) — the single point where
//!   all 15 checkers start;
//! * positions **1..=6** are that player's **home** (дом/двор) — the bear-off zone;
//! * a checker bears off by moving past position **1** (to the virtual position 0).
//!
//! A checker advances by *decreasing* its path position. The pip count of a
//! checker on position `p` is exactly `p`, so the starting pip count is
//! `15 * 24 = 360` for each side.
//!
//! White and Black paths are the same loop, offset by 12 cells, so the two heads
//! sit at diagonally opposite corners (White head = physical 0, Black head =
//! physical 12). Decreasing the path position for *either* player advances them in
//! the same rotational direction (`phys` increases mod 24) — exactly the long-nardy
//! semantics.

use crate::player::Player;

/// Number of physical points on the board.
pub const N_POINTS: usize = 24;
/// Checkers per side.
pub const N_CHECKERS: u8 = 15;
/// Path position of the head (голова).
pub const HEAD_POS: u8 = 24;
/// Highest home position; home is `1..=HOME_HIGH`.
pub const HOME_HIGH: u8 = 6;
/// Starting pip count per side (`15 * 24`).
pub const START_PIP: u16 = 360;

/// Physical cell index (`0..24`) of `player`'s path position `pos` (`1..=24`).
#[inline]
pub fn phys(player: Player, pos: u8) -> usize {
    debug_assert!((1..=HEAD_POS).contains(&pos), "pos out of range: {pos}");
    let base: u8 = match player {
        Player::White => 24,
        Player::Black => 36,
    };
    ((base - pos) % 24) as usize
}

/// Inverse of [`phys`]: the path position (`1..=24`) of physical cell `cell`
/// for `player`.
#[inline]
pub fn pos_of_phys(player: Player, cell: usize) -> u8 {
    debug_assert!(cell < N_POINTS, "cell out of range: {cell}");
    let base: usize = match player {
        Player::White => 24,
        Player::Black => 36,
    };
    let v = (base - cell) % 24;
    if v == 0 {
        HEAD_POS
    } else {
        v as u8
    }
}

/// The board: occupancy of every physical cell plus borne-off counts.
///
/// A cell can hold checkers of at most one colour (there is no hitting in long
/// nardy). Occupancy is sign-encoded: positive = White count, negative = Black
/// count, zero = empty.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Board {
    /// Sign-encoded occupancy per physical cell. `>0` White, `<0` Black, `0` empty.
    pub points: [i8; N_POINTS],
    /// Borne-off checkers indexed by [`Player::index`].
    pub off: [u8; 2],
}

impl Board {
    #[inline]
    const fn delta(player: Player) -> i8 {
        match player {
            Player::White => 1,
            Player::Black => -1,
        }
    }

    /// The standard starting position: all 15 checkers of each side on their head.
    pub fn starting() -> Board {
        let mut b = Board::empty();
        b.points[phys(Player::White, HEAD_POS)] = N_CHECKERS as i8;
        b.points[phys(Player::Black, HEAD_POS)] = -(N_CHECKERS as i8);
        b
    }

    /// The хачапури preset: 4 checkers on the head and 11 on home point 6 for
    /// each side. Played with the head rule disabled (see `Rules::head_limit`).
    pub fn starting_hachapuri() -> Board {
        let mut b = Board::empty();
        for &p in &Player::ALL {
            b.place(p, HEAD_POS, 4);
            b.place(p, 6, 11);
        }
        b
    }

    /// An empty board (no checkers placed, none borne off). Useful for building
    /// puzzle/analysis positions.
    pub fn empty() -> Board {
        Board {
            points: [0; N_POINTS],
            off: [0, 0],
        }
    }

    /// Whether each side has exactly 15 checkers (on board + borne off) — the
    /// basic validity check for a hand-built analysis/puzzle position.
    pub fn is_valid(&self) -> bool {
        Player::ALL
            .iter()
            .all(|&p| self.checkers_on_board(p) + self.off[p.index()] == N_CHECKERS)
    }

    /// Owner of a physical cell, if any.
    #[inline]
    pub fn owner(&self, cell: usize) -> Option<Player> {
        let v = self.points[cell];
        if v > 0 {
            Some(Player::White)
        } else if v < 0 {
            Some(Player::Black)
        } else {
            None
        }
    }

    /// Total checkers on a physical cell (regardless of owner).
    #[inline]
    pub fn count(&self, cell: usize) -> u8 {
        self.points[cell].unsigned_abs()
    }

    /// Number of `player`'s checkers at `player`'s path position `pos`.
    #[inline]
    pub fn own_at(&self, player: Player, pos: u8) -> u8 {
        let v = self.points[phys(player, pos)];
        match player {
            Player::White => v.max(0) as u8,
            Player::Black => (-v).max(0) as u8,
        }
    }

    /// Number of the *opponent's* checkers occupying the physical cell of
    /// `player`'s path position `pos`. Any non-zero value means the point is
    /// blocked for `player` (no hitting).
    #[inline]
    pub fn opp_at(&self, player: Player, pos: u8) -> u8 {
        let v = self.points[phys(player, pos)];
        match player {
            Player::White => (-v).max(0) as u8,
            Player::Black => v.max(0) as u8,
        }
    }

    /// Whether `player` may land on path position `pos` (empty or own).
    #[inline]
    pub fn is_open_for(&self, player: Player, pos: u8) -> bool {
        self.opp_at(player, pos) == 0
    }

    /// Whether all of `player`'s on-board checkers are in their home board
    /// (positions `1..=6`) — the precondition for bearing off.
    pub fn all_home(&self, player: Player) -> bool {
        for pos in (HOME_HIGH + 1)..=HEAD_POS {
            if self.own_at(player, pos) > 0 {
                return false;
            }
        }
        true
    }

    /// Highest occupied path position for `player` (0 if none on board).
    pub fn highest_occupied(&self, player: Player) -> u8 {
        for pos in (1..=HEAD_POS).rev() {
            if self.own_at(player, pos) > 0 {
                return pos;
            }
        }
        0
    }

    /// Pip count for `player` (sum of `pos * checkers`). Borne-off checkers
    /// contribute 0. Lower is better; 0 means everything is off.
    pub fn pip(&self, player: Player) -> u16 {
        let mut sum = 0u16;
        for pos in 1..=HEAD_POS {
            sum += pos as u16 * self.own_at(player, pos) as u16;
        }
        sum
    }

    /// Total checkers still on the board for `player` (excludes borne off).
    pub fn checkers_on_board(&self, player: Player) -> u8 {
        let mut n = 0u8;
        for pos in 1..=HEAD_POS {
            n += self.own_at(player, pos);
        }
        n
    }

    /// Place exactly `count` of `player`'s checkers on path position `pos`,
    /// overwriting whatever was there. For test/puzzle setup.
    pub fn place(&mut self, player: Player, pos: u8, count: u8) {
        let cell = phys(player, pos);
        let signed = count as i8;
        self.points[cell] = match player {
            Player::White => signed,
            Player::Black => -signed,
        };
    }

    /// Like [`place`](Self::place) but addressing a physical cell directly. Useful
    /// when constructing a position from the *opponent's* perspective in tests.
    pub fn place_phys(&mut self, owner: Player, cell: usize, count: u8) {
        let signed = count as i8;
        self.points[cell] = match owner {
            Player::White => signed,
            Player::Black => -signed,
        };
    }

    /// Add one of `player`'s checkers to path position `pos`.
    #[inline]
    pub(crate) fn add(&mut self, player: Player, pos: u8) {
        self.points[phys(player, pos)] += Self::delta(player);
    }

    /// Remove one of `player`'s checkers from path position `pos`.
    #[inline]
    pub(crate) fn remove(&mut self, player: Player, pos: u8) {
        self.points[phys(player, pos)] -= Self::delta(player);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heads_are_diagonally_opposite() {
        assert_eq!(phys(Player::White, HEAD_POS), 0);
        assert_eq!(phys(Player::Black, HEAD_POS), 12);
    }

    #[test]
    fn phys_round_trips_for_both_players() {
        for &p in &Player::ALL {
            for pos in 1..=HEAD_POS {
                assert_eq!(pos_of_phys(p, phys(p, pos)), pos, "player {p:?} pos {pos}");
            }
            // every physical cell maps back to a valid position
            for cell in 0..N_POINTS {
                let pos = pos_of_phys(p, cell);
                assert_eq!(phys(p, pos), cell);
            }
        }
    }

    #[test]
    fn white_pos12_is_black_head_cell() {
        // White advancing to pos 12 lands on Black's head cell.
        assert_eq!(phys(Player::White, 12), phys(Player::Black, HEAD_POS));
    }

    #[test]
    fn starting_position_is_balanced() {
        let b = Board::starting();
        assert_eq!(b.pip(Player::White), START_PIP);
        assert_eq!(b.pip(Player::Black), START_PIP);
        assert_eq!(b.own_at(Player::White, HEAD_POS), N_CHECKERS);
        assert_eq!(b.own_at(Player::Black, HEAD_POS), N_CHECKERS);
        assert_eq!(b.off, [0, 0]);
        assert_eq!(b.checkers_on_board(Player::White), N_CHECKERS);
    }
}
