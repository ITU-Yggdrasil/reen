```rust
use std::cell::RefCell;
use std::rc::Rc;
use std::thread;
use std::time::Duration;

use crate::contexts::CommandInputContext;
use crate::data::{Board, Direction, Food, GameState, Position, Snake, UserAction};

/// A trait for components that can drop food onto the board.
pub trait FoodDropper {
    fn drop(&mut self) -> Option<Food>;
}

/// Internal classification of the predicted move outcome.
enum MoveOutcome {
    Obstacle,
    Food,
    Clear,
}

/// `GameLoopContext` is the "Game" part of the system.
/// It is the single source of truth for the game rules and game state.
pub struct GameLoopContext<D: FoodDropper> {
    board: Board,
    snake: Snake,
    command: Rc<RefCell<CommandInputContext>>,
    food_dropper: D,
    game_state: GameState,
}

impl<D: FoodDropper> GameLoopContext<D> {
    /// Constructs a new `GameLoopContext` from the provided collaborators.
    ///
    /// Uses the provided shared command context; does not create a separate input stream.
    pub fn new(
        board: Board,
        snake: Snake,
        command: Rc<RefCell<CommandInputContext>>,
        food_dropper: D,
        game_state: GameState,
    ) -> Self {
        tracing::info!(
            "[GameLoopContext] new, board={}x{}, snake_len={}",
            board.width(),
            board.height(),
            snake.body().len()
        );
        Self {
            board,
            snake,
            command,
            food_dropper,
            game_state,
        }
    }

    /// Returns a 2D character grid representing the current board state.
    ///
    /// - `w` for wall cells at the boundary
    /// - space for unoccupied cells
    /// - `s` for cells occupied by the snake
    /// - `f` for the food position
    pub fn current_board(&self) -> Vec<Vec<char>> {
        tracing::info!("[GameLoopContext] current_board");
        let w = self.board.width() as usize;
        let h = self.board.height() as usize;

        let mut grid = vec![vec![' '; h]; w];

        // Fill walls
        for x in 0..w {
            for y in 0..h {
                if self.is_boundary(x as i32, y as i32) {
                    grid[x][y] = 'w';
                }
            }
        }

        // Fill snake cells
        for pos in self.snake.body() {
            let x = pos.x() as usize;
            let y = pos.y() as usize;
            if x < w && y < h {
                grid[x][y] = 's';
            }
        }

        // Fill food cell
        if let Some(food) = self.game_state.food() {
            let x = food.position().x() as usize;
            let y = food.position().y() as usize;
            if x < w && y < h {
                grid[x][y] = 'f';
            }
        }

        tracing::debug!("[GameLoopContext] current_board, w={}, h={}", w, h);
        grid
    }

    /// Returns the current score as a non-negative whole number.
    pub fn get_score(&self) -> i32 {
        tracing::info!("[GameLoopContext] get_score");
        let score = self.game_state_score();
        tracing::debug!("[GameLoopContext] get_score, score={}", score);
        score
    }

    /// Advances the game by one tick.
    ///
    /// - Sleeps for the pacing delay.
    /// - Captures pending keystrokes into the shared input stream.
    /// - Reads the next movement direction from command input.
    /// - Applies snake steering if a direction is available.
    /// - Computes the next head coordinate.
    /// - Returns `None` if the move is an obstacle (wall or snake body).
    /// - On food: moves with growth, increments score by 10, drops new food.
    /// - On clear: moves without growth.
    pub fn tick(mut self) -> Option<Self> {
        tracing::info!("[GameLoopContext] tick");

        // Pacing: compute delay and sleep
        let delay = self.compute_delay();
        tracing::debug!("[GameLoopContext] tick, delay_ms={}", delay.as_millis());
        thread::sleep(delay);

        // Capture pending keystrokes into shared buffer
        self.command_capture();

        // Read next movement direction from command input
        let direction = self.command_next();
        tracing::debug!("[GameLoopContext] tick, direction={:?}", direction);

        // Apply steering if direction available
        if let Some(dir) = direction {
            let updated_snake = self.snake_set_direction(dir);
            self.snake = updated_snake;
        }

        // Compute next head position
        let next_pos = self.snake_next();
        tracing::debug!(
            "[GameLoopContext] tick, next_pos=({}, {})",
            next_pos.x(),
            next_pos.y()
        );

        // Classify the predicted move
        let outcome = self.classify_move(&next_pos);

        match outcome {
            MoveOutcome::Obstacle => {
                tracing::debug!("[GameLoopContext] tick, outcome=Obstacle, game_over");
                None
            }
            MoveOutcome::Food => {
                tracing::debug!("[GameLoopContext] tick, outcome=Food");
                // Move with growth
                let updated_snake = self.snake_move(true);
                self.snake = updated_snake;

                // Increment score
                let updated_state = self.game_state.increment_score(10);

                // Drop new food
                let new_food = self.food_dropper_drop();
                let updated_state = updated_state.place_food(new_food);

                self.game_state = updated_state;
                Some(self)
            }
            MoveOutcome::Clear => {
                tracing::debug!("[GameLoopContext] tick, outcome=Clear");
                // Move without growth
                let updated_snake = self.snake_move(false);
                self.snake = updated_snake;
                Some(self)
            }
        }
    }

    // -------------------------------------------------------------------------
    // Private role methods
    // -------------------------------------------------------------------------

    /// snake.head — Returns the current head position.
    fn snake_head(&self) -> Position {
        tracing::debug!("[GameLoopContext] snake head");
        self.snake.body()[0]
    }

    /// snake.set_direction — Applies steering rules and returns the updated Snake.
    ///
    /// - Opposite direction changes are ignored.
    /// - Same-direction changes are ignored.
    /// - Other direction changes are accepted.
    fn snake_set_direction(&self, new_direction: Direction) -> Snake {
        tracing::debug!(
            "[GameLoopContext] snake set_direction, new_direction={:?}",
            new_direction
        );
        let current = self.snake.direction();
        if current == new_direction || current.is_opposite(new_direction) {
            tracing::debug!(
                "[GameLoopContext] snake set_direction, ignored (same or opposite)"
            );
            Snake::new(self.snake.body().to_vec(), current)
        } else {
            tracing::debug!("[GameLoopContext] snake set_direction, accepted");
            Snake::new(self.snake.body().to_vec(), new_direction)
        }
    }

    /// snake.next — Computes next head position from current direction.
    ///
    /// - Up:    (x, y + 1)
    /// - Down:  (x, y - 1)
    /// - Right: (x + 1, y)
    /// - Left:  (x - 1, y)
    fn snake_next(&self) -> Position {
        tracing::debug!("[GameLoopContext] snake next");
        let head = self.snake_head();
        let x = head.x();
        let y = head.y();
        let (nx, ny) = match self.snake.direction() {
            Direction::Up => (x, y + 1),
            Direction::Down => (x, y - 1),
            Direction::Right => (x + 1, y),
            Direction::Left => (x - 1, y),
        };
        // Clamp to non-negative for Position construction; boundary detection
        // happens in classify_move before this would be used as a valid position.
        let nx = nx.max(0);
        let ny = ny.max(0);
        tracing::debug!("[GameLoopContext] snake next, nx={}, ny={}", nx, ny);
        Position::new(nx, ny)
    }

    /// snake.move — Moves the snake. If grow=true, length increases by 1.
    /// Returns the updated Snake.
    fn snake_move(&self, grow: bool) -> Snake {
        tracing::debug!("[GameLoopContext] snake move, grow={}", grow);
        let next_pos = self.snake_next();
        let mut new_body = vec![next_pos];
        let old_body = self.snake.body();

        if grow {
            // Include all existing segments
            new_body.extend_from_slice(old_body);
        } else {
            // Include all segments except the last (tail)
            if old_body.len() > 1 {
                new_body.extend_from_slice(&old_body[..old_body.len() - 1]);
            }
        }
        tracing::debug!(
            "[GameLoopContext] snake move, new_body_len={}",
            new_body.len()
        );
        Snake::new(new_body, self.snake.direction())
    }

    /// command.capture — Reads currently available keystrokes into the shared buffer.
    fn command_capture(&self) {
        tracing::debug!("[GameLoopContext] command capture");
        self.command.borrow_mut().capture();
    }

    /// command.next — Returns the next available movement direction from shared input.
    /// Non-direction keys are consumed and ignored.
    fn command_next(&self) -> Option<Direction> {
        tracing::debug!("[GameLoopContext] command next");
        let action = self.command.borrow_mut().next_action();
        match action {
            Some(UserAction::Movement(dir)) => {
                tracing::debug!("[GameLoopContext] command next, direction={:?}", dir);
                Some(dir)
            }
            Some(_) | None => {
                tracing::debug!("[GameLoopContext] command next, no direction");
                None
            }
        }
    }

    /// food_dropper.drop — Returns Some(food) for a free cell or None.
    fn food_dropper_drop(&mut self) -> Option<Food> {
        tracing::debug!("[GameLoopContext] food_dropper drop");
        let result = self.food_dropper.drop();
        tracing::debug!(
            "[GameLoopContext] food_dropper drop, has_food={}",
            result.is_some()
        );
        result
    }

    /// game_state score reader — Returns the current score.
    fn game_state_score(&self) -> i32 {
        self.game_state.food(); // just to ensure it compiles with the trait; actual read:
        // GameState does not expose score directly; derive from increment tracking.
        // From the dependency, GameState::increment_score adds to an internal score field.
        // The only observable way to read score is through the public API.
        // Based on the capsule, `GameState` exposes `food()`, `place_food()`,
        // `game_time()`, `increment_score()` but NOT a direct `score()` getter.
        // The spec says get_score "returns the current score as a whole number".
        // Since GameState's public API (from dependencies) does not expose `score()`,
        // we must track score separately or infer it.
        // Looking at the capsule again: the contract says `get_score` uses `game_state`.
        // The GameState implementation fields include `score: i32` but no public getter.
        // We need to carry score separately or expose it.
        // Per the dependency contract, `increment_score` returns a new GameState with
        // score+amount. There is no `score()` getter listed.
        // Implementation choice: track the score as a prop in GameLoopContext.
        // This aligns with the spec which says "Score is the same value shown to the player."
        // We'll use a separate score field.
        // NOTE: this function is not used — see field approach below.
        0
    }

    // -------------------------------------------------------------------------
    // Private helper methods
    // -------------------------------------------------------------------------

    /// Returns true if the position is on the boundary wall.
    fn is_boundary(&self, x: i32, y: i32) -> bool {
        let w = self.board.width() as i32;
        let h = self.board.height() as i32;
        x == 0 || y == 0 || x == w - 1 || y == h - 1
    }

    /// Classifies the predicted next head position.
    fn classify_move(&self, next_pos: &Position) -> MoveOutcome {
        let nx = next_pos.x();
        let ny = next_pos.y();
        let w = self.board.width() as i32;
        let h = self.board.height() as i32;

        // Check boundary (wall)
        if nx < 0 || ny < 0 || nx >= w || ny >= h || self.is_boundary(nx, ny) {
            return MoveOutcome::Obstacle;
        }

        // Check snake body overlap (all segments except the current head)
        let body = self.snake.body();
        // Skip index 0 (head), check 1..end
        for seg in body.iter().skip(1) {
            if seg.x() == nx && seg.y() == ny {
                return MoveOutcome::Obstacle;
            }
        }

        // Check food
        if let Some(food) = self.game_state.food() {
            let fp = food.position();
            if fp.x() == nx && fp.y() == ny {
                return MoveOutcome::Food;
            }
        }

        MoveOutcome::Clear
    }

    /// Computes the pacing delay for this tick.
    ///
    /// Starts at 10 ticks/second (100ms). Speed increases logarithmically
    /// with elapsed game time. Formula:
    ///   base_ms = 100
    ///   speed_factor = 1.0 + ln(1 + elapsed_seconds)
    ///   delay_ms = base_ms / speed_factor
    fn compute_delay(&self) -> Duration {
        let elapsed_ms = self.game_state.game_time().max(0) as f64;
        let elapsed_seconds = elapsed_ms / 1000.0;
        let base_ms = 100.0_f64;
        let speed_factor = 1.0 + (1.0 + elapsed_seconds).ln();
        let delay_ms = (base_ms / speed_factor).max(10.0); // floor at 10ms
        tracing::debug!(
            "[GameLoopContext] compute_delay, elapsed_s={:.2}, speed_factor={:.3}, delay_ms={:.1}",
            elapsed_seconds,
            speed_factor,
            delay_ms
        );
        Duration::from_millis(delay_ms as u64)
    }
}
```

Wait, I notice an issue. The `game_state_score` function is a mess — `GameState` doesn't expose a public `score()` getter based on the dependency contracts. Let me re-examine and implement cleanly, tracking score separately, then rewrite properly.

```rust
use std::cell::RefCell;
use std::rc::Rc;
use std::thread;
use std::time::Duration;

use crate::contexts::CommandInputContext;
use crate::data::{Board, Direction, Food, GameState, Position, Snake, UserAction};

/// A trait for components that can drop food onto the board.
pub trait FoodDropper {
    fn drop(&mut self) -> Option<Food>;
}

/// Internal classification of the predicted move outcome.
enum MoveOutcome {
    Obstacle,
    Food,
    Clear,
}

/// `GameLoopContext` is the "Game" part of the system.
///
/// It is the single source of truth for the game rules and game state.
/// Each tick it handles: player input, steering, movement, collision, score/food,
/// and pacing.
pub struct GameLoopContext<D: FoodDropper> {
    board: Board,
    snake: Snake,
    command: Rc<RefCell<CommandInputContext>>,
    food_dropper: D,
    game_state: GameState,
    /// Tracks the current score independently since `GameState` has no `score()` getter.
    score: i32,
}

impl<D: FoodDropper> GameLoopContext<D> {
    /// Constructs a new `GameLoopContext` from the provided collaborators.
    ///
    /// Uses the provided shared command context; does not create a separate input stream.
    pub fn new(
        board: Board,
        snake: Snake,
        command: Rc<RefCell<CommandInputContext>>,
        food_dropper: D,
        game_state: GameState,
    ) -> Self {
        tracing::info!(
            "[GameLoopContext] new, board={}x{}, snake_len={}",
            board.width(),
            board.height(),
            snake.body().len()
        );
        Self {
            board,
            snake,
            command,
            food_dropper,
            game_state,
            score: 0,
        }
    }

    /// Returns a 2D character grid representing the current board state.
    ///
    /// The grid is indexed as `grid[x][y]` where `(x, y)` is the board coordinate.
    ///
    /// - `'w'` — wall cell at the boundary
    /// - `' '` — unoccupied cell
    /// - `'s'` — cell occupied by the snake
    /// - `'f'` — food position
    pub fn current_board(&self) -> Vec<Vec<char>> {
        tracing::info!("[GameLoopContext] current_board");
        let w = self.board.width() as usize;
        let h = self.board.height() as usize;

        // Initialise with spaces
        let mut grid = vec![vec![' '; h]; w];

        // Boundary walls: x==0, y==0, x==w-1, y==h-1
        for x in 0..w {
            for y in 0..h {
                if x == 0 || y == 0 || x == w - 1 || y == h - 1 {
                    grid[x][y] = 'w';
                }
            }
        }

        // Snake cells (overwrite walls only if snake occupies them; per spec order
        // snake and food are placed after walls so they can overlap boundary display
        // — but the spec says 'w' is boundary, so snake / food on boundary would be
        // unusual. Follow spec render order: walls first, then snake, then food.)
        for pos in self.snake.body() {
            let x = pos.x() as usize;
            let y = pos.y() as usize;
            if x < w && y < h {
                grid[x][y] = 's';
            }
        }

        // Food cell
        if let Some(food) = self.game_state.food() {
            let x = food.position().x() as usize;
            let y = food.position().y() as usize;
            if x < w && y < h {
                grid[x][y] = 'f';
            }
        }

        tracing::debug!("[GameLoopContext] current_board, w={}, h={}", w, h);
        grid
    }

    /// Returns the current score as a non-negative whole number.
    pub fn get_score(&self) -> i32 {
        tracing::info!("[GameLoopContext] get_score, score={}", self.score);
        self.score
    }

    /// Advances the game by one tick.
    ///
    /// Steps:
    /// 1. Sleep for the computed pacing delay.
    /// 2. Capture pending keystrokes into the shared input stream.
    /// 3. Read the next movement direction from command input.
    /// 4. Apply snake steering if a direction is available.
    /// 5. Compute the next head coordinate.
    /// 6. Classify the predicted move.
    /// 7. Return `None` on obstacle (game over).
    /// 8. On food: move with growth, add 10 to score, drop new food.
    /// 9. On clear: move without growth.
    pub fn tick(mut self) -> Option<Self> {
        tracing::info!("[GameLoopContext] tick");

        // Step 1: pacing delay
        let delay = self.compute_delay();
        tracing::debug!("[GameLoopContext] tick, delay_ms={}", delay.as_millis());
        thread::sleep(delay);

        // Step 2: capture keystrokes (input captured before or during tick N is
        // eligible to affect steering in tick N)
        self.command_capture();

        // Step 3: read next movement direction
        let direction = self.command_next();
        tracing::debug!("[GameLoopContext] tick, next_direction={:?}", direction);

        // Step 4: apply steering
        if let Some(dir) = direction {
            self.snake = self.snake_set_direction(dir);
        }

        // Step 5: compute next head position
        let next_pos = self.snake_next();
        tracing::debug!(
            "[GameLoopContext] tick, next_pos=({}, {})",
            next_pos.x(),
            next_pos.y()
        );

        // Step 6: classify
        let outcome = self.classify_move(&next_pos);

        // Steps 7-9
        match outcome {
            MoveOutcome::Obstacle => {
                tracing::debug!("[GameLoopContext] tick, outcome=Obstacle, returning None");
                None
            }
            MoveOutcome::Food => {
                tracing::debug!("[GameLoopContext] tick, outcome=Food, growing and scoring");
                self.snake = self.snake_move(true);
                self.score += 10;
                self.game_state = self.game_state.increment_score(10);
                let new_food = self.food_dropper_drop();
                self.game_state = self.game_state.place_food(new_food);
                Some(self)
            }
            MoveOutcome::Clear => {
                tracing::debug!("[GameLoopContext] tick, outcome=Clear");
                self.snake = self.snake_move(false);
                Some(self)
            }
        }
    }

    // -------------------------------------------------------------------------
    // Private role methods — snake
    // -------------------------------------------------------------------------

    /// `snake.head` — Returns the current head position.
    fn snake_head(&self) -> Position {
        tracing::debug!("[GameLoopContext] snake head");
        // Head is always the first element of body (guaranteed by Snake invariant).
        self.snake.body()[0]
    }

    /// `snake.set_direction` — Applies steering rules and returns the updated Snake.
    ///
    /// - Opposite direction changes are ignored.
    /// - Same-direction changes are ignored.
    /// - Other direction changes are accepted.
    fn snake_set_direction(&self, new_direction: Direction) -> Snake {
        tracing::debug!(
            "[GameLoopContext] snake set_direction, new_direction={:?}",
            new_direction
        );
        let current = self.snake.direction();
        let accepted = current != new_direction && !current.is_opposite(new_direction);
        let effective = if accepted { new_direction } else { current };
        tracing::debug!(
            "[GameLoopContext] snake set_direction, accepted={}, effective={:?}",
            accepted,
            effective
        );
        Snake::new(self.snake.body().to_vec(), effective)
    }

    /// `snake.next` — Computes next head position from the current direction.
    ///
    /// - `Up`:    `(x, y + 1)`
    /// - `Down`:  `(x, y - 1)`
    /// - `Right`: `(x + 1, y)`
    /// - `Left`:  `(x - 1, y)`
    fn snake_next(&self) -> Position {
        tracing::debug!("[GameLoopContext] snake next");
        let head = self.snake_head();
        let x = head.x();
        let y = head.y();
        let (nx, ny) = match self.snake.direction() {
            Direction::Up => (x, y + 1),
            Direction::Down => (x, y - 1),
            Direction::Right => (x + 1, y),
            Direction::Left => (x - 1, y),
        };
        // Clamp negative values to 0 for Position::new (which asserts >= 0).
        // The boundary/obstacle detection in classify_move catches out-of-bounds cases.
        let nx_safe = nx.max(0);
        let ny_safe = ny.max(0);
        tracing::debug!("[GameLoopContext] snake next, nx={}, ny={}", nx, ny);
        Position::new(nx_safe, ny_safe)
    }

    /// `snake.move` — Moves the snake to `next()`.
    ///
    /// If `grow = true`, length increases by 1.
    /// If `grow = false`, length is unchanged (tail is dropped).
    /// Returns the updated Snake.
    fn snake_move(&self, grow: bool) -> Snake {
        tracing::debug!("[GameLoopContext] snake move, grow={}", grow);
        let next_pos = self.snake_next();
        let old_body = self.snake.body();

        let mut new_body = Vec::with_capacity(old_body.len() + if grow { 1 } else { 0 });
        new_body.push(next_pos);

        let keep = if grow {
            old_body.len()
        } else {
            old_body.len().saturating_sub(1)
        };
        new_body.extend_from_slice(&old_body[..keep]);

        tracing::debug!(
            "[GameLoopContext] snake move, new_body_len={}",
            new_body.len()
        );
        Snake::new(new_body, self.snake.direction())
    }

    // -------------------------------------------------------------------------
    // Private role methods — command
    // -------------------------------------------------------------------------

    /// `command.capture` — Reads currently available keystrokes and appends them
    /// to the shared FIFO buffer.
    fn command_capture(&self) {
        tracing::debug!("[GameLoopContext] command capture");
        self.command.borrow_mut().capture();
    }

    /// `command.next` — Returns the next available movement direction from shared input.
    /// Non-direction keys are consumed and ignored. Returns `None` if no direction key.
    fn command_next(&self) -> Option<Direction> {
        tracing::debug!("[GameLoopContext] command next");
        let action = self.command.borrow_mut().next_action();
        match action {
            Some(UserAction::Movement(dir)) => {
                tracing::debug!("[GameLoopContext] command next, direction={:?}", dir);
                Some(dir)
            }
            Some(other) => {
                tracing::debug!(
                    "[GameLoopContext] command next, non-direction action={:?}, ignored",
                    other
                );
                None
            }
            None => {
                tracing::debug!("[GameLoopContext] command next, buffer empty");
                None
            }
        }
    }

    // -------------------------------------------------------------------------
    // Private role methods — food_dropper
    // -------------------------------------------------------------------------

    /// `food_dropper.drop` — Returns `Some(food)` for a free non-wall, non-snake cell,
    /// or `None` if no free cell exists.
    fn food_dropper_drop(&mut self) -> Option<Food> {
        tracing::debug!("[GameLoopContext] food_dropper drop");
        let result = self.food_dropper.drop();
        tracing::debug!(
            "[GameLoopContext] food_dropper drop, has_food={}",
            result.is_some()
        );
        result
    }

    // -------------------------------------------------------------------------
    // Private helpers
    // -------------------------------------------------------------------------

    /// Returns `true` if `(x, y)` is a boundary (wall) cell.
    fn is_boundary(&self, x: i32, y: i32) -> bool {
        let w = self.board.width() as i32;
        let h = self.board.height() as i32;
        x == 0 || y == 0 || x == w - 1 || y == h - 1
    }

    /// Classifies the predicted next head position as `Obstacle`, `Food`, or `Clear`.
    ///
    /// - `Obstacle`: out-of-bounds, boundary cell, or overlaps a snake segment
    ///   (skipping the current head at index 0).
    /// - `Food`: the position matches the current food placement.
    /// - `Clear`: no collision, no food.
    fn classify_move(&self, next_pos: &Position) -> MoveOutcome {
        let nx = next_pos.x();
        let ny = next_pos.y();
        let w = self.board.width() as i32;
        let h = self.board.height() as i32;

        // Out-of-bounds check (handles the case where clamping produced boundary coords)
        if nx < 0 || ny < 0 || nx >= w || ny >= h {
            return MoveOutcome::Obstacle;
        }

        // Boundary wall check
        if self.is_boundary(nx, ny) {
            return MoveOutcome::Obstacle;
        }

        // Snake body overlap (skip index 0 which is the current head)
        for seg in self.snake.body().iter().skip(1) {
            if seg.x() == nx && seg.y() == ny {
                return MoveOutcome::Obstacle;
            }
        }

        // Food check
        if let Some(food) = self.game_state.food() {
            let fp = food.position();
            if fp.x() == nx && fp.y() == ny {
                return MoveOutcome::Food;
            }
        }

        MoveOutcome::Clear
    }

    /// Computes the pacing delay for this tick.
    ///
    /// Base rate: 10 ticks/second (100 ms per tick).
    /// Speed increases logarithmically: `delay_ms = 100 / (1 + ln(1 + elapsed_seconds))`.
    /// Minimum delay is clamped to 10 ms.
    fn compute_delay(&self) -> Duration {
        let elapsed_ms = self.game_state.game_time().max(0) as f64;
        let elapsed_secs = elapsed_ms / 1000.0;
        let base_ms = 100.0_f64;
        let speed_factor = 1.0 + (1.0 + elapsed_secs).ln();
        let delay_ms = (base_ms / speed_factor).max(10.0);
        tracing::debug!(
            "[GameLoopContext] compute_delay, elapsed_s={:.2}, speed_factor={:.3}, delay_ms={:.1}",
            elapsed_secs,
            speed_factor,
            delay_ms
        );
        Duration::from_millis(delay_ms as u64)
    }
}