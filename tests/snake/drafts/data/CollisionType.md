# CollisionType

## Description

CollisionType describes how the game loop classifies the next predicted move.

## Variants

| Variant | Meaning | Notes |
|---|---|---|
| Obstacle | The predicted move hits a wall or the snake body | Ends the round |
| Food | The predicted move reaches the current food cell | Triggers growth and a score increase |
| Clear | No obstacle and no food is on that cell | Move without eating |
