use serde::{Deserialize, Serialize};

/// A value object representing a cell coordinate on the Board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}

impl Position {
    /// Checks if two positions are equal based on their coordinates.
    pub fn eq(&self, other: &Position) -> bool {
        self.x == other.x && self.y == other.y
    }
}