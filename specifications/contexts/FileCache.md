## Context Specification: FileCache

### Description
An implementation of the cache trait that keeps the cache artefacts in a file structure. The keys are used to derive the file name as well as the folder structure.

### Props

- **folder** An optional path to the root folder of the cache. Defaults to `.reen`.
- **instructions_model_hash** Hash of agent instructions + model name (used as subfolder).

### Functionality

The cache structure is organized to ensure that changes to agent instructions or model selection create separate cache folders, making it easier to benchmark different models and track the impact of instruction changes.

1. **Folder Structure Organization**
   - The folder structure is based on a hash of the agent instructions combined with the model name: `hash(agent_instructions + model_name)`.
   - This ensures that:
     - When agent instructions change, a new cache folder is created.
     - Different models have separate cache folders for easy benchmarking.

2. **Cache Key Generation**
   - The cache key is based on both agent instructions and input: `hash(agent_instructions + input_json)`.
   - The final path would be: `{folder}/{instructions_model_hash}/{input_hash}.cache`
     - `{folder}` defaults to `.reen`.
     - `{instructions_model_hash}` is `hash(agent_instructions + model_name)`.
     - `{input_hash}` is `hash(agent_instructions + input_json)`.

### Inferred Types or Structures (Non-Blocking)

- **Path to Cache Folder**
  - **Location**: Folder structure organization
  - **Inference**: The path to the cache folder is inferred to be a string, representing the combined path from the root folder to the subfolder.
  - **Basis**: Based on the use of a path in the folder structure.
- **Hash Function**
  - **Location**: Cache key generation
  - **Inference**: The hash function is inferred to be a function that takes a string input and returns a string output.
  - **Basis**: Based on the use of hash in the key generation process.

### Blocking Ambiguities

- **None identified.** The implementation choices do not affect externally observable behavior or conflict with any referenced dependencies.

### Implementation Choices Left Open

- **Path to Cache Folder**
  - **Non-blocking**: The exact path to the cache folder is left open, as it is a string and can be implemented in any way that meets the requirements.
- **Hash Function**
  - **Non-blocking**: The exact implementation of the hash function is left open, as long as it returns a string based on the input string.

### Validation Checklist

- [X] Every behavior is traceable to the draft text.
- [X] No new roles, rules, or flows were added.
- [X] Names exactly match the draft.
- [X] Referenced items in dependency context were resolved before adding any **Blocking Ambiguities** entry.
- [X] All inferences are explicitly documented as inferred.
- [X] Blocking ambiguities are truly behavior-impacting or contradictory.
- [X] Non-blocking technical details are captured under **Implementation Choices Left Open**.
- [X] A stakeholder could validate correctness against the draft.

This context specification accurately reflects the provided draft without adding new information or making unverifiable assumptions.