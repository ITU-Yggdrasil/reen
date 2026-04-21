# Data: CollisionType

## Purpose

CollisionType describes how the game loop classifies the next predicted move.

## Attributes

The variants are:

- Obstacle: the predicted move hits a wall or the snake body, ending the round
- Food: the predicted move reaches the current food cell, triggering growth and a score increase
- Clear: no obstacle and no food on that cell, allowing the snake to move without eating

## Invariants

## Operations

## Scenarios

### wall collision
Given: the next head position is a wall cell
When: the collision is classified
Then: the result is Obstacle

### food collision
Given: the next head position matches the current food cell
When: the collision is classified
Then: the result is Food

### clear cell
Given: the next head position is an empty interior cell
When: the collision is classified
Then: the result is Clear
