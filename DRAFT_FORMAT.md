# Reen Draft Format Reference

Three draft kinds. No types, no signatures, no Rust tokens. DCI terms only, expressed in domain language.

## Drafts speak BA — non-negotiable

Drafts are BA artefacts. They read like a use case being narrated by a domain expert. Every context and every projection IS a use case; every data draft is a domain concept described in domain language.

A draft may name another data type by its capitalised name (e.g. `Money`, `Transaction`, `Account`) wherever the prose calls for it — in role-player declarations, attribute descriptions, scenario `Given:` / `When:` / `Then:` lines, operation prose, and invariants. That's a domain reference, not a type annotation.

What a draft must NEVER contain:

- Parameter lists, parameter notation, or anything that looks like a function signature. No `Parameters:`, no `Returns:`, no `(x, y) -> Z`, no method-call shape, no `&` / `&mut`, no generic brackets.
- Multiplicity notation: no `list of T`, no `[T]`, no `optional T`, no `Vec<T>`. Plural intent is conveyed by writing in the plural ("the transactions", "five transactions with amounts ...") and by the shape of scenario `Given:` clauses.
- Type-system vocabulary: no nullability markers, no enum syntax, no struct syntax, no inheritance language. If a value is one of an enumeration, write the enumeration in prose ("must be one of \"idle\", \"awaiting-input\", \"processing\", or \"confirming\"").
- Implementation language of any kind — no mention of Rust, Vec, String, Option, Result, traits, modules, or files.

What a draft MUST do:

- Reference data types by their domain name when prose calls for it. "The source's withdraw role method takes the amount" is correct.
- Describe operations and functionalities as business actions. "Combines two Money values with the same currency, producing a new Money whose amount is the sum" is correct — it tells the reader what the operation means without prescribing arity in formal notation.
- Use scenarios with concrete domain values to pin down behaviour. Scenarios are how the BA conveys arity, multiplicity, and result shape: "Given: a Money of 5000 in DKK and a Money of 2500 in DKK / When: add is performed / Then: the result is a Money of 7500 in DKK" is the BA's way of saying "binary, returns Money" without ever writing `(other: Money) -> Money`.

The Reen pipeline derives signatures, parameter counts, return types, and multiplicities from this prose at prepare time. Anything that looks like signature notation in a draft is a bug — surfaced by `reen refine` as a blocking finding and by the Phase 3.5 code-token lint where applicable.

If a draft cannot express something in BA prose, that is a signal the draft is wrong — not that the rule should be relaxed.

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

The Operations list is for **non-default** operations only — every Data type automatically gets a `new(...)` constructor and a getter per attribute, emitted by the scaffolder. Do not list them under Operations.

For non-destructive mutation (returning a new instance with one field replaced), declare the operation with a `(kind: with)` marker in the title or bullet:

```
## Operations
### with_currency (kind: with)
Returns a new Money with the currency replaced.
```

The marker instructs the scaffolder to emit `pub fn with_currency(self, currency: ...) -> Result<Self>` whose body re-validates via `Self::new(...)`. Without `(kind: with)`, no `with_*` method is emitted.

## Projection (`drafts/projections/<name>.md`)

A Projection is **structurally identical to a Context** — same six sections, same vocabulary. The difference is immutability: nothing on a projection writes through its role players.

```
# Projection: <Name>

## Purpose
One paragraph: what read-only view this projection enacts.

## Role Players
- <role>: <data or projection name> — one-line responsibility
  (Projections may reference Data and Projection types only — never Context.)

## Props
- <prop>: <description> (scalar configuration only)

## Role Methods
### <role>
- <method name>: <what this role does in this projection>
  (Read-only — describe what the role *provides* or *computes*, not what it changes.)

## Functionalities
### <functionality name>
Prose describing which roles do what, in what order, referencing role methods by name.
The functionality returns a computed view; it never mutates role players.

## Scenarios
### <scenario name>
Given: <role players in specific states>
When: <functionality invocation>
Then: <observable returned view>
```

Projections are read-only — `reen prepare` rejects mutation-shaped prose, and `reen verify` rejects any projection whose functionalities take `&mut self` or whose role methods take role players as `&mut`.

> **Migration note.** The legacy `## Source` / `## Queries` shape is rejected at parse time. Old-shape projection drafts must be rewritten to the context-aligned template above: `## Source` becomes a `## Role Players` entry, each `## Queries` entry becomes a `## Functionalities` entry, and the role player gets `## Role Methods` describing what the projection asks of it.

## What the scaffolder emits

The kind contract from `DCI.md` translates into a fixed Rust shape per kind. The BA never writes these — the scaffolder emits them. They are shown here so you understand the contract your draft is committing to.

### Data emission shape

Given a Data draft with attributes `amount` and `currency` and one invariant:

```rust
// @generated by reen — edit freely; will be regenerated if this comment is removed
pub struct Money {
    amount: i64,        // private
    currency: String,   // private
}

impl Money {
    // Constructor — signature deterministic from attributes; body filled by /implement.
    pub fn new(amount: i64, currency: String) -> Result<Self> {
        todo!("enforce invariants and construct")
    }

    // Getter — emitted with deterministic body; not authored, not filled.
    pub fn amount(&self) -> i64 { self.amount }

    // Getter — by reference for non-Copy types.
    pub fn currency(&self) -> &String { &self.currency }
}
```

If the draft declares an Operation marked `kind: with` for `currency`, the scaffolder additionally emits:

```rust
impl Money {
    pub fn with_currency(self, currency: String) -> Result<Self> {
        todo!("construct via Self::new with currency replaced")
    }
}
```

### Projection emission shape

Given a Projection draft with one role player (`account: Account`), one role method (`account.read_balance`), and one functionality (`summary`):

```rust
// @generated by reen — edit freely; will be regenerated if this comment is removed
pub struct AccountBalanceSummary {
    account: Account,   // private; Data role player
}

impl AccountBalanceSummary {
    // Constructor — public; binds role players at construction time.
    pub fn new(account: Account) -> Self {
        todo!()
    }

    // Functionality — public, &self only (never &mut self).
    pub fn summary(&self) -> String {
        todo!()
    }

    // Role method — private, role player as & (never &mut).
    // Canonical <role>_<method> name.
    fn account_read_balance(&self, account: &Account) -> i64 {
        todo!()
    }
}
```

Identical in shape to a Context, with three structural enforcements: every functionality takes `&self`, every role method takes its role player as `&`, and struct fields are private.

### Context emission shape

Given a Context draft with one Functionality (`transfer`) and one role method (`source.withdraw`):

```rust
pub struct MoneyTransfer {
    source: Account,
    sink: Account,
    amount: Amount,
}

impl MoneyTransfer {
    // Functionality — public. Upgrades to &mut self when mutation is signalled.
    pub fn transfer(&mut self) -> Result<Receipt> {
        todo!()
    }

    // Role method — private, canonical <role>_<method> name,
    // role player as first explicit & parameter.
    fn source_withdraw(&self, source: &Account, amount: &Amount) -> Result<()> {
        todo!()
    }
}
```

## Boundary Draft (`drafts/apis/<name>.md`)

A boundary draft is a config file pointing the API ingest pipeline at an OpenAPI sidecar (`drafts/apis/<name>.yaml`) and declaring which operations to expose. It is **not** a DCI artefact — there is no Purpose-becomes-functionality flow; the file's job is to configure `reen apis ingest`. Everything else (data drafts for schemas, projection drafts for operations) is generated under `drafts/data/.api/` and `drafts/projections/.api/`.

```
# <API Name>

Free-form Markdown for human context (one or two paragraphs).

## Source

- OpenAPI: ./<name>.yaml
- Documentation: <docs URL>

## Configuration

\`\`\`yaml
include_operations:
  - GetAccounts
  - GetTransactions
  - InitiateDomesticPayment
\`\`\`

## Notes

- Free-form implementation hints (auth, rate limits, etc.).
```

**What goes here:** human context, the OpenAPI sidecar pointer, and a fenced YAML block declaring which operations to expose.

**What does NOT go here:** field lists, type definitions, code tokens. Those are generated under `drafts/data/.api/` and `drafts/projections/.api/` from the OpenAPI sidecar and are off-limits for hand-editing — `reen refine` flags any modification to an auto-generated file.

**The fenced YAML block** is the canonical Phase A shape. Each entry in `include_operations` matches an OpenAPI `operationId` (or path × method when no `operationId` is set). Omit the block (or pass an empty list) to include every operation in the sidecar.

**Legacy shape.** Pre-Phase-A boundary drafts used a `## Operations` bullet list to declare the whitelist. The parser still accepts that shape as a fallback; new drafts should use the fenced YAML config.

## Common Pitfalls

**Mixing role and role player.** "Account withdraws the amount" is wrong. "The source withdraws the amount" is correct. The role does the action; Account is just the data playing that role.

**Sneaking types into drafts.** No Rust types, no generic brackets, no path syntax. Write "a list of positions" instead of `Vec<Position>`. Write "a whole number" instead of `u32`. The code-token lint blocks these mechanically.

**Scenarios that restate the flow.** A scenario should enact a specific case with concrete values, not paraphrase the functionality prose. "Given: a valid transfer — When: transfer is called — Then: it succeeds" tells you nothing. Use real account numbers, balances, and amounts.

**Role methods that are getters.** "read_score: returns the current score" is a getter on the data, not a role responsibility. A role method should describe what the role *does* in the context, not what the data exposes. If the context just needs to read a field, that's an implementation detail — the role method should describe the business reason for reading it.

**Putting role methods on data.** If a behaviour only makes sense in a specific scenario, it is a role method on the context, not an operation on the data type. Operations are intrinsic to the data; role methods are assigned by contexts.
