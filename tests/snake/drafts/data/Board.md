# Board

## Description

The board describes the rectangular play area for a round of Snake.

The coordinate system is zero-based, starts in the lower-left corner `(0,0)`,
and extends to the upper-right corner `(width - 1, height - 1)`.

## Fields

| Field | Meaning | Notes |
|---|---|---|
| width | Width of the rectangular board | Positive whole number |
| height | Height of the rectangular board | Positive whole number |

## Rules

- `width` must be greater than `0`.
- `height` must be greater than `0`.
- Cells where `x == 0`, `y == 0`, `x == width - 1`, or `y == height - 1` are wall cells.