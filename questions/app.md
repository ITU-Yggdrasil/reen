# App

## Description
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

Direct dependency context (optional):
[{
  "content": "# FileCache\n\n\n## Description\nAn implementation of the cache trait that keeps the cache artefacts in a file structure. The keys are used to derive the file name as well as the folder structure.\n\n\n## Props\n\n- **folder** An optional path to the root folder of the cache. Defaults to `.reen`\n- **instructions_model_hash** Hash of agent instructions + model name (used as subfolder)\n\n## Functionality\n\nImplements the Cache trait. The cache structure is organized to ensure that changes to agent instructions or model selection create separate cache folders, making it easier to benchmark different models and track the impact of instruction changes.\n\nThe folder structure is based on a hash of the agent instructions combined with the model name: `hash(agent_instructions + model_name)`. This ensures that:\n- When agent instructions change, a new cache folder is created\n- Different models have separate cache folders for easy benchmarking\n- The cache key is based on both agent instructions and input: `hash(agent_instructions + input_json)`\n\nThe final path would be `{folder}/{instructions_model_hash}/{input_hash}.cache` where:\n- `{folder}` defaults to `.reen`\n- `{instructions_model_hash}` is `hash(agent_instructions + model_name)` \n- `{input_hash}` is `hash(agent_instructions + input_json)`\n",
  "name": "FileCache",
  "path": "drafts/contexts/FileCache.md",
  "sha256": "707e842368486065551d62416b1981e4acbbdf903ed453bb9994d87582256677",
  "source": "primary"
},
{
  "content": "# Agent\n\n## Description\nAn agent is a context used to execute an agent. \nIt has an in-built templating agent. An agent specification will include a system prompt. Said system prompt may include placeholders of the form `{{input.prop_name}}` these a references to the input prop and shuold be replaced with the actual values prioer to execution. It must be asserted that all placeholders a replaced with a proper value (No None or similar values) unless the special placeholder `{{input.prop-name?}}` is used, in which case the place holder is replaced with None if no specific value is provided. If a mandatory placeholder can't be replaced the agent runner shall fail. The path in the place holder might be several levels deep `{{ input.prop1.prop_of_prop1 }}`\n\n## Roles\n\n- **agent**: agent name\n  - **populate** runs the templating engine using the specifications from the agent registry with the values from the input prop. Returns the instantiated agent specification or fails if the requirement for replacing placeholders is not met\n\n  - **execute** uses the agent model registry to find the actual model to use, and the `populate` to instantiate the specifications. Based on this it executes the agent run. The actual invocation of the model can happen in rust if appropriate tolling exists but it's also allowed to do this with a python runner using stdio. If the agent supports conversation style, the conversation should be treaed as part of the template.\n\n  - **get_cached_artefact** Uses the file_cache, providing agent instructions and model name to generate a hash for the folder structure. The object returned implements the cache trait\n- **cache**: No object plays this role\n\n## Props\n\n- **input** A genericly typed argument\n- **agent registry** a registry used to load an agent specificaiton from it's agent name\n- **agent model registry** a registry to load the model used for the execution based on the agent name\n\n\n## Functionality\n\n- **run** Activates the agent by calling the execute method and awaits and returns the result. The agent runner will keep a persistent cache. It will use the cache prop for this. The cache structure ensures that changes to agent instructions or model selection create separate cache folders. The cache folder is based on `hash(agent_instructions + model_name)`, and the cache key is based on `hash(agent_instructions + input_json)`. This ensures that:\n  - When agent instructions change, a new cache folder is created (cache invalidation)\n  - Different models have separate cache folders for easy benchmarking\n  - Input changes create different cache entries within the same instruction/model folder\n  If there is a cache hit a result will be returned immediately if it's a cache miss we'd store the result in the cache and return the result. Storing should happen in the background and should not be able to stop the result from being returned. Even if storing in the cache fails\n",
  "name": "agent_runner",
  "path": "drafts/contexts/agent_runner.md",
  "sha256": "aea7e7ed630d86f419541f4f933aa8edfddcad1428a9e1016075c8a22225fc9a",
  "source": "primary"
}]