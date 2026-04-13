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

Rules:
- Receives the current board picture and score.
- Asks StringRenderer to format them as one text frame.
- Replaces the previously shown frame without visible flicker where possible.
- Displays the returned frame text exactly.
- Each line begins at the left edge of the terminal.
- Multi-line output stays left-aligned line by line.

| Given | When | Then |
|---|---|---|
| a new frame is ready | render is called | the terminal shows the new frame in place without visible flicker |
