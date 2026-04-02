# Direction

## Description
A value type representing movement direction for the Snake.

## Variants

| Variant | Meaning | Notes |
|---|---|---|
| Up | Move one cell upward on the board | |
| Down | Move one cell downward on the board | |
| Left | Move one cell left on the board | |
| Right | Move one cell right on the board | |

## Rules

- Opposites: `Up <-> Down`, `Left <-> Right`

## Functionalities

- **is_opposite** Takes another direction and returns true when the two directions are opposites.
