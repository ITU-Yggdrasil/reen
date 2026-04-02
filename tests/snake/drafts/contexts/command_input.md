# CommandInputContext

## Purpose

CommandInputContext defines how key presses are collected and read.
It supports both:
- menu/application controls (`s`/`q`), and
- gameplay controls (`w`/`a`/`s`/`d`, space).

The important behavior is that input is handled as one shared first-in-first-out (FIFO) stream for the whole application session.
This means the same input stream is used for the start menu and for gameplay, without resetting or replacing it.

---

## Role Players

| Role player | Why involved | Expected behaviour |
|---|---|---|
| stdin_source | Supplies keyboard input to the context | Provides non-blocking reads from standard input |

---

## Props

| Prop | Meaning | Notes |
|---|---|---|
| buffer | FIFO queue of captured keystrokes | Shared for the whole application session |

---

## Role Methods

### stdin_source

- **read_available**
  Returns all currently available keystrokes in arrival order without blocking.

---

## Functionalities

### new

| Started by | Uses | Result |
|---|---|---|
| application startup | stdin_source, buffer | command input context is created |

Rules:
- Starts with an empty input buffer.
- Uses one shared input stream for menus and gameplay.

| Given | When | Then |
|---|---|---|
| no keys have been captured yet | new is called | the buffer starts empty |

### capture

| Started by | Uses | Result |
|---|---|---|
| game loop or menu | stdin_source, buffer | available keys are appended to the buffer |

Rules:
- Reads currently available key presses without waiting.
- Adds captured keys to the end of the buffer in arrival order.
- Does not remove keys that were already buffered.

| Given | When | Then |
|---|---|---|
| two keys arrive while one is already buffered | capture runs | the new keys are appended after the existing key |

### next_key

| Started by | Uses | Result |
|---|---|---|
| caller that needs raw input | buffer | oldest buffered key is returned or `None` |

Rules:
- If the buffer is non-empty, returns and removes the oldest key.
- If the buffer is empty, returns `None`.

| Given | When | Then |
|---|---|---|
| an empty buffer | next_key is called | the result is `None` |

### next_action

| Started by | Uses | Result |
|---|---|---|
| gameplay logic | buffer | next valid UserAction is returned or `None` |

Rules:
- Reads from the same FIFO stream used by `next_key`.
- Mapping is case-insensitive.
- `w` maps to `Movement(Up)`.
- `a` maps to `Movement(Left)`.
- `s` maps to `Movement(Down)`.
- `d` maps to `Movement(Right)`.
- space maps to `Fire`.
- Non-action keys are ignored and consumed.
- Returns the first valid action found, or `None` if no action key is available.

| Given | When | Then |
|---|---|---|
| captured keys `x`, `w` | next_action is called | the result is `Movement(Up)` and both keys are consumed |
