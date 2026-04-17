# Processor App

## Application Kind

`cli_app`

## Description

Simple processor application.

## Collaborators and Wiring

| Collaborator | Responsibility |
|---|---|
| `Token` | Holds a text token value. |
| `ProcessorContext` | Processes input data. |

## Startup Sequence

- Let `token` be `Token::new("start")`.
- Let `reader` be `std::io::stdin()`.
- Let `processor` be `ProcessorContext::new(reader, token, 0, "main")`.

## Main Flow

- Let `count` be `processor.process()`.
- Call `println!("{}", count)`.
