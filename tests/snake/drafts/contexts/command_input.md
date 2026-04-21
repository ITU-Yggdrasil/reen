# Context: CommandInput

## Purpose

CommandInput defines how key presses are collected and read for the whole application session. Input is handled as one shared first-in-first-out stream — the same stream is used for the start menu and for gameplay, without resetting or replacing it.

## Role Players

- stdin_source: keyboard input — supplies key presses from standard input using non-blocking reads

## Props

- buffer: a queue of captured key presses, shared for the whole application session

## Role Methods

### stdin_source
- read_available: returns all currently available key presses in arrival order without waiting

## Functionalities

### new
Initialises the buffer as an empty queue and stores the stdin source and buffer as collaborators. One shared buffer is used for the whole application session; menus and gameplay read from the same queue.

### capture
Asks the stdin source for all currently available key presses without waiting, then appends each to the end of the buffer in arrival order. If no key presses are available, the buffer is unchanged. Keys already in the buffer before this call remain in place; new keys follow them.

### next_key
If the buffer is non-empty, removes and returns the oldest key event. If the buffer is empty, returns no key.

### next_action
Removes the oldest key from the buffer and maps it to a gameplay action using case-insensitive matching: w maps to move up, a maps to move left, s maps to move down, d maps to move right, and space maps to fire. If the key has no gameplay meaning, the process repeats until a valid key is found or the buffer is exhausted. If no valid action is found, returns no action.

## Scenarios

### capture appends in arrival order
Given: key presses are waiting in standard input
When: capture is called
Then: those key presses are appended to the shared buffer in arrival order

### next_key returns oldest
Given: the shared buffer starts with "q"
When: next_key is called
Then: "q" is returned and removed from the buffer

### next_action skips invalid keys
Given: the shared buffer starts with "x", then "d"
When: next_action is called
Then: the invalid key is discarded and a movement-right action is returned

### next_action on empty buffer
Given: the shared buffer is empty
When: next_action is called
Then: no action is returned
