# Docker Wrapper for Reen

The `reen` Docker wrapper lets you run `reen` as a containerized CLI without installing Rust, Python, or other dependencies locally. You only need Docker.

The wrapper scripts (`scripts/reen.sh` and `scripts/reen.ps1`) provide a convenient `reen` command that runs the container with your current directory mounted and environment variables passed through.

---

## Prerequisites

- **Docker** installed and running
- **API key(s)** for your chosen provider (Mistral, OpenAI, Anthropic, or Ollama)

---

## First Build

Before using the wrapper, build the Docker image once from the project root:

```bash
docker build -t reen:latest .
```

This builds the `reen:latest` image. You only need to rebuild when the project or its dependencies change.

---

## Platform-Specific Setup

### macOS

1. **Source the script** in your shell session (bash or zsh):

   ```bash
   source scripts/reen.sh
   ```

2. **Make it persistent** (optional): add to `~/.bashrc` or `~/.zshrc`:

   ```bash
   # Add reen wrapper when in the reen project
   if [[ -f "scripts/reen.sh" ]]; then
     source scripts/reen.sh
   fi
   ```

   Or, if you always want it when opening a terminal in this project:

   ```bash
   echo 'source scripts/reen.sh' >> ~/.zshrc   # for zsh
   # or
   echo 'source scripts/reen.sh' >> ~/.bashrc  # for bash
   ```

3. **Set your API key** (if not using `.env`):

   ```bash
   export MISTRAL_API_KEY='your-api-key-here'
   ```

4. **Run reen**:

   ```bash
   reen create specification
   reen create implementation app
   ```

---

### Windows (PowerShell)

1. **Dot-source the script** in your PowerShell session:

   ```powershell
   . .\scripts\reen.ps1
   ```

   > **Note:** The leading `.` is required to load the function into the current session.

2. **Make it persistent** (optional): add to your PowerShell profile:

   ```powershell
   # Open your profile
   notepad $PROFILE

   # Add this line (adjust path if reen is elsewhere):
   . C:\path\to\reen\scripts\reen.ps1
   ```

   Or, to load it only when in the reen project directory:

   ```powershell
   if (Test-Path "scripts\reen.ps1") { . .\scripts\reen.ps1 }
   ```

3. **Set your API key** (if not using `.env`):

   ```powershell
   $env:MISTRAL_API_KEY = "your-api-key-here"
   ```

4. **Run reen**:

   ```powershell
   reen create specification
   reen create implementation app
   ```

---

### WSL (Windows Subsystem for Linux)

Use the same steps as **macOS** (the shell script works in WSL):

1. **Source the script**:

   ```bash
   source scripts/reen.sh
   ```

2. **Make it persistent** (optional): add to `~/.bashrc` or `~/.zshrc`:

   ```bash
   if [[ -f "scripts/reen.sh" ]]; then
     source scripts/reen.sh
   fi
   ```

3. **Set your API key** (if not using `.env`):

   ```bash
   export MISTRAL_API_KEY='your-api-key-here'
   ```

4. **Run reen**:

   ```bash
   reen create specification
   reen create implementation app
   ```

---

## Environment Variables

The wrapper automatically:

- **Loads `.env`** from your current directory (if present) via `--env-file`
- **Passes `MISTRAL_API_KEY`** from your shell into the container (if set)

For other providers, use a `.env` file in your project root:

```env
OPENAI_API_KEY=your-openai-key
ANTHROPIC_API_KEY=your-anthropic-key
MISTRAL_API_KEY=your-mistral-key
OLLAMA_BASE_URL=http://host.docker.internal:11434
```

> **Note:** For Ollama on Docker Desktop (Windows/macOS), use `host.docker.internal` to reach the host. The wrapper does not add this automatically; put it in `.env` if needed.

---

## Usage Examples

After sourcing the script and building the image:

```bash
# Create specifications from drafts
reen create specification

# Create implementation for specific contexts
reen create implementation app file_cache

# Create tests
reen create tests app

# Compile, run, and test
reen compile
reen run
reen test

# With options
reen --verbose create specification
reen --dry-run create implementation
```

---

## Troubleshooting

### "Docker image 'reen:latest' was not found locally"

Build the image first:

```bash
docker build -t reen:latest .
```

### "command not found: reen"

You need to source the script in your current shell:

- **macOS / WSL:** `source scripts/reen.sh`
- **Windows PowerShell:** `. .\scripts\reen.ps1`

### API key not passed into container

- Ensure the variable is exported: `export MISTRAL_API_KEY='...'`
- Or use a `.env` file in your project root

### Path or permission issues on Windows

- Run PowerShell as Administrator if Docker volume mounts fail
- Ensure the project path does not contain special characters that cause issues with Docker on Windows
