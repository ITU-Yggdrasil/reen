use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq)]
pub struct GameState {
    snake: Snake,
    food: Food,
    board: Board,
    snake_direction: Direction,
    snake_body_history: Vec<Position>,
}

impl GameState {
    /// Creates a new game state with the given snake, food, and board.
    pub fn new(snake: Snake, food: Food, board: Board) -> Self {
        GameState {
            snake,
            food,
            board,
            snake_direction: snake.direction(),
            snake_body_history: vec![],
        }
    }

    /// Returns the current snake in the game state.
    pub fn snake(&self) -> &Snake {
        &self.snake
    }

    /// Returns the current food in the game state.
    pub fn food(&self) -> &Food {
        &self.food
    }

    /// Returns the current board in the game state.
    pub fn board(&self) -> &Board {
        &self.board
    }

    /// Updates the game state based on the snake's movement.
    pub fn update(&mut self, direction: Direction) {
        let new_head = match direction {
            Direction::Up => Position { x: self.snake.body()[0].x, y: self.snake.body()[0].y - 1 },
            Direction::Down => Position { x: self.snake.body()[0].x, y: self.snake.body()[0].y + 1 },
            Direction::Left => Position { x: self.snake.body()[0].x - 1, y: self.snake.body()[0].y },
            Direction::Right => Position { x: self.snake.body()[0].x + 1, y: self.snake.body()[0].y },
        };

        if self.board.is_inside(&new_head) {
            self.snake = Snake::new(
                [new_head, self.snake.body()[0..self.snake.body().len() - 1].to_vec()].concat(),
                direction,
            );
            self.snake_body_history.push(new_head);
        } else {
            // Game over condition
        }

        // Check for food collision
        if self.snake.body()[0] == self.food.position() {
            self.snake_body_history.clear();
            self.snake = Snake::new(
                [new_head, self.snake.body()[0..self.snake.body().len() - 1].to_vec()].concat(),
                direction,
            );
            self.food = Food::new(Position::new(
                self.snake_body_history
                    .choose(&mut rand::thread_rng())
                    .unwrap_or(&self.snake_body_history.last().unwrap())
                    .x,
                self.snake_body_history
                    .choose(&mut rand::thread_rng())
                    .unwrap_or(&self.snake_body_history.last().unwrap())
                    .y,
            ));
        }
    }
}