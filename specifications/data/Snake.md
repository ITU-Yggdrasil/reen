### Snake

1. **Description**
   Represents the main character of the snake game.

2. **Type Kind**  
   Struct

3. **Mutability**  
   Immutable

4. **Properties**
   - **body_** A list of position objects. The head being the first element in the list
   - **direction_** The direction the snake is travelling

5. **Functionalities**
   - **new** -> constructs the snake from a list of body positions and a direction

6. **Constraints & Rules**
   - body length > 0
   - All positions in `body` are unique (no self-overlap).

**Inferred Types or Structures (Non-Blocking)**

- **body_**: List of `Position` objects
- **direction_**: `Direction` object

**Blocking Ambiguities**

- None

**Implementation Choices Left Open**

- The exact collection type for `body_` (e.g., `Vec<Position>` vs. `LinkedList<Position>`) is left open.
- The `new` constructor inputs correspond to all explicitly listed properties/fields.

### Examples

- `Snake::new(vec![Position { x_: 0, y_: 0 }, Position { x_: 1, y_: 0 }], Direction::RIGHT)`

### Validation Checklist

- [X] All properties and functions originate in the draft
- [X] Constructor policy applied correctly (Struct gets `new` when missing)
- [X] Trailing-underscore fields include auto-implemented getters with one underscore removed (for example `body_` -> `body`)
- [X] No new fields, variants, or rules were added
- [X] Names exactly match the draft
- [X] Referenced items in dependency context were resolved before marking unspecified/blocking
- [X] All inferred structures are explicitly documented as inferred
- [X] Blocking ambiguities are truly behavior-impacting or contradictory
- [X] Non-blocking technical details are captured under **Implementation Choices Left Open**
- [X] A reviewer could trace every statement back to the draft

### Direct Dependency Context (Authoritative, Optional)

- The `Position` type is derived from the `Position` draft.
- The `Direction` type is derived from the `Direction` draft.
- The `Board` context is not directly relevant to the `Snake` type's definition.