#!/bin/bash

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo "Checking reen test setup..."
echo ""

ERRORS=0
WARNINGS=0

# Check if test directory exists
if [ -d "tests/money transfer" ]; then
    echo -e "${GREEN}✓${NC} Test directory exists"
else
    echo -e "${RED}✗${NC} Test directory 'tests/money transfer' not found"
    ERRORS=$((ERRORS + 1))
fi

# Check drafts
if [ -f "tests/money transfer/drafts/account.md" ]; then
    echo -e "${GREEN}✓${NC} Draft: account.md"
else
    echo -e "${RED}✗${NC} Draft missing: account.md"
    ERRORS=$((ERRORS + 1))
fi

if [ -f "tests/money transfer/drafts/money_transfer.md" ]; then
    echo -e "${GREEN}✓${NC} Draft: money_transfer.md"
else
    echo -e "${RED}✗${NC} Draft missing: money_transfer.md"
    ERRORS=$((ERRORS + 1))
fi

# Check types
if [ -f "tests/money transfer/src/types/mod.rs" ]; then
    echo -e "${GREEN}✓${NC} Types defined in src/types/mod.rs"
else
    echo -e "${RED}✗${NC} Types missing: src/types/mod.rs"
    ERRORS=$((ERRORS + 1))
fi

# Check Cargo.toml
if [ -f "tests/money transfer/Cargo.toml" ]; then
    echo -e "${GREEN}✓${NC} Cargo.toml exists"
else
    echo -e "${RED}✗${NC} Cargo.toml missing"
    ERRORS=$((ERRORS + 1))
fi

echo ""
echo "Checking reen setup..."
echo ""

# Check if reen can be built
if [ -f "Cargo.toml" ]; then
    echo -e "${GREEN}✓${NC} Main Cargo.toml exists"
else
    echo -e "${RED}✗${NC} Main Cargo.toml not found"
    ERRORS=$((ERRORS + 1))
fi

# Check Rust toolchain
if command -v cargo &> /dev/null; then
    CARGO_VERSION=$(cargo --version)
    echo -e "${GREEN}✓${NC} Cargo installed: $CARGO_VERSION"
else
    echo -e "${RED}✗${NC} cargo not found"
    ERRORS=$((ERRORS + 1))
fi

# Check API keys
echo ""
echo "Checking API keys..."

# Ollama doesn't require an API key (local by default)
if [ -n "$OLLAMA_BASE_URL" ]; then
    echo -e "${GREEN}✓${NC} OLLAMA_BASE_URL is set: $OLLAMA_BASE_URL"
else
    echo -e "${GREEN}✓${NC} Using default Ollama URL (http://localhost:11434)"
fi

# Anthropic API key (optional)
if [ -n "$ANTHROPIC_API_KEY" ]; then
    echo -e "${GREEN}✓${NC} ANTHROPIC_API_KEY is set"
else
    echo -e "${YELLOW}⚠${NC} ANTHROPIC_API_KEY not set (optional, only needed for Claude models)"
fi

# OpenAI API key (optional)
if [ -n "$OPENAI_API_KEY" ]; then
    echo -e "${GREEN}✓${NC} OPENAI_API_KEY is set"
else
    echo -e "${YELLOW}⚠${NC} OPENAI_API_KEY not set (optional, only needed for OpenAI models)"
fi

# Check agent configurations
echo ""
echo "Checking agent configurations..."

if [ -f "agents/agent_model_registry.yml" ]; then
    echo -e "${GREEN}✓${NC} Agent model registry exists"
else
    echo -e "${RED}✗${NC} Agent model registry not found"
    ERRORS=$((ERRORS + 1))
fi

if [ -f "agents/create_specifications_data.yml" ]; then
    echo -e "${GREEN}✓${NC} create_specifications_data agent exists"
else
    echo -e "${RED}✗${NC} create_specifications_data agent not found"
    ERRORS=$((ERRORS + 1))
fi

if [ -f "agents/create_implementation.yml" ]; then
    echo -e "${GREEN}✓${NC} create_implementation agent exists"
else
    echo -e "${RED}✗${NC} create_implementation agent not found"
    ERRORS=$((ERRORS + 1))
fi

if [ -f "agents/create_test.yml" ]; then
    echo -e "${GREEN}✓${NC} create_test agent exists"
else
    echo -e "${RED}✗${NC} create_test agent not found"
    ERRORS=$((ERRORS + 1))
fi

# Check if test script exists
echo ""
echo "Checking test scripts..."

if [ -f "tests/e2e_money_transfer_test.sh" ]; then
    echo -e "${GREEN}✓${NC} E2E test script exists"
    if [ -x "tests/e2e_money_transfer_test.sh" ]; then
        echo -e "${GREEN}✓${NC} E2E test script is executable"
    else
        echo -e "${YELLOW}⚠${NC} E2E test script is not executable (run: chmod +x tests/e2e_money_transfer_test.sh)"
        WARNINGS=$((WARNINGS + 1))
    fi
else
    echo -e "${RED}✗${NC} E2E test script not found"
    ERRORS=$((ERRORS + 1))
fi

# Summary
echo ""
echo "==============================================="
if [ $ERRORS -eq 0 ] && [ $WARNINGS -eq 0 ]; then
    echo -e "${GREEN}✓ All checks passed!${NC}"
    echo ""
    echo "You're ready to run the e2e test:"
    echo "  ./tests/e2e_money_transfer_test.sh"
    echo ""
    echo "Or run the Rust integration test:"
    echo "  cargo test e2e_money_transfer --test e2e_test -- --nocapture --ignored"
elif [ $ERRORS -eq 0 ]; then
    echo -e "${YELLOW}⚠ Setup complete with $WARNINGS warning(s)${NC}"
    echo ""
    echo "You can run the test, but some provider features may not work:"
    echo "  ./tests/e2e_money_transfer_test.sh"
else
    echo -e "${RED}✗ Setup incomplete: $ERRORS error(s), $WARNINGS warning(s)${NC}"
    echo ""
    echo "Please fix the errors before running tests."
fi
echo "==============================================="

exit $ERRORS
