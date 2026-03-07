use snake_game::Direction;

/// Represents a rectangular playfield in the Snake game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Board {
    width: usize,
    height: usize,
}

impl Board {
    /// Creates a new board with the given width and height.
    pub fn new(width: usize, height: usize) -> Result<Self, String> {
        if width == 0 || height == 0 {
            return Err("Board dimensions must be greater than 0".to_string());
        }
        Ok(Self { width, height })
    }

    /// Returns the width of the board.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Returns the height of the board.
    pub fn height(&self) -> usize {
        self.height
    }

    /// Checks if a given position is inside the board boundaries.
    pub fn is_inside(&self, position: &snake_game::Position) -> bool {
        position.x() < self.width && position.y() < self.height
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_board_creation() {
        let board = Board::new(10, 10).unwrap();
        assert_eq!(board.width, 10);
        assert_eq!(board.height, 10);
    }

    #[test]
    fn test_board_outside_position() {
        let board = Board::new(10, 10).unwrap();
        let position = snake_game::Position::new(10, 10);
        assert!(!board.is_inside(&position));
    }

    #[test]
    fn test_board_inside_position() {
        let board = Board::new(10, 10).unwrap();
        let position = snake_game::Position::new(9, 9);
        assert!(board.is_inside(&position));
    }
}