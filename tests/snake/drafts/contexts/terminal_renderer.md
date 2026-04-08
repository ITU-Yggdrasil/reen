# Terminal Renderer

## Purpose

TerminalRenderer draws the current game frame as text (ASCII) in the terminal.

It must use StringRenderer as the canonical formatter for each frame.

## Role Players

| Role player | Why involved | Expected behaviour |
|---|---|---|
| string_renderer | Produces the canonical plain-text frame | Returns the full frame string for a board and score |

## Role Methods

### string_renderer

- **render(board, score)**
  Formats the current game frame as plain text and returns the full frame string.

## Props

## Functionalities

### render

| Started by | Uses | Result |
|---|---|---|
| game loop or caller | string_renderer, board, score | current frame is shown in the terminal |

Rules:
- Uses coordinate system `(0,0)` bottom-left and `(width-1, height-1)` top-right.
- Calls `string_renderer.render(board, score)` to obtain the frame string.
- Replaces the previously shown frame with the new one.
- Normal rendering must not rely on visible blank-screen flashes or noticeable flicker.
- Displays the returned frame string exactly as produced by StringRenderer.
- Before writing to the terminal, interprets each `\n` in the returned frame as a terminal line break that restarts at column 0 (for example by emitting `\r\n`).
- In terminal mode, rendering restarts at column 0 before the frame is written.
- If the terminal update mechanism moves the cursor upward between frames, it also returns the cursor to column 0 before rewriting.
- Multi-line output preserves left alignment line by line.

| Given | When | Then |
|---|---|---|
| a new frame is ready | render is called | the terminal shows the new frame in place without visible flicker |
