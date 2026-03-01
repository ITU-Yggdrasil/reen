# The primary application

## Description
This is a simple terminal-based Snake game application.

The application is responsible for:
- initializing the game state
- running the main loop
- rendering the game state. Board and score can be obtained from the game loop. 
- restarting the game when it is not running
- managing terminal input mode for interactive keyboard control

Game progression logic itself is delegated to the GameLoopContext.

---

### Initial state
The application should display a message such as:
"Press s to start a new game"

When the game is started:

- A Board is created with predefined width and height matching the height and iwdth of the terminal (meassured in characters (width) and lines (heinght)).
  - The board should use the full available terminal size.
- A snake is created and place at the center most cell. defined as (width / 2, height / 2) as integer division (truncated)
  - the initial length is one i.e. only a head
  - direction RIGHT
- A GameState is created containing:
  - score = 0
  - a single food item placed at a valid coordinate i.e. not on the boundaries of the board and not overlapping with 
  the nake
  - the time the game was started represented as ms since 2026-01-01 0:00
- A GameLoopContext is constructed

---

### functionality

The application runs an outer loop with the following behavior:

0. Terminal input mode:
   - Before entering the main loop, enable raw terminal input mode so single key presses are available immediately (without pressing Enter), and disable input echo.
   - On application exit (normal or error), restore the previous terminal mode.

1. If there is no active GameLoopContext (including when the application first starts):
   - Render a start screen (including score from the previous game if one has been completed in this session).
   - Wait for user input as single key presses.
   - If the user presses the start key "S" or "s":
     - Recreate the initial state.
     - Create a new GameLoopContext.
   - If "Q" or "q" is pressed exit the program
   - Otherwise continue waiting.

2. If a GameLoopContext exists:
   - Render the current game state to the terminal:
     - Rendering is in-place: clear the terminal and move cursor to the top-left before drawing each frame.
     - Each rendered line must start at column 0 (use carriage return semantics so rows do not drift right in raw mode).
     - Draw board boundaries
     - Draw snake body
     - Draw food if food_placement is not None
     - Display current score

   - Call `gameLoopContext.tick()`
   - If the result is:
     - a new GameLoopContext → replace the current one and continue.
     - None → the game ends, set current context to None.

3. When the game ends:
   - Render final board state.
   - Render a "Game Over" message and final score.
   - Allow the user to start a new game.

The application continues running until explicitly terminated (presing "q" or "Q" when no gaming is running).

The exit code should be 0.

---

### Error handling

In case of a runtime error:

- The application should exit with a non-zero exit code.
- The exit code should be 42.
- If an error message is available, it should be printed to standard error.

No partial state recovery is required.
