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
  Creates a new input context with an empty buffer. The stdin_source role is implicitly bound to the process's standard input.

- **capture() -> CommandInputContext**
  Calls `stdin_source.read_available` and appends returned keystrokes to `buffer` in FIFO order.
  Returns a new updated context.

- **next_key() -> Option<char>**
  Pops and returns the next key from `buffer` if available; otherwise returns `None`.

- **next_action() -> Option<UserAction>**
  Calls next_key
    - if None returns None.
    - if a movement key is found (Some(`W`)/Some(`A`)/Some(`S`)/Some(`D`), case-insensitive), 
      - if `W` return `Some(Movement(UP))`
      - if `A` return `Some(Movement(LEFT))`
      - if `S` return `Some(Movement(DOWN))`
      - if `D` return `Some(Movement(RIGHT))`
    - if the fire key is found ` ` (space) in which case Some(Fire) is returned
    - if Some(c) is returned that is nonne of the above then call next_action recursively
  - or if the buffer becomes empty before a movement key is found, return `None`.
