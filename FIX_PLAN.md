# Fix plan — bugs surfaced by `tests/lupa` scaffold

Origin: bugs catalogued during the `prepare` + `scaffold` debug pass on `tests/lupa` (April 2026). Numbers below match that pass; bug 4 (API boundary code unreachable) and the API portion of bug 3 are deferred — see the TBD section at the end. Bug 11 (cosmetic citation typo) folds into the test-synth fixes.

The plan groups bugs by ownership area: parser/model, refine, codegen, test synthesis. Each section names the source file the change lives in, gives a concrete proposed change, and flags risk where it matters.

## Inviolable principle: drafts speak BA

Every fix below is bound by this rule, codified in `templates/CLAUDE.md`, `templates/DCI.md`, and `templates/DRAFT_FORMAT.md`:

> Every context and every projection IS a use case; every data draft is a domain concept in domain language. Drafts may reference data types by capitalised name (Money, Transaction, Account) wherever prose calls for it. Drafts MUST NOT contain parameter lists, return types, multiplicity notation (`list of T`, `[T]`), generic brackets, or any other formal signature shape. The pipeline derives signatures, arities, return types, and multiplicities from prose and scenarios at prepare time — not from the draft author. If a draft cannot say something in BA prose, the draft is wrong, not the rule.

No fix in this plan introduces draft syntax that would let a BA write a parameter list, a return type, a multiplicity marker, or any other signature notation. Fixes that need that information get it by **inferring from prose and scenarios at prepare time** (bugs 5 and 6). Fixes that need to surface a problem to the BA do so as `reen refine` findings phrased in BA terms.

Any future proposal that softens this rule must justify why BA prose cannot carry the information, and must propose a parser/inference change first.

## 1. Variant data types (ChatTurn, TransferRequest)

**Symptom.** `data/ChatTurn.md` and `data/TransferRequest.md` use `### <variant> attributes` H3 sub-headers under `## Attributes` to model sum types. The parser flattens every nested `- ` bullet into one attribute list, producing duplicate struct fields (`text`, `text`; `tool_name`, `tool_name`; three `card_reference`s; `recipient_name`, `recipient_name`). The H3 lines themselves leak into the previous bullet's description (`"…with timezone ### User-utterance attributes"`). The scaffolded Rust will not compile.

**Root cause.** `parse_attribute_bullets` (`src/draft_parser.rs:517`) calls `parse_named_bullets` on the raw body of `## Attributes`. `parse_named_bullets` (`src/draft_parser.rs:1317`) treats every line that starts with `- ` (after `trim`) as a top-level item, ignoring indentation and ignoring intervening `### …` headers. There is no concept of a variant data type in `DataDraft` (`src/draft_parser.rs` ~line 150) or in the prepared schema. `VariantDraft` exists but only for components (button variants etc., parsed via `parse_variant_bullets` for `ComponentDraft`).

**Fix — short-term (stop-the-bleeding).** Add a refine-time validator that rejects H3 sub-sections inside `## Attributes` of a data draft with a blocking finding, e.g.:

> Variant data types are not yet supported. `data/ChatTurn.md` has H3 sub-sections under `## Attributes` (`### User-utterance attributes`, `### Tool-call attributes`, …). Either flatten to a single attribute set, or split into one data type per variant.

Implementation: in `src/draft_refine.rs`, extend the per-data-draft scan to walk the markdown sections, find `## Attributes`, and look for any `### …` headers between `## Attributes` and the next `## …`. If found, emit a `Concreteness`-category finding. Block prepare on it.

**Fix — long-term (first-class variant data types).**

1. **Parser**: introduce `parse_variant_attribute_groups` that recognises `### <variant> attributes` blocks under `## Attributes`. Add `variants: Vec<VariantDataGroup>` to `DataDraft`, where each `VariantDataGroup` carries the variant name and its own `Vec<AttributeDraft>`. Keep top-level `attributes` for shared fields (kind, timestamp).
2. **Prepared schema**: extend `PreparedDataArtifact` with a `variants:` field. Bump schema version.
3. **Codegen** (`src/codegen.rs`): emit a Rust enum `pub enum ChatTurn { UserUtterance { text: () }, AgentResponse { text: () }, ToolCall { tool_name: (), tool_input: () }, … }` with shared fields lifted into a wrapper struct or duplicated per variant.
4. **Verify** (`src/verify.rs`): teach kind-discipline to recognise enum variants.

Risk: the long-term fix is a meaningful schema change. Ship the short-term fix first, then schedule the variant model with a phase plan and a migration for the two affected drafts.

**Affected drafts.** `data/ChatTurn.md`, `data/TransferRequest.md`. After short-term fix, the BA must redesign these — likely splitting `TransferRequest` into `OwnTransferRequest`, `DomesticTransferRequest`, `InternationalTransferRequest` and treating `ChatTurn` similarly.

## 2. Top-level `src/` orphan tree (Phase 10 layout)

**Symptom.** When the project is in Phase 10 multi-crate layout (workspace `Cargo.toml` listing `crates/lupa-{core,client,components,server}` and no top-level `[package]`), scaffold still emits a complete duplicate copy of every artefact under top-level `src/data/`, `src/contexts/`, `src/projections/`, `src/components/`, `src/brand/`, `src/routes/`. The files are byte-identical to their `crates/lupa-core/src/...` counterparts and Cargo silently ignores them — but they confuse `git diff`, `reen verify` (if it walks both trees), and any human reading the repo. CLAUDE.md's "src/ is machine-owned" still implies that path even though it no longer compiles.

**Root cause.** The codegen entrypoint emits to two targets without a guard that the project is in multi-crate mode. Likely `src/codegen.rs` writes to `workspace.src_dir` regardless, and `src/web_codegen.rs` separately writes to `crates/lupa-*/src/`.

**Fix.**

1. **Detect layout in `Workspace`** (`src/workspace.rs`): add a `layout: ProjectLayout` field on `Workspace`, computed at load time by inspecting `Cargo.toml`. Two values: `SingleCrate` and `MultiCrate { core: PathBuf, client: PathBuf, components: PathBuf, server: PathBuf }`.
2. **Gate emission paths** in `src/codegen.rs`: when `layout == MultiCrate`, the data/contexts/projections targets resolve to `crates/lupa-core/src/{data,contexts,projections}/` and the top-level `src/` is never written. When `SingleCrate`, behaviour is unchanged.
3. **Clean on first multi-crate scaffold**: in scaffold, if `layout == MultiCrate` and a top-level `src/` exists, delete it. Print a one-line notice (`removed legacy single-crate src/`).
4. **Update `reen clean`** to handle both layouts (`src/clean.rs` if it exists, or wherever `reen clean --force` lives).

Risk: deleting `src/` on first multi-crate scaffold is destructive. Gate it behind: top-level `src/` exists, AND every file inside has the `// @generated by reen — edit freely; will be regenerated if this comment is removed` header. Refuse to delete if any file lacks that header.

The API portion of this bug (`src/apis/nordea-openbanking/...` is currently the only source location for API code, and there is no `crates/lupa-*/apis/`) is parked — see TBD.

## 3. Contexts and projections lack `use` statements — DONE

Resolved on `claude-code-native`: `src/codegen.rs` now emits one `use crate::data::<snake>::<T>;` per unique role-player data type, sorted alphabetically, before the `pub struct` line. Applies to both eager and surgical emitters for context and projection. Regression tests live in `src/codegen.rs` (`scaffold_context_emits_use_statements_for_role_player_types`, `scaffold_projection_emits_use_statements_for_role_player_types`, `scaffold_skips_use_for_prose_role_player_types`, `scaffold_skips_use_for_rust_primitives`). `tests/lupa` regenerated; every `crates/lupa-core/src/{contexts,projections}/*.rs` file now imports each referenced data type.

**Symptom.** Every `crates/lupa-core/src/contexts/*.rs` and `crates/lupa-core/src/projections/*.rs` file references types like `AuthenticatedCustomer`, `Session`, `ChatConversation`, `ConfirmationCard`, `IdempotencyLedgerEntry`, `Transaction`, `Account`, `IBAN`, `BIC`, `DomesticAccountNumber`, `IdempotencyToken`, `AuditEntry` — but emits no `use` statements. None of the 15 contexts and 3 projections will compile.

The components emitter does this correctly (`use super::super::markup::Markup;`), so the codegen knows how to write imports.

**Root cause.** The context and projection emitter in `src/codegen.rs` writes the `pub struct` line directly without first walking the role players to collect their declared types and emitting an import per type.

**Fix.** In `src/codegen.rs`, in the function that emits a context (and the analogous one for projections):

1. Walk `role_players: Vec<RolePlayerDraft>` (and props with declared types).
2. For each unique type name `T`, look up its module path. For data types this is `crate::data::<snake>::<T>`. For projections referenced as role players (rare but possible), `crate::projections::<snake>::<T>`.
3. Emit one `use crate::data::<snake>::<T>;` per unique type, sorted alphabetically, before the `pub struct` line.
4. Mirror in the projection emitter for any role players whose types come from `data/`.

Add a unit test under `src/codegen.rs` `#[cfg(test)]` that emits a context with two role players and asserts both `use` lines appear.

Risk: low. Mechanical change with a clear test path.

## 4. Router has literal `{id}` matcher and unbound variable

**Symptom.** `crates/lupa-components/src/router.rs` contains:

```rust
} else if segments.len() == 2 && segments[0] == "accounts" && segments[1] == "{id}" {
    Ok(Route::AccountDetailScreen { id, })
```

Two problems: (a) the matcher checks `segments[1] == "{id}"` literally — so `/accounts/12345` falls through to 404, only `/accounts/{id}` matches; (b) `id` is referenced via struct shorthand without ever being declared. Compile error before the logic gets a chance to run.

**Root cause.** The router emitter in `src/web_codegen.rs` (around line 672, the `route_path` / param-binding logic) treats path segments uniformly as string literals. There is no branch that distinguishes a parameter segment (`{id}`) from a literal segment (`accounts`).

**Fix.** In `src/web_codegen.rs`:

1. When parsing each route's path, classify segments into `Literal(String)` and `Param(String)`. `{id}` becomes `Param("id")`, `accounts` becomes `Literal("accounts")`.
2. Emit the matcher as: literal segments produce `segments[i] == "literal"` clauses; param segments produce no clause (any segment is acceptable in that position).
3. Before constructing the `Route` variant, emit `let <param> = segments[<i>].to_string();` for each `Param` segment, in the order they appear.
4. Then `Ok(Route::AccountDetailScreen { id })` works because `id` is bound.

Add a unit test that emits a router for a routing table with `/accounts/{id}` and asserts the generated code contains `let id = segments[1].to_string();` and does NOT contain the literal string `"{id}"`.

Risk: low; localised to one emitter.

## 5. Operation, functionality, and query arity

**Symptom.** Scaffold emits every operation/functionality/query as `pub fn <name>(&self) { todo!() }` regardless of what the prose says or how many operands the operation logically takes. `Money::add(&self)` has no second `&Money` operand. `Money::display_as_major_units(&self)` has no `-> String`. `TransactionList::filtered(&self)` has no date range parameter. `TransactionList::by_amount_range(&self)` has no `min` or `max`. The implementer cannot fill in these bodies without changing the signature, and CLAUDE.md says `src/` is machine-owned — so `/implement` is in conflict with the contract.

**Constraint.** Drafts must remain BA-spoken — no parameter lists, no return-type lines, no signature notation. Adding `Parameters:` / `Returns:` to drafts is out. Signatures must be derived by the machine from the prose and scenarios that are already there.

**Root cause.** Prepare doesn't analyse scenario `Given:` / `Then:` clauses to back out a signature for the operation under test. Codegen therefore has no signature information beyond the operation name.

**Fix — derive signatures from scenarios at prepare time.** Every operation/functionality/query already has scenarios with concrete `Given:` and `Then:` lines. Those lines name domain entities and use plural/singular cues. Walk them and back-compute a signature:

- For each scenario whose `When:` invokes operation `O`, parse the `Given:` clause to count and type distinct entity references. The first reference to a value of the host data type is the receiver (`&self`); additional references of the same or different declared types are positional parameters in the order they appear.
- For each scenario, parse the `Then:` clause for a result phrase ("the result is ...", "the answer is ...", "returns ..."). The shape of that phrase classifies the return type: a quoted string ⇒ text; "affirmative"/"negative" or "yes"/"no" ⇒ boolean; "a Money of …" ⇒ Money; "the three transactions ..." ⇒ list of Transaction; absence of a result phrase ⇒ unit.
- Aggregate across all scenarios for the same operation. If they agree, that's the inferred signature. If they disagree, prepare emits a finding (informational on first divergence, blocking if the disagreement is fundamental — e.g. one scenario implies a binary operation and another implies a unary).

Worked examples from `tests/lupa`:

- `Money::add` — scenario "Given: a Money of 5000 in DKK and a Money of 2500 in DKK / When: add is performed / Then: the result is a Money of 7500 in DKK" ⇒ `&self`, second `&Money`, returns `Money`.
- `Money::display_as_major_units` — "Given: a Money of 10050 in DKK / When: display as major units is performed / Then: the result is '100.50 DKK'" ⇒ `&self`, returns text.
- `TransactionList::by_amount_range` — "Given: transactions with amounts 50, 150, 200, 250, 300, 500 / When: by_amount_range is queried with minimum 100 and maximum 250 / Then: returns the three transactions with amounts 150, 200, and 250" ⇒ `&self` (over the transaction list role player), `minimum: Money`, `maximum: Money`, returns list of Transaction.
- `Session::is_expired` — "Then: the answer is negative" / "the answer is affirmative" ⇒ `&self`, returns boolean.
- `Session::transition_chat_state` — "When: transition chat state to 'awaiting-input' is performed" ⇒ `&self`, takes the new chat state value as a parameter, returns unit (or `Result`).

Implementation:
1. **Prepare** (`src/prepare.rs`): add a `derive_signature` pass that runs after vocabulary resolution and before test synthesis. It walks scenarios per operation/functionality/query and produces a `DerivedSignature { receiver, parameters: Vec<ParamSpec>, returns: ReturnSpec }`. Reuse vocabulary resolution to bind named entities (Money, Transaction, etc.) to declared types.
2. **Prepared schema**: add `derived_signature` (optional) to operation/functionality/query entries. Bump schema version. Mark as `derived` rather than `authored` so future changes don't surprise the BA.
3. **Codegen** (`src/codegen.rs`): when `derived_signature` is present, emit the full signature; when absent (no scenarios for the operation, or unresolvable), fall back to today's `(&self)` stub and emit a `// TODO: signature could not be derived` comment so the implementer knows.
4. **Refine** (`src/draft_refine.rs`): emit a finding ("operation X has no scenario; signature cannot be derived") to nudge the BA to add a scenario rather than silently producing a degenerate stub.
5. **Surface disagreements as findings** in prepare, not as silent first-wins behaviour.

The draft format is unchanged. The BA writes scenarios the way they already do; the machine reads them.

Risk: brittleness of prose parsing. Mitigate with two safeties — (a) the receiver and primary parameter type must resolve to a vocabulary entry, otherwise the signature is dropped and a finding is raised; (b) the result-phrase patterns are an explicit allow-list, not regex magic. New shapes get added as drafts demand them, in a controlled way.

**Drafts that benefit immediately.** `data/Money.md` (add/subtract/display); `data/Session.md` (transition_chat_state, is_expired, touch); `data/Account.md` (credit/debit); `data/ChatConversation.md` (append_turn, last_turn); `data/ConfirmationCard.md` (is_valid, mark_used); `data/DomesticAccountNumber.md` (format_as_display_string); all three projections.

## 6. Role-player and attribute multiplicity

**Symptom.** `TransactionList` is scaffolded as `pub struct TransactionList { transactions: Transaction, }` — a single `Transaction`, but the projection clearly operates on a list. Same hazard for `Customer.accounts`, `AuthenticatedCustomer.accounts`, `Session.scopes`, `ChatConversation.turns` (all currently emit a single value where the prose plainly describes a collection).

**Constraint.** Drafts must stay BA-spoken. No `list of <T>`, no `[T]`, no `optional <T>` — those are signature notation in disguise.

**Root cause.** The parser ignores plural cues and scenario shape; it always emits the bare type.

**Fix — infer multiplicity from prose and scenarios.** The signal is already in the draft. Combine three cues, with the scenario evidence dominating:

1. **Role-player or attribute name.** A plural noun (`transactions`, `accounts`, `scopes`, `turns`) is a strong cue. The English plural rule (`s`/`es` ending, irregular forms cross-checked against a small list) catches the Lupa cases. Singular nouns (`account`, `session`) are a strong singular cue.
2. **Description prose.** Phrases like "the transactions", "a list of", "all the X", "the ordered X" indicate a collection. "The X" with a singular noun indicates a single value.
3. **Scenario `Given:` shape (decisive).** If scenarios consistently supply multiple instances of the entity ("transactions with amounts 50, 150, 200, …", "five transactions"), the role player is a list. If scenarios supply one ("a Session with chat state 'idle'"), it's a single value.

Aggregate the three: scenarios outweigh names outweigh prose. If they disagree (e.g. plural name but scenarios always supply one), emit a finding so the BA can clarify.

Implementation:
1. **Prepare** (`src/prepare.rs`): add a `derive_multiplicity` pass that runs alongside `derive_signature`. Same data sources, same output schema location.
2. **Prepared schema**: add `multiplicity: Multiplicity` (`Single` | `List`) to role-player and attribute entries. Bump schema version. Default `Single` so older prepared YAML stays valid.
3. **Codegen** (`src/codegen.rs`): when `multiplicity == List`, wrap the field type in `Vec<T>`.
4. **Refine** (`src/draft_refine.rs`): no new lint; the multiplicity inference itself surfaces findings on disagreement.

The draft format is unchanged.

Risk: false positives on singular-looking words that are actually plural in domain meaning, and vice versa. The three-cue aggregation with scenarios as tie-breaker handles the common cases. For edge cases the finding gets raised and the BA writes a clarifying scenario.

**Drafts that benefit immediately.** `projections/transaction_list.md` (transactions), `projections/transfer_summary.md` (likely transactions or movements), `data/ChatConversation.md` (turns), `data/Customer.md` (accounts), `data/Session.md` (scopes), `data/AuthenticatedCustomer.md` (accounts, scopes), `data/Account.md` (likely transactions). After variant fix lands: `data/TransferRequest.md` per-variant attributes.

## 7. Synthesised invariant tests are LLM hallucinations

**Symptoms.**

- `transaction_list / by_amount_range`: input `75, 100, 250, 251, 300` with range `100..250` expected to return "the three transactions with amounts 100, 250, and excludes 75 and 251" — only two values, mis-counted.
- `transaction_list / by_counterparty`: input includes counterparty `'john'`; search is `'smith'`; test claims `'john'` matches, which it cannot.
- `transaction_list / combined filter`: synthesises a test combining `filtered AND by_counterparty AND by_amount_range`, but the projection defines them as three independent functionalities with no composite.
- `BIC`: citation typo `"letters in their case, or digits"` (should be `"letters in either case, or digits"`).

**Root cause.** `src/test_synth.rs` calls an external LLM (`call_api`, line 130) to synthesise invariant tests when scenarios don't cover an invariant. There is no validation pass on what comes back: the synthesiser invents inputs/outputs without ground-truthing them, invents functionalities the spec doesn't declare, and copies citation text imprecisely.

**Fix.**

1. **Validate input/output consistency** for projection query tests. For each synthesised test whose `when` invokes a query like `by_amount_range`, parse the input set, the bounds, and the expected output set from the test prose. Verify by simulation that the expected output is exactly the subset of input matching the bounds. Reject (and re-synthesise or drop) any test that fails. This catches the `75, 100, 250, 251, 300` and `'john' matches 'smith'` errors.
2. **Restrict synthesis to declared functionalities.** Pass the list of declared functionality/query names as a hard constraint in the synth prompt; reject any synthesised test whose `when` mentions a name not in the list. This catches the combined-filter hallucination.
3. **Citation pass.** When a synthesised test cites an invariant or functionality, copy the citation text verbatim from the prepared YAML, not from the LLM's paraphrase. This catches the "their case" / "either case" typo.
4. **Surface synthesis failures**. If the synthesiser cannot produce a valid test for an invariant after N retries, emit a `prepare`-time finding (informational) so the BA knows to add an explicit scenario.

Risk: tightening the synthesiser may reduce coverage of invariants that the LLM can't ground-truth. That's the right trade-off — better to have fewer, correct tests than more, wrong ones. Pair with a metric: percentage of invariants with at least one valid synthesised test, tracked over time.

Implementation lives entirely in `src/test_synth.rs` plus a small extension to `src/prepare.rs` to surface no-synthesis-possible findings.

## TBD: API boundary code reachability

Deferred per current scope. Two related issues to investigate together:

- **`src/apis/nordea-openbanking/*.rs` is generated under top-level `src/`** but no Cargo crate has an `apis` module, so the generated boundary types (`GetAccounts`, `InitiateDomesticPayment`, etc.) are unreachable. They need a home in the multi-crate layout — likely `crates/lupa-core/src/apis/` or a separate `crates/lupa-apis/` with its own module declaration in `lupa-core/src/lib.rs`.
- **`drafts/apis/nordea-openbanking.md` has no `drafts/prepare/apis/...yml`** because boundary drafts go through `apis_ingest.rs`, not the regular prepare path. The relationship between boundary draft, ingested artefact (`drafts/.apis/`), and emitted code needs an end-to-end review before fixing the reachability problem — there appears to be drift between the apis pipeline and the rest.

A separate session should treat the apis pipeline as a unit: the ingest path, where its prepared artefacts live, where its scaffold emits, how `reen verify` reaches it, and how `/implement` interacts with it. Until that's done, fix-plan items 1–7 should not assume the apis path works the same way as data/contexts/projections.

## Order of work — coherent groups with review breaks

Groups land one at a time. Each group ends with a **review break**: the implementer stops, hands back, and waits for explicit go-ahead before starting the next. No group spans more than one PR.

1. **Bug 3 — `use` statements.** DONE. (Ref: four tests in `src/codegen.rs`; `tests/lupa` regenerated.)
2. **Bug 4 — router `{id}` binding.** Localised to `src/web_codegen.rs`; small enough to ship right after bug 3.
3. **Bug 2 — Phase 10 layout detection.** Pure mechanics, no draft-side implications. Unblocks the duplicate `src/` orphan that's currently confusing every `git status`. → REVIEW BREAK.
4. **Bugs 5 + 6 — signature and multiplicity inference (one PR).** Both are scenario-driven inference passes in `src/prepare.rs`; both bump the prepared schema; both emit findings the same way. They land together so the schema bump happens once and `tests/lupa` regenerates once. The BA-speak rule is the dominant constraint here — drafts must not change. → REVIEW BREAK.
5. **Bug 7 — test-synth validation.** Independent; lives in `src/test_synth.rs`. Can be done before or after bugs 5/6 if the schedule shifts, but follows them in this plan because the synth bugs are visible in the current `tests/lupa` prepared YAML and the validation pass closes them out cleanly. → REVIEW BREAK.
6. **Bug 1 long-term — first-class variant data types.** Largest single change: parser extension, prepared schema, codegen emits Rust enums with per-variant fields, verify teaches kind discipline about variants. Skips bug 1 short-term entirely — a hard refine block on H3-under-Attributes would be in tension with the BA-speak principle and would force a redesign of ChatTurn / TransferRequest that the long-term fix makes unnecessary. → REVIEW BREAK.
7. **TBD: APIs.** Out of scope until the apis pipeline gets its own end-to-end review. Bugs 1–7 above must not assume the apis path behaves like data/contexts/projections.
