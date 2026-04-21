# Data: Direction

## Purpose

Direction represents the four cardinal directions of travel for the snake. It is a fieldless enumeration.

## Attributes

The variants are:

- Up: moving upward on the screen; the y coordinate decreases by 1
- Down: moving downward on the screen; the y coordinate increases by 1
- Left: moving leftward on the screen; the x coordinate decreases by 1
- Right: moving rightward on the screen; the x coordinate increases by 1

The board uses screen coordinates where y = 0 is the top row, so "up" decreases y and "down" increases y.

## Invariants

- Direction does not know about coordinates itself; the mapping from direction to a position delta must be applied consistently wherever a direction is turned into a next position.

## Operations

## Scenarios

### up decreases y
Given: a snake head at (5, 5)
When: the direction is Up
Then: the next head position is (5, 4)

### down increases y
Given: a snake head at (5, 5)
When: the direction is Down
Then: the next head position is (5, 6)
