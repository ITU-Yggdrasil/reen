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

Rules:
- Uses the provided board, snake, shared command stream, food dropper, and game
  state.
- Input must come from the provided shared command stream.
- Must not create a separate input stream.

| Given | When | Then |
|---|---|---|
| board, snake, shared input, food dropper, and game state are available | new is called | a game loop context is created with those collaborators |

### current_board

| Started by | Uses | Result |
|---|---|---|
| renderer or caller | board, snake, game_state | a picture of the current board is returned |

Rules:
- Returns a board picture arranged by coordinate, where each location holds one
  display symbol, represented as `std::collections::HashMap<Position, char>`.
- Uses `w` for wall cells on the boundary.
- Uses a space for unoccupied interior cells.
- Uses `s` for cells occupied by the snake.
- Uses `f` for the food position when food exists.
- Builds the picture from the board, the snake body, and the current food
  placement.

| Given | When | Then |
|---|---|---|
| a round with walls, snake cells, and food | current_board is called | the returned picture shows `w`, space, `s`, and `f` in the correct places |

### get_score

| Started by | Uses | Result |
|---|---|---|
| renderer or caller | game_state | current score is returned |

Rules:
- Returns the current score from the game state.
- The returned score is the same score shown to the user.

| Given | When | Then |
|---|---|---|
| the current score is 20 | get_score is called | the result is 20 |

### tick

| Started by | Uses | Result |
|---|---|---|
| game loop scheduler | command, snake, board, food_dropper, game_state | Returns PlayerState::Alive if no obstacles were hit or PlayerState::Dead if an obstacle was hit |

Rules:
- Base delay is `100 ms` (10 ticks per second).
- Speed increases over time using a logarithmic curve. Let `now_ms` be the
  current UTC millisecond count and `game_started_ms` from game state. Let
  `elapsed_ms = now_ms.saturating_sub(game_started_ms)` and
  `elapsed_s = elapsed_ms / 1000` (integer whole seconds). The pacing wait in
  milliseconds before the rest of the tick runs is
  `max(10_u64, (100.0_f64 / (1.0_f64 + (1.0_f64 + elapsed_s as f64).ln())) as u64)`,
  giving a floor of 10 ms and slowing from 100 ms as the round ages.
- Uses the current UTC millisecond count and the recorded round start time to
  determine how long the round has been running.
- Waits for the computed pacing delay before continuing the rest of the tick.
- Calls `capture` on the shared command input.
- Reads the next gameplay action.
- A movement action may change direction, but repeating the current direction or
  reversing directly into the opposite direction is ignored.
- A fire action has no effect on this game.
- Computes the next head position from the current head and current direction.
- If the next head lands on a wall or on the snake body, the round ends.
- If food exists and the next head lands on the food position, the snake grows
  by one segment, the score increases by 10, and the food dropper is asked for
  a replacement food position using the updated snake.
- If the next head lands on an empty interior cell, the snake moves forward
  without growing and the score and food placement stay unchanged.
- The result indicates whether the player hit an obstacle and died or the game continues because the player is still alive
- Input consumed in one tick is not replayed automatically in the next.

| Given | When | Then |
|---|---|---|
| the next predicted head hits a wall | tick is called | the round ends |

## Notes

- GameLoopContext owns the game rules for steering, movement, collision
  decisions, scoring, food replacement, and pacing.
