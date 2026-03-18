# The primary application

## Description
This is a simple terminal-based Snake game application.

The application is responsible for:
- initializing game state,
- running the main loop,
- rendering board and score,
- restarting after game over,
- managing terminal input mode,
- maintaining one shared input stream used by menus and gameplay.

Game progression logic itself is delegated to the GameLoopContext.

---

### Initial state
On start, show a message such as:
"Press s to start a new game"

When the game is started:

- Create a Board using terminal width/height (characters/lines), leaving one line below for score.
- Create a Snake at center cell `(width/2, height/2)` using integer division:
  - initial length is 1 (head only),
  - initial direction is RIGHT.
- Create a GameState containing:
  - score = 0
  - one food item at a valid non-wall, non-snake coordinate
  - start timestamp `utc.now_ms`.
- Create a GameLoopContext.
- Create a TerminalRenderer.

---

### Behavior

The application runs an outer loop with the following behavior:

0. Terminal mode and startup checks:
   - Before entering the main loop, enable raw terminal input mode so single key presses are available immediately (without pressing Enter), and disable input echo.
   - On application exit (normal or error), restore the previous terminal mode.
   - Create one shared CommandInputContext before the loop and keep using the same shared input stream for the process lifetime.
   - If terminal width < 20 or terminal height < 20, print an error to stderr and exit with code `20`.

1. If there is no active GameLoopContext (including when the application first starts):
   - Render a start screen (including score from the previous game if one has been completed in this session).
   - Call CommandInputContext.capture and then poll keys via CommandInputContext.next_key.
   - If the user presses the start key "S" or "s":
     - Recreate the initial state.
     - Create a new GameLoopContext that uses the same shared command input stream.
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
