# Board

## Description

The board is the complete renderable picture for one Snake frame.

The coordinate system is zero-based and uses screen-style coordinates, matching
`Position`: the origin `(0, 0)` is the **top-left** cell, `x` increases to the
right, and `y` increases **downward**. The bottom-right cell is
`(width - 1, height - 1)`. This is the opposite of the mathematical convention
where `y` grows upward — do not assume the mathematical convention.

## Fields

| Field | Meaning | Accessible | Notes |
|---|---|---|---|
| width | Width of the rectangular board picture | yes | Type `u32`; positive whole number |
| height | Height of the rectangular board picture | yes | Type `u32`; positive whole number |
| cells | Symbol stored at each visible coordinate | no | Type `std::collections::HashMap<Position, char>`; contains every in-bounds visible coordinate exactly once |

## Rules

- `width` must be greater than `0`.
- `height` must be greater than `0`.
- `cells` contains one entry for every coordinate where `0 <= x < width` and `0 <= y < height`.
- `cells` contains no entry outside those bounds.
- Cells where `x == 0`, `y == 0`, `x == width - 1`, or `y == height - 1` may contain wall symbols.
- Interior cells may contain blank space, snake symbols, or food symbols depending on the current frame.

## Functionalities

| Method | Kind | Signature | Behavior |
|---|---|---|---|
| `symbol_at` | method | `symbol_at(&self, x: u32, y: u32) -> char` | Returns the stored symbol at the visible coordinate `(x, y)`. |
| `with_symbol_at` | method | `with_symbol_at(&self, position: Position, symbol: char) -> Board` | Returns a new `Board` with the same `width`, `height`, and `cells` as `self`, except the cell at `position` is replaced with `symbol`. If `position` is outside the board bounds (`x >= width` or `y >= height`), the returned board is an unchanged clone of `self`. This is the sole way to produce an overlaid board picture: callers that need to render snake or food cells on top of the base board must chain `with_symbol_at` calls, starting from a base board, rather than mutating `self`. |
