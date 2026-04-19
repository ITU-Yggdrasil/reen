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
  Signature: `width(&self) -> u32`
  Returns the number of columns in the picture.

- **height**
  Signature: `height(&self) -> u32`
  Returns the number of rows in the picture.

- **symbol_at**
  Signature: `symbol_at(&self, x: u32, y: u32) -> char`
  Returns the symbol to show at a given coordinate.

## Props

| Prop | Meaning | Notes |
|---|---|---|
| score | Score shown below the board | Type `u32`; same score the user sees during play |

## Functionalities

### new

| Started by | Uses | Result |
|---|---|---|
| application startup or TerminalRenderer | board, score | a string renderer ready to produce frames is created |

**Flow:**
1. Store `board` as the role player the renderer will query for on-screen symbols.

**Guarantee:** Construction captures only the provided renderer state; no other collaborators are fetched, inferred, or mutated.

| Given | When | Then |
|---|---|---|
| a board and a score of 12 | new is called | a string renderer bound to that board and score 12 is created |

### render

| Started by | Uses | Result |
|---|---|---|
| TerminalRenderer or the application | score | one text frame is returned |

**Flow:**
1. Signature: `render(&self,score: u32) -> String`
2. Iterate rows from top (y = 0) to bottom (y = `board.height() - 1`).
3. Within each row, iterate columns from left (x = 0) to right (x = `board.width() - 1`), appending the symbol returned by `board.symbol_at(x, y)`.
4. Append a newline character after each row.
5. After the last row, append the score line `Score: <score>` followed by a newline.
6. Return the complete string.

**Guarantee:** The renderer reads symbols from the supplied `board` and `score`; it does not decide what belongs in any cell.

| Given | When | Then |
|---|---|---|
| a board picture and score 12 | render is called | the returned text ends with `Score: 12` followed by a newline |
