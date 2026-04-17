# ProcessorContext

## Purpose

Processes a stream of input data through a reader role player.

## Role Players

| Role player | Why involved | Expected behaviour | Type |
|---|---|---|---|
| reader | Provides input bytes | Reads raw data from an external source | `std::io::Stdin` |
| token | The current token being processed | Holds the active token value | |

## Role Methods

### reader

- **read_line**: Signature: `read_line(&self) -> std::io::Result<usize>`

### token

- **value**: Signature: `value(&self) -> &String`

## Props

| Prop | Meaning | Notes |
|---|---|---|
| counter | Tracks how many items processed | Type is `i64` |
| label | Display name for the processor | Type is `String` |

## Functionalities

### new

| Started by | Uses | Result |
|---|---|---|
| app startup | reader, token, counter, label | a processor context is created |

**Flow:**
1. Signature: `new(reader: std::io::Stdin, token: Token, counter: i64, label: String) -> Self`

| Given | When | Then |
|---|---|---|
| all role players exist | new is called | a processor context is created |

### process

| Started by | Uses | Result |
|---|---|---|
| main loop | reader, counter | processed item count |

**Flow:**
1. Signature: `process(&self) -> i64`
2. Return `counter`.

| Given | When | Then |
|---|---|---|
| reader has data | process is called | an item is processed and count returned |
