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
