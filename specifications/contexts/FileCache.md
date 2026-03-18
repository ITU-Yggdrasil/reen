## Context Specification

### Props

- **folder** - An optional path to the root folder of the cache. Defaults to `.reen`.
- **instructions_model_hash** - A hash value derived from `agent_instructions` and `model_name`.

### Functionality

1. **Initialization**
   - When a new instance of `FileCache` is created, it sets up its internal state with the provided or default values for `folder` and `instructions_model_hash`.
   - The cache structure follows a specific folder organization that separates different cache folders based on changes in agent instructions or model selection.
   
2. **Cache Key Generation**
   - When storing data, the cache key is generated using `hash(agent_instructions + input_json)`. This ensures that the same combination of agent instructions and input JSON always produces the same cache file name.
   - The final path to store a cache entry for a given set of inputs would be:
     ```
     {folder}/{instructions_model_hash}/{input_hash}.cache
     ```
   - Where `{folder}` defaults to `.reen`, `{instructions_model_hash}` is `hash(agent_instructions + model_name)`, and `{input_hash}` is `hash(agent_instructions + input_json)`.

3. **Cache Usage**
   - When retrieving data, the system uses the generated cache key to locate the corresponding file in the cache structure.
   - If the file exists, it is read; otherwise, a new entry is created or an error is returned.

### Inferred Types or Structures (Non-Blocking)

1. **Inference Location**: The `FileCache` class or struct must handle caching logic.
   - **Inference Made**: The internal state of the cache will include fields for storing the current value of `folder`, `instructions_model_hash`, and the cache entries.
   - **Basis for Inference**: The behavior described in the draft implies a need to store these values.

2. **Inference Location**: The hash function used for generating keys.
   - **Inference Made**: The implementation will use a hashing algorithm (e.g., SHA-256) to generate both `instructions_model_hash` and `input_hash`.
   - **Basis for Inference**: The mention of "hash" functions in the context.

### Blocking Ambiguities

- **None identified**. All referenced dependencies and behaviors are clear from the provided text.

### Implementation Choices Left Open (Non-Blocking)

1. **Choice Description**: The exact implementation details of the hashing algorithm, such as which crate to use.
   - **Label**: Non-blocking
   - **Basis for Label**: While the choice impacts technical implementation, it does not affect the externally observable behavior described in the context.

2. **Choice Description**: The specific method for reading and writing cache entries (e.g., file format).
   - **Label**: Non-blocking
   - **Basis for Label**: This is a low-level detail that can be decided during implementation without affecting the broader requirements.

### Diagrams & Notation

- **Diagram**: Optional. If included, it should illustrate the folder structure and path generation process based on the given behavior description.

## Validation Checklist

- [X] Every behavior is traceable to the draft text.
- [X] No new roles, rules, or flows were added.
- [X] Names exactly match the draft.
- [X] Referenced items in dependency context were resolved before adding any **Blocking Ambiguities** entry.
- [X] All inferences are explicitly documented as inferred.
- [X] Blocking ambiguities are truly behavior-impacting or contradictory.
- [X] Non-blocking technical details are captured under **Implementation Choices Left Open**.