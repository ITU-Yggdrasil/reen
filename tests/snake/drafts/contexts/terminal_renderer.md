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
  Formats the current board picture and score as one text frame.

## Props

| Prop | Meaning | Notes |
|---|---|---|
| screen | Visible terminal area where the frame is shown | The same screen is updated from one frame to the next |

## Functionalities

### render

| Started by | Uses | Result |
|---|---|---|
| game loop or caller | string_renderer, screen | terminal shows the new frame |

**Flow:**
1. Ask `string_renderer` to format the current board picture and score as one text frame.
2. Move the terminal cursor to the home position (top-left) on `screen`.
3. Write the returned frame text to `screen`, left-aligned line by line.
4. Flush `screen` so the frame appears immediately.

**Extensions:**
- 2a. The terminal supports the home escape sequence → the previously shown frame is overwritten in place without visible flicker.

| Given | When | Then |
|---|---|---|
| a new frame is ready | render is called | the terminal shows the new frame in place without visible flicker |
