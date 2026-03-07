use std::fmt;

use crate::data::Direction;
use crate::data::Position;

/// Represents the main character of the snake game.
#[derive(Debug, Clone, PartialEq)]
pub struct Snake {
    body: Vec<Position>,
    direction: Direction,
}

impl Snake {
    /// Constructs the snake from a list of body positions and a direction.
    pub fn new(body: Vec<Position>, direction: Direction) -> Self {
        assert!(!body.is_empty(), "Snake body length must be greater than 0");
        assert!(body.iter().collect::<std::collections::HashSet<_>>().len() == body.len(), "All positions in `body` must be unique");

        Snake { body, direction }
    }

    /// Returns the current direction of the snake.
    pub fn direction(&self) -> &Direction {
        &self.direction
    }

    /// Returns the current positions of the snake's body.
    pub fn body(&self) -> &Vec<Position> {
        &self.body
    }
}

impl fmt::Display for Snake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Snake {{ body: {:?}, direction: {:?} }}", self.body, self.direction)
    }
}