# Reen

Reen is a compiler-like CLI for turning markdown drafts into internal contract bundles, Rust implementation, and executable BDD tests.

## Pipeline

The normal flow is:

1. `drafts/` -> `.reen/contracts/` + `.reen/specifications/`
2. `.reen/contracts/` + `.reen/specifications/` -> `src/`
3. `.reen/contracts/` + `.reen/specifications/` -> `tests/`

Reen keeps build-tracker state so unchanged work can be skipped on later runs.

## Project Layout

```text
reen/
├── agents/          # Agent specs and model registry files
├── drafts/          # Human-authored input markdown
├── .reen/           # Generated internal contracts, hidden specs, plans, and caches
├── src/             # Generated Rust implementation
└── tests/           # Generated BDD features, step files, and test runners
```

## Setup And Build

For platform-specific setup on Windows, Linux/WSL, and macOS, see [SETUP.md](SETUP.md).

Build the release binary with:

```bash
cargo build --release
```

Binary paths:

- macOS, Linux, WSL: `./target/release/reen`
- Windows PowerShell: `.\target\release\reen.exe`

All command examples below use `reen` as a placeholder for that compiled binary.

## CLI Guide

### Global Flags

These flags work with every command:

| Flag | Description | Example |
| --- | --- | --- |
| `--profile <name>` | Use `agents/agent_model_registry.<name>.yml` instead of the default registry. | `reen --profile sonnet check drafts` |
| `--verbose` | Print extra progress and debug output. | `reen --verbose create implementation app` |
| `--dry-run` | Show what would happen without changing files. | `reen --dry-run clear` |
| `-h, --help` | Show help for the current command. | `reen create --help` |

### Command Tree

```text
reen create <subcommand>
reen build [OPTIONS] [NAMES]...
reen check <subcommand>
reen fix
reen compile
reen run [-- <args...>]
reen test
reen clear [cache|implementation]
reen help <command>
```

You can also use `-h` or `--help` at any level, for example `reen clear --help` or `reen create implementation --help`.

### `reen.yml`

`reen.yml` is optional. When present, shared `create` settings resolve in this order:

1. CLI flags
2. `reen.yml`
3. Built-in defaults or environment/registry fallbacks

Example:

```yaml
create:
  contexts: false
  projections: true
  data: false
  parallel-limit: 6
  rate-limit: 2
  token-limit: 60000
```

### `create`

`create` is the main generation command. It has its own shared options before the stage subcommand:

| Flag | Description | Example |
| --- | --- | --- |
| `--clear-cache` | Ignore build-tracker state for this create run and refresh the stage cache first. | `reen create --clear-cache contract` |
| `--contexts` | Only include `drafts/contexts/`, `drafts/apis/`, and `drafts/external_apis/`. | `reen create --contexts contract` |
| `--projections` | Only include `drafts/projections/`. | `reen create --projections contract` |
| `--data` | Only include `drafts/data/`. | `reen create --data contract` |
| `--rate-limit <n>` | Maximum API requests per second. Overrides `REEN_RATE_LIMIT` and registry config. | `reen create --rate-limit 2 specification` |
| `--token-limit <n>` | Maximum tokens per minute. Overrides `REEN_TOKEN_LIMIT` and registry config. | `reen create --token-limit 60000 implementation` |
| `--parallel-limit <n>` | Maximum in-flight items per stage. `0` is clamped to `1`. Overrides `create.parallel-limit` in `reen.yml`. | `reen create --parallel-limit 8 implementation` |

Examples:

```bash
reen create --clear-cache contract
reen create --contexts contract
reen create --projections contract
reen create --data contract
reen create --rate-limit 2 --token-limit 60000 --parallel-limit 8 implementation
```

#### `create contract`

Create internal contract bundles from draft files. Alias: `contracts`.

Usage:

```text
reen create contract [OPTIONS] [NAMES]...
```

Arguments and options:

- `[NAMES]...`: Optional draft names without the `.md` extension.
- `--fix`: When blocking ambiguities are found, ask Reen to patch drafts and retry.
- `--max-fix-attempts <n>`: Limit automatic draft-fix retries. Default: `3`.

Examples:

```bash
reen create contract
reen create contract app agent_runner
reen create contract --fix
reen create contract --fix --max-fix-attempts 5 app
```

Notes:

- Drafts under `drafts/apis/` and `drafts/external_apis/` are expanded into hidden internal contracts under `.reen/`.
- Names are resolved by file stem, so pass `aisstream`, not `aisstream.md`.

#### `create implementation`

Create Rust implementation from drafts using synthesized internal contract bundles.

Usage:

```text
reen create implementation [OPTIONS] [NAMES]...
```

Arguments and options:

- `[NAMES]...`: Optional draft names without the `.md` extension.
- `--fix`: If compilation fails after generation, run the automatic compile-fix loop.
- `--max-compile-fix-attempts <n>`: Limit automatic compile-fix retries. Default: `3`.

Examples:

```bash
reen create implementation
reen create implementation app file_cache
reen create implementation --fix
reen create implementation --fix --max-compile-fix-attempts 5 app
```

Notes:

- Reen generates project structure files such as `Cargo.toml`, `src/lib.rs`, and `mod.rs` files before generating implementation files.
- If upstream contracts changed, Reen warns that you should rerun `reen check drafts` first.

#### `create tests`

Create executable BDD tests from drafts via synthesized internal contract bundles. Alias: `test`.

Usage:

```text
reen create tests [OPTIONS] [NAMES]...
```

Arguments:

- `[NAMES]...`: Optional draft names without the `.md` extension.

Examples:

```bash
reen create tests
reen create tests app
reen create tests account money_transfer
```

Generated test output goes under:

- `tests/features/`
- `tests/steps/`
- `tests/bdd_*.rs`

### `build`

Run draft checking/contract synthesis and then implementation generation in one command.

Under the hood this behaves like:

```bash
reen check drafts
reen create implementation --fix ...
```

If draft checking or contract synthesis fails, implementation generation does not run.

Usage:

```text
reen build [OPTIONS] [NAMES]...
```

Arguments and options:

- `[NAMES]...`: Optional draft names without the `.md` extension.
- `--clear-cache`: Ignore build-tracker state for both stages and refresh the stage cache first.
- `--contexts`: Only include `drafts/contexts/`, `drafts/apis/`, and `drafts/external_apis/`.
- `--projections`: Only include `drafts/projections/`.
- `--data`: Only include `drafts/data/`.
- `--fix`: Accepted for parity with `create`; build always enables draft and compilation repair.
- `--max-fix-attempts <n>`: Limit automatic draft-fix retries during contract synthesis. Default: `3`.
- `--max-compile-fix-attempts <n>`: Limit automatic compilation-fix retries during implementation creation. Default: `3`.
- `--rate-limit <n>`: Maximum API requests per second. Overrides `REEN_RATE_LIMIT` and registry config.
- `--token-limit <n>`: Maximum tokens per minute. Overrides `REEN_TOKEN_LIMIT` and registry config.
- `--parallel-limit <n>`: Maximum in-flight items per stage. `0` is clamped to `1`. Overrides `create.parallel-limit` in `reen.yml`.

Examples:

```bash
reen build
reen build app game_loop
reen build --contexts
reen build --projections
reen build --clear-cache --parallel-limit 8 --max-fix-attempts 5 --max-compile-fix-attempts 5 app
```

### `check`

`check` currently has one subcommand:

#### `check drafts`

Validate drafts, run internal contract synthesis, and fail on blocking ambiguities. Alias: `contracts`.

Usage:

```text
reen check drafts [NAMES]...
```

Examples:

```bash
reen check drafts
reen check drafts app agent_runner
```

### `fix`

Attempt to restore compilation by running a compile -> patch -> recompile loop.

Usage:

```text
reen fix [OPTIONS]
```

Options:

- `--clear-cache`: Ignore cached planning and compilation-fix agent responses for this run.
- `--max-compile-fix-attempts <n>`: Maximum automatic fix attempts. Default: `3`.
- `--rate-limit <n>`: Maximum API requests per second. Overrides `REEN_RATE_LIMIT` and registry config.
- `--token-limit <n>`: Maximum tokens per minute. Overrides `REEN_TOKEN_LIMIT` and registry config.

Examples:

```bash
reen fix
reen fix --clear-cache
reen fix --max-compile-fix-attempts 5
reen fix --rate-limit 1 --token-limit 60000
```

### `compile`

Run `cargo build` for the generated project.

```bash
reen compile
```

### `run`

Run `cargo run` for the generated project. Extra application arguments must come after `--`.

Examples:

```bash
reen run
reen run -- arg1 arg2
```

### `test`

Run `cargo test` for the generated project.

```bash
reen test
```

### `clear`

Run `reen` from your generated project root. Paths are relative to the current working directory.

| Command | Effect |
| --- | --- |
| `reen clear` | Clears **all** build-tracker entries and **all** agent response caches (contract, implementation, and tests stages), **and** deletes the entire `./src` directory if it exists. |
| `reen clear cache` | Full cache wipe only: build tracker plus agent response caches for every stage. Does not touch `./src`. |
| `reen clear implementation` | Deletes the `./src` directory tree only. Does not clear caches. |

Examples:

```bash
reen clear
reen --dry-run clear
reen clear cache
reen clear implementation
```

## Model Registry And Limits

The default registry file is `agents/agent_model_registry.yml`.

You can switch registries with:

```bash
reen --profile sonnet create specification
```

That command uses `agents/agent_model_registry.sonnet.yml`.

Rate and token limits for `create` and `fix` commands resolve in this order:

1. CLI flags
2. `REEN_RATE_LIMIT` / `REEN_TOKEN_LIMIT`
3. Top-level values in the active registry file

Examples:

```bash
export REEN_RATE_LIMIT=2
export REEN_TOKEN_LIMIT=60000

reen create specification
reen create --rate-limit 1 implementation
reen fix --rate-limit 1
```

## Provider Notes

Reen works with OpenAI, Anthropic, Mistral, and Ollama-backed models depending on your registry configuration. Reen also loads a `.env` file by searching upward from the current working directory, so running from the repo root or from a nested fixture project both work.

## Development

Common local commands:

```bash
cargo build
cargo test
cargo run -- --help
```

## More Docs

- [SETUP.md](SETUP.md)
- [docs/INCREMENTAL_BUILDS.md](docs/INCREMENTAL_BUILDS.md)
- [docs/SPECIFICATION_COMPLIANCE.md](docs/SPECIFICATION_COMPLIANCE.md)
- [docs/TRACING_STANDARDS.md](docs/TRACING_STANDARDS.md)
- [docs/DOCKER_WRAPPER.md](docs/DOCKER_WRAPPER.md)
