# Reen

Reen is a DCI-native spec compiler and verifier. It turns markdown drafts written in DCI vocabulary (Data, Context, Interaction) into prepared YAML contracts, generates Rust scaffolding with `todo!()` bodies, and verifies structural alignment between spec and implementation. Claude Code handles the conversational parts: ambiguity resolution, body-filling, and post-implementation audit.

## Install and Build

Requires Rust (stable). Build with:

```bash
cargo build --release
```

Link the binary into a project by running `reen` commands from the project root. Reen discovers `drafts/` in the current directory.

## Quick Start

Run on the snake test project:

```bash
cd tests/snake
reen prepare --verbose       # resolve vocabulary, synthesise tests
reen scaffold --verbose      # generate Rust stubs
```

Then in Claude Code: `/implement` fills the `todo!()` bodies, `/audit` checks semantic alignment.

## CLI

```
reen prepare [--contexts|--projections|--data] [names...]
reen refine [--drafts-only|--prepared-only] [names...]
reen scaffold [names...]
reen verify <path.rs> [--prepared path.yml]
reen clear prepared|generated|all
reen manifest types add-prefix <prefix>
reen manifest capabilities add <domain> <crate>
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
  prepare/           # Generated YAML (reen prepare output)
  vocabulary.yml     # Explicit vocabulary entries
src/
  contexts/          # Generated + implemented context Rust
  data/              # Generated + implemented data Rust
  projections/       # Generated + implemented projection Rust
```

## Development

```bash
cargo build
cargo test
cargo run -- --help
```

Test projects live under `tests/`: snake, money transfer, Connor.
