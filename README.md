# Reen

A compiler-like CLI tool for agent-driven specification and implementation.

## Overview

Reen is a meta-development tool that uses AI agents to transform draft documents into formal specifications and then into working code. It follows a pipeline approach:

1. **Drafts** → Specifications (via `create_specifications` agent)
2. **Specifications** → Implementation (via `create_implementation` agent)
3. **Specifications** → Tests (via `create_test` agent)

**Key Features**
- 🚀 **Incremental builds** - Only regenerates changed files
- 📦 **Smart caching** - Tracks file hashes to skip unnecessary work
- 🔗 **Dependency tracking** - Automatically detects when upstream files change
- 💰 **Cost efficient** - Minimizes LLM API calls

## Directory Structure

```
reen/
├── drafts/          # Draft documents describing features/components
├── contexts/        # Generated formal specifications
├── agents/          # Agent specifications (YAML)
├── src/             # Generated Rust source code
└── tests/           # Generated tests
```

## Installation

### Quick Start

1. Install Python dependencies:
```bash
pip install -r requirements.txt
```

2. Set your API key(s) for your chosen provider:
```bash
export OPENAI_API_KEY='your-api-key-here'
export ANTHROPIC_API_KEY='your-api-key-here'
export MISTRAL_API_KEY='your-api-key-here'
```

3. Build the project:
```bash
cargo build --release
```

The binary will be available at `target/release/reen`.

For detailed setup instructions, see [SETUP.md](SETUP.md).

## Usage

### Create Specifications

Transform draft documents into formal specifications:

```bash
# Process all drafts
reen create specification

# Process specific drafts
reen create specification app agent_runner

# Auto-fix drafts when blocking ambiguities are detected
reen create specification --fix

# Limit fix attempts (default: 3)
reen create specification --fix --max-fix-attempts 5
```

When `--fix` is used and the specification agent reports blocking ambiguities, a new agent (`fix_draft_blockers`) is invoked to propose patches to draft files. Blockers in one draft may require fixes in a dependency draft (e.g. an underspecified data draft can cause blockers in a context spec that depends on it). Patches are applied and specification creation retries until blockers are resolved or the max attempt limit is reached.

### Create Implementation

Generate implementation code from specifications:

```bash
# Implement all contexts
reen create implementation

# Implement specific contexts
reen create implementation app file_cache
```

### Create Tests

Generate tests from specifications:

```bash
# Create tests for all contexts
reen create tests

# Create tests for specific contexts
reen create tests app
```

### Compile, Run & Test

Wrapper commands around cargo:

```bash
# Compile the project
reen compile

# Build and run the application
reen run

# Pass arguments to the application
reen run -- arg1 arg2

# Run tests
reen test
```

## Global Options

- `--verbose` - Enable detailed debug output
- `--dry-run` - Show what would be done without executing

Examples:

```bash
# See what would happen without executing
reen --dry-run create specification

# Get detailed output during execution
reen --verbose create implementation app
```

## Agent Specifications

Agents are defined in YAML files in the `agents/` directory. Each agent specification includes:

- `name`: Agent identifier
- `description`: What the agent does
- `system_prompt`: Instructions for the agent (supports templating)

### Templating

System prompts support placeholders for dynamic content:

- `{{input.property}}` - Required property (fails if missing)
- `{{input.property?}}` - Optional property (replaced with None if missing)
- `{{input.prop1.prop2}}` - Nested properties

### Strict Implementation Rules

The `create_implementation` agent enforces **strict specification compliance**:

- **ONLY** functions in "Functionality" section can be public
- **ONLY** methods in "Role Methods" section can be private methods
- **NO** additional fields, methods, or functions allowed
- Implementations must match specifications **exactly**
- **ALL** methods must be instrumented with tracing

See [docs/SPECIFICATION_COMPLIANCE.md](docs/SPECIFICATION_COMPLIANCE.md) for details.

### Tracing Instrumentation

All generated code includes structured tracing for observability:

- Role methods: `"[ContextName] [role] [method], message"`
- Public methods: `"[ContextName] [method], message"`

See [docs/TRACING_STANDARDS.md](docs/TRACING_STANDARDS.md) for details.

### Agent-Model Registry

The `agents/agent_model_registry.yml` file maps agents to specific models.
Use the `provider/model` format to choose a provider explicitly:

```yaml
create_specifications_data:
  model: mistral/codestral-latest
create_implementation:
  model: openai/gpt-5
create_test:
  model: ollama/qwen2.5:7b
fix_draft_blockers:
  model: mistral/mistral-large-latest
```

Supported providers: **OpenAI**, **Anthropic**, **Mistral**, and **Ollama** (local).
Preset registry files are available for quick switching:

- `agents/agent_model_registry.gpt.yml` — OpenAI (GPT-5)
- `agents/agent_model_registry.mistral.yml` — Mistral API (Codestral / Mistral Large)
- `agents/agent_model_registry.qwen.yml` — Ollama (Qwen 2.5)

## Specification Format

Generated specifications use markdown with a specific structure:

```markdown
# Component Name

## System Prompt
[Instructions for implementation]

## Input Format
[Expected inputs]

## Output Format
[Expected outputs]

## Props
[Properties and their descriptions]

## Roles
[System roles]

## Role Methods
[Methods for each role]

## Description
[Overall description]

## Functionality
[Detailed functionality]
```

## Language Choice

While Rust is the default, agents can choose other languages for specific tasks. For example, Python might be used for model interaction. This is specified in the draft or determined by the agent.

## Error Handling

- Missing files are handled gracefully with clear error messages
- When processing multiple files, execution continues even if one fails
- Progress indication shows success/failure for each item
- Non-existent agent specifications cause immediate failure with helpful messages

## Interactive Mode

Agents can ask questions when they need clarification:

1. Agent generates a markdown file with context and questions
2. User is notified to update the file
3. User signals readiness by entering "ready" or an empty line
4. Answers are sent back to the agent
5. Conversational context is maintained

## Development

The project uses standard Rust tooling:

```bash
# Build
cargo build

# Run tests
cargo test

# Run with cargo
cargo run -- create specification app
```

## Incremental Builds

Reen automatically tracks file changes and dependencies to skip unnecessary regeneration:

```bash
# First run - generates everything
reen create specification

# Second run - skips unchanged files
reen create specification
# Output: "All specifications are up to date"

# Use --verbose to see what's skipped
reen --verbose create specification
# Output: "⊚ Skipping file_cache (up to date)"
```

See [docs/INCREMENTAL_BUILDS.md](docs/INCREMENTAL_BUILDS.md) for details.

## Future Enhancements

- Cross-file dependency tracking
- Incremental agent execution
- Build cache across git branches
- Parallel processing of independent tasks

## Docker CLI Image

You can run `reen` as a containerized CLI so local setup only needs Docker.

**Quick start:** Build once, then use the wrapper script:

```bash
docker build -t reen:latest .
source scripts/reen.sh   # macOS / Linux / WSL
# or on Windows PowerShell: . .\scripts\reen.ps1

reen create specification
```

For full instructions (first build, sourcing, and platform-specific setup for macOS, Windows, and WSL), see [docs/DOCKER_WRAPPER.md](docs/DOCKER_WRAPPER.md).

**Manual run** (without the wrapper):

```bash
docker run --rm -it \
  -e MISTRAL_API_KEY="your-key" \
  -v "$(pwd):/work" \
  -w /work \
  reen:latest create specification
```

If you use OpenAI/Anthropic/Ollama instead, pass the corresponding env vars (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `OLLAMA_BASE_URL`, etc.).
