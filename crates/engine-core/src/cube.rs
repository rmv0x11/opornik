//! The doubling cube.
//!
//! Classic длинные нарды are played without a cube, but the competitive
//! money-game and Nardegammon variants use one. This module models the cube state
//! and the legal operations (double, beaver), independent of the equity analysis
//! that *recommends* those actions (see [`crate::analysis`]).
//!
//! Convention: when a player **doubles**, the cube value doubles and ownership
//! passes to the *opponent* (the taker now holds it). A **beaver** is the taker's
//! immediate redouble in response — the value doubles again but the taker keeps
//! ownership.

use crate::player::Player;

/// Maximum cube value (1 → 2 → 4 → … → 64).
pub const MAX_CUBE: u16 = 64;

/// Why a cube action was rejected.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CubeError {
    /// This player may not double now (cube owned by the opponent).
    NotAvailable,
    /// The cube is already at its maximum value.
    AtMaximum,
}

/// Doubling cube state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cube {
    /// Current value (always a power of two, 1..=64).
    pub value: u16,
    /// Owner, or `None` when centered (either side may double).
    pub owner: Option<Player>,
    /// Whether the cube has ever been turned (used by the Jacoby rule).
    pub turned: bool,
}

impl Default for Cube {
    fn default() -> Self {
        Cube::centered()
    }
}

impl Cube {
    /// A fresh centered cube at value 1.
    pub const fn centered() -> Self {
        Cube {
            value: 1,
            owner: None,
            turned: false,
        }
    }

    /// Whether `player` owns the cube or it is centered.
    fn available_to(&self, player: Player) -> bool {
        match self.owner {
            None => true,
            Some(o) => o == player,
        }
    }

    /// Whether `player` may offer a double right now: the cube must be centered or
    /// owned by `player`, and below the maximum value.
    pub fn may_double(&self, player: Player) -> bool {
        self.value < MAX_CUBE && self.available_to(player)
    }

    /// `player` doubles: value doubles and the opponent becomes the owner.
    pub fn double(&mut self, player: Player) -> Result<(), CubeError> {
        if !self.available_to(player) {
            return Err(CubeError::NotAvailable);
        }
        if self.value >= MAX_CUBE {
            return Err(CubeError::AtMaximum);
        }
        self.value *= 2;
        self.owner = Some(player.opponent());
        self.turned = true;
        Ok(())
    }

    /// `player` beavers (an immediate redouble by the taker): value doubles again
    /// and the taker *keeps* ownership.
    pub fn beaver(&mut self, player: Player) -> Result<(), CubeError> {
        if self.value >= MAX_CUBE {
            return Err(CubeError::AtMaximum);
        }
        self.value *= 2;
        self.owner = Some(player);
        self.turned = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centered_cube_either_side_may_double() {
        let c = Cube::centered();
        assert!(c.may_double(Player::White));
        assert!(c.may_double(Player::Black));
    }

    #[test]
    fn doubling_transfers_ownership_to_opponent() {
        let mut c = Cube::centered();
        c.double(Player::White).unwrap();
        assert_eq!(c.value, 2);
        assert_eq!(c.owner, Some(Player::Black));
        assert!(c.turned);
        // now only Black may (re)double
        assert!(!c.may_double(Player::White));
        assert!(c.may_double(Player::Black));
    }

    #[test]
    fn beaver_keeps_ownership_with_the_taker() {
        let mut c = Cube::centered();
        c.double(Player::White).unwrap(); // Black is offered the cube at 2
        c.beaver(Player::Black).unwrap(); // Black beavers to 4 and keeps it
        assert_eq!(c.value, 4);
        assert_eq!(c.owner, Some(Player::Black));
    }

    #[test]
    fn cube_caps_at_maximum() {
        let mut c = Cube {
            value: MAX_CUBE,
            owner: None,
            turned: true,
        };
        assert!(!c.may_double(Player::White));
        assert_eq!(c.double(Player::White), Err(CubeError::AtMaximum));
    }
}
