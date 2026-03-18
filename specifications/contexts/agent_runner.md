## Context Specification: Agent

### Description
An agent is a context used to execute an agent. It has an in-built templating agent. An agent specification will include a system prompt. Said system prompt may include placeholders of the form `{{input.prop_name}}`, which are references to the input prop and should be replaced with the actual values prior to execution. It must be asserted that all placeholders are replaced with a proper value (No `None` or similar values) unless the special placeholder `{{input.prop_name?}}` is used, in which case the placeholder is replaced with `None` if no specific value is provided. If a mandatory placeholder cannot be replaced, the agent runner shall fail. The path in the placeholder might be several levels deep, e.g., `{{ input.prop1.prop_of_prop1 }}`.

### Roles

- **agent**: agent name
  - **populate** runs the templating engine using the specifications from the agent registry with the values from the input prop. Returns the instantiated agent specification or fails if the requirement for replacing placeholders is not met.
  - **execute** uses the agent model registry to find the actual model to use, and the `populate` to instantiate the specifications. Based on this, it executes the agent run. The actual invocation of the model can happen in Rust if appropriate tools exist but is also allowed to be done with a Python runner using stdio. If the agent supports conversation style, the conversation should be treated as part of the template.
  - **get_cached_artifact** uses the file cache, providing agent instructions and model name to generate a hash for the folder structure. The object returned implements the cache trait.

- **cache**: No object plays this role.

### Props

- **input**: A genericly typed argument.
- **agent registry**: a registry used to load an agent specification from its agent name.
- **agent model registry**: a registry to load the model used for the execution based on the agent name.

### Functionality

- **run**: Activates the agent by calling the `execute` method and awaits and returns the result. The agent runner will keep a persistent cache. It will use the `cache` prop for this. The cache structure ensures that changes to agent instructions or model selection create separate cache folders. The cache folder is based on `hash(agent_instructions + model_name)`, and the cache key is based on `hash(agent_instructions + input_json)`. This ensures that:
  - When agent instructions change, a new cache folder is created (cache invalidation).
  - Different models have separate cache folders for easy benchmarking.
  - Input changes create different cache entries within the same instruction/model folder.
  If there is a cache hit, a result will be returned immediately. If it's a cache miss, we'd store the result in the cache and return the result. Storing should happen in the background and should not be able to stop the result from being returned. Even if storing in the cache fails, the result should still be returned.

### Inferred Types or Structures

- **Inferred**: Placeholder values for placeholders, `serde` for JSON serialization, `chrono` for date/time values, `i32` for integer types, `print!` for printing, `format!` for formatting.
- **Unspecified**: No specific collection/sequence type (e.g., `Vec`), exact parameter names, or specific crates/libraries.

### Blocking Ambiguities

- **Unspecified**: How the templating engine works, exact collection/sequence type used for `cache`, specific parameter names, exact crates/libraries used, and timestamp precision.

### Implementation Choices Left Open

- **Unspecified**: Exact collection/sequence type (`Vec` vs alternatives), specific parameter names, exact crates/libraries, timestamp precision/storage granularity.

### Direct Dependency Context

- **FileCache**: An implementation of the cache trait that keeps the cache artifacts in a file structure. The keys are used to derive the file name as well as the folder structure. Props include:
  - **folder**: An optional path to the root folder of the cache. Defaults to `.reen`.
  - **instructions_model_hash**: Hash of agent instructions + model name (used as subfolder).
- **Functionality**: Implements the Cache trait. The cache structure is organized to ensure that changes to agent instructions or model selection create separate cache folders, making it easier to benchmark different models and track the impact of instruction changes. The folder structure is based on a hash of the agent instructions combined with the model name: `hash(agent_instructions + model_name)`. This ensures that:
  - When agent instructions change, a new cache folder is created.
  - Different models have separate cache folders for easy benchmarking.
  - The cache key is based on both agent instructions and input: `hash(agent_instructions + input_json)`.
- **Final Path**: `{folder}/{instructions_model_hash}/{input_hash}.cache` where:
  - `{folder}` defaults to `.reen`.
  - `{instructions_model_hash}` is `hash(agent_instructions + model_name)`.
  - `{input_hash}` is `hash(agent_instructions + input_json)`.

This specification ensures that all required behavior is captured while maintaining a clear, unambiguous format.