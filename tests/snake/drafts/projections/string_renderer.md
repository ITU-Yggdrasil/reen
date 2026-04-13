# String Renderer

## Purpose

StringRenderer turns one board picture and its score into plain text.

It is the canonical formatter for the visible game frame.

## Role Players

| Role player | Why involved | Expected behaviour |
|---|---|---|
| Board | Supplies the picture of the current board as a two-dimensionel array where the indices match (x,y) | Allows the renderer to read what symbol should appear at each visible position |

## Role Methods

### Board

- **width**
  Returns the number of columns in the picture.

- **height**
  Returns the number of rows in the picture.

- **symbol_at**
  Returns the symbol to show at a given coordinate.

## Props

| Prop | Meaning | Notes |
|---|---|---|
| score | Score shown below the board | Same score the user sees during play |

## Functionalities

### render

| Started by | Uses | Result |
|---|---|---|
| TerminalRenderer or the application | board, score | one text frame is returned |

Rules:
- Reads the current board picture and the score.
- Formats rows from top to bottom.
- Formats columns within each row from left to right.
- Each board row ends with a newline.
- After the last board row, append exactly one score line.
- Uses score line format `Score: <score>`.
- The score line also ends with a newline.
- The renderer works from the supplied board picture and score. It does not
  decide what belongs in a cell.

| Given | When | Then |
|---|---|---|
| a board picture and score 12 | render is called | the returned text ends with `Score: 12` followed by a newline |
