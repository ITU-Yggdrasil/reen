---
name: reen-draft-format
description: Draft grammar, section templates, and common pitfalls for Reen drafts
triggers:
  - glob: "drafts/**/*.md"
  - keyword: "reen draft"
  - keyword: "draft format"
---

# Reen Draft Format

Three draft kinds, each with a fixed section structure. No types, no signatures, no Rust tokens.

## Context Draft (`drafts/contexts/<name>.md`)

```
# Context: <Name>
## Purpose          — one paragraph, business terms
## Role Players     — bullet list: `- <role>: <DataType> — <responsibility>`
## Props            — bullet list: `- <name>: <description>` (scalar config only)
## Role Methods     — ### subsections per role, each with `- <method>: <description>` bullets
## Functionalities  — ### subsections, each with prose describing the enactment
## Scenarios        — ### subsections, each with Given:/When:/Then: lines
```

## Data Draft (`drafts/data/<name>.md`)

```
# Data: <Name>
## Purpose          — one paragraph
## Attributes       — bullet list: `- <name>: <description>`
## Invariants       — bullet list of prose invariants
## Operations       — ### subsections with prose descriptions
## Scenarios        — ### subsections with Given:/When:/Then:
```

## Projection Draft (`drafts/projections/<name>.md`)

```
# Projection: <Name>
## Purpose          — one paragraph
## Source           — bullet list referencing underlying data
## Queries          — ### subsections with prose descriptions
## Scenarios        — ### subsections with Given:/When:/Then:
```

## Common Pitfalls

- **Sneaking types into drafts**: No `u32`, `String`, `Vec<>`. Types are inferred during prepare.
- **Mixing role and role player**: "Account withdraws" is wrong. "The source withdraws" is correct — the role does the action.
- **Putting role methods on data**: If a method only makes sense in a specific scenario, it's a role method on the context, not an operation on the data type.
- **Missing scenarios**: Every functionality and operation should have at least one concrete Given/When/Then scenario.
- **Mutation in projections**: Projections are read-only. The parser will reject mutation-shaped prose.

## What not to put in a draft

`reen prepare` enforces a **code-token lint** that blocks any draft containing implementation-level content. The following families are rejected:

1. **Signature lines** — lines beginning with `Signature:` or backtick-fenced `fn` declarations. Describe what the role method *does*, not its function shape.
2. **Rust path syntax** — `Direction::Up`, `std::io::Stdout`, `rand::rngs::ThreadRng`. Use the domain name ("up", "standard output", "random number generator") instead.
3. **Generic type brackets** — `Vec<Position>`, `Option<Food>`, `Result<()>`, `&[T]`. Describe the shape in prose ("a list of positions", "a food placement if one exists", "succeeds or returns an error").
4. **Reference and mutability sigils** — `&self`, `&mut T`, `Box<T>`, `Arc<T>`. These are ownership concerns, not business language.
5. **Turbofish** — `::<Type>`.
6. **Trait-bound syntax** — `impl Trait`, `dyn Trait`, `where T: Clone`.
7. **Rust primitive types** — `u32`, `i64`, `f64`, `bool`, `char`, `str`, `String`, `usize` etc. Say "a whole number", "a decimal number", "true or false", "a text value" instead.
8. **Method-call syntax** — `state.score()`, `.unwrap()`, `.clone()`. Describe the action ("ask the game state for the score") instead of writing the call.
9. **Macro invocations** — `println!()`, `format!()`, `todo!()`.

**Also rejected:** return-type arrows (`->`), numeric type suffixes (`10_u64`, `1.0_f64`), and cast syntax (`as u64`).

**What is fine:** domain names (`Amount`, `Ledger`, `Account`), concrete values (`250.00 DKK`, `account 123456`), operation names used in prose (`to_str is called`, `withdraw`), and standard English words (`string`, `character`).
