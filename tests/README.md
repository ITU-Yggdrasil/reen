# Reen Integration Tests

This directory contains end-to-end tests for the `reen` workflow.

The default checked-in model profile is Mistral-first. If you use the sample flows below, set
`MISTRAL_API_KEY` unless you have intentionally switched the registry to another provider.

## Current BDD Flow

`reen create tests` now generates reviewable BDD artifacts instead of unit-test-style Rust files.

For a generated project, the expected flow is:

1. Draft markdown in `drafts/`
2. Formal specification markdown in `specifications/`
3. Rust implementation in `src/`
4. Gherkin features and Rust Cucumber glue in `tests/`

The generated BDD layout looks like this:

```text
tests/
├── features/
│   └── contexts/
│       ├── account.feature
│       └── money_transfer.feature
├── steps/
│   └── contexts/
│       ├── account_steps.rs
│       └── money_transfer_steps.rs
├── bdd_contexts_account.rs
└── bdd_contexts_money_transfer.rs
```

The `.feature` files are meant to be readable directly against the specifications, while the generated Rust files make them executable through the `cucumber` crate.

## Money Transfer Fixture

The `tests/money transfer/` directory contains a full sample project that exercises the generation pipeline:

```text
tests/money transfer/
├── drafts/
├── specifications/
├── src/
├── tests/
└── Cargo.toml
```

## Running The E2E Flow

Recommended first run:

```bash
./tests/e2e_money_transfer_test.sh
```

Rust e2e test:

```bash
cargo test e2e_money_transfer --test e2e_test -- --nocapture --ignored
```

These tests are ignored by default because they require API access, take longer than normal unit tests, and write generated files into the fixture project.

## Manual Verification

```bash
cd "tests/money transfer"

../../target/release/reen create specification
../../target/release/reen create implementation
../../target/release/reen create tests

cargo test
```

## Snake Fixture Workflow

Run the snake sample from inside its fixture directory so the built binary path resolves correctly:

```bash
cd tests/snake

../../target/release/reen create specification
../../target/release/reen create implementation
../../target/release/reen create tests
```

After generation, review:

- `specifications/` for the source-of-truth markdown specs
- `tests/features/` for business-readable Gherkin scenarios
- `tests/steps/` and `tests/bdd_*.rs` for executable Cucumber glue

## Troubleshooting

- If `reen create tests` fails, inspect the generated implementation first. The step definitions depend on real Rust APIs being present.
- If `cargo test` fails in the generated project, check that `Cargo.toml` includes the synchronized `cucumber` dev-dependencies and generated `[[test]]` targets.
- If generation is slow, verify API keys and model configuration before retrying.
