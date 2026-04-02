# CollisionType

## Description

A simple enum enumerating the collision outcomes the game loop distinguishes.

## Variants

| Variant | Meaning | Notes |
|---|---|---|
| Obstacle | The predicted move hits a wall or the snake body | Ends the game |
| Food | The predicted move reaches the current food cell | Causes growth and score increase |
