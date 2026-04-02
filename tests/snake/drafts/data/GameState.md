# GameState

## Description

Holds the current score, where the food is (if any), and how long the game has been running.

## Fields

| Field | Meaning | Notes |
|---|---|---|
| score | Current score as a whole number | Starts at `0` |
| food_placement | Current food on the board, if any | `None` means no food is available |
| game_started | Game start time expressed as `utc.now_ms` | Used to calculate elapsed game time |

## Rules

- `score` is never negative.
- `score` only increases.
- During normal play, `score` does not exceed `2,000,000,000`.

## Functionalities

- **new** Initializes the game state by setting `score = 0`, `food_placement = None`, and `game_started = utc.now_ms`.
- **place_food** Takes `Some(food)` or `None` and returns a new GameState with `food_placement` updated.
- **game_time** Returns `utc.now_ms - game_started` in milliseconds.
- **increment_score** Takes a positive whole number and returns a new GameState with the score increased by that amount.
- **food** Returns `food_placement`.
