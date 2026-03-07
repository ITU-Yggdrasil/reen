## Context: CommandInputContext

### Description

CommandInputContext encapsulates keyboard input capture for the application. It owns a FIFO buffer of keystrokes and provides read operations for both:
- menu/application controls (`s`/`q`), and
- gameplay steering (`w`/`a`/`s`/`d`).

The same CommandInputContext instance is created by the application and shared with GameLoopContext, so both use one consistent input stream.

### Roles

- **stdin_source**
  Reads keyboard input from standard input.

### Props

- **buffer**
  FIFO queue of captured keystrokes.

### Role methods

#### stdin_source

- **read_available**
  Reads all currently available keystrokes from stdin (non-blocking) and returns them in arrival order.

### Functionality

- **new() -> CommandInputContext**
  Creates a new input context with an empty buffer.

- **capture() -> CommandInputContext**
  Calls `stdin_source.read_available` and appends returned keystrokes to `buffer` in FIFO order.
  Returns a new updated context.

- **next_key() -> (Option<char>, CommandInputContext)**
  Pops and returns the next key from `buffer` if available; otherwise returns `None`.
  Returns the updated context.

- **next_action() -> Option<UserAction>**
  Pops keys from `buffer` until:
  - a movement key is found (`W`/`A`/`S`/`D`, case-insensitive), 
    - if `W` return `Some(Movemment(UP))`
    - if `A` return `Some(Movemment(LEFT))`
    - if `S` return `Some(Movemment(DOWN))`
    - if `D` return `Some(Movemment(RIGHT))`
  - or the fire key is found ` ` (space) in which case Some(Fire) is returned
  - or if the buffer becomes empty before a movement key is found, return `None`.

### Inferred Types or Structures (Non-Blocking)

- **UserAction**: 
  - Location in the specification: UserAction.md
  - Inference made: Enum with values `Movement` and `Fire`.
  - Basis for the inference: The UserAction enum is referenced in the next_action method.

### Blocking Ambiguities

- None identified. All references and inferences align with the provided context.

### Implementation Choices Left Open

- **Exact collection/sequence type for buffer** (`Vec` vs alternatives)
  - This is a non-blocking technical choice that does not impact observable behavior.

- **Parameter names in role methods** (e.g., `read_available`)
  - These are implementation details that do not affect the observable behavior.

## Dependencies

- **stdin_source**
  - The implementation of `read_available` is assumed to follow standard input handling practices.

- **UserAction**
  - The definition of `Movement` and `Fire` is assumed to match the provided description.

- **Direction**
  - The definition of `UP`, `DOWN`, `LEFT`, and `RIGHT` is assumed to match the provided description.

- **Position**
  - The definition of `x_` and `y_` is assumed to match the provided description.

- **Snake**
  - The definition of `body_` and `direction_` is assumed to match the provided description.

- **Board**
  - The definition of `width` and `height` is assumed to match the provided description.

These dependencies are treated as authoritative context metadata and are resolved from the provided direct dependency context.