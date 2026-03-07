## Board

1. **Description**
   - A rectangular playfield that defines valid positions and acts as the primary obstacle boundary (walls/bounds) for Snake.

2. **Type Kind**
   - Struct

3. **Mutability**
   - Immutable

4. **Properties**
   - **width** - An integer representing the width of the board.
   - **height** - An integer representing the height of the board.

5. **Functionalities**
   - None explicitly listed.

6. **Constraints & Rules**
   - `width > 0` and `height > 0`

**Inferred Types or Structures**
- **None**

**Blocking Ambiguities**
- **None**

**Implementation Choices Left Open**
- **Exact collection/sequence type for fields**: `Vec` or an alternative sequence type is left open.
- **Serialization format**: The format for serialization (e.g., JSON, custom format) is left open.

## Examples

```rust
let board = Board { width: 10, height: 10 };
```

**Validation Checklist**
- [X] All properties and functions originate in the draft.
- [X] Constructor policy applied correctly.
- [X] Trailing-underscore fields include auto-implemented getters with one underscore removed.
- [X] No new fields, variants, or rules were added.
- [X] Names exactly match the draft.
- [X] Referenced items in dependency context were resolved before marking unspecified/blocking.
- [X] All inferred structures are explicitly documented as inferred.
- [X] Blocking ambiguities are truly behavior-impacting or contradictory.
- [X] Non-blocking technical details are captured under **Implementation Choices Left Open**.
- [X] A reviewer could trace every statement back to the draft.