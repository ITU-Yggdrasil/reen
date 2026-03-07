## Food

1. **Description**
   A single consumable item on the Board that the Snake can eat to score and grow.

2. **Type Kind**
   Struct

3. **Mutability**
   Immutable

4. **Properties**
   - **position** describes the position on the board

5. **Functionalities**
   - **get_position** returns the position

6. **Constraints & Rules**
   - None explicitly stated

### Inferred Types or Structures (Non-Blocking)
- **position**: Inferred as `Position` based on the draft's reference.

### Blocking Ambiguities
- None

### Implementation Choices Left Open
- None

## Examples
- `Food { position: Position { x_: 5, y_: 5 } }`

## Validation Checklist
- [X] All properties and functions originate in the draft
- [X] Constructor policy applied correctly (Struct gets `new` when missing)
- [X] Trailing-underscore fields include auto-implemented getters with one underscore removed
- [X] No new fields, variants, or rules were added
- [X] Names exactly match the draft
- [X] Direct dependency context resolved `position` as `Position`
- [X] Blocking ambiguities are truly behavior-impacting or contradictory
- [X] Non-blocking technical details are captured under **Implementation Choices Left Open**