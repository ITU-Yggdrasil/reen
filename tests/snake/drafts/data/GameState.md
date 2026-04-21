# Data: GameState

## Purpose

GameState holds the current score, food placement, and game start time. It is an immutable value type; all operations that change state return a new GameState rather than modifying in place.

## Attributes

- score: the current score as a non-negative whole number, starting at 0
- food_placement: the current Food item on the board, or absent if no food is available
- game_started: the UTC millisecond timestamp when the game round started

## Invariants

- The score is never negative.
- The score only increases during play.
- The score never exceeds 2,000,000,000 during normal play; score increases saturate at this cap.

## Operations

### place_food
Returns a new GameState with the food placement updated to the given food (or absent to remove food). All other fields are unchanged.

### game_time
Returns the elapsed milliseconds since the game started (current UTC milliseconds minus game_started).

### increment_score
Returns a new GameState with the score increased by the given amount, saturating at 2,000,000,000. The amount must be greater than zero. All other fields are unchanged.

### food
Returns the current food placement, or absent if none.

### game_started
Returns the recorded start timestamp in milliseconds.

## Scenarios

### place food on a fresh state
Given: a GameState with score 0 and no food
When: place_food is called with a Food at position (5, 3)
Then: the returned GameState has food at (5, 3) and score 0

### increment score by 10
Given: a GameState with score 20
When: increment_score is called with amount 10
Then: the returned GameState has score 30

### score saturates at cap
Given: a GameState with score 1,999,999,995
When: increment_score is called with amount 10
Then: the returned GameState has score 2,000,000,000

### remove food
Given: a GameState with food at (5, 3)
When: place_food is called with absent
Then: the returned GameState has no food
