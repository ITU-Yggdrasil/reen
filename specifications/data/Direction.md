## Direction

1. **Description**
   A value type representing movement direction for the Snake. An enum with four different values:
   - UP
   - DOWN
   - LEFT
   - RIGHT

2. **Type Kind**
   Enum

3. **Mutability**
   Immutable

4. **Properties**
   - None (properties are inherent to the enum variant)

5. **Functionalities**
   - **is_opposite** -> takes another direction and returns true if self and other are opposites

6. **Constraints & Rules**
   - Opposites: `UP <-> DOWN`, `LEFT <-> RIGHT`

**Inferred Types or Structures**

No inferences were made.

**Blocking Ambiguities**

No blocking ambiguities were identified.

**Implementation Choices Left Open**

No non-blocking implementation choices were identified.

## Validation Checklist
- [X] All properties and functions originate in the draft
- [X] Constructor policy applied correctly (Enum does not get `new`)
- [X] Trailing-underscore fields include auto-implemented getters with one underscore removed (for example `field_` -> `field`)
- [X] No new fields, variants, or rules were added
- [X] Names exactly match the draft
- [X] Referenced items in dependency context were resolved before marking unspecified/blocking
- [X] All inferred structures are explicitly documented as inferred
- [X] Blocking ambiguities are truly behavior-impacting or contradictory
- [X] Non-blocking technical details are captured under **Implementation Choices Left Open**