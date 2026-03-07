## UserAction

### Description

An enum that communicates user actions. An action can either be Movement(Direction) or Fire.

### Type Kind

Enum

### Mutability

Immutable

### Properties

- **Movement(Direction)** 
- **Fire**

### Functionalities

- **Movement(Direction)** -> A function that takes a Direction and returns a Movement action.
- **Fire** -> A function that returns a Fire action.

### Constraints & Rules

- The `Movement` variant must take a `Direction` as input.
- The `Fire` variant must not take any input.

### Inferred Types or Structures (Non-Blocking)

- **Movement(Direction)**: Inferred from the `Direction` type, which is defined as an enum with variants `UP`, `DOWN`, `LEFT`, and `RIGHT`.
- **Fire**: No inner structure is inferred.

### Blocking Ambiguities

- None.

### Implementation Choices Left Open

- The exact variant names and their associated functions are left to implementation decisions, as they are directly defined in the draft.

---

### Examples

- `UserAction::Movement(Direction::UP)`
- `UserAction::Fire`

These examples use the `Direction` type defined in the `Direction` data type specification.