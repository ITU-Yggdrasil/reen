# GameState

## Description

Holds the current score, the placement of food, and a representation of how the long the game has been active

## Fields

- **score_** an integer >=0 representing the current score
- **food_placement** None if no food is available Some(food) if there's food on the board
- **game_started** start time of the game represented as an integer denoting signed milliseconds from the UTC baseline `2026-01-01T00:00:00Z` to the game start time, i.e. `game_started = start_utc_ms - baseline_utc_ms`


## Functionality

- **new** initialises the game state by setting score to 0 and `game_started = utc.now_ms - baseline_utc_ms`, where `baseline_utc_ms` is `2026-01-01T00:00:00Z`; `food_placement` is set to None
- **place_food** gets a food object (f) and creates a new state object with food_placement updated to Some(f)
- **game time** returns utc.now - game_started in milliseconds
- **increament_score** gets a positive integer and returns a new GameState with the score incremented by the argument value
- **food** returns food_placement
