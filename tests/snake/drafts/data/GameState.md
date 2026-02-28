# GameState

## Description

Holds the current score, the placement of food, and a representation of when the game was started

## Fields

- **score** a positive integer  (including 0) representing the current score
- **food_placement** None if no food is available Some(food) if there's food on the board
- **game_started** start time of the game represented as an integer denoting the ms between start and 2026-01-01 0:00
- **place_food** gets a food object (f) and creates a new state object with food_placement updated to Some(f)