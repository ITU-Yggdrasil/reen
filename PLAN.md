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

- Keep: `tests/snake`, `tests/money transfer`, `tests/Connor`, `tests/world data`. All four are migrated in Phase 4. Snake and money transfer are the small reference projects; Connor and world data are larger real-world drafts that stress-test the schema and lint.
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
| `src/lib.rs`, `src/main.rs` | Trim: CLI shrinks to `prepare`, `scaffold`, `verify`, `clear`, `config`, `manifest`. |

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
6. Confirm `tests/Connor` and `tests/world data` can be deleted. Delete.
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

**Goal:** all four test projects — snake, money transfer, Connor, world data — built under the new pipeline from BA-authorable drafts that pass the Phase 3.5 lint.

**Ordering.** The first pass on snake and money transfer (before Phase 3.5 existed) reshaped the schema but left code-level content in the snake drafts. Redo snake first so it becomes the clean reference for the larger migrations, then re-check money transfer against the lint, then migrate Connor, then world data. Each project is its own Claude Code session with `/clear` before it starts.

**Draft acceptance bar (applies to every project).** Before `reen prepare` is run on any draft, the draft must read as business vocabulary:

- No `Signature:` lines anywhere. Role methods and functionalities are described as responsibilities, not as function shapes.
- No Rust type names in role-method, functionality, or scenario prose: no `Vec<T>`, `Option<T>`, `Result<T>`, `&[T]`, `&str`, `String`, `u8`..`u64`, `i32`..`i64`, `f32`/`f64`, `bool`, `usize`, etc., except when referring to a concept already defined in a manifest-bound data or projection draft by its domain name.
- No `::` path syntax, no turbofish, no `&`/`&mut` sigils, no `impl`/`dyn`/`where` tokens.
- No method-call prose on identifiers (`foo.bar()`, `.unwrap()`, `.clone()`).
- No macro invocations (`println!`, `todo!`, `format!`).
- No integer-widening or FFI-boundary discussion in prose — if an application-level draft needs to describe a boundary, it does so by naming the responsibilities on each side, not by specifying casts.

The lint enforces all of the above mechanically. The bar above is the *reason* the lint exists; the lint is the *mechanism*.

1. Per project, in order — **snake (redo first)**, then **money transfer** (lint sweep), then **Connor**, then **world data**:
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

**Exit criteria:** `cargo test` at the workspace root is green; all four test projects — snake, money transfer, Connor, world data — can be built end-to-end from their rewritten drafts; every draft in every project passes the Phase 3.5 code-token lint without `--allow-code-tokens`.

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

Phases 0–2 can run linearly over a focused week each. Phase 3 can start in parallel with late Phase 2 once the CLI surface is frozen. Phase 4 is the longest-running because it forces real use and will kick bugs back into 1–3. Phase 5 needs Phase 4 complete. Phase 6 runs last.
