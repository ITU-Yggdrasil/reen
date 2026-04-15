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

**Flow:**
1. Iterate rows from top (y = 0) to bottom (y = `board.height() - 1`).
2. Within each row, iterate columns from left (x = 0) to right (x = `board.width() - 1`), appending the symbol returned by `board.symbol_at(x, y)`.
3. Append a newline character after each row.
4. After the last row, append the score line `Score: <score>` followed by a newline.
5. Return the complete string.

**Guarantee:** The renderer reads symbols from `board` and `score`; it does not decide what belongs in any cell.

| Given | When | Then |
|---|---|---|
| a board picture and score 12 | render is called | the returned text ends with `Score: 12` followed by a newline |
