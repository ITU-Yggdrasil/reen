# Terminal Renderer

## Purpose

TerminalRenderer shows the current game frame in the terminal.

It uses StringRenderer as the formatter for each frame before writing that frame
to the screen.

## Role Players

| Role player | Why involved | Expected behaviour |
|---|---|---|
| string_renderer | Produces the text for the current frame | Returns one complete text frame for the supplied board picture and score |

## Role Methods

### string_renderer

- **render**
  Signature: `render(&self, board: &Board, score: u32) -> String`
  Formats the current board picture and score as one text frame.

## Props

| Prop | Meaning | Notes |
|---|---|---|
| screen | Visible terminal area where the frame is shown | Type `std::io::Stdout`; the same screen is updated from one frame to the next |

## Functionalities

### new

| Started by | Uses | Result |
|---|---|---|
| application startup or caller | string_renderer, screen | a terminal renderer ready to show frames is created |

**Flow:**
1. Signature: `new(string_renderer: StringRenderer, screen: std::io::Stdout) -> Self`
2. Store `string_renderer` as the role player used to format each frame.
3. Store `screen` as the terminal output handle updated on each render call.

**Guarantee:** `string_renderer` and `screen` are captured at construction; each subsequent `render` call uses exactly those collaborators.

| Given | When | Then |
|---|---|---|
| a string renderer and a screen handle | new is called | a terminal renderer bound to that renderer and screen handle is created |

### render

| Started by | Uses | Result |
|---|---|---|
| game loop or caller | string_renderer, screen | terminal shows the new frame |

**Flow:**
1. Signature: `render(&self, board: &Board, score: u32) -> anyhow::Result<()>`
2. Ask `string_renderer` to format the current board picture and score as one text frame.
3. Split the frame text into rows on the `'\n'` character. Empty rows (including any trailing one produced by a final `'\n'`) are skipped.
4. For each row, starting at row index `0` and incrementing per row, position the cursor at column `0` of that row on `screen` using `crossterm::cursor::MoveTo(0, row)` and then write the row's characters. This makes every row begin at column `0` even when the terminal is in raw mode (where a bare `'\n'` does not return the cursor to column `0`).
5. After all rows have been written, flush `screen` so the frame appears immediately.

**Extensions:**
- 2a. The terminal supports the home escape sequence → the previously shown frame is overwritten in place without visible flicker.

| Given | When | Then |
|---|---|---|
| a new frame is ready | render is called | the terminal shows the new frame in place without visible flicker, every row beginning at column 0 |
