use snake_game::Direction;
use snake_game::Position;

#[derive(Debug, Clone)]
pub struct CommandInputContext {
    pub input_queue: Vec<UserAction>,
}

impl CommandInputContext {
    /// Creates a new `CommandInputContext` with an empty input queue.
    pub fn new() -> Self {
        CommandInputContext {
            input_queue: Vec::new(),
        }
    }

    /// Adds a user action to the input queue.
    pub fn add_user_action(&mut self, action: UserAction) {
        self.input_queue.push(action);
    }

    /// Retrieves and removes the first user action from the input queue.
    pub fn get_user_action(&mut self) -> Option<UserAction> {
        self.input_queue.pop()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_input_context_add_and_get_user_action() {
        let mut context = CommandInputContext::new();
        context.add_user_action(UserAction::new_move(Direction::Up));
        context.add_user_action(UserAction::new_move(Direction::Right));

        assert_eq!(
            context.get_user_action(),
            Some(UserAction::new_move(Direction::Up))
        );
        assert_eq!(
            context.get_user_action(),
            Some(UserAction::new_move(Direction::Right))
        );
        assert_eq!(context.get_user_action(), None);
    }
}