# E2E Test Quickstart Guide

This guide runs the money-transfer fixture from end to end.

## Prerequisites

1. Build Reen:

   ```bash
   cargo build --release
   ```

2. Configure at least one supported provider before running the test.

   Examples:

   ```bash
   export OPENAI_API_KEY="your-key-here"
   export ANTHROPIC_API_KEY="your-key-here"
   export MISTRAL_API_KEY="your-key-here"
   export OLLAMA_BASE_URL="http://localhost:11434"
   ```

   For full setup details, see [SETUP.md](../SETUP.md).

## Running The Test

### Step 1: Verify setup

```bash
./tests/check_setup.sh
```

### Step 2: Run the e2e shell flow

```bash
./tests/e2e_money_transfer_test.sh
```

This flow:

1. Builds Reen
2. Generates specifications from drafts
3. Generates implementation from specifications
4. Generates BDD tests from specifications
5. Compiles the fixture project
6. Runs the generated and manual verification tests

### Step 3: Inspect the generated fixture output

After a successful run, inspect:

```bash
ls -la "tests/money transfer/specifications/"
ls -la "tests/money transfer/src/contexts/"
ls -la "tests/money transfer/tests/"
```

Key locations:

- `tests/money transfer/specifications/`
- `tests/money transfer/src/`
- `tests/money transfer/tests/features/`
- `tests/money transfer/tests/steps/`

## Alternative: Run The Rust Test

```bash
cargo test e2e_money_transfer --test e2e_test -- --nocapture --ignored
```

## What The Test Proves

The fixture checks that Reen can:

1. Turn `drafts/` into `specifications/`
2. Turn `specifications/` into Rust implementation under `src/`
3. Turn `specifications/` into executable BDD tests under `tests/`
4. Produce code that compiles and passes the money-transfer scenario

## Troubleshooting

### `Native runner failed`

- Rebuild Reen with `cargo build --release`
- Recheck provider credentials and model configuration

### `Agent not found`

- Run from the project root so Reen can find `agents/`

### Compilation failed inside the fixture

- Inspect the generated Rust code and compiler output
- Regenerate with different model settings or retry with fix-enabled implementation flow

### The test takes a long time

- First runs can take several minutes because they make live model calls

## More Help

- [tests/README.md](README.md)
- [SETUP.md](../SETUP.md)
- [README.md](../README.md)
