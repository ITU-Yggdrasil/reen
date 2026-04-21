# Context: TerminalRenderer

## Purpose

TerminalRenderer shows the current game frame in the terminal. It uses a text formatter to produce each frame before writing that frame to the screen.

## Role Players

- string_renderer: StringRenderer — produces the text representation of the current frame from a board picture and a score

## Props

- screen: the visible terminal output where each frame is shown; the same output is updated from one frame to the next

## Role Methods

### string_renderer
- format_frame: formats the current board picture and score as one complete text frame

## Functionalities

### new
Stores the string renderer and the screen as collaborators. The string renderer is used to format each frame; the screen is updated on each render call.

### render
Asks the string renderer to format the current board picture and score as a text frame. Splits the frame into rows. For each row, positions the cursor at the start of that row on the screen and writes the row's text. After all rows are written, flushes the screen so the frame appears immediately. Every row begins at the start of its line so that the frame displays correctly in immediate-input terminal mode.

## Scenarios

### render a frame
Given: a new frame is ready with a board picture and a score
When: render is called
Then: the terminal shows the new frame in place, every row beginning at the start of the line

### successive frames overwrite in place
Given: two frames are rendered in sequence
When: the second render completes
Then: the terminal shows only the second frame without leftover content from the first
