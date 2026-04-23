# The primary application

## Description
This is a simple Snake game application that runs in a terminal.

Depends on: game_loop, command_input, terminal_renderer

The application is responsible for:
- initializing game state,
- running the main loop,
- showing the board and score,
- restarting after game over,
- putting the terminal in a mode where single key presses are read immediately,
- keeping exactly one shared input stream that is used by both menus and gameplay.

Game progression logic itself is delegated to the GameLoopContext.

### Helpers the App Uses
The Primary Application uses these helpers (they must exist as part of the overall system):
- **CommandInputContext**: captures key presses into one shared FIFO stream for the whole session.
- **GameLoopContext**: holds the game rules and advances the game by one tick at a time.
- **TerminalRenderer**: draws the board and the score to the terminal.
- **food_dropper**: chooses the next food placement on a free interior (non-wall, non-snake) cell.

The Primary Application must not implement the game rules itself (movement, collisions, scoring, food placement).
It only (1) collects input, (2) asks the GameLoopContext to advance, and (3) shows what the game looks like.

---

### Initial state
On start, show a message such as:
"Press s to start a new game"

When the game is started:

- Create a Board using terminal width/height (characters/lines), leaving one line below for score.
- Create a Snake at center cell `(width/2, height/2)` using integer division:
  - initial length is 1 (head only),
  - initial direction is RIGHT.
- Create one `food_dropper` collaborator for this game session.
  - When asked to drop food, it must return:
    - `Some(food)` on a free interior (non-wall, non-snake) cell, or
    - `None` if no free interior cell exists.
- Determine initial food by calling `food_dropper.drop` using the initial Board and Snake.
  - If the result is `None`, continue the game with no food on the board.
- Create a GameState containing:
  - score = 0
  - the initial food returned by `food_dropper.drop`
  - start timestamp `utc.now_ms`.
- Create a GameLoopContext using Board, Snake, shared CommandInputContext, the same `food_dropper`, and GameState.
- Create a TerminalRenderer.

---

### Behavior

The application runs an outer loop with the following behavior:

0. Terminal mode and startup checks:
   - Before entering the main loop, enable raw terminal input mode so single key presses are available immediately (without pressing Enter), and disable input echo.
   - On application exit (normal or error), restore the previous terminal mode.
   - Create one shared CommandInputContext before the loop and keep using the same shared input stream for the process lifetime (menus + gameplay).
   - If terminal width < 20 or terminal height < 20, print an error to stderr and exit with code `20`.

1. If there is no active GameLoopContext (including when the application first starts):
   - Render a start screen (including score from the previous game if one has been completed in this session).
   - Call CommandInputContext.capture and then poll keys via CommandInputContext.next_key.
   - If the user presses the start key "S" or "s":
     - Recreate the initial state.
     - Create a new GameLoopContext that uses the same shared command input stream and a `food_dropper` as defined above.
   - If "Q" or "q" is pressed, exit the program.
   - Otherwise continue waiting.

2. If a GameLoopContext exists:
   - Render current frame using `gameLoopContext.current_board` and `gameLoopContext.get_score`.
   - Call `gameLoopContext.tick()`
   - If the result is:
     - a new GameLoopContext → replace the current one and continue.
     - None → the game ends, set current context to None.

3. When the game ends:
   - Render final board state.
   - Render a "Game Over" message and final score.
   - Allow the user to start a new game.

The application continues until the user exits (`q` or `Q` while no game is active).

Normal exit code is `0`.

---

### Error handling

In case of a runtime error:

- The application should exit with a non-zero exit code.
- The exit code should be 42.
- If an error message is available, it should be printed to standard error.

No partial state recovery is required.
