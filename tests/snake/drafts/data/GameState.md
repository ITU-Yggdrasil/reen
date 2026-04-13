# GameState

## Description

Holds the current score, where the food is (if any), and how long the game has been running.
`Board` is not held or referenced by `GameState`; it is not a dependency of this type.
`GameState` is an immutable value type; all mutating operations return a new `GameState` rather than modifying in place.
`Food` implements both `Clone` and allows ownership transfer (move); it does not implement `Copy`. No additional trait bounds beyond `Clone` are intended.

## Fields

| Field | Meaning | Notes |
|---|---|---|
| score | Current score as a whole number | Starts at `0` |
| food_placement | Current `Food` item on the board, if any | `None` means no food is available; type is `Option<Food>` |
| game_started | Game start time expressed as `utc.now_ms` | Used to calculate elapsed game time; type is `u64` |

## Rules

- `score` is never negative.
- `score` only increases.
- During normal play, `score` does not exceed `2,000,000,000`.
- `score` is stored as `u32`. The saturation cap of `2,000,000,000` is enforced by logic in `increment_score` (saturating addition), not at the type level. `u32::MAX` (~4,294,967,295) exceeds the cap, so the cap is a domain invariant maintained by the method.

## Functionalities

| Method | Kind | Signature | Behavior |
|---|---|---|---|
| `place_food` | Instance method | `place_food(&self, food: Option<Food>) -> GameState` | Takes `&self` (borrowing) and `food: Option<Food>` by value (owned); returns a new `GameState` with `food_placement` updated. `Food` is moved into the new state. Passing `None` is always legal and removes the current food placement. All other fields are unchanged. `Food` implements `Clone` and allows ownership transfer (move); it does not implement `Copy`. |
| `game_time` | Instance method | `game_time(&self) -> u64` | Takes `&self` (borrowing). Returns `utc.now_ms - game_started` as a `u64` whole number of milliseconds elapsed since construction. Uses the same `utc.now_ms` global platform primitive as `new`. |
| `increment_score` | Instance method | `increment_score(&self, amount: u32) -> GameState` | Takes `&self` (borrowing). Returns a new `GameState` with `score` increased by `amount`. If `score + amount` would exceed `2,000,000,000`, saturates `score` at `2,000,000,000`. All other fields are unchanged. `amount` must be greater than 0; passing `amount = 0` is a caller contract violation and the implementation may panic or debug-assert; it is not a recoverable error. |
| `food` | Instance getter | `food(&self) -> Option<Food>` | Takes `&self` (borrowing) and returns a clone of `food_placement` as `Option<Food>`. `Food` implements `Clone`, so this does not consume the state. |
| `game_started` | Instance getter | `game_started(&self) -> u64` | Takes `&self` (borrowing). Returns the `game_started` field (the recorded start timestamp in milliseconds) as `u64`. The getter name intentionally matches the field name; this is a deliberate design choice. |