Based on the provided draft, here is a clear, unambiguous specification without adding new behavior, roles, rules, or assumptions:

## Context

### Roles

- **agent**: agent name
  - **populate** runs the templating engine using the specifications from the agent registry with the values from the input prop. Returns the instantiated agent specification or fails if the requirement for replacing placeholders is not met.

  - **execute** uses the agent model registry to find the actual model to use, and the `populate` to instantiate the specifications. Based on this it executes the agent run. The actual invocation of the model can happen in Rust if appropriate tools exist but it's also allowed to do this with a Python runner using stdio. If the agent supports conversation-style, the conversation should be treated as part of the template.

  - **get_cached_artefact** Uses the `FileCache` to generate a hash for the folder structure. The object returned implements the `Cache` trait.

- **cache**: No object plays this role

## Props

- **input**: A genericly typed argument
- **agent registry**: A registry used to load an agent specification from its agent name
- **agent model registry**: A registry to load the model used for the execution based on the agent name

## Functionality

### `run`

Activates the agent by calling the `execute` method and awaiting and returning the result. The agent runner keeps a persistent cache. It uses the `cache` prop for this. The cache structure ensures that changes to agent instructions or model selection create separate cache folders. The cache folder is based on `hash(agent_instructions + model_name)`, and the cache key is based on `hash(agent_instructions + input_json)`. This ensures that:
- When agent instructions change, a new cache folder is created.
- Different models have separate cache folders for easy benchmarking.
- Input changes create different cache entries within the same instruction/model folder.

If there is a cache hit, a result will be returned immediately. If there is a cache miss, the result will be stored in the cache and returned.

Storing should happen in the background and should not be able to stop the result from being returned. Even if storing in the cache fails, the result will still be returned.

## Inferred Types or Structures

**Inferred Types or Structures**

No additional inferred types or structures are present.

## Blocking Ambiguities

No blocking ambiguities exist.

## Implementation Choices Left Open

- The exact collection/sequence type for cache.
- The specific crate/library choice for the `FileCache`.
- The formatting mechanics for cache entries.

These choices are non-blocking and do not change the required behavior.
