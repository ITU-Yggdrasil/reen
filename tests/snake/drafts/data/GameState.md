# GameState

## Description

Holds the current score, the placement of food, and a representation of how the long the game has been active

## Fields

- **score_** an integer >=0 representing the current score
- **food_placement** None if no food is available Some(food) if there's food on the board
- **game_started** start time of the game represented as an integer denoting utc.now_ms of when the game was started


## Functionality

- **new** initialises the game state by setting score to 0 and `game_started = utc.now_ms`; `food_placement` is set to None
- **place_food** gets a food object Some(f) or None and creates a new state object with food_placement updated
- **game time** returns utc.now_ms - game_started in milliseconds
- **increament_score** gets a positive integer and returns a new GameState with the score incremented by the argument value
- **food** returns food_placement
