# Direction

## Description

The Direction type represents the four cardinal directions of travel for the snake. It is a fieldless enum.

## Variants

| Variant | Meaning | Notes |
|---|---|---|
| Up | Moving upward on the screen | In board coordinates (see `Position`, origin top-left, `y` grows downward), `Up` means the `y` component **decreases** by 1. |
| Down | Moving downward on the screen | `y` component **increases** by 1. |
| Left | Moving leftward on the screen | `x` component **decreases** by 1. |
| Right | Moving rightward on the screen | `x` component **increases** by 1. |

## Notes

- `Direction` derives `Copy` and `Clone`.
- `Direction` does not know about coordinates itself; the mapping from `Direction` to a `Position` delta is stated above and must be applied consistently wherever a direction is turned into a next position (e.g. `GameLoopContext::tick` when it computes the next head cell). The board uses screen coordinates — `y = 0` is the top row — so "up" decreases `y` and "down" increases `y`. Do not use the mathematical convention where `y` grows upward.