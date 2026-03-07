use snake_game::Direction;

#[derive(Debug, Clone, PartialEq)]
pub enum UserAction {
    Move(Direction),
}

impl UserAction {
    /// Creates a new `UserAction` to move the snake in a given direction.
    pub fn new_move(direction: Direction) -> Self {
        UserAction::Move(direction)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_action_move() {
        let action = UserAction::new_move(Direction::Up);
        assert_eq!(action, UserAction::Move(Direction::Up));
    }
}