### GameState

1. **Description**

   Holds the current score, the placement of food, and a representation of how long the game has been active.

2. **Type Kind**

   Struct

3. **Mutability**

   Immutable

4. **Properties**

   - **score_** - An integer >= 0 representing the current score.
   - **food_placement** - `Option<Food>`; `None` if no food is available, `Some(Food)` if there's food on the board.
   - **game_started** - An integer representing the start time of the game in signed milliseconds from the UTC baseline `2026-01-01T00:00:00Z`, calculated as `game_started = start_utc_ms - baseline_utc_ms`.

5. **Functionalities**

   - **new** - Initialises the game state by setting `score` to 0 and `game_started = utc.now_ms - baseline_utc_ms`, where `baseline_utc_ms` is `2026-01-01T00:00:00Z`; `food_placement` is set to `None`.
   - **place_food** - Gets a `Food` object (f) and creates a new `GameState` object with `food_placement` updated to `Some(f)`.
   - **game_time** - Returns the game duration in milliseconds: `utc.now - game_started`.
   - **increment_score** - Gets a positive integer and returns a new `GameState` with the score incremented by the argument value.
   - **food** - Returns `food_placement`.

6. **Constraints & Rules**

   - The `game_started` field is calculated based on the `baseline_utc_ms` which is `2026-01-01T00:00:00Z`.

**Inferred Types or Structures (Non-Blocking)**

- **game_time** returns `i64` for the game duration in milliseconds.

**Blocking Ambiguities**

- None

**Implementation Choices Left Open**

- The `baseline_utc_ms` value is set to `2026-01-01T00:00:00Z`.
- The exact implementation of `utc.now_ms` is left to the developer's choice, but it should return an `i64` representing the current time in milliseconds since the baseline.
- The `Food` type is inferred to be a struct with a `position` field, but the exact implementation details are left to the implementation.

### Constructor Policy

- `new` is explicitly listed under Functionalities, so no additional constructor is needed.

### Auto-Implemented Getter Rule

- Getter for `score_` is `score`.
- Getter for `game_started` is `game_started`.

### Validation Checklist

- [X] All properties and functions originate in the draft.
- [X] Constructor policy applied correctly (Struct gets `new` when missing; Enum does not unless explicit).
- [X] Trailing-underscore fields include auto-implemented getters with one underscore removed (for example `score_` -> `score`).
- [X] No new fields, variants, or rules were added.
- [X] Names exactly match the draft.
- [X] Referenced items in dependency context were resolved before adding any **Blocking Ambiguities** entry.
- [X] All inferred structures are explicitly documented as inferred.
- [X] Blocking ambiguities are truly behavior-impacting or contradictory.
- [X] Non-blocking technical details are captured under **Implementation Choices Left Open**.