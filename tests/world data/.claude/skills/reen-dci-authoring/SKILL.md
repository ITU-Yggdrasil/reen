---
name: reen-dci-authoring
description: BA-facing DCI thinking for authoring Reen drafts
triggers:
  - glob: "drafts/**/*.md"
  - keyword: "reen draft"
  - keyword: "DCI"
  - keyword: "scenario"
---

# DCI Authoring for Reen

When helping a BA author or edit a draft, think in DCI vocabulary.

## The Three Artifact Kinds

- **Data** — a domain concept with attributes, invariants, and operations. Example: `Amount`, `Account`, `Board`.
- **Projection** — a read-only view over underlying data. Example: `AccountBalance`. Projections never mutate.
- **Context** — a scenario enactment. Contexts bind role players (data/projections) to roles, define role methods as per-role responsibilities, compose them into functionalities, and specify scenarios with concrete Given/When/Then.

## Key Distinctions

- **Role vs Role Player**: A role is a responsibility in a context ("source"). A role player is the data type that fills that role ("Account"). The same data type can play different roles in different contexts.
- **Role methods**: Per-role responsibilities. They live on the context, not the data type. Written as what the role *does*, not how it is implemented. Example: "withdraw: removes the transfer amount from the source balance."
- **Functionalities**: Named use cases that compose role methods. The prose describes which roles do what in what order.
- **Scenarios**: Concrete enactments with Given/When/Then. They are the primary source of test cases.

## Manifest as Shared Vocabulary

Every data and projection name must be in the vocabulary manifest. If `reen prepare` flags an unresolved name, either add it to `drafts/vocabulary.yml` or create the corresponding data/projection draft.

## Mini-Example

A money transfer context binds two Account role players (source, sink), defines role methods withdraw and deposit on each role, composes them into a "transfer" functionality, and specifies a scenario: "Given source has balance 100, sink has 0, amount is 30 — When transfer — Then source has 70, sink has 30."
