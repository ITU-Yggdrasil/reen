# Setup Instructions for Reen

## Prerequisites

### 1. Rust
Ensure you have Rust installed. If not, install from [rustup.rs](https://rustup.rs/)

### 2. Python 3
You need Python 3.7 or later installed.

### 3. Python Dependencies
Set up a Python virtual environment and install dependencies:

```bash
./setup_venv.sh
```

This script will:
- Create a virtual environment in `.venv/` (if it doesn't exist)
- Install all required packages from `requirements.txt`
- Make them available for `runner.py`

Alternatively, you can install manually:
```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
```

**Note** The `runner.py` script will automatically detect and use the `.venv` virtual environment if it exists, so you don't need to manually activate it.

## LLM Provider Setup

### Ollama (Recommended - Local, No API Key Required)

Ollama is the default provider and runs models locally. No API key is needed!

1. **Install Ollama** (if not already installed):
   ```bash
   curl -fsSL https://ollama.com/install.sh | sh
   ```

2. **Start Ollama** (if not running):
   ```bash
   ollama serve
   ```

3. **Pull a model** (e.g., for testing):
   ```bash
   ollama pull qwen2.5:7b
   # or
   ollama pull llama3.1:8b
   ```

4. **Optional**: Set a custom Ollama server URL (for remote instances):
   ```bash
   export OLLAMA_BASE_URL='http://your-ollama-server:11434'
   ```

### Anthropic (Claude) - Optional
Set your Anthropic API key:
```bash
export ANTHROPIC_API_KEY='your-api-key-here'
```

### OpenAI (GPT) - Optional
Set your OpenAI API key:
```bash
export OPENAI_API_KEY='your-api-key-here'
```

**Tip** Add these to your `~/.bashrc`, `~/.zshrc`, or `~/.bash_profile` to make them permanent.

## Building

```bash
cargo build --release
```

The binary will be available at `target/release/reen`.

## Model Selection

The system uses the `agents/agent_model_registry.yml` file to map agents to models:

```yaml
create_specifications: gpt-4
create_implementation: claude-3-opus-20240229
create_test: gpt-4
```

Supported model names:
- **Ollama** (default): Any model name containing "ollama", "qwen", "llama", "mistral", "phi", "gemma", or "codellama" (e.g., `qwen2.5:7b`, `llama3.1:8b`, `mistral:7b`)
- **Anthropic**: Any model name containing "claude" or "anthropic" (e.g., `claude-3-opus-20240229`, `claude-3-5-sonnet-20241022`)
- **OpenAI**: Any model name containing "gpt", "openai", "o1", or "o3" (e.g., `gpt-4`, `gpt-4-turbo`, `gpt-3.5-turbo`)

**Note** Unknown model names default to Ollama (local, no API key required).

## Verification

Test that everything is set up correctly:

```bash
# Build the project
cargo build

# Check that the Python runner works with Ollama
echo '{"model": "qwen2.5:7b", "system_prompt": "Say hello"}' | python3 runner.py
```

You should see a JSON response with success=true and output containing a greeting.

## Usage

See the main README.md for usage instructions.
