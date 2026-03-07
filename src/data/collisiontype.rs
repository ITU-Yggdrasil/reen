use serde::{Deserialize, Serialize};

/// Represents the type of collision in the Snake game.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CollisionType {
    Body,
    Wall,
}

impl CollisionType {
    /// Checks if this collision type is with the body.
    pub fn is_body(&self) -> bool {
        matches!(self, CollisionType::Body)
    }

    /// Checks if this collision type is with the wall.
    pub fn is_wall(&self) -> bool {
        matches!(self, CollisionType::Wall)
    }
}