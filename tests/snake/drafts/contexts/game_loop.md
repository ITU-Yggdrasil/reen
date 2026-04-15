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
| food_dropper | Chooses new food positions | Chooses a free interior food position, or no position if none is available |
| game_state | Score, food placement, and round start time | Can provide score, food placement, and round start time, and can return updated copies with changed score or food placement |

## Role Methods

### snake

- **body**
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
  Returns the current score.

- **food**
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
| board | Playfield size and wall boundaries | The outer ring of the board is treated as walls |

## Functionalities

### new

| Started by | Uses | Result |
|---|---|---|
| application startup | board, snake, command, food_dropper, game_state | game loop context is created |

**Flow:**
1. Store the provided board, snake, command stream, food dropper, and game state as collaborators.

**Guarantee:** Input comes exclusively from the shared command stream; no separate stream is created.

| Given | When | Then |
|---|---|---|
| board, snake, shared input, food dropper, and game state are available | new is called | a game loop context is created with those collaborators |

### current_board

| Started by | Uses | Result |
|---|---|---|
| renderer or caller | board, snake, game_state | a picture of the current board is returned |

**Flow:**
1. Create an empty map from `Position` to `char` (`std::collections::HashMap<Position, char>`).
2. For each boundary cell of `board`, set the symbol to `w`.
3. For each unoccupied interior cell, set the symbol to a space.
4. For each cell in the snake body, set the symbol to `s`.
5. If food exists in `game_state`, set the food cell's symbol to `f`.
6. Return the completed map.

| Given | When | Then |
|---|---|---|
| a round with walls, snake cells, and food | current_board is called | the returned picture shows `w`, space, `s`, and `f` in the correct places |

### get_score

| Started by | Uses | Result |
|---|---|---|
| renderer or caller | game_state | current score is returned |

**Flow:**
1. Return the score from `game_state`.

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
