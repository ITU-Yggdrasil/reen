use serde::{Deserialize, Serialize};

/// Direction for the Snake in the game.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    /// Checks if this direction is the opposite of another direction.
    ///
    /// # Examples
    ///
    ///
}