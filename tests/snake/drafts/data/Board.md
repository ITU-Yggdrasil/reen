# Board

## Description

A rectangular playfield that defines valid positions and acts as the primary obstacle boundary (walls/bounds) for Snake.

The coordinate system is zero-based, starts in the lower-left corner `(0,0)`, and extends
to the upper-right corner `(width - 1, height - 1)`.

## Fields

| Field | Meaning | Accessible | Notes |
|---|---|---|---|
| width | Width of the rectangular board | X | Positive whole number |
| height | Height of the rectangular board | X | Positive whole number |

## Rules

- `width` must be greater than `0`.
- `height` must be greater than `0`.
- Cells where `x == 0`, `y == 0`, `x == width - 1`, or `y == height - 1` are boundary walls.
