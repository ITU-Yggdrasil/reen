# Reen Setup

This guide gets Reen running on Windows, Linux/WSL, and macOS.

## 1. Install Rust

### Windows

Open PowerShell and install Rust with:

```powershell
winget install Rustlang.Rustup
```

Close and reopen PowerShell, then verify:

```powershell
rustc --version
cargo --version
```

If Rust later fails at the linker step, install the Visual Studio C++ build tools and retry.

### Linux / WSL

Install Rust with:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

Verify:

```bash
rustc --version
cargo --version
```

### macOS

Install Rust with:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

Verify:

```bash
rustc --version
cargo --version
```

## 2. Clone And Enter The Repo

```bash
git clone <your-repo-url>
cd reen
```

## 3. Configure A Model Provider

Reen reads credentials from shell environment variables or from a `.env` file found by searching upward from your current working directory.

You only need the variables for the provider(s) you actually use.

### PowerShell examples

```powershell
$env:OPENAI_API_KEY = "your-openai-key"
$env:ANTHROPIC_API_KEY = "your-anthropic-key"
$env:MISTRAL_API_KEY = "your-mistral-key"
$env:OLLAMA_BASE_URL = "http://localhost:11434"
```

### Bash / zsh examples

```bash
export OPENAI_API_KEY="your-openai-key"
export ANTHROPIC_API_KEY="your-anthropic-key"
export MISTRAL_API_KEY="your-mistral-key"
export OLLAMA_BASE_URL="http://localhost:11434"
```

### Optional `.env` file

You can also create a `.env` file in the repo root:

```dotenv
OPENAI_API_KEY=your-openai-key
ANTHROPIC_API_KEY=your-anthropic-key
MISTRAL_API_KEY=your-mistral-key
OLLAMA_BASE_URL=http://localhost:11434
```

## 4. Optional: Set Up Ollama

Use this if your model registry points to local Ollama models.

### Windows

```powershell
winget install Ollama.Ollama
ollama serve
ollama pull qwen2.5:7b
```

### Linux / WSL

```bash
curl -fsSL https://ollama.com/install.sh | sh
ollama serve
ollama pull qwen2.5:7b
```

### macOS

```bash
brew install --cask ollama
ollama serve
ollama pull qwen2.5:7b
```

Notes:

- If Ollama runs on another machine, point Reen at it with `OLLAMA_BASE_URL`.
- In WSL, a remote or host-level Ollama server often works better than installing it twice.

## 5. Build Reen

From the repo root:

```bash
cargo build --release
```

Compiled binary paths:

- macOS, Linux, WSL: `./target/release/reen`
- Windows PowerShell: `.\target\release\reen.exe`

## 6. Verify The Install

### Quick verification

macOS, Linux, WSL:

```bash
./target/release/reen --help
```

Windows PowerShell:

```powershell
.\target\release\reen.exe --help
```

### Optional full verification

```bash
cargo test
```

## 7. Choose A Model Registry Profile

By default Reen uses:

```text
agents/agent_model_registry.yml
```

You can switch to a profiled registry file:

```bash
./target/release/reen --profile sonnet create specification
```

That resolves to:

```text
agents/agent_model_registry.sonnet.yml
```

## 8. Optional Rate Limits

These only affect `create` commands.

### Environment variables

```bash
export REEN_RATE_LIMIT=2
export REEN_TOKEN_LIMIT=60000
```

### CLI overrides

```bash
./target/release/reen create --rate-limit 2 specification
./target/release/reen create --token-limit 60000 implementation
```

Resolution order:

1. CLI flags
2. `REEN_RATE_LIMIT` / `REEN_TOKEN_LIMIT`
3. Top-level values in the active model registry file

## 9. First Commands To Try

```bash
./target/release/reen create specification
./target/release/reen create implementation
./target/release/reen create tests
```

See [README.md](README.md) for the full CLI reference.

## Troubleshooting

### `cargo` or `rustc` not found

Restart your shell after installing Rust, then re-run `rustc --version`.

### API key errors

Confirm the required provider variable is set in your current shell or `.env` file.

### Running from a nested project directory

That is supported. Reen searches upward for `.env` and for the active `agents/agent_model_registry*.yml` file.

### `reen` command not found

Use the full binary path from `target/release/` or add that directory to your `PATH`.
