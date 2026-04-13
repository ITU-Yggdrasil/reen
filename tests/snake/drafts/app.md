# The primary application

## Application Kind
`cli_app`

## Description

This is a terminal Snake application.

Depends on:

- `game_loop` (`GameLoopContext`)
- `command_input` (`CommandInputContext`)
- `terminal_renderer` (`TerminalRenderer`)
- `string_renderer` (`StringRenderer`)

The application is responsible for:

- initializing game state
- running the main loop
- showing the board and score
- restarting after game over
- putting the terminal in a mode where single key presses are read immediately
- keeping exactly one shared input stream used by both menus and gameplay

Game progression logic is delegated to `GameLoopContext`.

---

## Runtime Topology

The application runs as one terminal process with one shared input stream for
the whole session.

At any point in time it has either:

- no active `GameLoopContext` and is showing a menu or start screen, or
- one active `GameLoopContext` and is running gameplay

---

## Command Interface

No command-line arguments or subcommands are defined.

---

## Transport Surface

Not applicable. The application does not expose network routes.

---

## Static Surface

Not applicable. The application does not serve static pages or assets.

---

## Collaborators and Wiring

| Collaborator | Responsibility |
|---|---|
| `CommandInputContext` | Captures key presses into one shared FIFO stream for the whole session. Provides raw keys for menus and gameplay actions for the game loop. |
| `GameLoopContext` | Holds the game rules and advances gameplay one tick at a time. Provides the current board picture and score. |
| `StringRenderer` | Formats a board picture and score into one plain-text frame. |
| `TerminalRenderer` | Uses `StringRenderer` and shows the latest frame in the terminal. |
| `food_dropper` | Chooses the next food placement on a free interior non-wall, non-snake cell. |

The application must not implement movement, collisions, scoring, or food
placement rules itself.

It only:

1. collects input
2. asks `GameLoopContext` to advance
3. renders the board picture from `current_board` together with the score from `get_score`

---

## Startup Sequence

On startup, show a message such as:

`Press s to start a new game`

When a new game is started:

- Create a board from terminal width and height, leaving one line below for the
  score.
- Create the initial head position at the center of the board.
- Create the initial snake as a one-segment snake facing right.
- Create one food dropper for that game session.
- Ask the food dropper to choose the initial food using the current board and
  snake.
- If no valid food position exists, start the round without food on the board.
- Create the initial game state with:
  - score `0`
  - the chosen initial food position, if any
  - the current UTC millisecond time as the round start time
- Create a `GameLoopContext` using the board, snake, shared command input,
  food dropper, and game state.
- Prepare a `StringRenderer` and, when terminal output is used, a
  `TerminalRenderer`.

---

## Main Loop Behavior

The application runs an outer loop with the following behavior.

### 0. Terminal mode and startup checks

- Read environment variable `SNAKE_RENDERER`.
- If `SNAKE_RENDERER=string`, use string-renderer-only output mode.
- Otherwise use terminal-renderer output mode.
- Before entering the main loop, enable raw terminal input mode so single key
  presses are available immediately and input echo is disabled.
- On application exit, restore the previous terminal mode.
- Create one shared `CommandInputContext` before the loop and keep using the
  same shared input stream for the process lifetime.
- If terminal width < 20 or terminal height < 20, print an error to stderr and
  exit with code `20`.

### 1. No active `GameLoopContext`

- Render or update a start screen, including score from the previous game if
  one exists.
- In terminal mode, the start screen appears as one stable screen while waiting
  for input; idle iterations must not visibly flicker.
- In terminal mode, each line must begin at column 0.
- Call `CommandInputContext.capture`.
- Poll raw keys via `CommandInputContext.next_key`.
- If the user presses `"S"` or `"s"`:
  - recreate the initial state from the startup sequence
  - create a new `GameLoopContext` using the same shared command input stream
- If the user presses `"Q"` or `"q"`, exit the program.
- Otherwise continue waiting.

### 2. Active `GameLoopContext` exists

- Obtain the current frame data from:
  - `game_loop_context.current_board()`
  - `game_loop_context.get_score()`
- Render using the board picture and the score only:
  - `StringRenderer.render`
  - `TerminalRenderer.render`
- Renderers work from the board picture and the score. They do not decide what
  belongs in each cell.
- In terminal mode, `TerminalRenderer` updates the terminal in place so the
  latest frame is shown as one stable screen without accumulated old frames.
- In string mode, print the `StringRenderer` output directly with no
  terminal-clearing behavior.
- Call `game_loop_context.tick()`.
- If `tick()` indicates that the round has ended, clear the active context and
  proceed to game-over behavior.
- If `tick()` indicates that play should continue, keep the same game context
  and continue looping.

### 3. Game ends

- Render the final board state again using the current board picture and score,
  or reuse the last picture already available.
- Render a `"Game Over"` message and the final score.
- In terminal mode, each game-over line must begin at column 0.
- Allow the user to start a new game.
- If `SNAKE_RENDERER=string`, also print
  `REEN_SNAKE_TEST_RESULT game_over score=<score>` to standard error and then
  exit with code `0`.

The application continues until the user exits with `q` or `Q` while no game is
active.

Normal exit code is `0`.

---

## Error Handling

In case of a runtime error:

- exit with non-zero code `42`
- print any available error message to standard error

No partial state recovery is required.
