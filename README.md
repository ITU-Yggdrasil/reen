# Reen

Reen is a DCI-native spec compiler and verifier. It turns markdown drafts written in DCI vocabulary (Data, Context, Interaction) into prepared YAML contracts, generates Rust scaffolding with `todo!()` bodies, and verifies structural alignment between spec and implementation. Claude Code handles the conversational parts: ambiguity resolution, body-filling, and post-implementation audit.

## Install

Requires Rust (stable).

```bash
cargo install reen
```

Or build from source:

```bash
cargo build --release
```

## Getting Started

In a new or existing Rust project:

```bash
cd your-project
reen init
```

This installs the full Reen authoring environment: `.claude/` skills, commands, agents, and hooks; `DCI.md` and `DRAFT_FORMAT.md` references; a starter manifest; and the `drafts/` directory structure. No network access needed — everything ships inside the `reen` binary.

Then start authoring drafts under `drafts/` and use the pipeline:

```bash
reen prepare --verbose       # resolve vocabulary, synthesise tests
reen scaffold --verbose      # generate Rust stubs
```

In Claude Code: `/implement` fills the `todo!()` bodies, `/audit` checks semantic alignment.

## Upgrading

When you update the `reen` binary, refresh the managed files in your project:

```bash
reen init --upgrade          # overwrite managed files, preserve user edits
reen init --check            # dry-run: see what would change
reen init --upgrade --force  # also overwrite files you've edited
```

Managed files (`.claude/`, `DCI.md`, `DRAFT_FORMAT.md`) are overwritten on upgrade. Seed files (`CLAUDE.md`, `reen.manifest.yml`, `.gitkeep` scaffolding) are never touched after initial install.

## External APIs

Reen supports consuming external APIs described by OpenAPI specs. The authored surface is small: a **boundary draft** (`drafts/apis/<name>.md`) states purpose, source, and an operations whitelist. The generated surface is machine-owned: data drafts and operation specs under `drafts/.apis/` (gitignored). Generated types and operations are referenceable from DCI drafts as `@<api>/<Name>` — contexts can reference operations, data drafts cannot.

```bash
reen apis ingest [<api-name>]    # generate from OpenAPI snapshot
reen scaffold                     # also scaffolds operation clients
reen verify src/apis/<api>/<op>.rs  # verifies against operation spec
```

## CLI

```
reen init [--path <dir>] [--upgrade] [--check] [--force]
reen prepare [--contexts|--projections|--data] [names...]
reen refine [--drafts-only|--prepared-only] [names...]
reen scaffold [names...]
reen verify <path.rs> [--prepared path.yml]
reen clear prepared|generated|all
reen manifest types add-prefix <prefix>
reen manifest capabilities add <domain> <crate>
reen apis ingest [<api-name>]
```

Global flags: `--verbose`, `--debug`, `--dry-run`.

## Slash Commands (Claude Code)

| Command | Purpose |
|---------|---------|
| `/prepare` | Run `reen prepare`, surface ambiguities in conversation |
| `/refine` | Review drafts for completeness and concreteness, resolve findings |
| `/scaffold` | Generate Rust stubs from prepared YAML |
| `/implement` | Fill `todo!()` bodies guided by drafts + prepared YAML + tests |
| `/audit` | Post-implementation discrepancy report (role methods, flow, invariants) |

## Pipeline

1. **Draft** (BA, human) — `drafts/*.md` in DCI vocabulary, no types
2. **Prepare** (`reen prepare`) — resolves vocabulary, synthesises tests → `drafts/prepare/*.yml`
3. **Refine** (`/refine`) — resolve ambiguities in conversation
4. **Scaffold** (`reen scaffold`) — Rust stubs with `todo!()` bodies
5. **Implement** (`/implement`) — Claude Code fills bodies
6. **Verify** (`reen verify`) �� structural alignment check (PostToolUse hook)
7. **Audit** (`/audit`) — semantic discrepancy report

## For BAs

- [DCI.md](DCI.md) — what DCI is and how to think in it when writing drafts
- [DRAFT_FORMAT.md](DRAFT_FORMAT.md) — the three draft templates with annotated examples and common pitfalls

## For Claude Code Users

Three skills fire automatically on relevant files:

- **reen-dci-authoring** — DCI thinking for authoring drafts (fires on `drafts/**/*.md`)
- **reen-draft-format** — draft grammar and pitfalls (fires on `drafts/**/*.md`)
- **reen-dci-code** — Rust DCI realisation: naming, receivers, verifier invariants (fires on `src/**/*.rs`)

## Project Layout

```
drafts/
  contexts/          # Context drafts (DCI scenarios)
  data/              # Data drafts (domain objects)
  projections/       # Projection drafts (read-only views)
  apis/              # Boundary drafts + OpenAPI snapshots (committed)
  .apis/             # Generated data drafts + operation specs (gitignored)
  prepare/           # Generated YAML (reen prepare output)
  vocabulary.yml     # Explicit vocabulary entries
src/
  contexts/          # Generated + implemented context Rust
  data/              # Generated + implemented data Rust
  projections/       # Generated + implemented projection Rust
  apis/              # Generated operation client stubs
```

## Development

```bash
cargo build
cargo test
cargo run -- --help
```

Test projects live under `tests/`: snake, money transfer, Connor, world data.
