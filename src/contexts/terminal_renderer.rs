use std::fmt;

#[derive(Debug, Clone, PartialEq)]
struct GameState {
    snake: Snake,
    food: Food,
    board: Board,
    snake_direction: Direction,
    snake_body_history: Vec<Position>,
}

impl GameState {
    pub fn new(snake: Snake, food: Food, board: Board) -> Self {
        GameState {
            snake,
            food,
            board,
            snake_direction: snake.direction(),
            snake_body_history: vec![],
        }
    }

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

    pub fn render(&self) {
        let width = self.board.width();
        let height = self.board.height();

        // Create a vector to hold the board representation
        let mut board_representation = vec![vec![' '; width]; height];

        // Fill the board with the appropriate characters
        for position in &self.snake_body_history {
            board_representation[position.y as usize][position.x as usize] = '*';
        }
        board_representation[self.food.position().y as usize][self.food.position().x as usize] = 'o';

        // Print the board
        for row in board_representation {
            println!("{}", row.iter().collect::<String>());
        }
    }
}

// Other structs and enums are assumed to be defined as provided in the prompt

fn main() {
    let board = Board::new(20, 20);
    let snake = Snake::new(vec![Position::new(10, 10), Position::new(10, 11)], Direction::Right);
    let food = Food::new(Position::new(15, 15));

    let mut game_state = GameState::new(snake, food, board);

    // Update the game state and render the board
    game_state.update(Direction::Down);
    game_state.render();
}