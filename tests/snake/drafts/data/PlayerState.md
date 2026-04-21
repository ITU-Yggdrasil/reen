# Data: PlayerState

## Purpose

PlayerState describes whether the player is alive or dead after a game tick.

## Attributes

The variants are:

- Alive: the game continues
- Dead: the game is over

## Invariants

## Operations

## Scenarios

### alive after clear move
Given: the snake moved to a clear cell
When: the tick completes
Then: the player state is Alive

### dead after wall hit
Given: the snake moved into a wall
When: the tick completes
Then: the player state is Dead
