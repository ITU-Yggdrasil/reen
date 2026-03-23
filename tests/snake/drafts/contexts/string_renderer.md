# String Renderer

## Description

StringRenderer formats the current game frame as plain text (ASCII) and returns it as a string.

It is the canonical frame formatter for the Snake application.

## Functionalities

- **render(board, score)**
  - Coordinate system is `(0,0)` bottom-left and `(width-1, height-1)` top-right.
  - Input:
    - `board`: 2D char grid where `board[x][y]` is cell `(x,y)`.
    - `score`: current score (a whole number from 0 to 2,000,000,000).

  - Output rules:
    - Return a single string containing the full rendered frame.
    - Format rows from top (`y=height-1`) to bottom (`y=0`).
    - Within each row, format columns left-to-right (`x=0` to `x=width-1`).
    - End each board row with a newline.
    - After the last board row, append exactly one additional score line.
    - Score line format is exactly `Score: <score>` where `<score>` is the score written in normal base-10 digits with no padding, separators, or extra decoration.
    - End the score line with a newline.
