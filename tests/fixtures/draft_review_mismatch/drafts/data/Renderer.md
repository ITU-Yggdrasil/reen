# Renderer

## Description

Renders a board picture to text.

## Fields

| Field | Meaning | Notes |
|---|---|---|
| theme | Renderer theme name | Type is `String`. |

## Functionalities

| Method | Kind | Signature | Behavior |
|---|---|---|---|
| `new` | Constructor | `new() -> Self` | Creates a renderer. |
| `render` | Instance method | `render(&self, board: Board) -> String` | Returns the rendered board text. |
