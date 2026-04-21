# Data: Position

## Purpose

A Position represents a single cell coordinate on the game board. The coordinate system uses screen-style conventions: the origin (0, 0) is the top-left cell, x increases to the right, and y increases downward. This is the opposite of the mathematical convention where y grows upward.

## Attributes

- x: horizontal coordinate; 0 is the leftmost column, grows to the right
- y: vertical coordinate; 0 is the top row, grows downward (screen convention)

## Invariants

- Both x and y are non-negative whole numbers.

## Operations

## Scenarios

### top-left corner
Given: x is 0 and y is 0
When: a Position is created
Then: it represents the top-left cell of the board

### moving down increases y
Given: a Position at (3, 2)
When: y is increased by 1
Then: the new Position is (3, 3), which is one row below
