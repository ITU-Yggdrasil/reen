# Data: Board

## Purpose

The Board is the complete renderable picture for one Snake frame. It is a rectangular grid of cells, each holding a single display symbol. The coordinate system is screen-style: origin (0, 0) is the top-left cell, x increases to the right, y increases downward.

## Attributes

- width: positive whole number of columns in the board
- height: positive whole number of rows in the board
- cells: a mapping from each in-bounds position to its display symbol; contains exactly one entry per coordinate within the board dimensions

## Invariants

- Width and height are both greater than zero.
- Every coordinate within bounds has exactly one cell entry; no entries exist outside bounds.
- Cells on the outer ring (edges of the board) may contain wall symbols.
- Interior cells may contain blank space, snake symbols, or food symbols depending on the current frame.

## Operations

### symbol_at
Returns the display symbol stored at a given coordinate.

### with_symbol_at
Returns a new Board identical to this one except that the cell at the given position is replaced with the given symbol. If the position is outside the board bounds, the returned board is an unchanged copy. This is the sole way to produce an overlaid board picture — callers that need to render snake or food cells on top of the base board chain calls to with_symbol_at starting from a base board.

## Scenarios

### read a wall symbol
Given: a 10 by 10 board where (0, 0) contains a wall symbol "w"
When: symbol_at is called for (0, 0)
Then: the result is "w"

### overlay a snake symbol
Given: a 10 by 10 board where (3, 3) contains a space
When: with_symbol_at is called for position (3, 3) with symbol "s"
Then: the returned board has "s" at (3, 3) and all other cells unchanged

### out-of-bounds overlay is a no-op
Given: a 10 by 10 board
When: with_symbol_at is called for position (15, 15) with symbol "x"
Then: the returned board is identical to the original
