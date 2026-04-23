# Reen rewrite — Claude Code–native implementation plan

This plan replaces Reen's LLM-driven build pipeline with a Claude Code–native setup, where Reen owns specification and verification and Claude Code owns implementation. The repository is trimmed aggressively: only artefacts still load-bearing under the new model survive.

## North star

Reen becomes a **DCI-native spec compiler and verifier**. BAs author drafts in DCI's own vocabulary — data, contexts, projections, role players, role methods, functionalities, scenarios. Reen resolves those drafts deterministically against a shared manifest, synthesises behavioural tests, and generates Rust scaffolding with `todo!()` bodies. Claude Code — driven by slash commands, subagents, skills, and hooks — does refine-dialogue, implementation, and post-hoc audit. The prepared YAML is the machine contract; the markdown draft is the human (DCI) contract; Rust is a deterministic consequence of the first two plus Claude Code's body-filling.

## Design principles

1. **DCI is the BA authoring model.** Drafts are written in DCI vocabulary. A context draft lists role players, their role methods, the context's functionalities, and scenarios that enact them. Data and projection drafts describe domain artefacts and their invariants. BAs think in DCI; Reen's schemas reflect that directly. No DCI concept is abstracted away to make drafting "simpler" — the whole point is that DCI *is* the simpler mental model.
2. BA-authorable drafts. No types, no signatures, no Rust tokens in drafts. DCI terms only, expressed in domain language.
3. Deterministic where possible. Prepare resolves vocabulary and conventions mechanically; LLM is invoked only for behavioural test synthesis (at prepare time) and discrepancy audit (at review time).
4. Reen does not re-implement what Claude Code does natively. No compile-repair loop, no body-writing agent, no progress tracker.
5. Spec alignment is mechanical. Verifier checks structural equivalence between Rust and prepared YAML after every write — including the DCI-prescribed shape of role methods and functionalities.
6. Scenarios are enactments, not just tests. Every functionality is specified by at least one named scenario describing the role players in specific states, the interaction, and the observable outcome. Tests are a consequence of scenarios, not a parallel artefact.

## Pipeline after the rewrite

| Step | Owner | Input | Output | LLM? |
|------|-------|-------|--------|------|
| 1. Draft | BA (human) | — | `drafts/*.md` | No |
| 2. Prepare | `reen prepare` | draft markdown + manifest | prepared YAML + synthesised tests | Yes (test synthesis only) |
| 3. Refine | Claude Code | prepared YAML ambiguities | updated draft | Yes (conversational) |
| 4. Scaffold | `reen scaffold` | prepared YAML | Rust stub files | No |
| 5. Implement | Claude Code | draft + stubs + tests | filled Rust bodies | Yes (native loop) |
| 6. Verify | `reen verify` | Rust + prepared YAML | pass/fail + structured diff | No |
| 7. Audit | Claude Code (subagent) | draft + final Rust | discrepancy report | Yes (one-shot) |

Verify runs as a PostToolUse hook after every write. Audit runs on demand after implement concludes.

---

## What gets removed

### Source files (delete)

- `src/fix_agent.rs`
- `src/build_agent.rs`
- `src/build_compile_agent.rs`
- `src/build_tracker.rs`
- `src/agent_runner.rs`
- `src/compile_repair.rs`
- `src/draft_refine_llm.rs`
- `src/spec_context.rs` (prompt-rendering helper; no longer needed)

### Source files (remove LLM pathways, keep deterministic core)

- `src/draft_refine.rs` — keep the deterministic review categories (Completeness, Concreteness, Ambiguity) as they feed prepare's ambiguity list. Remove `BehavioralAmbiguity` and anything LLM-backed. Extend the Concreteness category with a **code-token lint** (see Phase 3.5): a deterministic rejector that refuses drafts carrying Rust syntax or type notation in DCI prose. Move surviving logic into `prepare.rs` if thin; otherwise rename to `review.rs`.

### CLI subcommands (delete)

- `reen fix`
- `reen build`
- `reen compile`
- `reen run`
- `reen test`

`cargo` is invoked by Claude Code directly or by hooks; Reen does not wrap it.

### Documentation (rewrite or delete)

- `DCI Primer.md` — **rewritten, not deleted**, as `DCI.md`: a slim, BA-facing reference for how DCI is expressed in Reen drafts. Covers data/context/projection, roles vs role players, role methods as responsibilities, scenarios as enactments, and the manifest as ubiquitous vocabulary. Target 2–4 pages; no Rust specifics. The current primer's code-level material (receiver rules, `<role>_<method>` naming, mutability table) moves into the `reen-dci-code` skill (see below). Historical background, further-reading appendices, and anything not load-bearing for authoring or realisation is dropped.
- Current `CLAUDE.md` — replaced by a slim version (see Phase 6).

### Test projects

- Migrated in Phase 4: `tests/snake`, `tests/money transfer`, `tests/Connor`. Snake and money transfer are the small reference projects; Connor is the larger real-world draft that stress-tests the schema and lint.
- In scope for Phase 7 as the external-API ingestion test case: `tests/world data/`. Retained but deferred out of Phase 4 because it is a distinct use case — consumption of an external API described by an OpenAPI schema — that the Phase 1–4 pipeline did not cover.
- Retained on disk but currently out of scope: `tests/snake_gh/`. GitHub-repo ingestion is another distinct use case; revisit as a future phase if repo ingestion becomes part of the schema.
- All `target/` directories under tests get `.gitignore`'d if not already.

---

## What gets kept and refactored

| File | Change |
|------|--------|
| `src/draft_parser.rs` | Rewrite for new BA-authorable schema (no type notation required). |
| `src/prepare.rs` | Rewrite: vocabulary resolution, example extraction, test synthesis, ambiguity raising in business language. |
| `src/prepared.rs` | Schema v2: add `synthesised_tests[]`, `vocabulary_bindings[]`, business-language ambiguities. |
| `src/prepared_contracts.rs` | Update contracts to match v2 schema. |
| `src/codegen.rs` | Trim: scaffolding only, no agent hooks, no stub-replacement logic. |
| `src/manifest.rs` | Becomes the vocabulary manifest; central source of domain names. |
| `src/workspace.rs` | Trim: only the file-layout helpers Reen still needs. |
| `src/lib.rs`, `src/main.rs` | Trim: CLI shrinks to `prepare`, `scaffold`, `verify`, `clear`, `config`, `manifest`. Phase 7 adds `apis`. Phase 8 adds `init`. |

## What gets added

| File | Purpose |
|------|---------|
| `src/verify.rs` | Structural alignment: parse generated Rust (syn), compare to prepared YAML field-for-field and method-for-method. |
| `src/test_synth.rs` | Prose-to-test synthesis invoked by prepare. Calls out to LLM (Claude) with narrow, structured prompt. |
| `.claude/commands/prepare.md` | Wraps `reen prepare`, surfaces ambiguities in conversation. |
| `.claude/commands/refine.md` | Ambiguity-resolution dialogue; edits drafts; re-runs prepare. |
| `.claude/commands/scaffold.md` | Wraps `reen scaffold`. |
| `.claude/commands/implement.md` | Opens a context's stub + draft + tests; instructs Claude Code to fill bodies. |
| `.claude/commands/audit.md` | Invokes the auditor subagent after implementation. |
| `.claude/agents/reen-refiner.md` | Cheap-model subagent for ambiguity Q&A; read-only on drafts. |
| `.claude/agents/reen-auditor.md` | Fresh-context subagent that produces structured discrepancy reports. Read-only. |
| `.claude/skills/reen-dci-authoring/SKILL.md` | **BA-facing.** How to think in DCI when writing a draft: data vs context vs projection, the distinction between roles and role players, role methods as per-role responsibilities, scenarios as enactments, using the manifest as shared vocabulary. Fires on `drafts/**.md`. |
| `.claude/skills/reen-draft-format/SKILL.md` | Draft grammar, section templates, common pitfalls. Fires on drafts and prepared YAML. Complements the authoring skill — one teaches DCI thinking, the other teaches the syntactic shape. |
| `.claude/skills/reen-dci-code/SKILL.md` | **Dev-facing.** How DCI realises in Rust under Reen: `<role>_<method>` naming, receiver mutability rules, spec-alignment invariants the verifier enforces. Fires on `.rs` files under `src/contexts/`, `src/data/`, `src/projections/`. |
| `.claude/hooks.json` | PostToolUse on Write in `drafts/**.md` → `reen prepare`; PostToolUse on Write in `src/**.rs` → `reen verify` + `cargo check`. |
| `CLAUDE.md` (new, slim) | Points to skills, states the implementation contract, names DCI as the authoring model. |
| `DCI.md` | BA-facing rewrite of `DCI Primer.md`. Canonical reference for how DCI is expressed in Reen drafts. Referenced by the `reen-dci-authoring` skill. |
| `DRAFT_FORMAT.md` | One-page BA-facing reference for the three draft templates (data, projection, context). Complements `DCI.md` — `DCI.md` is the "why", `DRAFT_FORMAT.md` is the "how". |
| `README.md` | Developer setup: install, build, link into a project. |

---

## New draft schema (BA-authorable, DCI-native)

Drafts come in three kinds — **data**, **projection**, **context** — reflecting DCI. Each kind has its own template. No code-level tokens appear in any of them. Types are either resolved from the shared manifest or introduced locally with a prose definition. Return shapes and parameter types are inferred from scenarios.

### Context draft (`drafts/contexts/<name>.md`)

A context draft is the specification of one scenario enactment. Every section below is DCI-native.

```
# Context: <Name>

## Purpose
One paragraph: what scenario this context enacts, in business terms.

## Role Players
- <role>: <data or projection name from manifest> — one-line responsibility

## Props
- <prop>: <description> (scalar configuration only; never a collaborator)

## Role Methods
### <role>
- <method name>: <what this role does in this enactment, in business language>
  (Role methods are the responsibilities of the role in this scenario.
  They are how each role plays its part. The BA writes them in the
  language of what the role *does*, not how it is implemented.)

## Functionalities
### <functionality name>
<Prose describing the enactment: which roles do what, in what order,
with what guarantees and extensions. Reference role methods by the
names introduced above.>

## Scenarios
### <scenario name>
Given: <role players in specific states, data values>
When: <functionality invocation with concrete values>
Then: <observable outcome; state of role players; returned values>
```

The Role Methods section is load-bearing. It is where the BA decomposes the enactment into per-role responsibilities, which is the DCI design act. The Functionalities section then composes those responsibilities into named use cases.

### Data draft (`drafts/data/<name>.md`)

```
# Data: <Name>

## Purpose
One paragraph: what domain concept this represents.

## Attributes
- <name>: <prose description — type inferred from scenarios or manifest>

## Invariants
- <prose invariant> (e.g. "an Amount is never negative")

## Operations
### <operation name>
<Prose describing what this operation does, expressed as a business rule.>

## Scenarios
### <scenario name>
Given: <data value>
When: <operation invoked with concrete values>
Then: <resulting value or observable effect>
```

### Projection draft (`drafts/projections/<name>.md`)

```
# Projection: <Name>

## Purpose
One paragraph: what view this projection provides over underlying data.

## Source
- <prose or reference to underlying data>

## Queries
### <query name>
<Prose describing what is observable through this projection.>

## Scenarios
### <scenario name>
Given: <underlying data state>
When: <query invoked>
Then: <returned view>
```

Projections are read-only; prepare enforces this by refusing to accept a projection draft that describes mutation.

## New prepared YAML schema (v2)

Additions to the current `reen.prepare/v1` shape:

- `kind` — explicit marker: `"data"`, `"projection"`, or `"context"`. Parser and verifier branch on this.
- `role_methods[]` (context only) — the canonical list of role methods, grouped by role, with `canonical_name` (`<role>_<method>`), `signature_hint` (inferred from scenarios), and `citation`.
- `synthesised_tests[]` — test cases derived from scenarios and prose. Each has `source` (`"scenario"` or `"invariant"`), `given/when/then` in resolved DCI vocabulary, `citation` pointing to the draft location.
- `vocabulary_bindings[]` — each name used in the draft with its resolution (role player | role | role method | data | projection | prop | primitive | ambiguous).
- `ambiguities[]` — messages in business (DCI) language, not code language. Each has a suggested resolution the refine dialogue can offer.

Schema bumps to `reen.prepare/v2`. Old prepared YAML is not migrated; snake and money transfer re-prepare from scratch.

---

## Phases

### Phase 0 — audit and strip

**Goal:** get to a minimal working tree before introducing anything new.

1. Tag current HEAD as `v0-archive`; push the tag.
2. Delete the files listed under "What gets removed → Source files (delete)".
3. Remove the deleted CLI subcommands from `main.rs`; make the binary compile again with the reduced surface.
4. Review `draft_refine.rs` line-by-line; strip LLM pathways, keep deterministic review categories.
5. Delete `DCI Primer.md`. Replace `CLAUDE.md` with a two-line placeholder.
6. Keep `tests/Connor` (in scope for Phase 4) and `tests/world data` (retained but deferred out of Phase 4). No test-project deletions at this stage; the Phase 0 strip is Rust-side only.
7. Ensure `cargo build` passes on the reduced tree.

**Exit criteria:** repo compiles, contains only surviving Rust files, and has no references to removed agents or CLI commands.

### Phase 1 — new schema and prepare

**Goal:** end-to-end `reen prepare` on the three new draft kinds, producing v2 YAML with synthesised tests and full DCI structure preserved.

1. Specify v2 schema in `prepared.rs` + `prepared_contracts.rs`. The schema distinguishes data, projection, and context artefacts. Context artefacts carry `role_players[]`, `role_methods[]` (grouped by role), `props[]`, `functionalities[]`, `scenarios[]`. Data artefacts carry `attributes[]`, `invariants[]`, `operations[]`, `scenarios[]`. Projections carry `source`, `queries[]`, `scenarios[]`.
2. Rewrite `draft_parser.rs` for the three BA-authorable kinds. Parser is intentionally permissive on prose; strict on section headers. Each kind has its own parse path; the frontmatter or filename path determines which.
3. Extend `manifest.rs` to act as the DCI ubiquitous vocabulary — it is the authoritative list of data and projection names available to any context draft. Prepare refuses to resolve a name not present in the manifest or locally introduced in the same draft.
4. Rewrite `prepare.rs`:
   - Load the DCI vocabulary manifest.
   - For context drafts: resolve every role player type, every role-method reference in functionality prose, every data name used in scenarios, against manifest + local introductions + the context's own role names. Derive the canonical `<role>_<method>` name for each role method and carry it into the prepared YAML.
   - For data drafts: resolve attribute references; extract invariants and operations.
   - For projection drafts: resolve the source data and extract queries; refuse mutation-shaped prose.
   - Extract `Scenarios` into `synthesised_tests[]` with `source: "scenario"`.
   - Invoke `test_synth.rs` to produce additional tests from prose invariants, extensions, and guarantees, tagged `source: "invariant"`.
   - Collect every unresolved name into `ambiguities[]` with a business-language question.
5. Implement `test_synth.rs` — single-shot Claude API call with: draft prose, resolved DCI vocabulary, existing scenario tests, the artefact kind. Output: structured test cases conforming to v2 schema. Prompt is strict about output format and explicitly DCI-aware (tests exercise role-method responsibilities and functionality enactments, not arbitrary behaviour). Failures surface as `ambiguities[]`, not malformed tests.
6. Unit tests for prepare: vocabulary lookup, role-method canonicalisation, scenario extraction, ambiguity formatting, projection-mutation rejection. Golden-file tests for v2 YAML output on minimal fixture drafts — one per kind.

**Exit criteria:** `reen prepare` on a hand-written minimal context draft produces valid v2 YAML with role methods correctly decomposed per role, at least one synthesised test from an invariant, and flags at least one planted ambiguity in business language. Equivalent minimal data and projection drafts also prepare cleanly.

### Phase 2 — scaffold and verify

**Goal:** deterministic stubs and mechanical alignment checking.

1. Trim `codegen.rs`: drop any agent interaction; emit `.rs` files with exact field sets and method signatures derived from v2 YAML; bodies are `todo!()`; allow private helper functions to be added later by the implementer.
2. Build `verify.rs` using `syn`:
   - Parse the `.rs` file.
   - For each struct, assert field set = `role_players[*].name ∪ props[*].name`, with matching types.
   - For each `impl` on a context, assert public method set = `functionalities[*].name` and private method set ⊇ `role_methods[*].canonical_name` (the `<role>_<method>` form derived in prepare).
   - For each role method, assert the DCI-prescribed signature shape: receiver is `&self` (or `&mut self` only when the body calls a `&mut` method on the role player), first explicit parameter is `<role>_: &<RolePlayerType>`, then the business parameters.
   - Check receiver mutability: `&mut self` iff `mutable: true` for functionalities; `&self` for role methods (with the documented escape hatch).
   - For data and projection artefacts, assert their respective method sets match the prepared YAML's `operations[]` or `queries[]`.
   - Emit structured diff on failure, in DCI vocabulary ("role method `payer_debit` is missing from the context") rather than in generic code terms.
3. Wire `reen verify <path>` subcommand with exit code 0 on match, 1 on drift, 2 on parse error.

**Exit criteria:** on a minimal prepared YAML, `reen scaffold` produces a `.rs` file that `reen verify` accepts; and a modified `.rs` file that drops a field or renames a method is rejected with a structured diff.

### Phase 2.5 — runnable scaffold via App artefact

**Goal:** the scaffold Phase 2 produces actually runs. Today it emits a library crate with no `[[bin]]` target and no `main.rs`, so `cargo run` in a fresh scaffold fails. Phase 2.5 introduces a first-class `App` artefact kind whose prepared entry carries just enough to parameterise the scaffolder, and extends codegen to emit a runnable binary.

**Why this phase exists.** Snake has a `drafts/app.md` that declares `Application Kind: cli_app` and describes its startup sequence, runtime, and main-loop behaviour, but nothing in the Reen pipeline reads that file. The scaffolder only walks data / projections / contexts / operations and emits `.rs` files under `src/`. Cargo.toml and `main.rs` are hand-maintained, so a freshly scaffolded project cannot be run and the app.md file — which is the one place the BA *has* described the application envelope — is informationally live but mechanically dead. Phase 10's web-app codegen is about to need a sibling App kind (`web_app`) with its own scaffold rules (workspace layout, multiple crates, WASM target); bolting that onto a codegen path that doesn't yet know CLI apps exist would be worse than fixing the gap first.

**Design**

1. **New artefact kind.** `ArtifactKind::App` in `draft_parser.rs`. Filename convention: `drafts/app.md` (exactly one per project; multiple is an ambiguity).
2. **Minimal schema.** The prepared App entry carries:
   - `kind`: `cli_app` | `tui_app` (Phase 10 adds `web_app`).
   - `entry_context`: the context the application launches into, resolved against the manifest.
   - `entry_functionality`: the functionality on that context invoked at startup. Defaults to the only public functionality if unambiguous.
   - `dependencies[]`: crate names the BA declares the app needs (`crossterm`, `chrono`, ...). These feed Cargo.toml.
   - Everything else in app.md (Startup Sequence, Main Loop Behavior, Collaborators and Wiring) stays as BA prose. It is informational, not schema-enforced in this phase. If a later phase wants teeth, add sections then.
3. **Scaffolder.**
   - Emits `src/main.rs` with a minimal entry stub: construct the entry context with its role players (left as `todo!()` for the implementer to wire), call the declared entry functionality. The body the BA actually needs to write lives inside the context — main.rs is just the envelope.
   - Patches `Cargo.toml`: adds `[[bin]]` with `name = <package-name>` and `path = "src/main.rs"`. Idempotent — re-running scaffold does not duplicate the entry or clobber user-added dependencies.
   - Adds declared dependencies if absent. Never removes dependencies — that is the user's call.
4. **Verify.** If an App artefact exists, `src/main.rs` must exist and its `main()` must construct the declared entry context. If Cargo.toml is missing a `[[bin]]` entry, verify flags it as drift in DCI language ("the app declares `snake` as the binary entry but Cargo.toml has no `[[bin]]` section").
5. **Manifest.** The App entry does not appear in the manifest (there is only one; it is addressable by kind, not by name). References from other drafts to "the app" are not expected and would be smell — the Startup Sequence prose is the right home for that.
6. **Hooks.** PostToolUse Write on `drafts/app.md` triggers `reen prepare` (same pattern as other drafts).
7. **Migration.** Each test project (`snake`, `money transfer`, `Connor`, and `world data` if Phase 7 has landed) gets its `drafts/app.md` updated to the new App template. The existing hand-written `main.rs` + Cargo.toml `[[bin]]` are checked to match what scaffold would now emit. After migration, `reen scaffold` run fresh on each project produces a runnable binary.

**Exit criteria:** a project with a valid `drafts/app.md` scaffolds to a runnable binary (`cargo run` either succeeds or fails cleanly on `todo!()` inside a context body, never on a missing entry point); Cargo.toml carries a `[[bin]]` entry after scaffold; verify flags a mismatch between the declared entry_context and the actual `src/main.rs`; snake, money transfer, and Connor continue to build end-to-end after re-scaffolding; and Phase 10 can extend the kind enum with `web_app` and the scaffold switch with its branch without having to invent the App artefact at the same time.

**Out of scope.** Runtime behaviour — this phase does not generate an event loop, terminal setup, or UI rendering; the Main Loop prose remains implementation guidance for Claude Code, not for Reen. Multi-binary projects (one App per project). Web apps (`web_app` lands in Phase 10).

### Phase 3 — Claude Code integration

**Goal:** every manual step is available as a slash command; hooks enforce spec alignment automatically.

1. Write `.claude/skills/reen-dci-authoring/SKILL.md` — BA-facing DCI thinking: the three artefact kinds, roles vs role players, role methods as per-role responsibilities, scenarios as enactments, manifest as shared vocabulary. Worked mini-example. Trigger fires on `drafts/**.md` and on mention of "reen draft", "DCI", or "scenario".
2. Write `.claude/skills/reen-draft-format/SKILL.md` — the three draft templates, section grammar, common pitfalls (e.g. mixing role and role player, sneaking types into the BA draft). Trigger same as above.
3. Write `.claude/skills/reen-dci-code/SKILL.md` — Rust realisation of DCI under Reen: `<role>_<method>` naming, receiver rules, spec-alignment invariants, what the verifier enforces. Trigger on `.rs` files under `src/contexts/`, `src/data/`, `src/projections/`.
4. Write the five slash commands. Keep them thin — each is ≤ 40 lines and mostly shells to `reen`.
5. Write `.claude/agents/reen-refiner.md` — read-only on drafts, allowed to propose edits via a reply that the command's frontmatter writes out. Cheap model (Haiku). System prompt anchors on DCI authoring vocabulary so ambiguity questions are phrased in DCI terms.
6. Write `.claude/hooks.json`:
   - PostToolUse Write on `drafts/**.md` → `reen prepare <affected-draft>`.
   - PostToolUse Write on `src/**.rs` → `reen verify <affected-file> && cargo check -q`; on failure, surface the structured diff.
7. Write new `CLAUDE.md` (≤ 80 lines) pointing Claude Code at the skills, naming DCI as the authoring model, and stating the implementation contract.
8. Write `DCI.md` (2–4 pages) as the BA-facing canonical reference.
9. Write `DRAFT_FORMAT.md` (≤ 1 page) as the BA-facing template reference for the three draft kinds.

**Exit criteria:** in a test project, the user can invoke `/prepare`, `/refine`, `/scaffold`, `/implement` in Claude Code and drive a single draft from empty Rust to a verified, compiling implementation without running any CLI manually.

### Phase 3.5 — concreteness enforcement (code-token lint)

**Goal:** a deterministic gate that refuses drafts carrying code-level content, so the "BA-authorable" bar is enforced by Reen rather than left to migration discipline.

**Why this phase exists.** The first Phase 4 pass on snake and money transfer exposed a real failure mode: drafts were reshaped into the new schema structurally (headings, Role Methods section, scenarios) while still leaking Rust everywhere — explicit `Signature: ...` lines on role methods, full constructor signatures (`fn new(... ) -> Self`), type names (`Vec<Position>`, `&[Position]`, `Option<Food>`, `anyhow::Result`), code idioms (`saturating_sub`, `.log(10)`, `as u64`, `::`), and whole sections discussing integer widening between `u16` and `u32`. The DCI shape was satisfied; the DCI *vocabulary* was not. Prepare and `/refine` had no deterministic mechanism to notice, because `draft_refine.rs` only warned about vague phrases, not about concreteness-in-the-wrong-direction.

1. Extend `src/draft_refine.rs` with a code-token detector. Reject any draft (all three kinds) that contains, in role-method prose, functionality prose, or scenario Given/When/Then prose:
   - Explicit signature lines beginning with `Signature:` or fenced as `` `fn name(...)` ``.
   - Rust path syntax: `::` between identifiers (e.g. `Direction::Up`, `crossterm::terminal::Clear`).
   - Generic-type brackets around type identifiers: `Vec<...>`, `Option<...>`, `Result<...>`, `&[...]`, `HashMap<...>`.
   - Reference/mutability sigils applied to type names: `&self`, `&mut self`, `&T`, `&mut T`, `Box<...>`, `Arc<...>`, `Rc<...>`.
   - Turbofish (`::<...>`) and trait-bound syntax (`where`, `impl Trait`).
   - Primitive type tokens as standalone type annotations (`u8`, `u16`, `u32`, `u64`, `i32`, `i64`, `usize`, `isize`, `f32`, `f64`, `bool`, `char`, `str`, `String`) when appearing in sentences that are clearly type-annotating rather than domain prose ("the body is stored as `Vec<Position>`", "returns a `u32`"). The lint must distinguish "an amount of 250 DKK" (fine) from "amount: u32" (reject).
   - Method-call syntax on identifiers: `foo.bar()`, `ctx.game_state.score()`, `.unwrap()`, `.clone()`, `.iter()`, etc.
   - Macro invocations: `println!`, `print!`, `todo!`, `assert!`, `format!` and the broader `<ident>!(...)` shape.
2. The detector emits findings in the existing `DraftReviewFinding` shape with `category: Concreteness` and a message that names the offending token and quotes the surrounding phrase. Findings point the BA at what to rewrite, in DCI vocabulary ("role method `snake.body` specifies a Rust signature; describe the responsibility instead").
3. Wire the detector into `reen prepare` as a **blocking** check: if any code-token finding is present, prepare exits non-zero and emits the report. Prepare does not produce v2 YAML while a draft is still carrying code-level content. A `--allow-code-tokens` escape hatch is available for the one case that matters — migrating legacy drafts where the code notation is being preserved on purpose during a transitional commit — and prints a visible warning when used.
4. The lint lives in `draft_refine.rs` (or its successor `review.rs`) alongside the existing Completeness and Concreteness-vagueness checks. Unit-test it against:
   - A known-bad snake fragment (must flag `Signature: body(&self) -> &Vec<Position>`, `Direction::Up`, `saturating_sub`, `as u64`).
   - A known-good money-transfer fragment (must pass cleanly).
   - Borderline prose cases: "250.00 DKK" must pass; "returns a `u32`" must flag.
5. Document the lint in the `reen-draft-format` skill: a short "What not to put in a draft" section with the rejected token families and the in-domain alternatives.

**Exit criteria:** `reen prepare` on the pre-redo snake drafts exits non-zero with a concreteness report citing at least the Signature lines and the `::` paths; `reen prepare` on the money-transfer drafts passes the lint without changes; the lint has ≥ 90% line coverage in unit tests.

### Phase 4 — migrate test projects

**Goal:** three test projects — snake, money transfer, Connor — built under the new pipeline from BA-authorable drafts that pass the Phase 3.5 lint. `tests/world data/` and `tests/snake_gh/` are retained on disk but out of Phase 4 scope (distinct use cases — external-API-boundary codegen and repo ingestion — that don't fit the current four-artifact-kind pipeline).

**Ordering.** The first pass on snake and money transfer (before Phase 3.5 existed) reshaped the schema but left code-level content in the snake drafts. Redo snake first so it becomes the clean reference for the larger migrations, then re-check money transfer against the lint, then migrate Connor. Each project is its own Claude Code session with `/clear` before it starts.

**Draft acceptance bar (applies to every project).** Before `reen prepare` is run on any draft, the draft must read as business vocabulary:

- No `Signature:` lines anywhere. Role methods and functionalities are described as responsibilities, not as function shapes.
- No Rust type names in role-method, functionality, or scenario prose: no `Vec<T>`, `Option<T>`, `Result<T>`, `&[T]`, `&str`, `String`, `u8`..`u64`, `i32`..`i64`, `f32`/`f64`, `bool`, `usize`, etc., except when referring to a concept already defined in a manifest-bound data or projection draft by its domain name.
- No `::` path syntax, no turbofish, no `&`/`&mut` sigils, no `impl`/`dyn`/`where` tokens.
- No method-call prose on identifiers (`foo.bar()`, `.unwrap()`, `.clone()`).
- No macro invocations (`println!`, `todo!`, `format!`).
- No integer-widening or FFI-boundary discussion in prose — if an application-level draft needs to describe a boundary, it does so by naming the responsibilities on each side, not by specifying casts.

The lint enforces all of the above mechanically. The bar above is the *reason* the lint exists; the lint is the *mechanism*.

1. Per project, in order — **snake (redo first)**, then **money transfer** (lint sweep), then **Connor**:
   - Rewrite every draft to the new DCI-native schema, using the right template for each kind (data, projection, context). For contexts specifically: recover the role methods that were previously implicit in flow prose and surface them as an explicit Role Methods section grouped by role. Remove all signatures, types, and method body references. Add Scenarios sections with concrete Given/When/Then.
   - Run `reen prepare` purely to exercise the Phase 3.5 lint before touching anything else; fix every code-token finding before proceeding. No `--allow-code-tokens`.
   - Consolidate the manifest so it is the single source of DCI vocabulary for the project.
   - Delete old `drafts/prepare/` contents.
   - Run `reen prepare` for real; resolve any remaining ambiguities through `/refine`.
   - Run `reen scaffold`.
   - Implement via Claude Code until `reen verify` and `cargo test` pass.
   - Commit with a message that names the project and the Phase 4 step (e.g. `phase 4: snake redo under BA-authorable bar`). One project per commit; do not batch.
2. Rewrite integration tests. Replace the existing `integration_test.rs` with:
   - `prepares_without_ambiguity`: after prepare, `ambiguities[]` with blocking severity is empty.
   - `scaffold_verifies`: scaffold output passes `reen verify`.
   - `examples_run`: synthesised tests from scenarios compile and pass when run against a golden reference implementation (committed alongside the draft).
   - `planted_ambiguity_caught`: a deliberately underspecified variant of a draft raises the expected ambiguity.
   - `planted_drift_caught`: a deliberately-modified `.rs` (extra field) fails `reen verify` with the expected diff.
3. Add a workspace-level test that runs `reen prepare && reen scaffold && reen verify` across all kept test projects as a smoke test.

**Exit criteria:** `cargo test` inside each of the three in-scope projects is green; snake, money transfer, and Connor can be built end-to-end from their rewritten drafts; every draft in every in-scope project passes the Phase 3.5 code-token lint without `--allow-code-tokens`.

### Phase 5 — auditor

**Goal:** post-implementation discrepancy detection.

1. Write `.claude/agents/reen-auditor.md`:
   - Read-only.
   - Receives: draft markdown, prepared YAML, implementation `.rs` file(s).
   - System prompt is DCI-aware: the auditor checks that every role method's responsibility is observable in the corresponding `<role>_<method>` fn, every functionality's enactment matches its prose, and every invariant is enforced. Discrepancies are cited in DCI vocabulary.
   - Output schema: a list of discrepancies, each with `kind` (`role_method_responsibility_missing` | `functionality_divergent_flow` | `invariant_not_enforced` | `code_not_in_spec`), `spec_citation`, `code_citation` (file:line), `description`, `severity`.
2. Write `.claude/commands/audit.md`:
   - Takes a context name.
   - Loads the relevant files, invokes the auditor, renders the report as markdown.
   - Does not auto-apply changes. Triage is manual or delegated back to `/implement` with the report attached.
3. Add an integration test `auditor_catches_planted_discrepancy`: take a passing implementation, remove a guard clause that the spec's invariant requires, invoke the auditor, assert the discrepancy appears.

**Exit criteria:** auditor flags known planted discrepancies in a test project and produces zero false positives on a known-good implementation.

### Phase 6 — documentation

**Goal:** minimal, accurate, BA- and dev-facing docs.

1. Finalise `CLAUDE.md` (already drafted in Phase 3; polish).
2. Finalise `DCI.md` — the canonical BA-facing DCI reference. Target 2–4 pages. Cover: data/context/projection distinction, roles vs role players, role methods as responsibilities, functionalities as composed enactments, scenarios as concrete enactments, the manifest as shared vocabulary. Two worked mini-examples (one from snake, one from money transfer) showing a small context draft from scratch.
3. Finalise `DRAFT_FORMAT.md` — the three templates with annotated examples. One page.
4. Write `README.md`:
   - What Reen is (one paragraph, naming DCI explicitly).
   - Install, build, link to a project.
   - The five slash commands and when to use each.
   - Pointer to `DCI.md` and `DRAFT_FORMAT.md` for BAs.
5. Ensure no references remain to removed subcommands, the old DCI Primer, or old schema version.

**Exit criteria:** a new BA can go from zero to a valid context draft using only `DCI.md` and `DRAFT_FORMAT.md`. A new developer can go from `git clone` to running snake end-to-end using only the README and the three skill files.

### Phase 7 — external API consumption (API ingestion)

**Goal:** a deterministic path from an OpenAPI (or equivalent) spec to a usable set of Reen artifacts that describes, constrains, and enables consumption of an external API from inside a Reen project. The authored surface is small and BA-legible (a boundary draft that states *what this API is for*). The generated surface is large, machine-owned, and implementation-optimised. Generated data types and operations are referenceable from DCI drafts under the same scoping rules that govern intra-project references, and the generated set acts as a whitelist — you cannot reference an external type or operation that the API does not expose.

**Why this phase exists.** Phase 1–6 cover domain modelling end-to-end within a project. They do not cover the seam where the project reaches out to an existing, externally specified API. Treating API types as hand-authored data drafts is wrong on two counts: it duplicates the spec (drift), and it asks a BA to transcribe fields that no one intends to change. Treating API types as raw Rust generated into `src/` is also wrong: contexts, projections, and refine dialogue have no way to reference them in DCI vocabulary, and the prepare-time vocabulary manifest cannot see them. Phase 7 places the API between these extremes: one BA-authored boundary document per API captures purpose and scope; everything else is generated into the existing draft vocabulary so the rest of the pipeline treats API artifacts like any other.

**Design.**

*Authored artifacts (committed):*

- `drafts/apis/<name>.md` — boundary draft. Human-authored, BA-legible. One paragraph on purpose, the authoritative upstream source (URL + local-snapshot path), an explicit operations whitelist (names from the OpenAPI spec; if absent, all operations are ingested), any cross-cutting notes (auth mode, rate-limiting, pagination quirks). No types, no field lists, no code tokens. Subject to the Phase 3.5 lint.
- `drafts/apis/<name>.yaml` — committed OpenAPI snapshot. Verbatim copy of the upstream spec at the version the project consumes. This is the input to ingestion; committing it means ingestion is reproducible without network access and that spec drift is observable in `git diff`.

*Generated artifacts (gitignored under `drafts/.apis/`):*

- `drafts/.apis/data/<api>/<TypeName>.md` — one file per referenced schema in the OpenAPI spec. Uses the existing data-draft markdown template so every downstream consumer (parser, prepare, manifest resolution, lint, refine) works unchanged. The Purpose paragraph cites the OpenAPI `$ref` path; Attributes are derived from OpenAPI properties; Invariants are derived from OpenAPI constraints (`required`, `minLength`, `enum`, etc.); Operations and Scenarios sections are empty for ingested types — ingested data is a vocabulary surface, not a behavioural one.
- `drafts/.apis/operations/<api>/<OperationName>.yml` — one file per operation. YAML, not markdown: these are not meant to be read or written by BAs, they are implementation specifications for Reen's scaffolder. Schema shown below.
- `drafts/.apis/manifest.yml` — per-project index of every ingested API: the set of data types, the set of operations, the source OpenAPI hash. Consulted during prepare so the ubiquitous-vocabulary manifest sees ingested types as resolvable names.

*Operation spec schema (`reen.operation/v1`):*

```
schema: reen.operation/v1
api: <api name>                          # matches the boundary draft filename
name: <OperationName>                    # matches the OpenAPI operationId
transport:
  kind: rest | websocket | grpc
  url: <base or exact URL>
  method: GET | POST | ...               # REST only
  path: /path/with/{params}              # REST only
auth:
  kind: api_key_header | api_key_query | api_key_body | bearer | none
  field: <header or field name>          # when applicable
input:
  kind: none | data                      # "data" means a single ingested type
  data_type: <TypeName>                  # when kind is "data"
output:
  kind: none | single | list | stream_of
  data_type: <TypeName>                  # when not "none"
errors:
  - data_type: <TypeName>                # ingested error types, in priority order
notes:
  - <free-form implementation hint>
```

The schema is intentionally narrow. v1 does not model pagination, batching, retry budgets, or rate-limit metadata. Operations that require those things capture them as `notes[]` so the scaffolder treats the reader (or Claude Code during `/implement`) as the authority on how to realise them. A follow-up phase can promote recurring note shapes to structured fields.

*Reference graph.*

The DCI scoping rules extend to ingested artifacts without special cases:

- data → data (local or ingested)
- projections → data + projections
- contexts → data + projections + operations
- operations → data (ingested or local, to declare input/output/error shapes)

Operations are not referenceable by data or projections. They are a context-level concern — contexts coordinate the operation call, not the data itself.

The ingested set is the whitelist. If an OpenAPI spec does not expose a type or operation, it does not exist for downstream drafts. Prepare refuses to resolve any `@<api>/<Name>` reference that is not in `drafts/.apis/manifest.yml`.

*Manifest binding.*

Ingested types and operations appear in the vocabulary manifest under a namespace that matches the boundary draft name (e.g. `aisstream/AisStreamMessage`, `aisstream/Subscribe`). `manifest.rs` gains a capabilities section for `apis[]`; each entry points at the generated manifest file and is consumed by `prepare` when resolving names prefixed by the api namespace.

*Lint exemption.*

`drafts/.apis/**` is generated; the code-token lint does not scan it. Authored boundary drafts (`drafts/apis/<name>.md`, no dot) are linted normally.

**Steps.**

1. **Model the new artifact.** Add `ArtifactKind::Operation` to `draft_parser.rs`. Operation specs are YAML from the outset, so the parser path for operations deserialises directly via `serde_yaml` into an `OperationSpec` struct; there is no prose layer. Add a minimal parser for boundary drafts (`drafts/apis/<name>.md`) with sections `Purpose`, `Source`, `Operations`, `Notes`.
2. **Extend the prepared schema.** Operations produce a prepared entry too, so `reen.prepare/v2` gains an operation kind. For operations, prepare's job is simply to validate that every referenced data type exists in the api's generated manifest, and to surface errors in DCI vocabulary. No test synthesis for operations.
3. **Build ingestion.** New CLI subcommand: `reen apis ingest [<api-name>]`. With no argument, ingests all boundary drafts in `drafts/apis/`. Per boundary draft:
   - Load `drafts/apis/<name>.yaml` (the committed OpenAPI snapshot).
   - Validate the OpenAPI version and basic shape.
   - Filter to the operations whitelist (if present in the boundary draft); otherwise take all.
   - For every schema referenced transitively by whitelisted operations, emit a generated data draft under `drafts/.apis/data/<name>/<TypeName>.md`.
   - For every whitelisted operation, emit an operation spec under `drafts/.apis/operations/<name>/<OperationName>.yml`.
   - Write `drafts/.apis/manifest.yml` with the full set and the OpenAPI snapshot hash.
   - Ingestion is deterministic: same OpenAPI snapshot → byte-identical output. A change in the `.yaml` snapshot is the only thing that should change the `.apis/` tree; the gitignore keeps the tree out of the working-copy commit, but a CI job (or `reen apis ingest --check`) can assert freshness by re-ingesting and diffing.
4. **Gitignore and hooks.** Add `drafts/.apis/` to `.gitignore`. Extend `.claude/hooks.json` so PostToolUse Write on `drafts/apis/**.md` or `drafts/apis/**.yaml` runs `reen apis ingest <api>`. The existing PostToolUse on `drafts/**.md` continues to cover the generated data drafts via their existing trigger, but with a lint exemption for `.apis/**` so they never fail the code-token check.
5. **Wire prepare to the api manifest.** During vocabulary resolution, prepare consults `drafts/.apis/manifest.yml` for every namespaced reference (`@<api>/<Name>`). Unresolved references become ambiguities phrased in DCI terms ("the context references `@aisstream/SubscribeMessage`, which is not in the aisstream API manifest — either add the operation to the whitelist or correct the name").
6. **Scaffold operations.** `reen scaffold` learns to emit a thin Rust client per operation. The generated file sits under `src/apis/<api>/<operation>.rs` with `todo!()` bodies. Struct fields mirror the operation spec: transport config, auth config, input/output/error types. The scaffold is deliberately minimal — Claude Code fills the body during `/implement`, guided by the operation spec and the notes[].
7. **Verify operations.** Extend `reen verify` to cover the operations case: struct fields = spec's transport + auth + input/output/error; public method set = `{ call }` for single-response kinds, `{ subscribe, next }` or similar for stream kinds (the naming table is spelled out in a new short section of `reen-dci-code`). Parser and diff messages use DCI-adjacent vocabulary ("the `Subscribe` operation on the aisstream api is missing the auth field `APIKey`").
8. **Rebuild world-data.** Use `tests/world data/` as the test case. Rewrite its existing `drafts/apis/aisstream.md` under the new boundary-draft template. Commit `drafts/apis/aisstream.yaml` as the OpenAPI snapshot. Run `reen apis ingest aisstream`. Wire a small context that exercises at least one ingested operation end-to-end, to prove the reference graph works from contexts through operations to ingested data.
9. **Document.** Add a short "External APIs" section to `README.md`, a BA-facing note in `DRAFT_FORMAT.md` that describes only the boundary draft (generated artifacts are off-limits for authoring), and a dev-facing note in `reen-dci-code` covering the scaffold shape and verify expectations for operations. `DCI.md` gains a one-paragraph sidebar explaining where operations sit in the reference graph.

**Exit criteria:** world-data builds end-to-end from the committed boundary draft + OpenAPI yaml. Running `reen apis ingest aisstream` on a fresh clone produces `drafts/.apis/data/aisstream/` and `drafts/.apis/operations/aisstream/` deterministically. A context draft in world-data successfully references `@aisstream/<OperationName>` and `@aisstream/<TypeName>` and prepares without ambiguity. `reen scaffold` emits a compilable operation client; `reen verify` accepts it; the Phase 3.5 lint continues to pass on the authored boundary draft and is not run against `drafts/.apis/**`.

### Phase 8 — packaging and distribution (`reen init`)

**Goal:** one command brings the full Reen authoring and realisation experience into any project. A user who has installed the reen binary can run `reen init` in a fresh or existing Rust project and, without additional steps, get the skills, slash commands, subagents, hooks, canonical docs, starter manifest, and directory layout that Phases 3, 6, and 7 defined. The same command, with `--upgrade`, pulls newer versions of those bundled files when the reen binary is updated, without clobbering the user's drafts or hand-edited local files.

**Why this phase exists.** The value of Phases 3–7 is locked in this repository today. A new project that wants to adopt Reen would have to hand-copy `.claude/`, `DCI.md`, `DRAFT_FORMAT.md`, and assorted scaffolding — and would then drift from upstream as Reen evolves. Packaging the bundle into the reen binary itself makes adoption a one-liner, makes upgrades observable (a diff per managed file), and keeps a single source of truth.

**Design.**

*Bundle layout (source of truth in-repo).*

A new top-level `templates/` directory holds the canonical content that ships inside the reen binary:

```
templates/
  .claude/
    skills/reen-dci-authoring/SKILL.md
    skills/reen-draft-format/SKILL.md
    skills/reen-dci-code/SKILL.md
    commands/prepare.md
    commands/refine.md
    commands/scaffold.md
    commands/implement.md
    commands/audit.md
    agents/reen-refiner.md
    agents/reen-auditor.md
    hooks.json
  DCI.md
  DRAFT_FORMAT.md
  CLAUDE.md
  reen.manifest.yml        # starter manifest
  .gitignore.fragment      # lines to merge into an existing .gitignore
  drafts/.gitkeep
  drafts/data/.gitkeep
  drafts/projections/.gitkeep
  drafts/contexts/.gitkeep
  drafts/apis/.gitkeep
  drafts/components/.gitkeep   # Phase 9 — web-app component drafts
  drafts/brands/.gitkeep       # Phase 9 — brand drafts (plural: multi-brand projects allowed)
  drafts/routes/.gitkeep       # Phase 9 — routing-table drafts
  drafts/apps/.gitkeep         # Phase 12 — multi-app directory form; supersedes the singleton drafts/app.md
```

The `drafts/` subdirectory list is the canonical filesystem layout for every artefact kind Reen knows about; it grows whenever a phase introduces a new kind. Kinds listed but empty in a given project are fine — `.gitkeep` keeps the structure committed without forcing authorship. Any phase that adds a new artefact kind must retrofit this list (and `templates/bundle.toml`) as part of its own scope; the contributor checklist in `CLAUDE.md` spells this out.

Contents are embedded at build time via the `include_dir` crate (or equivalent), so the compiled `reen` binary carries the templates with it. No network access at init time; no parallel template repo to keep in sync; no post-install scripts.

*File classification (drives upgrade behaviour).*

- **Managed** — reen owns these; overwritten on `--upgrade` unless the user has edited them. Everything under `.claude/`, `DCI.md`, `DRAFT_FORMAT.md`.
- **Seed** — reen drops these once, then never touches them. `CLAUDE.md`, `reen.manifest.yml`, every `drafts/**/.gitkeep`, and the user-level `.gitignore` merge.

The classification is declared explicitly in a `templates/bundle.toml` index so the rule set is data-driven rather than hardcoded in upgrade logic.

*Bundle manifest (on-disk, per project).*

`reen init` writes `.reen-bundle.yml` at the project root:

```
schema: reen.bundle/v1
reen_version: <binary version at install>
installed_at: <UTC timestamp>
managed_files:
  - path: .claude/skills/reen-dci-authoring/SKILL.md
    sha256: <hash at install>
  - path: DCI.md
    sha256: <hash at install>
  # … one entry per managed file actually written
```

On `--upgrade`, the upgrader consults this manifest to detect (a) managed files removed in the new bundle (delete them), (b) managed files added in the new bundle (write them), (c) managed files changed in the new bundle where the on-disk hash still matches the stored install hash (overwrite), and (d) managed files changed in the new bundle where the on-disk hash differs from the stored install hash (the user has edited them — refuse to overwrite without `--force` and print a clear diff summary).

*CLI surface.*

```
reen init                     # initial install in the current directory
reen init --path <dir>        # install into another directory
reen init --upgrade           # refresh managed files to the current bundle
reen init --check             # dry-run report of what upgrade would do
reen init --force             # allow overwriting user-edited managed files on upgrade
```

`reen init` refuses to run if any managed file already exists and no `.reen-bundle.yml` is present — the user is told to run `--upgrade`. Seed files that already exist are left alone silently.

*Interaction with the rest of reen.*

Templates are authored once and the reen repo itself dogfoods them: running `reen init --upgrade` inside the reen repo regenerates the repo-root `.claude/`, `CLAUDE.md`, `DCI.md`, `DRAFT_FORMAT.md` from `templates/` byte-for-byte. A CI check asserts that after `cargo build` + `reen init --upgrade` inside the repo, `git status` reports no changes. This is the self-consistency gate — the same one that protects spec/code alignment elsewhere in reen, applied to docs and skills.

*What Phase 8 does not install.*

- No Rust source files. Reen is a tool consumed from the user's `PATH`; it does not inject itself as a crate dependency.
- No `README.md` in target projects. Reen has opinions about drafts, not about how a project documents itself.
- No starter drafts. The directory structure is scaffolded; authoring is the BA's work, and opinionated seed drafts would mislead.

**Steps.**

1. **Add the templates directory.** Create `templates/` at repo root and populate it by moving the canonical Phase 6/7 assets (`.claude/`, `CLAUDE.md`, `DCI.md`, `DRAFT_FORMAT.md`) into it. Author a starter `templates/reen.manifest.yml` (empty DCI vocabulary) and `templates/.gitignore.fragment` (lines: `drafts/prepare/`, `drafts/.apis/`, `target/`). Author `templates/bundle.toml` as the file classification index.
2. **Embed the bundle.** Add `include_dir = "0.7"` (or the maintained equivalent) to `Cargo.toml`. Introduce `src/bundle.rs` that exposes a typed view over the embedded directory: iterate files, read contents, read classification from `bundle.toml`, compute hashes.
3. **Implement `reen init`.** New module `src/init.rs`. Responsibilities: refuse to run on an already-initialised project without `--upgrade`; materialise managed and seed files; merge `.gitignore.fragment` into the project's `.gitignore` (creating one if absent, appending missing lines if present); write `.reen-bundle.yml`; print a summary of what was created.
4. **Implement `reen init --upgrade`.** Read `.reen-bundle.yml`; diff against the embedded bundle; apply the add/remove/update rules above; refuse to overwrite edited managed files without `--force`. After a successful upgrade, rewrite `.reen-bundle.yml` with the new hashes and binary version.
5. **Implement `reen init --check`.** Same diff logic as `--upgrade` but writes nothing. Exit code 0 if up to date, 1 if changes are pending, 2 if user edits would be overwritten (the `--force` case).
6. **Dogfood the bundle.** Run `reen init --upgrade` inside the reen repo itself. Confirm `git status` is clean. Add a `cargo test` integration test — `dogfood_is_clean` — that invokes `reen init --upgrade` in a clone-under-tempdir of the repo's templates vs. the repo root and asserts byte-identity.
7. **Migrate the test projects.** Running the test projects under `tests/` through `reen init` replaces their hand-maintained `.claude/` copies. Each project keeps its own `drafts/`, `reen.manifest.yml`, and committed implementations; everything else comes from the bundle. A small migration commit per project.
8. **Documentation.** Update the reen `README.md` with the install + init instructions (`cargo install reen` → `cd project && reen init`). Add a one-paragraph "Upgrading" section covering `reen init --upgrade` and `--check`. Update `SETUP.md` (from Phase 6) to mention that `reen init` replaces the copy-skills steps.
9. **Publishing prep.** Audit `Cargo.toml` package metadata (description, license, repository, readme, keywords). Run `cargo publish --dry-run`; fix any warnings. Do not publish in this phase — the act of publishing is a separate step the user chooses when ready.

**Exit criteria:** `reen init` in a fresh empty directory produces a ready-to-use project whose `.claude/` matches the bundle and whose `.reen-bundle.yml` reflects the current binary version. `reen init --upgrade` in the reen repo itself leaves the repo byte-identical — the CI check passes. A user who has edited `DCI.md` and runs `reen init --upgrade` sees a clear refusal with a diff summary and can opt into `--force`. All three in-scope test projects (snake, money transfer, Connor) build end-to-end after their migration commit.

### Phase 9 — web-app artefact schema

**Goal:** extend the Reen schema so a BA can specify web applications end-to-end in the user's mental model. Three new artefact kinds land: **component**, **brand**, and **routing table**. Screens are components (addressability is a property of the composition, not of the component itself). Stores are deliberately absent — mutable state across interactions is a framework concern, not a vocabulary concern; contexts return new data values and the runtime rebinds live references. No codegen in this phase.

**Why this phase exists.** Phases 0–8 cover terminal and backend applications. The UI surface is missing. The obvious framing — "components are data, themes are tokens, stores are the model" — is mostly wrong: components have slot and event surfaces that don't fit the data template; design tokens are a narrow subset of a brand; stores leak the developer's mental model into the BA's vocabulary. Phase 9 takes the DCI-purist path: UI artefacts describe *what the user sees and does*, brand captures *the identity being expressed* (voice, tone, visual, motion), and the mutation-across-interactions question is answered by DCI value semantics with the runtime carrying live references.

**Design.**

*Filesystem layout.* Drafts for the three new kinds live at canonical paths — `drafts/components/<Name>.md` (PascalCase filename matching the component name), `drafts/brands/<Name>.md` (plural directory since multi-brand white-label projects are allowed), and `drafts/routes/<name>.md` (snake_case; most projects have a single `main.md` routing table). The parser uses the path to determine the kind, the filename stem to determine the artefact name. Phase 8's bundle ships `.gitkeep` files for each of these directories, so `reen init` produces the full skeleton on day one.

*Three new kinds.*

**Component.** Reusable UI building blocks. Leaf components (Button, Label, Icon) just render; coordinating components (Form, List, LoginScreen) compose child components and bind their events to context invocations. Same template for both — the Composition section tells you which flavour you're looking at. Screens are components with a route mapped to them from the routing table; they carry no schema-level marker that says "I am a screen."

Authored surface:

```
# Component: <Name>

## Purpose
One paragraph: what this component lets the user see and do.

## Props
- <name>: <data/projection name or prose description> — what the prop conveys

## Slots
- <slot name>: <what the slot is for, who can fill it>

## Events
- <event name>: <what just happened; payload described in business language>

## Variants
- <variant name>: <when to use this variant>

## Composition
(Optional — present when the component coordinates child components)
- <child component>: <slot/prop bindings, in prose>
- when <child>.<event>: invoke <context>

## Invariants
- <invariant>

## Scenarios
### <name>
Given: <props, slots, ambient brand>
When: <user action or event>
Then: <observable outcome — what the user sees, what events fire>
```

No render prose beyond "what the user sees." No HTML, no JSX, no Tailwind class names, no style rules. Pixel-level specificity lives in a design source linked from the component's Purpose paragraph; the draft constrains behaviour, not appearance.

**Brand.** Brand identity as one coherent artefact: voice, tone, visual tokens, motion, iconography, imagery, logo rules. Not every section is machine-consumable, and that's intentional — brand is the source of truth both the scaffolder and the copywriter read. The scaffolder compiles the machine-consumable parts (colours, typography scale, spacing scale, motion curves) to whatever realisation the agent picks in Phase 10 (CSS custom properties, Rust constants, stylesheet modules); the human-facing sections (voice, logo rules, imagery style) travel unchanged as reference material.

Multiple brands are allowed (white-label); one is default. Tokens are named by role, not value — the value is a realisation concern. The draft describes "primary CTA background," not "#3355ff."

**Routing table.** URL-pattern-to-component binding. Most projects have one table; modular apps can have several, each mounting a namespace. Path parameters bind to data types from the manifest, resolved exactly like any other data reference. The form is YAML list-of-maps (`- path: /...`, `component: ...`, optional `params:` block) — a deliberate choice over path-as-map-key because list form is robust against paths containing `:`, `<`, `>`, `,`, or a leading `-`.

*Reference graph (revised and authoritative after this phase).*

- data → data
- projections → data + projections
- operations → data
- contexts → data + projections + operations + contexts + components + brand + routing table
- components → data + projections + brand + components + contexts
- routing table → components + data
- brand → nothing

Two pairs that were latent but are now explicit:

- **components → contexts.** A coordinating component binds child events to context invocations. The form component invokes the validator context when the input emits `changed`. This is how the "context-like component" pattern realises — not a schema distinction, just a use of the Composition section.
- **contexts → contexts.** One context can enact another. The application-level context that orchestrates a login invokes the session-setup context.

No store kind. No screen kind. Addressability moves to routing. State-across-interactions is handled by DCI value semantics: a context takes data in, returns updated data out, the runtime rebinds the live reference. The draft vocabulary never names storage.

*Concreteness lint extensions.*

Phase 3.5's lint applies to the new kinds with tightened rules:

- Component drafts may not contain: HTML tag names (`div`, `span`, `input`, ...), style/class attributes, CSS property syntax, Tailwind or other utility class names, JSX/template syntax, framework-specific hook names (`useState`, `useEffect`, `createSignal`), or raw hex / rgb / hsl colour values.
- Brand drafts may not contain hex/rgb/hsl colour values, specific font stacks, or px/em/rem/vh/vw measurements. Tokens are named by role; realisation is the scaffolder's job.
- Routing table drafts may not contain framework-specific route-matching syntax beyond the documented path template format; handler names must be component names, not function identifiers.

*Context classification (for Phase 10).* Every context's prepared entry carries a `realisation_target` field computed by prepare from the transitive dependency graph: `client_also` (touches only data, projections, brand, and other contexts), `server_only` (transitively touches an authenticated operation or a secret-bearing transport), or `unknown` (graph cycle or unresolved dependency). Phase 10's WASM inference consumes this; Phase 9 only has to compute and store it correctly.

**Exit criteria:** `reen prepare` on minimal hand-written drafts of a leaf component, a coordinating component, a brand, and a single-route routing table produces valid v3 YAML. The reference-graph rules are enforced by prepare with clear error messages. The lint catches HTML tags, hex colour values, and framework-specific hook names on appropriate drafts. The authoring skills name the new vocabulary. No codegen yet — verify still operates only on pre-Phase-9 kinds.

### Phase 9.5 — prose-reference lint and routing-table parser refinements

**Goal:** tighten two rough edges surfaced during Phase 9 implementation before they calcify into Phase 10's codegen assumptions. No schema changes; behaviour and UX only.

**Why this phase exists.** Phase 9 introduced (a) a prose-reference check in prepare that scans draft prose for manifest names using whole-word matching and (b) a routing-table parser that accepts the YAML list-of-maps form (`- path: /...`). Both work. Both have specific, isolated sharp edges that are better fixed now than after Phase 10 builds on top of them.

**Design.**

*Prose-reference check — tightened matching, two-tier severity.*

The whole-word scan produces false positives on common English words that happen to match domain type names — "in order to" collides with an `Order` data type, "the user types their email" collides with a `User` data type. Over a realistic draft the false-positive rate dominates, the check becomes noise, and a BA learns to ignore it. The fix is to raise the bar on what counts as "probably a reference."

1. **PascalCase-only scan.** Restrict the whole-word regex to `\b[A-Z][A-Za-z0-9]*\b`. Manifest-bound names follow PascalCase by convention across every kind (data, projections, components, brand names, operation types). Role players, props, and scenario-local identifiers are lowercase by convention and context-local — never manifest references. Excluding them cuts the false-positive rate dramatically without missing legitimate references.
2. **Exclude match sites where references don't belong.** The scanner does not scan inside backtick-delimited code spans (drafts shouldn't contain them, but belt-and-braces), inside heading text (`# Component: Button` must not flag `Button` as unresolved — that's the draft's own title), and inside `Given:` / `When:` / `Then:` lines where the content is a concrete scenario value rather than prose (a body of `[(5,5), (4,5)]` has no references to scan).
3. **Stopword list for PascalCase tokens common at English sentence starts.** The scanner holds a stopword list: `The`, `A`, `An`, `This`, `That`, `These`, `Those`, `When`, `If`, `Then`, `Given`, `And`, `Or`, `But`, `So`, `For`, `On`, `In`, `At`, `Of`, `With`, `Without`. These collide with sentence-start capitalisation in English prose and are never legitimate manifest names. The list lives in code as a named constant, not in user config — reen does not invite users to tune the stopword set.
4. **Two-tier severity.** Split findings into high-confidence and low-confidence:
   - **High-confidence (blocking):** a reference using the explicit syntax `@<api>/<Name>` or `@brand/<token>` that doesn't resolve; or a PascalCase token that already appears resolved elsewhere in the same draft and then appears again somewhere it doesn't resolve (very likely a typo of a known reference).
   - **Low-confidence (informational):** a standalone PascalCase token not otherwise resolved in the same draft and not in the manifest. Listed in the prepared YAML's `ambiguities[]` with `severity: low`, printed in `reen prepare` output under a clearly separated "Possible prose references" heading, and never blocks prepare.
5. **Keep the check on by default.** No `--skip-prose-check` escape hatch; no opt-in flag. The tightened rules eliminate the need.

*Routing-table parser — keep list form, tighten skill + error message.*

The list-of-maps form (`- path: /...`, `component: ...`) is the right canonical form. The path-as-map-key alternative looks cleaner for simple cases but degrades on paths containing `:`, `<`, `>`, `,`, or a leading `-` — which forces quoted keys, which is uglier than the preamble it was trying to replace. Dual-form parsing (accept either) is a predictable source of ambiguity bugs and error-message difficulty. The list form stays as the only accepted shape.

1. **Skill documentation.** `.claude/skills/reen-draft-format/SKILL.md` (template and root twin) gains a dedicated routing-table section that shows the list form, says explicitly that it is the only accepted shape, and briefly explains why (YAML idiom, robust to path syntax). The section preempts the "can I write it like `/: { component: ... }` instead" question rather than leaving it as a gotcha.
2. **Targeted error message.** When the parser sees a routing-table body whose first non-blank, non-header line is a path-shaped token followed by `:` (i.e. the user attempted the map form), it fails with a specific message: *"Routes must be a list of entries, each starting with `- path: <url>`. The map form (`/: { component: ... }`) is not supported — see the reen-draft-format skill for the routing-table template."* Generic YAML parser errors are caught and rewritten; the BA sees the actionable message rather than a `serde_yaml` stack trace.

**Steps.**

1. Replace the whole-word scan in the prose-reference checker with the PascalCase-only regex. Add unit tests covering: a PascalCase token that resolves (passes), a PascalCase token in the stopword list (ignored), an `@api/Name` reference that doesn't resolve (blocking), a PascalCase token that appears once resolved and once unresolved in the same draft (blocking on the unresolved site), a PascalCase token that appears only unresolved (informational).
2. Add the exclusion zones: backtick code spans, heading text, scenario concrete-value lines. Unit-test each exclusion against a known-bad fragment that would otherwise flag.
3. Introduce the two-tier severity split in the prepared YAML's `ambiguities[]` shape. High-confidence entries stay as they are; low-confidence entries get `severity: low` and render under a separate heading in `reen prepare` output.
4. Tighten the routing-table parser's error handling: detect the map-form attempt at the top of the routes section and emit the targeted message. Unit-test that error against a draft that uses the map form.
5. Update `reen-draft-format` skill: add the routing-table section. Edit in `templates/` if Phase 8 has landed; otherwise edit `.claude/` and reconcile in the next `reen init --upgrade`.
6. Regression tests on the existing Phase 9 fixtures: none of them should start failing or start emitting new low-confidence findings. If they do, the fixtures are the thing that's wrong, not the lint.

**Exit criteria:** `reen prepare` on a draft that mentions `Order` in the English phrase "in order to" does not flag anything. A draft that references `@aisstream/MissingType` still fails prepare. A draft that references `Accoutn` (typo of a resolved `Account`) flags high-confidence blocking with a message naming the typo. A routing-table draft written in the map form fails prepare with the targeted error message, not a generic YAML one. All pre-Phase-9 test projects continue to prepare without new warnings.

### Phase 10 — Rust SSR codegen with WASM target inference

**Goal:** turn Phase 9's schema into compilable Rust. Scaffold + verify for components, brand, and routing tables. SSR is the preferred realisation path; the agent picks the specific framework (Leptos, Dioxus, Axum+Askama/Maud, or another reasonable choice) and reen does not hardcode the choice. The scaffolder figures out which contexts need to compile to both native and WASM, and structures the crate accordingly.

**Why this phase exists.** The schema lands nothing by itself. Phase 10 is where a Reen project becomes a runnable web app. It is the largest new engineering surface since Phase 2, because the scaffold shape is load-bearing for every web project reen will ever produce, and because verify has to work from the Rust AST regardless of which framework the agent picked.

**Design.**

*Framework non-commitment.* The plan does not pick a framework. The scaffolded crate is conventional Rust — no framework-specific macros in reen-owned code, no assumption about whether rendering is Leptos' `view!` macro or Dioxus' RSX or Askama templates or raw `Display` impls. The scaffolder emits the contract (struct shape, method signatures, public surface) and leaves the body to Claude Code. Verify checks the contract. The agent is free to introduce framework-specific macros inside method bodies; verify's structural check ignores body contents.

*SSR preference.* When the scaffold has no other constraint, it emits an SSR-first shape: a server-side Rust binary with routes handled per entry in the routing table, components rendered to HTML on the server, brand realised as a stylesheet plus Rust constants for inline access. CSR/hybrid variants are permitted — a component scaffold must be compatible with being mounted client-side after hydration — but SSR is the default shape the scaffolder emits.

*WASM target inference.* The scaffolder classifies each context as server-only, client-also, or client-only based on its transitive dependencies (the `realisation_target` computed in Phase 9). A context invoked from a component's Composition section must be reachable from the client. If its classification is server-only, the scaffolder emits an RPC edge — a server endpoint that the client-side component calls — rather than compiling it to WASM. The BA's draft still reads "when the form emits submit, invoke SubmitOrder"; the scaffolder turns that into `POST /api/submit-order` transparently.

*Crate layout.* A Reen-initialised web project, after Phase 10 scaffolding, is structured as a Cargo workspace:

```
<project>/
  crates/
    app-core/          # data, projections, contexts, operations (client-also)
    app-server/        # server-only contexts + operations; binary entry for SSR
    app-client/        # WASM build; client-side handlers for component events
    app-components/    # components, brand, routing — shared between server and client
```

The four-crate split is an implementation convention the scaffolder enforces. The server binary pulls in `app-core`, `app-components`, and `app-server`; the client WASM pulls in `app-core`, `app-components`, and `app-client`. A scaffold flag lets server-only projects skip `app-client` entirely.

*Scaffold-per-kind.*

- **Component.** A `Component` struct per draft, fields matching props + ambient brand reference. Public methods: `render(&self) -> Markup` (or framework-equivalent), plus one method per event (`fn on_<event>(...)`). The Composition section drives child-component instantiation and event-to-context wiring.
- **Brand.** A module of typed constants and a stylesheet source. Colour tokens become CSS custom properties plus `&'static str` constants. Typography and spacing scales become indexable tables. Voice, logo rules, and imagery travel as doc comments.
- **Routing table.** A single `Router` struct with entries for every route. Path parameters decoded to declared data types; the scaffolder emits the decode calls.

*Verify extensions.* `reen verify` gains new cases for components, routing tables, and brand. Plus a WASM-classification sanity check: a context classified `client_also` must not depend on a server-only symbol.

**Exit criteria:** a minimal Reen web project — one brand, one leaf component, one coordinating component, a routing table with a single route, and a validator context — scaffolds to a compilable workspace, `cargo check` passes across all crates, and `reen verify` accepts the scaffold. A deliberately server-only context (one that transitively touches an authenticated operation) is classified as such and its component invocation emits an RPC edge rather than WASM-compiled inline code.

### Phase 11 — web-app test project (Lupa)

**Goal:** a real web application, built end-to-end under the new pipeline, that stresses the schema and the Phase 10 scaffolder the way snake, money transfer, and Connor stressed Phases 1–6.

**Why this phase exists.** Every previous phase that tried to validate a schema purely on fixture drafts shipped bugs. Real use kicks them out. Phase 11 is where Phases 9, 9.5, and 10 earn their keep.

**Test case: Lupa.** A net bank for a Nordea retail-banking customer, designed to be production-ready though not production-deployed. The feature set is small — accounts listing, transactions listing per account, transfer between own accounts, transfer domestically, transfer internationally (IBAN + BIC/SWIFT). The architectural ask is not: every feature must be reachable via a conventional keyboard-and-mouse web UI *and* invokable by a conversational chat agent embedded in the app, and the server must handle many concurrent user sessions with no cross-session leakage. Dual access — web UI plus chat agent — is the design spine, but it is not a surface multiplication: Lupa ships one HTTP surface (the SSR web app, with whatever JSON endpoints the UI's own interactive fragments need), and the chat agent runs in-process inside the server as an ordinary collaborator that dispatches to business contexts via plain Rust function calls. The agent has no network privileges the UI doesn't have — it has no network at all between itself and the contexts; they share a process and a tool registry derived from the same context catalogue. Mutating actions initiated from chat route through a signed, single-use confirmation card that the user clicks in the UI before the context is actually invoked. Nordea's OpenBanking API is the external specification authority, consumed via Phase 7's ingestion path — Lupa does not hand-author banking primitives, it ingests them.

**Shape.** A brand ("Lupa"), a routing table with entry points for accounts/transactions/transfer-hub/chat, screen components per route, coordinating components per transfer flow, leaf components for money/IBAN input plus a `ConfirmationCard` for chat-initiated mutations. Business contexts for each of the five features, plus infrastructure contexts (`AuthenticateWithNordea` with PSD2 scopes and SCA, `AuthenticateSession`, `RefreshNordeaSession`, `RevokeSession`, `RecordAuditEntry`, `EnforceIdempotency`, `ConfirmTransfer`, `ChatSession`). Every context classifies `server_only`; Phase 10's RPC edge emission is the primary call path from components. Every mutating context takes an `IdempotencyToken` role player and writes an `AuditEntry` before returning. Sessions are per-request and backed by a pluggable store (in-memory for dev/fixture tests; durable for staging). The chat agent's tool registry is an in-process Rust data structure built once at startup from the context catalogue and shared across sessions; invocation is session-scoped. Reference implementation runs in fixture mode by default (committed Nordea response corpus that simulates multi-customer behaviour); a `sandbox` mode smoke-tests against Nordea's sandbox. The chat agent backend (Anthropic Messages API by default) is declared as a second ingested API. Deployment shape ships as a `Dockerfile` + `docker-compose.yml` with Postgres for durable-mode testing.

**Exit criteria:** Lupa builds end-to-end from drafts. `reen prepare && reen scaffold && reen verify && cargo check --workspace` is clean. `cargo run -p app-server` boots in fixture mode with all five features reachable via both the web UI and the in-process chat agent, serving multiple concurrent sessions without cross-session leakage (concurrency test explicitly asserts this in CI, not `#[ignore]`d). Surface-parity tests pass: invoking a context via an SSR form POST and via the chat agent's tool-dispatch path with equivalent inputs produces equivalent outcomes — same audit entries, same idempotency behaviour, same return data modulo surface format. A prompt-injection test confirms no chat path executes a mutating action without an explicit confirmation-card Accept. CSRF protection, secure cookie flags, CSP nonces, PII-redacting logs, and graceful shutdown are all in place and tested. The project surfaces at least one concrete defect in Phases 2.5 / 7 / 8 / 9 / 9.5 / 10 that gets fixed in its own commit; if it does not, Lupa is not exercising the schema hard enough and should be expanded before Phase 11 closes. A production-readiness assessment in Lupa's `README.md` enumerates which work remains before a real deployment and who would need to sign off.

### Phase 12 — feature flags on artefacts and multiple apps per project

**Goal:** let BAs declare compile-time feature flags on individual artefacts (contexts, projections, data, components, operations) and let a single project ship more than one runnable app, each with its own feature closure. Validated by splitting snake into `snake_terminal` and `snake_web` variants that share a core game lib but compile to different binaries.

**Why this phase exists.** Today every context a project declares is in every binary the project produces — there is no way for a BA to say "this renderer is terminal-only" or "this screen exists only in the web variant," and no way to produce two binaries from one project. Runtime switching via env vars (as snake's `SNAKE_RENDERER` hack does today) is the workaround; it ships both renderers in every build and can't describe a terminal-vs-web split where the platforms differ structurally. Feature flagging is a Cargo-native capability that Reen currently ignores. Reintroducing it at the draft level, and letting apps declare their feature sets, turns the snake-web/snake-terminal story from a runtime env-var kludge into a schema-level split. It also gives Lupa a clean way to express fixture-mode vs sandbox-mode as compile-time variants after the fact.

**Shape.** Draft-level addition: an optional `Feature: <snake-name>` line on any context / projection / data / component / operation draft. Prepared YAML carries the feature name on the artefact entry. Unfeatured artefacts behave exactly as today — always compiled, no gate. App artefacts move from the singleton `drafts/app.md` to the directory form `drafts/apps/<app_name>.md`; each app gains an optional `Requires Features:` list. Codegen wraps gated modules in `#[cfg(feature = "<name>")]` at the `mod` declaration (module files are always on disk; Cargo decides whether to compile them). Cargo.toml's `[features]` section is managed as scaffolder-owned content: every feature named on an artefact becomes `<feature> = []`; every app becomes `<app_snake> = [<its feature closure>]`. Multiple `[[bin]]` entries — one per app — each with `required-features = [...]` and `path = "src/bin/<app_snake>.rs"`. Verify gains a structural check that each declared app's transitive context closure is satisfiable under its required feature set, reported in DCI language. The snake test project is migrated to two apps: `snake_terminal` (today's binary, gated `terminal_renderer`) and a new `snake_web` (Phase 10's web-app shape, gated `web_renderer`) that shares the Board / Snake / GameLoop / CommandInput / StringRenderer core unfeatured. Money transfer and Connor migrate trivially — one app, no features.

**Exit criteria:** `reen prepare && reen scaffold && reen verify && cargo check --workspace` is clean on snake with both app variants declared. `cargo build --no-default-features --features snake_terminal` and `cargo build --no-default-features --features snake_web` each produce a runnable binary; the terminal variant contains no WebRenderer symbols, the web variant contains no crossterm-dependent symbols (check by `nm` or `cargo tree -e features`). A deliberately-broken app declaration (a feature named on the app that no artefact provides; a context reachable from the app's entry that requires a feature the app does not include) is rejected by `reen verify` with a DCI-language message. Snake's two apps round-trip through the pipeline without losing features across re-scaffolds. The phase surfaces at least one concrete defect in Phase 2.5 / 7 / 9.5 / 10 that gets fixed in its own commit — typically something in codegen's Cargo.toml round-trip or verify's cross-artefact walk. A third validation case: Lupa gains a compile-time `sandbox_mode` feature that flips the Nordea client between fixture and live sandbox — evidence that Phase 12 composes with Phase 11 without requiring Lupa rework.

---

## Testing strategy

- Unit tests close to each module: `draft_parser` (grammar), `prepare` (resolution, extraction), `verify` (structural diff), `codegen` (deterministic output).
- Golden-file tests for v2 YAML outputs on small fixture drafts. Store fixtures under `tests/fixtures/`.
- Snake and money transfer as full end-to-end reference projects; their integration tests assert the pipeline passes and a committed reference implementation is functionally correct.
- No test exercises an LLM-backed path directly in CI (cost, flakiness). `test_synth` and the auditor are exercised via recorded fixtures: a canned Claude response is checked in, and the test asserts the post-processing is correct given that response. Optional `--live` test mode behind a feature flag for real API calls.
- Snapshot tests for prepared YAML and for verify's diff output so changes are reviewed explicitly.

## Risks and mitigations

- **Test-synthesis hallucination.** LLM invents tests that codify a misreading of the prose. *Mitigation:* synthesised tests are clearly tagged in the YAML; refine dialogue surfaces them for BA approval before scaffold runs; tests are regenerable so a BA veto deletes cleanly.
- **Vocabulary drift across drafts.** Same concept named two ways. *Mitigation:* manifest is the only source of truth; prepare refuses to resolve undefined names, raising an ambiguity that forces manifest updates.
- **Verifier too strict.** Blocks legitimate helpers. *Mitigation:* verifier checks `⊇` for the private method set, not `=`. Private helpers that don't shadow role-method names are allowed.
- **Auditor false-positive fatigue.** Report ignored. *Mitigation:* severity levels; only `high` discrepancies block close-out; `low` are informational. Tune on real runs before making any part of it blocking.
- **Claude Code hook failures disrupt authoring flow.** Hooks fire too often or too slowly. *Mitigation:* verify + cargo check run async where possible; hook timeout bounded; a `--skip-verify` escape hatch for in-progress work.
- **DCI concept drift.** Under pressure a BA (or Claude Code) elides the distinction between role and role player, or puts a use-case-only behaviour on a data type instead of as a role method. *Mitigation:* the `reen-dci-authoring` skill calls out these specific confusions with named examples; prepare refuses drafts where a data artefact exposes operations that read like scenario coordination; refine dialogue surfaces "this looks like a role method on the context, not a data operation" as a suggestion rather than a silent fix.
- **Structural migration without vocabulary migration.** A draft adopts the new schema — right headings, Role Methods section, scenarios — while still carrying code-level content (Rust signatures, type names, method-call syntax, widening discussion). Schema compliance gives false confidence that the draft is BA-authorable. *Mitigation:* Phase 3.5's code-token lint is a **blocking** gate in `reen prepare`. Structural compliance is necessary but not sufficient; a draft is only acceptable when the lint is clean. The lint is mechanical and deterministic, so the judgment "does this read as business language?" is no longer delegated to the migrating agent.

## Open questions

1. LLM invocation from Rust — direct Anthropic SDK, shell to `claude -p`, or MCP server? Prefer direct SDK for test_synth (fast, controlled), shell for commands the user is already orchestrating.
2. Should `audit` run automatically at the end of `/implement` or stay opt-in? Recommend opt-in through Phase 5; reassess after a few real uses.
3. How to version the vocabulary manifest as it evolves? Not urgent; defer until after Phase 4.
4. Should drafts support cross-context functionality references explicitly (`@other-context/functionality`) or keep them purely prose-driven through the manifest? Defer; see what Phase 4 surfaces.

## Estimated sequencing

Phases 0–2 can run linearly over a focused week each. Phase 2.5 is a retrofit — it was identified after Phases 3–9 had shipped, when it became clear that scaffolds were library-only and `cargo run` did not work; it is named 2.5 because conceptually it belongs with Phase 2's scaffold story, but in practice it runs before Phase 10 (Phase 10 depends on the App artefact kind existing). Phase 3 can start in parallel with late Phase 2 once the CLI surface is frozen. Phase 4 is the longest-running because it forces real use and will kick bugs back into 1–3. Phase 5 needs Phase 4 complete. Phase 6 closes the main-use-case pipeline. Phase 7 is additive and independent of Phases 4–6 in principle (it introduces a new artifact kind rather than changing existing ones), but is sequenced after Phase 6 so the core pipeline is stable before the ingestion path is grafted on. Phase 8 is packaging only — no new pipeline behaviour — so it can run in parallel with Phase 7 if desired, but dogfooding the bundle is simplest once Phase 7's additions are already in the canonical `.claude/` tree. Phase 9 adds the web-app schema; Phase 9.5 refines the two specific rough edges surfaced by the Phase 9 run (prose-reference false positives, routing-table parser error message). Phase 10 depends on both Phase 9.5 (sharper lint and parser) and Phase 2.5 (App artefact kind) — it extends each with a web-app branch rather than inventing the pattern from scratch. Phase 11 is test-project work that only begins once Phases 9, 9.5, 2.5, and 10 are stable. Phase 12 runs after Phase 11 — it needs Phase 10's web-app scaffolder to have something to gate (the snake-web variant is the test case) and benefits from Lupa as a second stress project against which to validate the feature-flag/multi-app machinery. Phase 12 is cross-cutting: it touches the parser, the prepared schema (additive; no version bump unless the multi-app rework forces one, in which case v4 is acceptable), codegen's Cargo.toml round-trip, verify's cross-artefact walk, and every migrated test project — but no phase depends on it, so it can slip in the schedule without blocking anything downstream.
