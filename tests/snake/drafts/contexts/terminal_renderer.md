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
    - Rendering is in-place: each call must replace the previously shown frame with the new one so the terminal shows one stable current view.
    - Normal rendering must not rely on visible blank-screen flashes or other noticeable flicker.
    - Display the returned frame string exactly as produced by StringRenderer.
    - In terminal mode, rendering must restart at column 0 before the frame is written.
    - If the terminal update mechanism moves the cursor upward between frames, it must also return the cursor to column 0 before rewriting the next frame.
    - Multi-line terminal output must preserve left alignment line-by-line; no line may inherit the previous line's horizontal cursor position.
