# Snake App

## Application Kind

`cli_app`

## Description

Minimal app fixture that wires a board producer into a renderer.

## Collaborators and Wiring

| Collaborator | Responsibility |
|---|---|
| `GameLoop` | Produces the current board picture. |
| `Renderer` | Renders the board picture. |

## Startup Sequence

- Let `game_loop` be `GameLoop::new()`.
- Let `renderer` be `Renderer::new()`.

## Main Flow

- Let `board_picture` be `game_loop.current_board()`.
- Let `rendered` be `renderer.render(board_picture)`.
