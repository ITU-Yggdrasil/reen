## Context Specification for Terminal Renderer

### Description

This context is a render used to render the current frame for a game. Specifically, it renders the game frame as ASCII art in the terminal.

### Functionality

- **render**
  Renders the current game state to the terminal.
  - **input:**
    - **board** - A two-dimensional array of characters representing the current board. The array represents the cells in the grid. `board[x][y]` matches the coordinate `(x, y)`.
    - **score** - The current score of the game.

  - **behavior:**
    - **Rendering Procedure:**
      1. Clear the terminal.
      2. Move the cursor to the top-left of the terminal.
      3. For each cell in the board, print the character starting from the topmost line (row `height - 1`) to the bottommost line (row `0`), working from left to right.
      4. Each rendered line must start at column 0 (use carriage return semantics to ensure rows do not drift right in raw mode).
      5. Display the score to the left of the board below the last line of the board.

### Inferred Types or Structures (Non-Blocking)

- **None**

### Blocking Ambiguities

- **None**

### Implementation Choices Left Open

- **Non-blocking technical details:**
  - The exact collection type used for the board (e.g., `Vec<Vec<char>>`, `BTreeMap`, etc.) can be chosen by the implementation.
  - The mechanism for clearing the terminal and moving the cursor can be chosen by the implementation.
  - The method of displaying the score can be chosen by the implementation (e.g., using `print!` or `println!`).

---

### Inferred Types or Structures (Non-Blocking)

**None**

### Blocking Ambiguities

**None**

### Implementation Choices Left Open

**Non-blocking technical details:**
- The exact collection type used for the board can be chosen by the implementation.
- The mechanism for clearing the terminal and moving the cursor can be chosen by the implementation.
- The method of displaying the score can be chosen by the implementation (e.g., using `print!` or `println!`).