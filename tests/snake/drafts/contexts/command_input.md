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
| stdin_source | Supplies keyboard input from stdin | Supports non-blocking reads from standard input |

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

**Flow:**
1. Initialise `buffer` as an empty queue.
2. Store `stdin_source` and `buffer` as collaborators.

**Guarantee:** One shared buffer is used for the whole application session; menus and gameplay read from the same queue.

| Given | When | Then |
|---|---|---|
| the application starts | new is called | a command input context with an empty shared buffer is created |

### capture

| Started by | Uses | Result |
|---|---|---|
| game loop or menu | stdin_source, buffer | available keys appended |

**Flow:**
1. Ask `stdin_source` for all currently available key presses without waiting.
2. Append each returned key press to the end of `buffer` in arrival order.

**Extensions:**
- 1a. No key presses are available → flow ends; `buffer` is unchanged.

**Guarantee:** Keys already in `buffer` before this call remain in place; new keys follow them.

| Given | When | Then |
|---|---|---|
| key presses are waiting in standard input | capture is called | those key presses are appended to the shared buffer in arrival order |

### next_key

| Started by | Uses | Result |
|---|---|---|
| caller that needs raw keys | buffer | oldest key removed and returned, or no key |

**Flow:**
1. If `buffer` is non-empty, remove and return the oldest key event.
2. If `buffer` is empty, return no key.

| Given | When | Then |
|---|---|---|
| the shared buffer starts with `q` | next_key is called | `q` is returned and removed from the buffer |

### next_action

| Started by | Uses | Result |
|---|---|---|
| gameplay logic | buffer | next gameplay action, or no action |

**Flow:**
1. Remove the oldest key from `buffer` via the same queue as `next_key`.
2. Map the key to a gameplay action using case-insensitive matching: `w` → move up, `a` → move left, `s` → move down, `d` → move right, space → fire.
3. If the key maps to an action, return it.
4. If the key has no gameplay meaning, discard it and repeat from step 1.
5. If `buffer` is exhausted with no valid action found, return no action.

| Given | When | Then |
|---|---|---|
| the shared buffer starts with `x`, then `d` | next_action is called | the invalid key is discarded and a movement-right action is returned |

## Notes

- `next_action` returns user actions. The game loop decides how a movement
  action changes direction and how fire is handled.
