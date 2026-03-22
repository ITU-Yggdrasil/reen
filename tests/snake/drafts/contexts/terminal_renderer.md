# Terminal Renderer

## Description

TerminalRenderer draws the current game frame as text (ASCII) in the terminal.

It must use StringRenderer as the canonical formatter for each frame.

## Roles

- **string_renderer**
  Formats the current game frame as plain text and returns the full frame string.

## Functionalities

- **render(board, score)**
  - Coordinate system is `(0,0)` bottom-left and `(width-1, height-1)` top-right.
  - Input:
    - `board`: 2D char grid where `board[x][y]` is cell `(x,y)`.
    - `score`: current score (a whole number from 0 to 2,000,000,000).

  - Output rules:
    - Call `string_renderer.render(board, score)` to obtain the full frame string.
    - Rendering is in-place: clear terminal and move cursor to top-left before printing each frame.
    - Print the returned frame string exactly as produced by StringRenderer.
