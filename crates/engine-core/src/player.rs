//! The two sides.

/// One of the two players. In длинные нарды both players move in the *same*
/// rotational direction around the board (counter-clockwise), each from their
/// own head toward their own home — they never move head-on toward each other.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Player {
    White,
    Black,
}

impl Player {
    /// The other side.
    #[inline]
    pub const fn opponent(self) -> Player {
        match self {
            Player::White => Player::Black,
            Player::Black => Player::White,
        }
    }

    /// Stable 0/1 index for array storage (`off[..]`, ratings, etc.).
    #[inline]
    pub const fn index(self) -> usize {
        match self {
            Player::White => 0,
            Player::Black => 1,
        }
    }

    /// Iterator-friendly list of both players.
    pub const ALL: [Player; 2] = [Player::White, Player::Black];
}
