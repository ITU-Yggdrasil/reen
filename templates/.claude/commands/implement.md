---
name: implement
description: Fill todo!() bodies in scaffolded Rust files guided by drafts and tests
---

Implement the `todo!()` bodies in scaffolded Rust files, guided by the draft specification and synthesised tests.

Steps:
1. **Read the build tracker** (`.reen/build.json`) if it exists.
   - If the tracker exists, work **only** on entries with `state: "stale"` or `state: "not_implemented"`.
   - Skip entries with `state: "fresh"` — their bodies are already valid.
   - If an entry has `reason: "audit_finding"`, read the `audit_finding` text in the tracker entry and use it as additional context when filling the method.
   - If no tracker exists, fall back to filling all `todo!()` bodies.
2. Read the prepared YAML artifact for the target context/data/projection.
3. Read the corresponding draft markdown for business-level intent.
4. Read the scaffolded `.rs` file to see the stub structure.
5. Pick one stale/not_implemented method at a time. Fill its `todo!()` body following these rules:
   - Functionalities orchestrate role method calls.
   - Role methods operate on their role player parameter.
   - Data operations implement business rules from the draft.
   - Honour invariants described in the draft.
6. For `component_render` entries: read the component's draft (`drafts/components/<Name>.md`). The render body must actualise every child listed in the Composition section by constructing it with declared prop bindings and calling `.render()`. It must also enforce every Invariant whose enforcement is visible in the rendered output (e.g. "must not display credential input fields" means no password/text input elements). A render body that paraphrases the Purpose in prose (`<p>Your accounts will appear here.</p>`) is a stub and must be rejected — the body must either reference declared children/props or be converted back to `todo!()`.
7. After each method is filled, the PostToolUse hook will run `reen verify && cargo check`.
   The verify hook will automatically flip the tracker entry from `stale` to `fresh` when the method passes verification.
8. Fix any verification or compilation errors before proceeding to the next method.
9. Stop when the tracker shows no `stale` or `not_implemented` entries (or when all `todo!()` bodies are filled if no tracker exists).
10. When all bodies are filled, run `cargo test` to confirm synthesised tests pass.

## Kind-specific patterns

The scaffolder emits kind-shaped stubs. Follow the pattern that matches the kind of the body you are filling.

**Filling a Data type's `new()` constructor.** The body takes the declared field values, enforces every invariant from the prepared YAML's `invariants[]` list (returning `Err` with a specific message per failed invariant), and constructs `Self`. Do not do additional work — no logging, no audit emission, no side effects, just validate and construct. If an invariant cannot be expressed in the constructor (e.g. cross-instance uniqueness), state so explicitly with a `// invariant <name> delegated to <where>` comment naming the enforcement site, so verify and audit can recognise the delegation.

**Filling a Data type's attribute Rust types (`data_attribute_type` tracker entries).** The scaffolder leaves every newly-declared attribute as `pub <name>: (), // TODO: type to be filled by /implement — <description>`. Your job is to choose a Rust type for each attribute based on the BA's prose in the draft, then rewrite the field declaration. The BA's description is authoritative for *meaning*; the Rust type is your implementation detail. Use the description's vocabulary as your guide: "a text value" → `String`, "a whole number" / "minor units" → `i64`, "a date-and-time value" → `chrono::DateTime<chrono::Utc>` (or your project's date type), "the consent scope identifiers" with multiplicity `list` → already wrapped as `Vec<()>`; replace the inner `()` (e.g. `Vec<String>`). Once you write the type into the field, the constructor parameter and getter return type are regenerated to match — do not edit them by hand. After the rewrite, the PostToolUse hook will flip the tracker entry from `stale`/`not_implemented` to `fresh`, and subsequent `reen scaffold` runs will preserve your decision. If the prose is ambiguous, leave the `()` placeholder and surface the question by file an issue or asking the BA — do not guess types.

**Filling a variant data type's per-variant attribute Rust types (`data_variant_attribute_type` tracker entries).** Sum-typed Data drafts (e.g. `ChatTurn`, `TransferRequest`) emit a wrapper struct + `<Name>Kind` enum + `<Name>Payload` enum. Each variant of `<Name>Payload` carries its own named fields, and each named field starts as `field: (), // TODO: type to be filled by /implement`. Your job is identical to the top-level case above, only the field lives inside an enum variant. Each per-variant attribute has its own `data_variant_attribute_type` tracker entry, keyed `data/<snake>/variant/<snake>/attribute/<snake>`. The lifecycle is the same — Fresh entries survive re-scaffolds; renaming the variant or editing the description marks the entry stale and resets the field to `()`. Do NOT edit the `<Name>Kind` enum or the wrapper struct — those are derived from the variant list and are emitted in full by the scaffolder.

**Filling a Data type's getter.** Mechanical — return `&self.<field>` for non-`Copy` types, `self.<field>` for `Copy` types. The scaffolder emits the body fully; only touch a getter if the user has manually deleted or modified it. Never add logic to a getter.

**Filling a Data type's `with_*` operation.** Construct a new instance with the field replaced and re-validate via `Self::new(...)` so the same invariant logic applies. Never mutate `&mut self` — return a new `Self` by value, propagating the constructor's `Result`.

**Filling a Projection's functionalities and role methods.** Identical to the context pattern in every respect except mutability. Functionalities take `&self`, walk their declared role methods, and return computed output. Role methods take the role player as `&` (never `&mut`) and return computed output without writing back. The projection itself is referentially transparent — given the same role-player state, the same functionality returns the same output. No logging, no audits, no mutation. If a projection's intended computation requires mutating its source data, the projection is wrong by design — author it as a context instead.

**Filling a Context's functionality.** Unchanged from the existing guidance above. Compose role methods, mutate role-player state through them where needed, and return the computed result.

## Implementation contract — explicit obligations

The patterns below are recurring obligations the audit found broken across many contexts. Every implement invocation MUST honour them.

1. **Stale-with-finding entries are priority.** Before working any `not_implemented` entries, scan the tracker for entries in state `stale` carrying a populated `audit_finding` field. Work those first. The finding text is the spec for the rework — it names what the previous implementation got wrong. After a fix, the entry will flip back to `fresh` on the next `reen verify` if the structural checks pass; the next `/audit` run will confirm whether the semantic divergence is resolved. Example: a tracker entry `contexts/list_customer_accounts/functionality/list` in state `stale` with audit_finding "no audit entry on scope-denial path" tells you exactly what to repair.

2. **Scenario-driven flow.** For each functionality you fill, walk every entry in its prepared YAML's `scenarios[]` array. The body must contain a code path that observably produces each scenario's `then:` outcome under its `given:` setup and `when:` trigger. Failure scenarios are equally binding — a scenario whose `then:` says "an audit entry is recorded with outcome 'denied'" must produce that audit entry on that path before any error-returning short-circuit. Example: `list_customer_accounts.yml`'s scope-denial scenario binds the body's denial path to emit the audit *before* the `?` propagates.

3. **Canonical role method names.** Use the names in `role_method_refs[]` exactly. If the YAML lists `conversation_load_history`, the impl block defines a private method named `conversation_load_history` — not `session_load_conversation`, not `load_conversation`. The naming is contractual; `reen verify` enforces this as a hard gate, but the implement prompt's job is not to produce drift in the first place. Example: a context whose YAML declares `role_method_refs: [ledger_entry_claim]` requires a `fn ledger_entry_claim(&self, ledger_entry: &IdempotencyLedgerEntry, ...)` method — never `fn idempotency_ledger_claim`.

4. **Role players over infrastructure.** Mutating state on a role player goes through a declared role method that takes the role player as the first explicit `&` parameter. Direct calls to infrastructure fields bypass the role layer and are violations even when an infrastructure method exists. Example: `self.session_store.save(session)` is wrong if a declared role method `session_persist(&self, session: &Session)` exists in `role_method_refs[]` — call the role method, which delegates to the store internally.

5. **Audit emission on failure paths.** When a scenario's `then:` clause names an audit outcome on a failure path ("denied", "failed", "rejected"), the corresponding code path emits that audit entry before the `?` or `return`. Pattern: scope-check failure → emit denial audit → return error. Do not let the `?` operator swallow the audit emission. Example: `if !customer.has_scope(scope) { self.audit_sink_record(...denial...)?; return Err(...); }`.

6. **Idempotency-record-on-success.** When a context has an idempotency-token role player and a successful outcome, the success path records the outcome against the token before returning. The duplicate-token path on a subsequent invocation returns the recorded outcome verbatim — never a fabricated stand-in like `format!("PRIOR-{}", token)`. Pattern: success → `ledger.record(token, outcome)` → return outcome; duplicate path → `ledger.lookup(token)` → return stored outcome unchanged.

7. **No undeclared struct fields.** Struct fields are constrained to the role players, props, attributes, and source declared in the prepared YAML. Internal bookkeeping fields are permitted but MUST be prefixed `internal_` so verify and audit can distinguish them from spec-declared state. Example: adding `audit_sink: Vec<AuditEntry>` to a context struct without YAML declaring it is a violation; if you genuinely need a per-instance audit accumulator, name it `internal_audit_buffer`.
