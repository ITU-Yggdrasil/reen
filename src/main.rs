use std::io::{self, Write};
use crate::data::{Direction, Snake, UserAction, Position, Food, GameState};
use crate::logic::{game_loop, initialize_game_state};
use crate::ui::render_game_state;

fn main() {
    // Initialize the game state
    let mut game_state = initialize_game_state();

    // Run the game loop
    game_loop(&mut game_state);
}