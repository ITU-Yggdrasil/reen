# String Renderer

## Purpose

StringRenderer formats the current game frame as plain text (ASCII) and returns it as a string.

It is the canonical frame formatter for the Snake application.

## Role Players

## Role Methods

## Props

## Functionalities

### render

| Started by | Uses | Result |
|---|---|---|
| terminal renderer or caller | board, score | plain-text frame string is returned |

Rules:
- Uses coordinate system `(0,0)` bottom-left and `(width-1, height-1)` top-right.
- Returns a single string containing the full rendered frame.
- Formats rows from top (`y = height - 1`) to bottom (`y = 0`).
- Formats columns within each row from left to right.
- Ends each board row with a newline.
- Appends exactly one score line after the final board row.
- Uses score line format `Score: <score>`.
- Ends the score line with a newline.

| Given | When | Then |
|---|---|---|
| a board and score of 12 | render is called | the returned frame ends with `Score: 12` followed by a newline |
