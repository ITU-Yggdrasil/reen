# Hello App

## Application Kind

`cli_app`

## Description

Simple application that prints a prepared greeting.

## Collaborators and Wiring

| Collaborator | Responsibility |
|---|---|
| `Message` | Holds the printable text value. |
| `GreeterContext` | Returns the message text. |

## Startup Sequence

- Let `message` be `Message::new("Hello, world!")`.
- Let `greeter` be `GreeterContext::new(message)`.

## Main Flow

- Let `rendered` be `greeter.render()`.
- Call `println!("{}", rendered)`.

## Error Handling

- Normal exit code is `0`.
