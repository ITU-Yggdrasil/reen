use snake_game::Position;

/// Represents a food item in the Snake game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Food {
    position: Position,
}

impl Food {
    /// Creates a new food item at a given position.
    pub fn new(position: Position) -> Self {
        Food { position }
    }

    /// Returns the current position of the food.
    pub fn position(&self) -> &Position {
        &self.position
    }
}

impl fmt::Display for Food {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Food at {:?}", self.position)
    }
}