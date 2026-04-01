# Reen

Reen is a compiler-like CLI for turning markdown drafts into structured specifications, Rust implementation, and executable BDD tests.

## Pipeline

The normal flow is:

1. `drafts/` -> `specifications/`
2. `specifications/` -> `src/`
3. `specifications/` -> `tests/`

Reen keeps build-tracker state so unchanged work can be skipped on later runs.

## Project Layout

```text
reen/
├── agents/          # Agent specs and model registry files
├── drafts/          # Human-authored input markdown
├── specifications/  # Generated specification markdown
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
| `--profile <name>` | Use `agents/agent_model_registry.<name>.yml` instead of the default registry. | `reen --profile sonnet create specification` |
| `--verbose` | Print extra progress and debug output. | `reen --verbose create implementation app` |
| `--dry-run` | Show what would happen without changing files. | `reen --dry-run clear artefact specification app` |
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
reen clear <subcommand>
reen help <command>
```

You can also use `-h` or `--help` at any level, for example `reen clear --help` or `reen create implementation --help`.

### `create`

`create` is the main generation command. It has its own shared options before the stage subcommand:

| Flag | Description | Example |
| --- | --- | --- |
| `--clear-cache` | Ignore build-tracker state for this create run and refresh the stage cache first. | `reen create --clear-cache specification` |
| `--contexts` | Only include `drafts/contexts/`, `drafts/apis/`, and `drafts/external_apis/`. | `reen create --contexts specification` |
| `--data` | Only include `drafts/data/`. | `reen create --data specification` |
| `--rate-limit <n>` | Maximum API requests per second. Overrides `REEN_RATE_LIMIT` and registry config. | `reen create --rate-limit 2 specification` |
| `--token-limit <n>` | Maximum tokens per minute. Overrides `REEN_TOKEN_LIMIT` and registry config. | `reen create --token-limit 60000 implementation` |

Examples:

```bash
reen create --clear-cache specification
reen create --contexts specification
reen create --data specification
reen create --rate-limit 2 --token-limit 60000 implementation
```

#### `create specification`

Create specifications from draft files. Alias: `specifications`.

Usage:

```text
reen create specification [OPTIONS] [NAMES]...
```

Arguments and options:

- `[NAMES]...`: Optional draft names without the `.md` extension.
- `--fix`: When blocking ambiguities are found, ask Reen to patch drafts and retry.
- `--max-fix-attempts <n>`: Limit automatic draft-fix retries. Default: `3`.

Examples:

```bash
reen create specification
reen create specification app agent_runner
reen create specification --fix
reen create specification --fix --max-fix-attempts 5 app
```

Notes:

- Drafts under `drafts/apis/` and `drafts/external_apis/` are written under `specifications/contexts/external/`.
- Names are resolved by file stem, so pass `aisstream`, not `aisstream.md`.

#### `create implementation`

Create Rust implementation from specification files.

Usage:

```text
reen create implementation [OPTIONS] [NAMES]...
```

Arguments and options:

- `[NAMES]...`: Optional specification names without the `.md` extension.
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
- If upstream specifications changed, Reen warns that you should rerun `reen create specification` first.

#### `create tests`

Create executable BDD tests from specification files. Alias: `test`.

Usage:

```text
reen create tests [OPTIONS] [NAMES]...
```

Arguments:

- `[NAMES]...`: Optional specification names without the `.md` extension.

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

Run specification generation and then implementation generation in one command.

Under the hood this behaves like:

```bash
reen create specification --fix ...
reen create implementation --fix ...
```

If specification creation fails, implementation generation does not run.

Usage:

```text
reen build [OPTIONS] [NAMES]...
```

Arguments and options:

- `[NAMES]...`: Optional draft/specification names without the `.md` extension.
- `--clear-cache`: Ignore build-tracker state for both stages and refresh the stage cache first.
- `--contexts`: Only include `drafts/contexts/`, `drafts/apis/`, and `drafts/external_apis/`.
- `--data`: Only include `drafts/data/`.
- `--fix`: Accepted for parity with `create`; build always enables draft and compilation repair.
- `--max-fix-attempts <n>`: Limit automatic draft-fix retries during specification creation. Default: `3`.
- `--max-compile-fix-attempts <n>`: Limit automatic compilation-fix retries during implementation creation. Default: `3`.
- `--rate-limit <n>`: Maximum API requests per second. Overrides `REEN_RATE_LIMIT` and registry config.
- `--token-limit <n>`: Maximum tokens per minute. Overrides `REEN_TOKEN_LIMIT` and registry config.

Examples:

```bash
reen build
reen build app game_loop
reen build --contexts
reen build --clear-cache --max-fix-attempts 5 --max-compile-fix-attempts 5 app
```

### `check`

`check` currently has one subcommand:

#### `check specification`

Check whether each requested draft has a generated specification and whether the specification still contains blocking ambiguities. Alias: `specifications`.

Usage:

```text
reen check specification [NAMES]...
```

Examples:

```bash
reen check specification
reen check specification app agent_runner
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

`clear` removes either cache entries or generated artifacts.

#### `clear cache`

Clear build-tracker entries and agent response cache entries for a stage.

Specification clears also remove cached `fix_draft_blockers` responses. Implementation clears also remove cached `create_plan` and `resolve_compilation_errors` responses.

Targets:

- `reen clear cache specification [NAMES]...`
- `reen clear cache implementation [NAMES]...`
- `reen clear cache tests [NAMES]...`

Target aliases:

- `specification` -> `specifications`
- `implementation` -> `implementations`
- `tests` -> `test`

Examples:

```bash
reen clear cache specification
reen clear cache specification app
reen clear cache implementation app file_cache
reen clear cache tests account
```

#### `clear artefact`

Remove generated files for a stage. The command name is spelled `artefact`, and it also accepts the alias `artifact`.

Targets:

- `reen clear artefact specification [NAMES]...`
- `reen clear artefact implementation [NAMES]...`
- `reen clear artefact tests [NAMES]...`

Examples:

```bash
reen clear artefact specification
reen clear artefact specification app
reen clear artefact implementation app
reen clear artifact tests account
```

Behavior:

- Omitting names removes all artifacts for that target.
- Supplying names removes only the matching generated files for that target.

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
