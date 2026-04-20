# GameLoop

## Description

Produces a board picture for rendering.

## Fields

| Field | Meaning | Notes |
|---|---|---|
| tick | Current tick counter | Type is `u32`. |

## Functionalities

| Method | Kind | Signature | Behavior |
|---|---|---|---|
| `new` | Constructor | `new() -> Self` | Creates a game loop. |
| `current_board` | Instance method | `current_board(&self) -> std::collections::HashMap<Position, char>` | Returns the board picture for the current tick. |
