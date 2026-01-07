# E2E Test Quickstart Guide

This guide will help you run the money transfer end-to-end test in 3 simple steps.

## Prerequisites

1. **Install Python dependencies**:
   ```bash
   pip install anthropic openai
   ```

2. **Set API key** (choose one):
   ```bash
   # Option A: Anthropic (Claude)
   export ANTHROPIC_API_KEY='your-key-here'

   # Option B: OpenAI (GPT)
   export OPENAI_API_KEY='your-key-here'
   ```

## Running the Test

### Step 1: Verify Setup

```bash
./tests/check_setup.sh
```

This will check if everything is configured correctly.

### Step 2: Run the E2E Test

```bash
./tests/e2e_money_transfer_test.sh
```

This will:
- Build reen
- Generate specifications from drafts
- Generate implementation from specifications
- Generate tests
- Compile the code
- Run a money transfer test (transfer 100 from account A to B)

### Step 3: Examine Results

After a successful run, check:

```bash
# View generated specifications
cat "tests/money transfer/contexts/account.md"
cat "tests/money transfer/contexts/money_transfer.md"

# View generated implementation
ls -la "tests/money transfer/src/contexts/"
cat "tests/money transfer/src/contexts/account.rs"
cat "tests/money transfer/src/contexts/money_transfer.rs"

# View generated tests
ls -la "tests/money transfer/tests/"
```

## What to Expect

### Successful Output

You should see:
```
=====================================
Money Transfer E2E Integration Test
=====================================

Step 0: Building reen...
✓ reen built successfully

Step 1: Creating specifications from drafts...
✓ Specifications created successfully

Step 2: Creating implementation from specifications...
✓ Implementation created successfully

Step 3: Creating tests from specifications...
✓ Tests created successfully

Step 4: Compiling the generated code...
✓ Project compiled successfully

Step 5: Running generated tests...
✓ Generated tests passed

Step 6: Creating manual integration test...
✓ Manual integration test created

Step 7: Running manual integration test...
Initial balance A: 1000
Initial balance B: 500
Transfer successful!
Final balance A: 900
Final balance B: 600
✓ Transfer test passed!

=====================================
✓ E2E TEST SUCCESSFUL!
=====================================
```

## Troubleshooting

### "Python runner failed"
- Make sure you installed the Python packages: `pip install anthropic openai`
- Check that your API key is set: `echo $ANTHROPIC_API_KEY`

### "Agent not found"
- Run from the project root directory (where `Cargo.toml` is)

### "Compilation failed"
- The AI-generated code may need manual adjustments
- Check the compiler errors for specific issues
- This is normal - reen is a development tool that generates a starting point

### Test takes a long time
- LLM API calls can be slow (several minutes)
- This is normal for the first run

## Alternative: Rust Integration Test

Instead of the shell script, you can run the Rust integration test:

```bash
cargo test e2e_money_transfer --test e2e_test -- --nocapture --ignored
```

This does the same thing but from within Rust's test framework.

## Understanding the Test

The test verifies that reen can:

1. **Transform drafts → specifications**
   - Input: Plain language descriptions in `drafts/`
   - Output: Formal specifications in `contexts/`

2. **Transform specifications → code**
   - Input: Formal specifications
   - Output: Working Rust implementation in `src/contexts/`

3. **Transform specifications → tests**
   - Input: Formal specifications
   - Output: Test files

4. **Verify correctness**
   - The generated code compiles
   - The money transfer logic works:
     - Account A starts with 1000
     - Account B starts with 500
     - Transfer 100 from A to B
     - Account A ends with 900
     - Account B ends with 600

## Next Steps

After a successful test:

1. Modify the drafts and re-run to see how reen handles changes
2. Add your own draft documents
3. Use reen for your own projects
4. Check out the main [README.md](../README.md) for more commands

## Getting Help

- See [tests/README.md](README.md) for detailed documentation
- See [SETUP.md](../SETUP.md) for installation details
- Check the [main README](../README.md) for command reference
