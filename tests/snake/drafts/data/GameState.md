# GameState

## Description

Holds the current score, where the food is (if any), and how long the game has been running.

## Fields

- **score_** a whole number that is never negative, representing the current score.
  - Score starts at 0.
  - Score only increases.
  - Score will not exceed 2,000,000,000 during normal play.
- **food_placement** None if no food is available; Some(food) if there is food on the board.
- **game_started** start time of the game as a whole number representing `utc.now_ms` when the game was started.


## Functionality

- **new** initializes the game state by setting score to 0 and `game_started = utc.now_ms`; `food_placement` is set to None.
- **place_food** takes Some(food) or None and returns a new GameState with `food_placement` updated.
- **game time** returns `utc.now_ms - game_started` in milliseconds.
- **increment_score** takes a positive whole number and returns a new GameState with the score increased by that amount.
- **food** returns `food_placement`.
