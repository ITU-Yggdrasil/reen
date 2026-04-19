# GameLoopContext

## Purpose

GameLoopContext is the part of the system that runs the rules of the Snake
game. It decides movement, turning, collision outcomes, scoring, food
replacement, and pacing from one tick to the next.

## Role Players

| Role player | Why involved | Expected behaviour |
|---|---|---|
| snake | Current snake positions and travel direction | Can provide the ordered body and the current direction |
| command | Shared user input stream | Captures pending input and returns the next gameplay action from the shared queue |
| food_dropper | an RNG used to choose new food positions | Chooses a free interior food position, or no position if none is available |
| game_state | Score, food placement, and round start time | Can provide score, food placement, and round start time, and can return updated copies with changed score or food placement |

## Role Methods

### snake

- **body**
  Signature: `body(&self) -> &Vec<Position>`
  Returns the snake positions in order, with the head first.

- **direction**
  Returns the current travel direction.

### command

- **capture**
  Adds newly available key presses to the shared queue without waiting.

- **next_action**
  Returns the next gameplay action from the shared queue, if one is available.

### food_dropper

- **drop**
  Chooses a food placement using the current board and the current snake.
  Returns a placement on a free interior cell, or no placement when none is
  available.

### game_state

- **score**
  Signature: `score(&self) -> u32`
  Returns the current score.

- **food**
  Signature: `food(&self) -> Option<Food>`
  Returns the current food placement, if any.

- **game_started_ms**
  Returns the recorded start time of the round.

- **with_score**
  Returns the same game state with a different score.

- **with_food_placement**
  Returns the same game state with a different food placement.

## Props

| Prop | Meaning | Notes |
|---|---|---|
| board | Base board picture for the current round | The outer ring of the board is treated as walls. `board` is the reusable board picture whose walls and empty interior are overlaid with snake and food for the current tick. |

## Functionalities

### new

| Started by | Uses | Result |
|---|---|---|
| application startup | board, snake, command, food_dropper, game_state | game loop context is created with an initial food placement chosen by the food dropper |

**Flow:**
1. Signature: `new(snake: Snake, command: CommandInputContext, food_dropper: rand::rngs::ThreadRng, game_state: GameState, board: Board) -> Self`
2. Construct a mutable local context value `ctx` that stores the provided `board`, `snake`, `command`, `food_dropper`, and `game_state` in the corresponding fields exactly as supplied (the `game_state` passed in is expected to have no food placement yet — the caller is not responsible for picking initial food).
3. Ask the food dropper for an initial food position by invoking the `food_dropper.drop()` role method on `ctx` (the same role method that `tick` uses in step 11 to replace food after the snake eats — the initial food placement is chosen with exactly the same logic).
4. If step 3 returned `Some(position)`, replace `ctx.game_state` with `ctx.game_state.place_food(Some(Food::new(position)))`. If step 3 returned `None` (the board has no free interior cell — effectively impossible at the start of a normal round), leave `ctx.game_state` unchanged so the round starts with no food on the board.
5. Return `ctx`.

**Guarantee:** Input comes exclusively from the shared command stream; no separate stream is created. The `game_state` stored on the returned context reflects an initial food placement chosen by the food dropper — the caller does not place initial food itself, and the `Option<Food>` the caller put on the incoming `game_state` is overwritten by step 4 when the food dropper returns a position.

| Given | When | Then |
|---|---|---|
| board, snake, shared input, food dropper, and a game state with no food are available | new is called | a game loop context is created and its stored game state has a food placement on a free interior cell when one exists |

### current_board

| Started by | Uses | Result |
|---|---|---|
| renderer or caller | board, snake, game_state | a `Board` representing the current board picture is returned |

**Flow:**
1. Signature: `current_board(&self) -> Board`
2. Let `board_picture` be a clone of `board`.
3. For each `Position` in the snake body, reassign `board_picture` to the result of `board_picture.with_symbol_at(position, 's')`. Do not attempt to mutate `board_picture` in place; `Board` is an immutable value and exposes no in-place setter — `with_symbol_at` is the only way to produce an overlaid board picture.
4. If `game_state.food()` is `Some(food)`, reassign `board_picture` to `board_picture.with_symbol_at(food.position(), 'f')`.
5. Return `board_picture`.

**Guarantee:** The returned `Board` is the complete rendered board picture for the current tick. It is derived from the base `board` prop together with `snake` and `game_state`, and every snake body cell and food cell in the returned board shows `s` or `f` respectively.

| Given | When | Then |
|---|---|---|
| a round with walls, snake cells, and food | current_board is called | the returned `Board` shows `w`, space, `s`, and `f` in the correct places |

### get_score

| Started by | Uses | Result |
|---|---|---|
| renderer or caller | game_state | current score is returned |

**Flow:**
1. Signature: `get_score(&self) -> u32`
2. Return `game_state.score()`.

| Given | When | Then |
|---|---|---|
| the current score is 20 | get_score is called | the result is 20 |

### tick

| Started by | Uses | Result |
|---|---|---|
| game loop scheduler | command, snake, board, food_dropper, game_state | Returns PlayerState::Alive if no obstacles were hit or PlayerState::Dead if an obstacle was hit |

**Flow:**
1. Read the current UTC millisecond count into `now_ms`.
2. Let `elapsed_ms` be `now_ms.saturating_sub(game_state.game_started_ms())`.
3. Let `elapsed_s` be `elapsed_ms / 1000` (integer whole seconds).
4. Let `wait_ms` be `max(10_u64, (100.0_f64 / (1.0_f64 + (1.0_f64 + elapsed_s as f64).ln())) as u64)`.
5. Sleep `wait_ms` milliseconds.
6. Call `command.capture()` to collect pending input.
7. Read the next gameplay action via `command.next_action()`.
8. If a movement action was received and it neither repeats nor directly reverses the current direction, update the snake's direction.
9. Compute the next head position from the current head and the current direction.
10. If the next head lands on a wall cell or a snake body cell, return `PlayerState::Dead`.
11. If food exists and the next head matches the food position, grow the snake by one segment, increase the score by 10, and call `food_dropper.drop()` for a replacement food position using the updated snake.
12. Otherwise, advance the snake without growing; leave score and food placement unchanged.
13. Return `PlayerState::Alive`.

**Extensions:**
- 7a. No action is available → skip direction update; continue from step 9.
- 7b. A fire action is received → ignore it; continue from step 9 with the current direction.
- 11a. `food_dropper.drop()` returns no position → clear food from `game_state`.

**Guarantee:** Input consumed in one tick is not replayed in the next.

| Given | When | Then |
|---|---|---|
| the next predicted head hits a wall | tick is called | the round ends |

## Notes

- GameLoopContext owns the game rules for steering, movement, collision
  decisions, scoring, food replacement, and pacing.
