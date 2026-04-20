# DCI in Reen — A BA's Reference

DCI (Data, Context, Interaction) is the authoring model for Reen. This document covers how to think in DCI when writing drafts.

## The Three Artefact Kinds

### Data

A **data** artefact represents a domain concept — a thing that exists independently of any use case. Examples: `Account`, `Amount`, `Board`, `Snake`.

Data has:
- **Attributes** — the properties that describe it ("balance", "value", "width")
- **Invariants** — rules that must always hold ("an Amount is never negative")
- **Operations** — behaviours intrinsic to the data itself ("add two amounts")

Data does not know about contexts or roles. It is a standalone, reusable concept.

### Projection

A **projection** provides a read-only view over underlying data. Example: `AccountBalance` gives a view of an Account's current balance without exposing the full Account.

Projections have:
- **Source** — the data type(s) they project over
- **Queries** — what you can observe through the projection

Projections never mutate. If you find yourself describing changes, it belongs on the data type or in a context.

### Context

A **context** enacts a scenario. It is where the DCI design act happens — you decide which data plays which role, what each role does, and how roles interact.

Contexts have:
- **Role players** — data or projection types cast into roles
- **Props** — scalar configuration (an amount, a timeout)
- **Role methods** — what each role does in *this* scenario
- **Functionalities** — named use cases that compose role methods
- **Scenarios** — concrete Given/When/Then enactments

## Roles vs Role Players

This is the core DCI distinction.

A **role** is a responsibility in a context. "The source" is a role — it's the thing money comes from.

A **role player** is the data type that fills the role. "Account" is a role player — it's the actual data structure.

The same Account can be the "source" in one transfer and the "sink" in another. The role changes; the data stays the same.

When writing a draft, always say what the *role* does, not what the data type does: "the source withdraws the amount" rather than "the Account withdraws."

## Role Methods

Role methods are where DCI decomposes behaviour. They answer: "What does this role do in this scenario?"

Each role has its own subsection under `## Role Methods`:

```
## Role Methods
### source
- withdraw: removes the transfer amount from the source balance

### sink
- deposit: adds the transfer amount to the sink balance
```

Role methods:
- Are written in business language, not code
- Describe what the role *does*, not how it is implemented
- Live on the context, not on the data type
- Become private `<role>_<method>` functions in Rust (e.g., `source_withdraw`)

## Functionalities

A functionality is a named use case that composes role methods into a sequence:

```
## Functionalities
### transfer
The source withdraws the amount via its withdraw role method,
then the sink deposits the amount via its deposit role method.
```

Functionalities become public methods on the context struct.

## Scenarios as Enactments

Every functionality is specified by at least one scenario — a concrete enactment with specific values:

```
## Scenarios
### successful transfer
Given: source has balance 100, sink has balance 0, amount is 30
When: transfer is invoked
Then: source has balance 70, sink has balance 30
```

Scenarios are not just tests. They are the primary specification of what the functionality does. Tests are a consequence of scenarios.

## The Vocabulary Manifest

Every data and projection name used in a context draft must be in the vocabulary manifest. This ensures all contexts share a consistent vocabulary.

The manifest lives at `drafts/vocabulary.yml` and is also auto-discovered from draft file names under `drafts/data/` and `drafts/projections/`.

When `reen prepare` encounters a name not in the manifest, it raises a blocking ambiguity: "Role player `source` references `Account` which is not in the vocabulary manifest."

## Worked Example: Money Transfer

**Data drafts:**
- `drafts/data/Account.md` — attributes: balance; invariant: balance never negative; operations: deposit, withdraw
- `drafts/data/Amount.md` — attributes: value; invariant: never negative; operations: add

**Context draft:** `drafts/contexts/money_transfer.md`
- Purpose: enacts a transfer of money between two accounts
- Role players: source (Account), sink (Account)
- Props: amount (the quantity to transfer)
- Role methods: source.withdraw, sink.deposit
- Functionality: transfer (source withdraws, then sink deposits)
- Scenario: source 100, sink 0, amount 30 → source 70, sink 30
