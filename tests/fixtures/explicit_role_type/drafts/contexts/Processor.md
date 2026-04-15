# ProcessorContext

## Purpose

Processes a stream of input data through a reader role player.

## Role Players

| Role player | Why involved | Expected behaviour | Type |
|---|---|---|---|
| reader | Provides input bytes | Reads raw data from an external source | `std::io::Stdin` |
| counter | Tracks how many items processed | Incremented after each item | integer |
| label | Display name for the processor | Shown in output | string |
| token | The current token being processed | Holds the active token value | |

## Role Methods

### reader

- **read_line**: Signature: `read_line(&self) -> std::io::Result<usize>`

### counter

- **increment**: Signature: `increment(&self) -> i64`

### label

- **display**: Signature: `display(&self) -> &String`

### token

- **value**: Signature: `value(&self) -> &String`

## Props

| Prop | Meaning | Notes |
|---|---|---|

## Functionalities

### new

| Started by | Uses | Result |
|---|---|---|
| app startup | reader, counter, label, token | a processor context is created |

**Flow:**
1. Signature: `new(reader: std::io::Stdin, counter: i64, label: String, token: Token) -> Self`

| Given | When | Then |
|---|---|---|
| all role players exist | new is called | a processor context is created |

### process

| Started by | Uses | Result |
|---|---|---|
| main loop | reader, counter | processed item count |

**Flow:**
1. Signature: `process(&self) -> i64`
2. Return `counter.increment()`.

| Given | When | Then |
|---|---|---|
| reader has data | process is called | an item is processed and count returned |
