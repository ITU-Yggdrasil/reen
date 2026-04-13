# Snake

## Description

The snake records which cells are occupied by the snake and which direction it
is currently traveling.

## Fields

| Field | Meaning | Notes |
|---|---|---|
| body | Ordered list of `Position` values occupied by the snake | Stored as `Vec<Position>` internally; the head is the first element; `Position` is defined in the Position data type; field is private, exposed only via getter returning `&[Position]` |
| direction | Current direction of travel | Uses the Direction data type |

## Rules

- `body` length is greater than `0`.
- All positions in `body` must be unique.
- The first position in `body` is the head.
- Every pair of consecutive positions in `body` must be orthogonally adjacent (differ by exactly 1 in `x` or `y`, not both).
- A single-element `body` is valid; the adjacency rule is vacuously satisfied when there are no consecutive pairs.
- The `direction` field does not need to be consistent with the head-to-neck orientation; it records the current intended direction of travel independently.
- Both `body` and `direction` are private fields; they are not publicly accessible directly.
- The `body` getter returns `&[Position]` (a slice reference into the internal `Vec<Position>`).
- The `direction` getter returns `Direction` by copy (assuming `Direction` derives `Copy`).
- Construction failures return an opaque `anyhow::Result` error; no distinct error variants are required.

## Construction Rules

- `new` receives the full ordered body and the current direction.
- The `body` parameter type is `Vec<Position>`; ownership is transferred into `Snake` at construction.
- Construction fails if the body is empty.
- Construction fails if the body overlaps itself.
- Construction fails if two adjacent elements in the body are not adjacent cells on the board (i.e. only x or y differs and only by +1 or -1).

## Notes

- `Position` is sourced from the `Position` data type. Its fields `x` and `y` are both `u32`.
- `Direction` is sourced from the `Direction` data type. It is a fieldless enum with variants `Up`, `Down`, `Left`, and `Right`, and it derives `Copy`.
- The `body` getter exposes the internal storage as `&[Position]` (a borrowed slice), not `&Vec<Position>` or a cloned `Vec<Position>`.
- The `direction` getter returns `Direction` by copy because `Direction` is `Copy`.
- All three construction failure modes (empty body, overlap, non-adjacency) return an opaque `anyhow::Result` error; no distinct error type or variants are defined.