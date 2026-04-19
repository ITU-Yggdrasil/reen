# Position

## Description

The Position type represents a single cell coordinate on the game board.

## Fields

| Field | Meaning | Notes |
|---|---|---|
| x | Horizontal coordinate | Type `u32`. `x = 0` is the leftmost column; `x` grows to the right. |
| y | Vertical coordinate | Type `u32`. `y = 0` is the **top** row; `y` grows **downward**. This matches terminal screen coordinates and the way the board is rendered (row 0 is printed first, then row 1 below it, and so on). It is the opposite of the mathematical convention where `y` grows upward — do not assume the mathematical convention. |

## Coordinate convention

The board uses screen-style coordinates: the origin `(0, 0)` is the top-left cell, `x` increases to the right, and `y` increases downward. Any code that translates a `Direction` into a position delta must use this convention:

- moving **up** on the screen → `y` **decreases** (`y - 1`)
- moving **down** on the screen → `y` **increases** (`y + 1`)
- moving **left** on the screen → `x` **decreases** (`x - 1`)
- moving **right** on the screen → `x` **increases** (`x + 1`)