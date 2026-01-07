## Agent

## Description
An agent is a context used to execute an agent. 
It has an in-built templating agent. An agent specification will include a system propmt. Said system prompt may include placeholders of the form `{{input.prop_name}}` these a references to the input prop and shuold be replaced with the actual values prioer to execution. It must be asserted that all placeholders a replaced with a proper value (No None or similar values) unless the special placeholder `{{input.prop-name?}}` is used, in which case the place holder is replaced with None if no specific value is provided. If a mandatory placeholder can't be replaced the agent runner shall fail. The path in the place holder might be several levels deep `{{ input.prop1.prop_of_prop1 }}`

## Roles

- **agent**: agent name
  - **populate** runs the templating engine using the specifications from the agent registry with the values from the input prop. Returns the instantiated agent specification or fails if the requirement for replacing placeholders is not met

  - **execute** uses the agent model registry to find the actual model to use, and the `populate` to instantiate the specifications. Based on this it executes the agent run. The actual invocation of the model can happen in rust if appropriate tolling exists but it's also allowed to do this with a python runner using stdio. If the agent supports conversation style, the conversation should be treaed as part of the template.

  - **get_cached_artefact** Uses the file_cache, providing agent instructions and model name to generate a hash for the folder structure. The object returned implements the cache trait
- **cache**: No object plays this role

## Props

- **input** A genericly typed argument
- **agent registry** a registry used to load an agent specificaiton from it's agent name
- **agent model registry** a registry to load the model used for the execution based on the agent name


## Functionality

- **run** Activates the agent by calling the execute method and awaits and returns the result. The agent runner will keep a persistent cache. It will use the cache prop for this. The cache structure ensures that changes to agent instructions or model selection create separate cache folders. The cache folder is based on `hash(agent_instructions + model_name)`, and the cache key is based on `hash(agent_instructions + input_json)`. This ensures that:
  - When agent instructions change, a new cache folder is created (cache invalidation)
  - Different models have separate cache folders for easy benchmarking
  - Input changes create different cache entries within the same instruction/model folder
  If there is a cache hit a result will be returned immediately if it's a cache miss we'd store the result in the cache and return the result. Storing should happen in the background and should not be able to stop the result from being returned. Even if storing in the cache fails
