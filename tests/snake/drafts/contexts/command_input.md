# CommandInputContext

## Description

CommandInputContext encapsulates keyboard input capture for the application.
It owns a FIFO buffer of keystrokes and provides read operations for both:
- menu/application controls (`s`/`q`), and
- gameplay steering (`w`/`a`/`s`/`d`).

The same CommandInputContext instance is created by the application and shared with GameLoopContext, so both use one consistent input stream.

---

## Roles

- **stdin_source**
  Reads keyboard input from standard input.

---

## Props

- **buffer**
  FIFO queue of captured keystrokes.

---

## Role methods

### stdin_source

- **read_available**
  Reads all currently available keystrokes from stdin (non-blocking) and returns them in arrival order.

---

## Functionality

- **new() -> CommandInputContext**
  Creates a new input context with an empty buffer.

- **capture() -> CommandInputContext**
  Calls `stdin_source.read_available` and appends returned keystrokes to `buffer` in FIFO order.
  Returns a new updated context.

- **next_key() -> (Option<char>, CommandInputContext)**
  Pops and returns the next key from `buffer` if available; otherwise returns `None`.
  Returns the updated context.

- **next_direction() -> (Option<Direction>, CommandInputContext)**
  Pops keys from `buffer` until:
  - a movement key is found (`W`/`A`/`S`/`D`, case-insensitive), 
    - if `W` return `Some(UP)`
    - if `A` return `Some(LEFT)`
    - if `S` return `Some(DOWN)`
    - if `D` return `Some(RIGHT)`
  - or if the buffer becomes empty before a movement key is found, return `None`.
