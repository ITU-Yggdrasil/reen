# GameLoopContext

## Purpose

GameLoopContext is the "Game" part of the system.
It is the single source of truth for the game rules and game state.

Each tick (one step forward), it handles:
- reading any buffered player input,
- steering and movement,
- collisions (wall or snake body),
- score and food updates,
- pacing (how fast the game runs).

---

## Role Players

| Role player | Why involved | Expected behaviour |
|---|---|---|
| snake | Holds occupied cells and current direction | Supports steering, head lookup, next position calculation, and movement |
| command | Supplies shared player input | Uses the same shared command stream as the rest of the application |
| food_dropper | Produces new food when needed | Returns a free valid food placement or `None` |
| game_state | Holds score, food placement, and start time | Carries the evolving game state across ticks |

---

## Props

| Prop | Meaning | Notes |
|---|---|---|
| board | Board dimensions and boundary rules | Defines wall cells and playable cells |

---

## Role Methods

### snake

- **head**
  Returns the current head position.

- **set_direction(new_direction)**
  Applies steering rules:
  - opposite direction changes are ignored,
  - same-direction changes are ignored,
  - other direction changes are accepted.

- **next**
  Computes next head position from current direction:
  - UP: `(x, y + 1)`
  - DOWN: `(x, y - 1)`
  - RIGHT: `(x + 1, y)`
  - LEFT: `(x - 1, y)`

- **move(grow)**
  Moves to `next()`.
  If `grow=true`, length increases by 1.
  If `grow=false`, length is unchanged.

### command

- **next**
  Returns the next available movement direction from shared input, if any.

### food_dropper

- **drop**
  Returns `Some(food)` on a free non-wall, non-snake cell, or `None` if no free cell exists.

---

## Functionalities

### new

| Started by | Uses | Result |
|---|---|---|
| application startup | board, snake, command, food_dropper, game_state | game loop context is constructed |

Rules:
- Uses the provided collaborators.
- Input must come from the provided shared command context.
- Must not create a separate input stream.

| Given | When | Then |
|---|---|---|
| board, snake, command, food_dropper, and game_state are available | new is called | the game loop stores the provided collaborators |

### current_board

| Started by | Uses | Result |
|---|---|---|
| renderer or caller | board, snake, game_state | current board picture is returned |

Rules:
- Returns a 2D character grid where `board[x][y]` maps to coordinate `(x,y)`.
- Uses `w` for wall cells at the boundary.
- Uses a space for unoccupied cells.
- Uses `s` for cells occupied by the snake.
- Uses `f` for the food position.

| Given | When | Then |
|---|---|---|
| a board with walls, snake cells, and food | current_board is called | the returned grid uses `w`, space, `s`, and `f` at the correct coordinates |

### get_score

| Started by | Uses | Result |
|---|---|---|
| renderer or caller | game_state | current score is returned |

Rules:
- Returns the current score as a whole number.
- Score is never negative.
- Score is the same value shown to the player.

| Given | When | Then |
|---|---|---|
| the current score is 20 | get_score is called | the result is 20 |

### tick

| Started by | Uses | Result |
|---|---|---|
| game loop scheduler | command, snake, board, food_dropper, game_state | next game state is produced or the game ends |

Rules:
- Starts at 10 ticks per second.
- Increases speed over time with logarithmic growth.
- Waits for the computed pacing delay before the tick continues.
- Captures pending keystrokes into the shared input stream.
- Reads the next movement direction from command input.
- If a direction is available, applies snake steering rules.
- If no direction is available, keeps the current direction.
- Computes the next head coordinate before deciding the outcome.
- Treats the predicted move as `Obstacle` when the next head is on a boundary cell or overlaps any snake segment except the current head.
- Treats the predicted move as `Food` when food exists and the next head equals the food position.
- Returns `None` when the predicted move is `Obstacle`.
- On `Food`, moves with growth, adds 10 score, places new food through `food_dropper.drop`, and returns a continued game state.
- On no collision, moves without growth, keeps score and food unchanged, and returns a continued game state.
- Input captured before or during tick `N` must be eligible to affect steering in tick `N`.
- Movement input consumed in tick `N` must not be re-applied automatically in tick `N+1`.

| Given | When | Then |
|---|---|---|
| the next predicted head hits a wall | tick runs | the result is `None` |
