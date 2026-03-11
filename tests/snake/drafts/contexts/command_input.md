# CommandInputContext

## Description

CommandInputContext defines how key presses are collected and read.
It supports both:
- menu/application controls (`s`/`q`), and
- gameplay controls (`w`/`a`/`s`/`d`, space).

The important behavior is that input is handled as one shared first-in-first-out (FIFO) stream for the whole application session.
This means the same input stream is used for the start menu and for gameplay, without resetting or replacing it.

---

## Roles

- **stdin_source**
  Provides non-blocking reads from standard input.

---

## Props

- **buffer**
  FIFO queue of captured keystrokes.

---

## Role methods

### stdin_source

- **read_available**
  Returns all currently available keystrokes in arrival order without blocking.

---

## Behavior

- **new()**
  - Starts with an empty input buffer (no keys captured yet).

- **capture()**
  - Reads currently available key presses (without waiting).
  - Adds them to the end of the buffer in the order they arrived.
  - Does not remove keys that were already buffered.

- **next_key() -> Option<char>**
  - If the buffer is non-empty, returns and removes the oldest key.
  - If the buffer is empty, returns `None`.

- **next_action() -> Option<UserAction>**
  - Reads from the same FIFO stream used by `next_key()`.
  - Mapping is case-insensitive:
    - `w` -> `Movement(UP)`
    - `a` -> `Movement(LEFT)`
    - `s` -> `Movement(DOWN)`
    - `d` -> `Movement(RIGHT)`
    - space -> `Fire`
  - Non-action keys are ignored and consumed.
  - Returns the first valid action found, or `None` if no action key is available.

---

## Acceptance examples

- Given an empty buffer, when `next_key()` is called, then the result is `None`.
- Given captured keys `x`, `w`, when `next_action()` is called, then the result is `Movement(UP)` and both keys are consumed.
- Given captured keys `a`, `d`, when `next_action()` is called twice, then results are `Movement(LEFT)` then `Movement(RIGHT)`.
