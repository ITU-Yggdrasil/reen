# Data: Snake

## Purpose

The Snake records which cells are occupied by the snake body and which direction it is currently traveling.

## Attributes

- body: an ordered list of positions occupied by the snake; the head is the first element
- direction: the current direction of travel

## Invariants

- The body contains at least one position.
- All positions in the body are unique (no overlap).
- The first position is the head.
- Every pair of consecutive positions is orthogonally adjacent (they differ by exactly 1 in either x or y, not both).
- A single-element body is valid; the adjacency rule is satisfied trivially.
- The direction does not need to be consistent with the head-to-neck orientation; it records the intended direction of travel independently.

## Operations

### new
Creates a Snake from a full ordered body and a direction. Fails with an error if the body is empty, if the body overlaps itself, or if consecutive positions are not adjacent cells.

### body
Returns the ordered body as a read-only sequence of positions, head first.

### direction
Returns the current direction of travel.

## Scenarios

### construct a valid snake
Given: a body of [(5,5), (4,5), (3,5)] and direction Right
When: new is called
Then: a Snake is returned with head at (5,5) and length 3

### empty body is rejected
Given: an empty body and direction Right
When: new is called
Then: an error is returned

### overlapping body is rejected
Given: a body of [(5,5), (4,5), (5,5)] and direction Right
When: new is called
Then: an error is returned

### non-adjacent body is rejected
Given: a body of [(5,5), (7,5)] and direction Right
When: new is called
Then: an error is returned
