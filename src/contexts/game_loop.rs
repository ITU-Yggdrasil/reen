use rand::prelude::*;
use std::time::Duration;
use std::thread;

use crate::data::Direction;
use crate::data::Food;
use crate::data::Snake;
use crate::data::UserAction;

#[derive(Debug)]
pub struct GameLoopContext {
    state: GameState,
    delay: Duration,
    user_input_queue: Vec<UserAction>,
}

impl GameLoopContext {
    pub fn new(snake: Snake, food: Food, board: Board, delay: Duration) -> Self {
        GameLoopContext {
            state: GameState::new(snake, food, board),
            delay,
            user_input_queue: Vec::new(),
        }
    }

    pub fn update(&mut self) {
        if let Some(action) = self.user_input_queue.pop() {
            self.state.update(action);
        }

        // Simulate user input (for demonstration purposes)
        self.user_input_queue.push(UserAction::Move(Direction::Right));

        // Update the state based on the game logic
        self.state.update(Direction::Right);

        // Sleep for the specified delay
        thread::sleep(self.delay);
    }

    pub fn get_state(&self) -> &GameState {
        &self.state
    }

    pub fn get_state_mut(&mut self) -> &mut GameState {
        &mut self.state
    }

    pub fn get_user_input_queue(&self) -> &Vec<UserAction> {
        &self.user_input_queue
    }

    pub fn set_user_input_queue(&mut self, queue: Vec<UserAction>) {
        self.user_input_queue = queue;
    }
}