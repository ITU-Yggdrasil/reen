# App

## Description

This is a command-line interface (CLI) application that functions as a compiler. It can also generate specifications, implementations, and tests based on provided files.

The app has three main commands:

1. **create**
   - **specification** This operation reads all files in the `drafts` folder and creates corresponding specifications in the `specifications/` folder.
     - Files are processed in order: first from the `data/` folder (simple data types), then the `contexts/` folder (use cases with role players), and finally root files like `app.md`.
     - An optional list of file names can be provided, which will process only those specific files.
     - This is driven by an agent named "create_specifications" and utilizes the `agent_runner`. If the agent is configured to run in parallel mode (`parallel: true`), it will process multiple files concurrently for faster execution.
   - **implementation** This operation reads all files in the `specifications/` folder and implements them in Rust, unless otherwise specified. Files are processed in order as with `specification`.
     - An optional list of named contexts can be supplied. They are assumed to be `.md` files without extensions.
     - This is driven by an agent named "create_implementation" and utilizes the `agent_runner`. If configured for parallel processing, it will handle multiple files concurrently.
   - **tests** This operation reads all files in the `specifications/` folder and generates corresponding tests using a Rust approach. Files are processed as with `implementation`.
     - An optional list of named contexts can be provided. They are also assumed to be `.md` files without extensions.
     - This is driven by anagent named "create_test" and utilizes the `agent_runner`. If configured for parallel processing, it will handle multiple files concurrently.

2. **compile** Compiles the generated project using Rust's built-in compiler tool.
3. **run** Similar to `cargo run`, this command builds and runs the application.
4. **test** Tests the project using Rust's test framework.

Agent specifications are stored in the `agents` folder, along with a YAML file holding the agent-model registry which maps each agent to a model and optionally specifies whether it can process multiple files in parallel.

A hidden `.reen` folder is created for storing build artifacts. For each input file:
- A hash of the input content
- A hash of the corresponding output

The draft folder structure supports the following paths:
- `drafts/data/X.md` → `specifications/data/X.md` → `src/data/X.rs`
- `drafts/contexts/X.md` → `specifications/contexts/X.md` → `src/contexts/X.rs`
- `drafts/X.md` → `specifications/X.md` → `src/X.rs` (or `src/main.rs` for `app.md`)

The build stages have the following dependencies:
- specification → implementation → {compile, run, test}

Before running any action, check the stored hashes to verify that upstream stages don't need re-execution. If inputs have changed, re-run the affected stages first.

## Blocking Ambiguities and Assumptions

1. **File Structure Processing**:
   - The folder structure is preserved when creating specifications and implementations.
   
2. **Agent Runner Configuration**:
   - The agent runner supports parallel execution if configured (`parallel: true`).

3. **Cache Management**:
   - A `FileCache` trait implementation is used to manage cache storage, with dependencies derived from agent instructions and input content.

4. **Placeholder Handling in Agents**:
   - Agents handle placeholders within system prompts, ensuring that all required values are replaced properly before execution. Special handling for optional values using the placeholder pattern `{{input.prop-name?}}`.

## Supporting Contexts

### FileCache
- **Description**: Implements a cache mechanism with a file-based structure.
- **Props**:
  - **folder**: Optional root folder path, defaults to `.reen`.
  - **instructions_model_hash**: Hash of agent instructions + model name (used as subfolder).
- **Functionality**: 
  - Organizes the cache structure based on `hash(agent_instructions + model_name)`, ensuring separate folders for different models and changes in instructions.
  - The final path is `{folder}/{instructions_model_hash}/{input_hash}.cache`.

### Agent Runner
- **Description**: Manages agent execution, supports templating and conversation styles.
- **Props**:
  - **input**: Generic argument used as input to the agent.
  - **agent registry**: Registry for loading agent specifications by name.
  - **agent model registry**: Registry for loading models based on agent names.
- **Functionality**:
  - Runs agents, handles caching using `FileCache`, and manages parallel execution if configured.

## Notes
- The cache keys are derived from a combination of agent instructions, input JSON content, and the specific model used.
- Caching ensures that different stages (specification, implementation) use appropriate cached results to avoid redundant processing.