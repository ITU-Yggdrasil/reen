# Reen Draft Format Reference

Three draft kinds. No types, no signatures, no Rust tokens. DCI terms only, expressed in domain language.

## Context (`drafts/contexts/<name>.md`)

```
# Context: <Name>

## Purpose
One paragraph: what scenario this context enacts, in business terms.

## Role Players
- <role>: <data or projection name> — one-line responsibility

## Props
- <prop>: <description> (scalar configuration only; never a collaborator)

## Role Methods
### <role>
- <method name>: <what this role does in this enactment>

## Functionalities
### <functionality name>
Prose describing which roles do what, in what order, referencing role methods by name.

## Scenarios
### <scenario name>
Given: <role players in specific states>
When: <functionality invocation with concrete values>
Then: <observable outcome>
```

**Annotated example** — from the money-transfer project (`contexts/money_transfer.md`):

- **Role Players** lists `source: Account` and `sink: Account`. Both are the same type playing different roles.
- **Role Methods** has a subsection per role. The source's `withdraw` is described as a business action ("creates an unsettled ledger entry"), not as a function signature.
- **Functionalities** describes `transfer` as a composition of role methods in order: "the source's withdraw role method, then the sink's deposit role method, then committing the entry."
- **Scenarios** use concrete values: "account 123456 has balance 1000.00 DKK, amount is 250.00 DKK" — not "some account with sufficient funds."

## Data (`drafts/data/<name>.md`)

```
# Data: <Name>

## Purpose
One paragraph: what domain concept this represents.

## Attributes
- <name>: <prose description>

## Invariants
- <prose invariant>

## Operations
### <operation name>
Prose describing what this operation does as a business rule.

## Scenarios
### <scenario name>
Given: <data value>
When: <operation invoked>
Then: <resulting value>
```

**Annotated example** — from the money-transfer project (`data/amount.md`):

- **Attributes** lists `value` and `currency` with prose descriptions. No types — "the amount in minor units (hundredths of the currency unit)" tells the implementer what the value means without prescribing a numeric type.
- **Invariants** states "Two Amounts can only be combined when their currencies match." This is enforced in the addition and subtraction operations.
- **Scenarios** use concrete numbers: "an Amount of 1050 in DKK" → "the result is 10.50." Five scenarios cover success paths, error paths, and edge cases.

## Projection (`drafts/projections/<name>.md`)

```
# Projection: <Name>

## Purpose
One paragraph: what view this provides over underlying data.

## Source
- <data name or description>

## Queries
### <query name>
Prose describing what is observable through this projection.

## Scenarios
### <scenario name>
Given: <underlying data state>
When: <query invoked>
Then: <returned view>
```

Projections are read-only. `reen prepare` rejects mutation-shaped prose.

**Annotated example** — from the money-transfer project (`projections/Account.md`):

- **Source** is "Ledger, filtered by account identifier" — the projection views Ledger entries for one account.
- **Queries** include `balance`, `get_currency`, `transactions`. Each describes what you can observe, not how it's computed internally.
- **Scenarios** test construction (account exists), rejection (no entries), balance computation, and ordering.

## Common Pitfalls

**Mixing role and role player.** "Account withdraws the amount" is wrong. "The source withdraws the amount" is correct. The role does the action; Account is just the data playing that role.

**Sneaking types into drafts.** No Rust types, no generic brackets, no path syntax. Write "a list of positions" instead of `Vec<Position>`. Write "a whole number" instead of `u32`. The code-token lint blocks these mechanically.

**Scenarios that restate the flow.** A scenario should enact a specific case with concrete values, not paraphrase the functionality prose. "Given: a valid transfer — When: transfer is called — Then: it succeeds" tells you nothing. Use real account numbers, balances, and amounts.

**Role methods that are getters.** "read_score: returns the current score" is a getter on the data, not a role responsibility. A role method should describe what the role *does* in the context, not what the data exposes. If the context just needs to read a field, that's an implementation detail — the role method should describe the business reason for reading it.

**Putting role methods on data.** If a behaviour only makes sense in a specific scenario, it is a role method on the context, not an operation on the data type. Operations are intrinsic to the data; role methods are assigned by contexts.
