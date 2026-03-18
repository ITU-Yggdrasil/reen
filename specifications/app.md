# App

## Description

This is the main application. It is a CLI that functions as a compiler. Based on individual use cases and functional areas, the system will be able to create specifications. The specifications can then be reviewed and subsequently implemented and compiled.

The app has three commands:

- **create** The create command has a few sub commands:
  - **specification** This operation will read all files in the draft folder and create specifications for them in the `specifications/` folder. Files are processed in order: first the `data/` folder (simple data types), then the `contexts/` folder (use cases with role players), and finally root files (like `app.md`). An optional list of names can be provided. In which case only those files will be read. (It's assumed that they are all md files and only the name will be supplied). This is driven by an agent named "create_specifications" and utilises the agent_runner. If the agent is configured with `parallel: true` in the agent-model registry, multiple files will be processed concurrently for faster execution.
  - **implementation** This operation will read all files in the specifications folder and implement them in Rust, unless otherwise specified. Files are processed in order: first the `data/` folder (simple data types), then the `contexts/` folder (use cases with role players), and finally root files. As with `specifications`, an optional list of named contexts can be supplied. They are also assumed to be .md files and only the name will be supplied not the extension. This is driven by an agent named "create_implementation" and utilises the agent_runner. If the agent is configured with `parallel: true` in the agent-model registry, multiple files will be processed concurrently for faster execution.
  - **tests** This operation will read all files in the specifications folder and implement matching tests using an idiomatic Rust approach. As with `implementation`, an optional list of named contexts can be supplied. They are also assumed to be .md files and only the name will be supplied not the extension. This is driven by an agent named "create_test" and utilises the agent_runner. If the agent is configured with `parallel: true` in the agent-model registry, multiple files will be processed concurrently for faster execution.

The folder structure is preserved when creating specifications and implementations:
- `drafts/data/X.md` → `specifications/data/X.md` → `src/data/X.rs`
- `drafts/contexts/X.md` → `specifications/contexts/X.md` → `src/contexts/X.rs`
- `drafts/X.md` → `specifications/X.md` → `src/X.rs` (or `src/main.rs` for `app.md`)

The build stages have these dependencies:
- `specification` → `implementation` → {compile, run, test}

Before running any action, check the stored hashes to verify that upstream stages don't need re-execution. If inputs have changed, re-run the affected stages first.

### Direct dependency context (optional)

- [ ] **FileCache**:
  - Description: An implementation of the cache trait that keeps the cache artifacts in a file structure. The keys are used to derive the file name as well as the folder structure.
  - Props:
    - **folder** An optional path to the root folder of the cache. Defaults to `.reen`.
    - **instructions_model_hash** Hash of agent instructions + model name (used as subfolder).
  - Functionality:
    - Implements the Cache trait. The cache structure is organized to ensure that changes to agent instructions or model selection create separate cache folders, making it easier to benchmark different models and track the impact of instruction changes.
    - The folder structure is based on a hash of the agent instructions combined with the model name: `hash(agent_instructions + model_name)`. This ensures:
      - When agent instructions change, a new cache folder is created.
      - Different models have separate cache folders for easy benchmarking.
      - The cache key is based on both agent instructions and input: `hash(agent_instructions + input_json)`.

### Agent context

- [ ] **agent_runner**:
  - Description: An agent is a context used to execute an agent. It has an in-built templating agent. An agent specification will include a system prompt. Said system prompt may include placeholders of the form `{{input.prop_name}}`, these references to the input prop and should be replaced with the actual values prior to execution. It must be asserted that all placeholders are replaced with a proper value (No `None` or similar values) unless the special placeholder `{{input.prop-name?}}` is used, in which case the placeholder is replaced with `None` if no specific value is provided. If a mandatory placeholder can't be replaced, the agent runner shall fail. The path in the placeholder might be several levels deep `{{ input.prop1.prop_of_prop1 }}`.
  - Roles:
    - **agent**: agent name
    - **populate** runs the templating engine using the specifications from the agent registry with the values from the input prop. Returns the instantiated agent specification or fails if the requirement for replacing placeholders is not met.
    - **execute** uses the agent model registry to find the actual model to use, and the `populate` to instantiate the specifications. Based on this, it executes the agent run. The actual invocation of the model can happen in Rust if appropriate tooling exists but it's also allowed to do this with a Python runner using stdio. If the agent supports conversation style, the conversation should be treated as part of the template.
    - **get_cached_artefact** uses the file_cache, providing agent instructions and model name to generate a hash for the folder structure. The object returned implements the cache trait.
    - **cache**: No object plays this role.
  - Props:
    - **input**: A genericly typed argument.
    - **agent registry**: a registry used to load an agent specification from its agent name.
    - **agent model registry**: a registry to load the model used for the execution based on the agent name.
  - Functionality:
    - **run**: Activates the agent by calling the execute method and awaits and returns the result. The agent runner will keep a persistent cache. It will use the cache property for this. The cache structure ensures that changes to agent instructions or model selection create separate cache folders. The cache folder is based on `hash(agent_instructions + model_name)`, and the cache key is based on `hash(agent_instructions + input_json)`. This ensures:
      - When agent instructions change, a new cache folder is created (cache invalidation).
      - Different models have separate cache folders for easy benchmarking.
      - Input changes create different cache entries within the same instruction/model folder.
      - If there is a cache hit, a result will be returned immediately. If there is a cache miss, the result will be stored in the cache and returned. Storing should happen in the background and should not be able to stop the result from being returned. Even if storing in the cache fails.

---

Before running any action, check the stored hashes to verify that upstream stages don't need re-execution. If inputs have changed, re-run the affected stages first.
