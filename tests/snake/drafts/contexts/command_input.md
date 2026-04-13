# CommandInputContext

## Purpose

CommandInputContext defines how key presses are collected and read for the whole
application session.

The important behavior is that input is handled as one shared first-in-first-out
stream. The same stream is used for the start menu and for gameplay, without
resetting or replacing it.

## Role Players

| Role player | Why involved | Expected behaviour |
|---|---|---|
| stdin_source | Supplies keyboard input | Supports non-blocking reads from standard input |

## Role Methods

### stdin_source

- **read_available**
  Returns all currently available key presses in arrival order without waiting.

## Props

| Prop | Meaning | Notes |
|---|---|---|
| buffer | Queue of captured key presses | Shared for the whole application session |

## Functionalities

### new

| Started by | Uses | Result |
|---|---|---|
| application startup | stdin_source, buffer | empty buffer, ready to capture |

Rules:
- Starts with an empty input buffer.
- One shared input stream is used for menus and gameplay.

| Given | When | Then |
|---|---|---|
| the application starts | new is called | a command input context with an empty shared buffer is created |

### capture

| Started by | Uses | Result |
|---|---|---|
| game loop or menu | stdin_source, buffer | available keys appended |

Rules:
- Reads currently available key presses without waiting.
- Appends captured keys to the end of the buffer in arrival order.
- Does not remove keys that were already buffered.

| Given | When | Then |
|---|---|---|
| key presses are waiting in standard input | capture is called | those key presses are appended to the shared buffer in arrival order |

### next_key

| Started by | Uses | Result |
|---|---|---|
| caller that needs raw keys | buffer | oldest key removed and returned, or no key |

Rules:
- Returns and removes the oldest buffered key event when the buffer is non-empty.
- Returns no key when the buffer is empty.

| Given | When | Then |
|---|---|---|
| the shared buffer starts with `q` | next_key is called | `q` is returned and removed from the buffer |

### next_action

| Started by | Uses | Result |
|---|---|---|
| gameplay logic | buffer | next gameplay action, or no action |

Rules:
- Reads from the same first-in-first-out stream as `next_key`.
- Mapping is case-insensitive for letter keys.
- `w` means move up.
- `a` means move left.
- `s` means move down.
- `d` means move right.
- Space means fire.
- Keys with no gameplay meaning are skipped and consumed while scanning for the
  next action.
- Returns the first valid gameplay action found, or no action if none is
  available.

| Given | When | Then |
|---|---|---|
| the shared buffer starts with `x`, then `d` | next_action is called | the invalid key is discarded and a movement-right action is returned |

## Notes

- `next_action` returns user actions. The game loop decides how a movement
  action changes direction and how fire is handled.
