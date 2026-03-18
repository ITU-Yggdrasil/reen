# FileCache

## Description
An implementation of the cache trait that keeps the cache artefacts in a file structure. The keys are used to derive the file name as well as the folder structure.

## Props

- **folder** An optional path to the root folder of the cache. Defaults to `.reen`
- **instructions_model_hash** Hash of agent instructions + model name (used as subfolder)

## Functionality

Implements the Cache trait. The cache structure is organized to ensure that changes to agent instructions or model selection create separate cache folders, making it easier to benchmark different models and track the impact of instruction changes.

The folder structure is based on a hash of the agent instructions combined with the model name: `hash(agent_instructions + model_name)`. This ensures that:
- When agent instructions change, a new cache folder is created
- Different models have separate cache folders for easy benchmarking
- The cache key is based on both agent instructions and input: `hash(agent_instructions + input_json)`

The final path would be:
```
{folder}/{instructions_model_hash}/{input_hash}.cache
```
where:
- `{folder}` defaults to `.reen`
- `{instructions_model_hash}` is `hash(agent_instructions + model_name)`
- `{input_hash}` is `hash(agent_instructions + input_json)`

## Inferred Types or Structures

### Inferred Types or Structures

- **instructions_model_hash** is inferred to be a string based on the context of being a hash of agent instructions combined with the model name.
- **input_hash** is inferred to be a string based on the context of being a hash of agent instructions combined with input JSON.

## Blocking Ambiguities

- None

## Implementation Choices Left Open

- The exact collection or sequence type for folder paths and cache file names is left open as `Vec<String>` for flexibility.
- The specific library used for hashing is left open as the implementation is not defined, but it must be a library that provides `hash` functionality.
- The output format for printing the cache keys and paths is left open as the output type is not specified, but it is assumed to be a `String`.
- The handling of errors in case the hashing or path creation fails is left open as the implementation is not specified, but it should be documented.