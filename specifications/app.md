## App

### Description

This is the main application. It's a CLI that functions as a compiler. It is however much more than that. Based on draft for individual use cases and functional areas, the system will be able to create specifications. The specifications can then be reviewed and subsequently implemented and compiled.

### Commands

The app has three commands:

- **create**
  - **specification** 
    - **Description** 
      - This operation will read all files in the `drafts/` folder and create specifications for them in the `specifications/` folder. Files are processed in order: first the `data/` folder (simple data types), then the `contexts/` folder (use cases with role players), and finally root files (like `app.md`). An optional list of names can be provided. In which case only those files will be read. (It's assumed that they are all md files and only the name will be supplied).
    - **Driven by** "create_specifications" agent and utilises the `agent_runner`. If the agent is configured with `parallel: true` in the agent-model registry, multiple files will be processed concurrently for faster execution.
  - **implementation**
    - **Description** 
      - This operation will read all files in the `specifications/` folder and implement them in Rust, unless otherwise specified. Files are processed in order: first the `data/` folder (simple data types), then the `contexts/` folder (use cases with role players), and finally root files. As with `specification` an optional list of named contexts can be supplied. They are also assumed to be .md files and only the name will be supplied not the extension.
    - **Driven by** "create_implementation" agent and utilises the `agent_runner`. If the agent is configured with `parallel: true` in the agent-model registry, multiple files will be processed concurrently for faster execution.
  - **tests**
    - **Description** 
      - This operation will read all files in the `specifications/` folder and implement matching tests using a idiomatic Rust approach. As with `implementation` an optional list of named contexts can be supplied. They are also assumed to be .md files and only the name will be supplied not the extension.
    - **Driven by** "create_test" agent and utilises the `agent_runner`. If the agent is configured with `parallel: true` in the agent-model registry, multiple files will be processed concurrently for faster execution.

- **compile**
  - **Description** 
    - Compiles generated project. Uses the `rustc` CLI.
- **run**
  - **Description** 
    - Similar to `cargo run`. Will build and run the application.
- **test**
  - **Description** 
    - Tests the project. Uses the `cargo` CLI.

### Agent Specifications

Agent specifications are kept in the `agents/` folder, as is a YAML file holding the agent-model registry. The agent-model registry maps each agent to a model and optionally specifies whether the agent can process multiple files in parallel. This allows for faster processing when agents support concurrent execution.

### Hidden Folder

A hidden `.reen` folder should be created to store build artifacts. For each input file, store:
- A hash of the input content
- A hash of the corresponding output

### Folder Structure

The draft folder structure supports:
- `data/` folder: Contains simple data types with no interaction between properties. These are processed first.
- `contexts/` folder: Contains use cases with different objects acting as actors/role players. These are processed second.
- Root files: Files like `app.md` in the root of the drafts folder. These are processed last.

### Build Stages

The build stages have these dependencies:
- `specification` → `implementation` → `{compile, run, test}`

### Cache Handling

Before running any action, check the stored hashes to verify that upstream stages don't need re-execution. If inputs have changed, re-run the affected stages first.

### Direct Dependency Context

```yaml
[
  {
    "name": "FileCache",
    "path": "drafts/contexts/FileCache.md",
    "sha256": "707e842368486065551d62416b1981e4acbbdf903ed453bb9994d87582256677",
    "source": "primary",
    "content": "# FileCache\n\n## Description\nAn implementation of the cache trait that keeps the cache artefacts in a file structure. The keys are used to derive the file name as well as the folder structure.\n\n## Props\n\n- **folder** An optional path to the root folder of the cache. Defaults to `.reen`\n- **instructions_model_hash** Hash of agent instructions + model name (used as subfolder)\n\n## Functionality\n\nImplements the Cache trait. The cache structure is organized to ensure that changes to agent instructions or model selection create separate cache folders, making it easier to benchmark different models and track the impact of instruction changes.\n\nThe folder structure is based on a hash of the agent instructions combined with the model name: `hash(agent_instructions + model_name)`. This ensures that:\n- When agent instructions change, a new cache folder is created\n- Different models have separate cache folders for easy benchmarking\n- The cache key is based on both agent instructions and input: `hash(agent_instructions + input_json)`\n\nThe final path would be `{folder}/{instructions_model_hash}/{input_hash}.cache` where:\n- `{folder}` defaults to `.reen`\n- `{instructions_model_hash}` is `hash(agent_instructions + model_name)` \n- `{input_hash}` is `hash(agent_instructions + input_json)`\n"
  },
  {
    "name": "agent_runner",
    "path": "drafts/contexts/agent_runner.md",
    "sha256": "aea7e7ed630d86f419541f4f933aa8edfddcad1428a9e1016075c8a22225fc9a",
    "source": "primary",
    "content": "## Agent\n\n## Description\nAn agent is a context used to execute an agent. \nIt has an in-built templating agent. An agent specification will include a system prompt. Said system prompt may include placeholders of the form `{{input.prop_name}}` these a references to the input prop and shuold be replaced with the actual values prior to execution. It must be asserted that all placeholders a replaced with a proper value (No None or similar values) unless the special placeholder `{{input.prop-name?}}` is used, in which case the place holder is replaced with None if no specific value is provided. If a mandatory placeholder can't be replaced the agent runner shall fail. The path in the place holder might be several levels deep `{{ input.prop1.prop_of_prop1 }}`\n\n## Roles\n\n- **agent**: agent name\n  - **populate** runs the templating engine using the specifications from the agent registry with the values from the input prop. Returns the instantiated agent specification or fails if the requirement for replacing placeholders is not met\n  - **execute** uses the agent model registry to find the actual model to use, and the `populate` to instantiate the specifications. Based on this it executes the agent run. The actual invocation of the model can happen in Rust if appropriate tolling exists but it's also allowed to do this with a python runner using stdio. If the agent supports conversation style, the conversation should be treaed as part of the template.\n  - **get_cached_artefact** Uses the file_cache, providing agent instructions and model name to generate a hash for the folder structure. The object returned implements the cache trait\n- **cache**: No object plays this role\n\n## Props\n\n- **input** A genericly typed argument\n- **agent registry** a registry used to load an agent specificaiton from it's agent name\n- **agent model registry** a registry to load the model used for the execution based on the agent name\n\n\n## Functionality\n\n- **run** Activates the agent by calling the execute method and awaits and returns the result. The agent runner will keep a persistent cache. It will use the cache prop for this. The cache structure ensures that changes to agent instructions or model selection create separate cache folders. The cache folder is based on `hash(agent_instructions + model_name)`, and the cache key is based on `hash(agent_instructions + input_json)`. This ensures that:\n  - When agent instructions change, a new cache folder is created (cache invalidation)\n  - Different models have separate cache folders for easy benchmarking\n  - Input changes create different cache entries within the same instruction/model folder\n  If there is a cache hit a result will be returned immediately if it's a cache miss we'd store the result in the cache and return the result. Storing should happen in the background and should not be able to stop the result from being returned. Even if storing in the cache fails\n"
  }
]
```

### Notes

- The paths in the `agent_runner.md` and `FileCache.md` documents are placeholders. The actual paths and hashes will be derived based on the input and instructions provided.
- The agent runner and FileCache are used to handle caching and execution of the agents, ensuring efficient re-execution when inputs or instructions change.