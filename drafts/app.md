# App


# Description

This is the main application. It's a CLI that functions as a compiler. It is however much more than that. Based on draft for individual use cases and functionalt areas, the system will be able to create specifications. The specifications can then be reviewed and subsequently implemented and compiled.

The app has three commands:

- **create** The create command has a few sub commands
  - **specification** This operation will read all files in the draft folder and create specifications for them in the `specifications/` folder. Files are processed in order: first the `data/` folder (simple data types), then the `contexts/` folder (use cases with role players), and finally root files (like `app.md`). An optional list of names can be provided. In which case only those files will be read. (It's assumed that they are all md files and only the name will be supplied). This is driven by an agent named "create_specifications" and utilises the agent_runner. If the agent is configured with `parallel: true` in the agent-model registry, multiple files will be processed concurrently for faster execution.
  - **implementation** This operation will read all files in the specifications folder and implement them in rust, unless otherwise specified. Files are processed in order: first the `data/` folder (simple data types), then the `contexts/` folder (use cases with role players), and finally root files. As with `specifications` an optional list of named contexts can be supplied. They are also assumed to be .md files and only the name will be supplied not the extension. This is driven by an agent named "create_implementation" and utilises the agent_runner. If the agent is configured with `parallel: true` in the agent-model registry, multiple files will be processed concurrently for faster execution.
  - **tests** This operation will read all files in the specifications folder and implement matching tests using a idiomatic rust approach, As with `implementation` an optional list of named contexts can be supplied. They are also assumed to be .md files and only the name will be supplied not the extension. This is driven by an agent named "create_test" and utilises the agent_runner. If the agent is configured with `parallel: true` in the agent-model registry, multiple files will be processed concurrently for faster execution.
- **compile** Compiles generated project. Uses the rust-cli
- **run** Similar to cargo run. Will build and run the application
- **test** Tests the project. Uses the rust-cli


Agent specifications are kept in the agents folder, as is a yml file holding the agent-model registry. The agent-model registry maps each agent to a model and optionally specifies whether the agent can process multiple files in parallel. This allows for faster processing when agents support concurrent execution.

The agents are instructed not to assume anything but ask questions, so the cli should be able to accept the answers and send them to the agent as part of the conversation, so we might have to keep a conversational context.

A hidden `.reen` folder should be created to store build artifacts. For each input file, store:
- A hash of the input content
- A hash of the corresponding output

The draft folder structure supports:
- `data/` folder: Contains simple data types with no interaction between properties. These are processed first.
- `contexts/` folder: Contains use cases with different objects acting as actors/role players. These are processed second.
- Root files: Files like `app.md` in the root of the drafts folder. These are processed last.

The folder structure is preserved when creating specifications and implementations:
- `drafts/data/X.md` → `specifications/data/X.md` → `src/data/X.rs`
- `drafts/contexts/X.md` → `specifications/contexts/X.md` → `src/contexts/X.rs`
- `drafts/X.md` → `specifications/X.md` → `src/X.rs` (or `src/main.rs` for `app.md`)

The build stages have these dependencies:
specification → implementation → {compile, run, test}

Before running any action, check the stored hashes to verify that upstream stages don't need re-execution. If inputs have changed, re-run the affected stages first.


