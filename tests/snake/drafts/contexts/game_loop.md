# GameLoopContext

## Description

GameLoopContext advances the game by exactly one tick.
Per tick it handles:
- input capture and steering,
- movement and collision outcomes,
- score/food updates,
- tick pacing (difficulty).

---

## Roles

- **snake**
  Represents occupied cells and current direction.

- **command**
  Provides shared user input for this tick.

- **food_dropper**
  Produces a valid next food placement when needed.

- **game_state**
  Holds score, food placement, and game start time.

---

## Props

- **board**
  Board dimensions and boundary rules.

---

## Role methods

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

## Behavior

- **new(board, snake, command, food_dropper, game_state)**
  - Uses the provided collaborators.
  - Input must come from the provided shared command context; no separate stdin stream is allowed.

- **current_board**
  Returns a 2D char grid where `board[x][y]` maps to coordinate `(x,y)`:
  - 'w' for wall at the boundary
  - ' ' for unoccupied cells
  - 's' for cells occupied by the snake
  - 'f' for where the food is placed

- **get_score**
  Returns current score.

- **tick**
  Executes one tick and returns:
  - `Some(new GameLoopContext)` if the game continues
  - `None` if the game ends

  Steps:

  1. **Pacing**
     - Start at 10 ticks/second.
     - Increase speed over time with logarithmic growth (exact formula is implementation-defined).
     - Wait for the computed delay before continuing.

  2. **Capture input**
      - Capture pending keystrokes into the shared input stream.

  3. **Steering**
      - Read next movement direction from command input.
      - If a direction is available, apply snake steering rules.
      - If no direction is available, keep current direction.

  4. **Predict move**
      - Compute the next head coordinate.

  5. **Classify collision at predicted head**
      - `Obstacle` if next head is on boundary cell
        (`x==0`, `y==0`, `x==width-1`, `y==height-1`)
        or overlaps any snake segment except current head.
      - `Food` if food exists and next head equals food position.
      - `None` otherwise.

  6. **Apply outcome**
      - If `Obstacle`: return `None`.
      - If `Food`:
        - move with growth,
        - add 10 score,
        - place new food via `food_dropper.drop`,
        - return continued game state.
      - If `None`:
        - move without growth,
        - keep score/food unchanged,
        - return continued game state.

---

## Cross-tick input guarantees

- Input captured before or during tick `N` must be eligible to affect steering in tick `N` (if a movement key is available).
- Movement input consumed in tick `N` must not be re-applied automatically in tick `N+1`.
- If no movement key is available at steering time, the snake keeps its current direction.

---

## Acceptance examples

- Given the next predicted head hits a wall, when `tick()` runs, then result is `None`.
- Given the next predicted head reaches food, when `tick()` runs, then score increases by 10 and snake length increases by 1.
- Given buffered keys `x`, `w` before a tick, when `tick()` runs, then the snake steers `UP` (non-action keys ignored).
