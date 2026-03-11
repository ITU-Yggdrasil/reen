# Terminal Renderer

## Description

TerminalRenderer draws the current game frame as ASCII in the terminal.

## Behavior

- **render(board, score)**
  - Coordinate system is `(0,0)` bottom-left and `(width-1, height-1)` top-right.
  - Input:
    - `board`: 2D char grid where `board[x][y]` is cell `(x,y)`.
    - `score`: current score.

  - Output rules:
    - Rendering is in-place: clear terminal and move cursor to top-left before each frame.
    - Print rows from top (`y=height-1`) to bottom (`y=0`).
    - Within each row, print columns left-to-right (`x=0` to `x=width-1`).
    - Each row starts at terminal column 0.
    - After printing the last board row, print exactly one additional line for score at terminal column 0.
    - Score line format is exactly `Score: <score>` where `<score>` is the base-10 integer value with no padding, separators, or extra decoration.
    - End the score line with a newline.
