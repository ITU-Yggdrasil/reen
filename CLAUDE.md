# Reen — DCI-native spec compiler and verifier

Reen owns specification and verification. Claude Code owns implementation.

## Authoring model

DCI (Data, Context, Interaction). BAs author drafts in DCI vocabulary.
See `DCI.md` for the conceptual reference and `DRAFT_FORMAT.md` for templates.

## Pipeline

1. **Draft** (human) → `drafts/*.md` in DCI vocabulary, no types
2. **Prepare** (`reen prepare`) → resolves vocabulary, synthesises tests → `drafts/prepare/*.yml`
3. **Refine** (`/refine`) → resolve ambiguities in conversation
4. **Scaffold** (`reen scaffold`) → Rust stubs with `todo!()` bodies
5. **Implement** (`/implement`) → Claude Code fills bodies
6. **Verify** (`reen verify`) → structural alignment check (runs as PostToolUse hook)
7. **Audit** (`/audit`) → post-implementation discrepancy report

## Skills

- `reen-dci-authoring` — BA-facing DCI thinking (fires on `drafts/**/*.md`)
- `reen-draft-format` — draft templates and pitfalls (fires on `drafts/**/*.md`)
- `reen-dci-code` — Rust DCI realisation (fires on `src/contexts/`, `src/data/`, `src/projections/`)

## Slash commands

- `/prepare` — run `reen prepare`, surface ambiguities
- `/refine` — review drafts, resolve findings
- `/scaffold` — generate Rust stubs
- `/implement` — fill `todo!()` bodies guided by spec + tests
- `/audit` — discrepancy check after implementation
- `/clean` — remove tracker, prepared YAML, and reen-owned source

## Implementation contract

- Struct fields include the prepared YAML's role players, props, attributes, or source. Contexts and projections may carry additional internal bookkeeping fields.
- Public methods = functionalities (context), operations (data), queries (projection)
- Private methods ⊇ role methods with `<role>_<method>` canonical names
- Role methods take `&self` on the context, role player as first explicit `&` parameter
- Functionalities default to `&self`; upgrade to `&mut self` where mutation is needed
- `reen verify` runs after every Rust write via PostToolUse hook
- `/audit` checks semantic alignment after implementation: role method responsibilities, functionality flow, invariant enforcement

## Build tracker

`.reen/build.json` records method-level provenance. When a draft changes, `reen prepare` marks affected methods as `stale` and cascades through the dependency graph. `reen scaffold` preserves `fresh` method bodies and resets only `stale` methods to `todo!()`. `/implement` reads the tracker and works only on stale entries. `reen verify` flips `stale → fresh` when a method passes. `/audit` can flip entries back to `stale` with finding text attached.

Source under `src/contexts/`, `src/data/`, `src/projections/` is machine-owned. Users never hand-edit it. `reen clean` removes the tracker, prepared YAML, and machine-owned source.

## Hooks

PostToolUse hooks auto-run `reen prepare` on draft edits and `reen verify && cargo check` on Rust edits.

## Bundle (contributors)

`templates/` is the source of truth for `.claude/`, `DCI.md`, `DRAFT_FORMAT.md`, and this file's seed content. Edit files in `templates/` first, then run `reen init --upgrade` to regenerate the repo-root copies. PRs that edit repo-root copies without updating templates/ will fail the dogfood check.

### Adding a new artefact kind — checklist

When a phase introduces a new artefact kind (Component, Brand, RoutingTable, App, Operation, and future ones), the following touchpoints must all move together in the same phase. Skipping any one produces a silently broken bundle or a scaffold that can't materialise into a user's project via `reen init`:

1. **Parser** — `ArtifactKind::<Kind>` in `src/draft_parser.rs` plus a per-kind parse path.
2. **Prepared schema** — new variant in `src/prepared.rs` / `src/prepared_contracts.rs`; bump the schema version only if the change is non-additive.
3. **Canonical filesystem path** — pick a `drafts/<subdir>/` home, document it in the phase's `PLAN.md` section and prompt, use it consistently in every test project.
4. **Codegen** — emission into the appropriate target location (single-crate `src/<kind>/` or Phase 10's `crates/app-*/src/<kind>/`).
5. **Verify** — a case per kind, messages in DCI language.
6. **Phase 8 bundle** — add `drafts/<subdir>/.gitkeep` to `templates/` and classify it `seed` in `templates/bundle.toml`; update the Phase 8 bundle layout in `PLAN.md` and `PROMPT_PHASE_8.md` so future retrofit work does not have to re-discover the list.
7. **Skills** — extend `templates/.claude/skills/reen-draft-format/SKILL.md` (template), `reen-dci-authoring/SKILL.md` (concept), and `reen-dci-code/SKILL.md` (emission shape) with a section per kind.
8. **Hooks** — add the draft path pattern to `templates/.claude/hooks.json` if `drafts/**.md` does not already match.
9. **Test fixtures** — at least one golden-file test with a minimal draft of the new kind under `tests/fixtures/`.
10. **Dogfood** — after every edit to `templates/`, run `reen init --upgrade` inside the repo and confirm `git status` is clean. Never edit `.claude/` at the repo root directly; always go through `templates/`.

The contributor who adds a kind is responsible for all ten. Reviewers check the full list before approving.

## CLI

```
reen prepare [--contexts|--projections|--data] [names...]
reen refine [--drafts-only|--prepared-only] [names...]
reen scaffold [names...]
reen verify <path.rs> [--prepared path.yml]
reen clear prepared|generated|all
reen clean [--force]
reen manifest types add-prefix <prefix>
reen manifest capabilities add <domain> <crate>
```
