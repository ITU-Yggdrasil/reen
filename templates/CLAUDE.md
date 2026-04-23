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
