# DCI in Reen — A BA's Reference

DCI (Data, Context, Interaction) is the authoring model for Reen. Every draft you write uses DCI vocabulary. This document teaches you to think in DCI so your drafts are clear, complete, and ready for the pipeline.

## The Three Artefact Kinds

### Data

A **data** artefact represents a domain concept — a thing that exists independently of any use case. It has attributes, invariants, and operations.

- **Attributes** describe what the data *is*: "value: the amount in minor units", "currency: the Currency denomination."
- **Invariants** state rules that always hold: "Two Amounts can only be combined when their currencies match."
- **Operations** are behaviours intrinsic to the data itself: "addition: adds two amounts with the same currency."

Data does not know about contexts or roles. It is standalone and reusable.

### Projection

A **projection** provides a read-only view over underlying data. It exposes queries but never mutates. If you find yourself describing a change, that behaviour belongs on the data type or in a context.

A projection names its **source** (the data it views) and its **queries** (what you can observe through it).

### Context

A **context** enacts a scenario. This is where the DCI design act happens: you decide which data plays which role, what each role does, and how roles compose into a use case.

A context has:
- **Role players** — data or projection types cast into roles
- **Props** — scalar configuration values (an amount, a URL, a time window)
- **Role methods** — what each role does in *this* scenario
- **Functionalities** — named use cases that compose role methods
- **Scenarios** — concrete Given/When/Then enactments

## Roles vs Role Players

This is the most important DCI distinction.

A **role** is a responsibility in a context. In a money transfer, "source" is a role — the provider of funds. "Sink" is a role — the receiver.

A **role player** is the data or projection type that fills the role. "Account" is a role player — the actual domain object.

The same Account can be the "source" in one transfer and the "sink" in another. The role changes; the data type stays the same. When writing a draft, say what the *role* does: "the source withdraws the amount," not "the Account withdraws."

## Role Methods

Role methods decompose behaviour per role. They answer: "What does this role do in this scenario?"

Each role gets its own subsection under Role Methods. Write each method as a responsibility — what the role *does*, in business language:

> **source**
> - withdraw: creates an unsettled ledger entry representing a withdrawal of the transfer amount from the source account; the transfer amount must not exceed the source's current balance
>
> **sink**
> - deposit: settles the unsettled entry created by withdraw, recording the sink as the destination

Role methods describe responsibilities, not implementation. "Creates an unsettled ledger entry" is a business action. Compare with "calls Ledger::create_entry" — that would be implementation language, and the draft lint rejects it.

## Functionalities

A functionality is a named use case that composes role methods into a sequence:

> **transfer**
> Executes the transfer by calling the source's withdraw role method, then the sink's deposit role method, then committing the resulting entry to the ledger. If any step fails, the operation returns an error immediately.

Functionalities are the connective tissue between role methods. They say which roles act, in what order, with what guarantees.

## Scenarios as Enactments

Every functionality is specified by at least one scenario. A scenario is a concrete enactment with specific values — not an abstract test case:

> **successful transfer**
> Given: account 123456 has balance 1000.00 DKK, account 654321 has balance 1000.00 DKK, amount is 250.00 DKK
> When: transfer is called
> Then: the returned ledger shows account 123456 with balance 750.00 DKK and account 654321 with balance 1250.00 DKK

Good scenarios use real-looking values (account numbers, currency amounts, coordinates). They exercise the functionality through a concrete path. Tests are a consequence of scenarios — you write the scenario first, and the tests follow.

Cover the important paths: the success case, the key error cases, and any boundary that the invariants describe.

## The Vocabulary Manifest

Every data and projection name used in a context must be resolvable. Reen auto-discovers names from draft file names under `drafts/data/` and `drafts/projections/`. For types that don't have their own draft (external infrastructure, types from another context), add them to `drafts/vocabulary.yml`.

When `reen prepare` encounters an unresolvable name, it raises a blocking ambiguity. This forces you to either create the missing draft or declare the name in the vocabulary — the manifest is the single source of DCI vocabulary for the project.

---

## Worked Example: MoneyTransfer Context

This is the actual context draft from the money-transfer test project. It shows the full DCI decomposition.

**The domain**: transferring money between accounts through an immutable ledger.

**Role players**: Account plays two roles — "source" (provides funds) and "sink" (receives funds). Both are the same projection type, but each has a different responsibility.

**Props**: "amount" (how much to transfer) and "ledger" (where transactions are recorded).

**Role method decomposition**:
- The source role's responsibility is *withdraw*: create an unsettled ledger entry, validate sufficient balance.
- The sink role's responsibility is *deposit*: settle the entry, recording the destination.

**Functionality**: "transfer" composes these two role methods in order — withdraw first, then deposit, then commit — with error handling at each step.

**Scenarios**: three paths tested — successful transfer (balances update correctly), insufficient funds (error returned), currency mismatch on construction (error returned).

Notice how the context owns the interaction (withdraw, then deposit, then commit) while the data types (Account, Ledger, Amount) are passive. The roles give the data temporary responsibilities for this specific use case.

## Worked Example: Amount Data

This is the actual data draft from the money-transfer test project.

**Attributes**: value (in minor units) and currency.

**Invariant**: "Two Amounts can only be combined when their currencies match." This invariant appears in prose and is enforced whenever addition or subtraction is called.

**Operations**: get_amount (display the decimal value), zero (create a zero amount), addition, subtraction. Each is a business action intrinsic to the data — Amount knows how to add itself to another Amount, regardless of which context is using it.

**Scenarios**: five concrete cases — display, add, subtract, currency mismatch panic, and zero construction. Each uses real values: "an Amount of 1050 in DKK" yields "10.50".

The key difference from a context: data operations are permanent capabilities of the type. Role methods are temporary responsibilities assigned by a context. Amount will always know how to add; only the MoneyTransfer context makes it play the role of "transfer amount."
