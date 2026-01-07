use crate::contexts::FileCache;
use crate::data::Cache;
use serde::Serialize;
use serde_json;
use sha2::{Digest, Sha256};
use std::fmt;

/// Errors that can occur during agent population
#[derive(Debug)]
pub enum PopulateError {
    MissingMandatoryPlaceholder(String),
    InvalidPlaceholderPath(String),
    AgentNotFound(String),
    InvalidSpecification(String),
}

impl fmt::Display for PopulateError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PopulateError::MissingMandatoryPlaceholder(ph) => {
                write!(f, "Required placeholder '{}' could not be resolved", ph)
            }
            PopulateError::InvalidPlaceholderPath(path) => {
                write!(f, "Invalid path '{}' in placeholder", path)
            }
            PopulateError::AgentNotFound(name) => {
                write!(f, "Agent '{}' not found in registry", name)
            }
            PopulateError::InvalidSpecification(details) => {
                write!(f, "Agent specification is invalid: {}", details)
            }
        }
    }
}

impl std::error::Error for PopulateError {}

/// Errors that can occur during agent execution
#[derive(Debug)]
pub enum ExecutionError {
    ModelNotFound(String),
    ExecutionFailed(String),
    PythonRunnerError(String),
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ExecutionError::ModelNotFound(name) => {
                write!(f, "Model for agent '{}' not found", name)
            }
            ExecutionError::ExecutionFailed(details) => {
                write!(f, "Agent execution failed: {}", details)
            }
            ExecutionError::PythonRunnerError(details) => {
                write!(f, "Failed to communicate with Python runner: {}", details)
            }
        }
    }
}

impl std::error::Error for ExecutionError {}

/// Errors that can occur in the agent runner
#[derive(Debug)]
pub enum AgentRunnerError {
    Populate(PopulateError),
    Execution(ExecutionError),
}

impl fmt::Display for AgentRunnerError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AgentRunnerError::Populate(e) => write!(f, "{}", e),
            AgentRunnerError::Execution(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for AgentRunnerError {}

impl From<PopulateError> for AgentRunnerError {
    fn from(e: PopulateError) -> Self {
        AgentRunnerError::Populate(e)
    }
}

impl From<ExecutionError> for AgentRunnerError {
    fn from(e: ExecutionError) -> Self {
        AgentRunnerError::Execution(e)
    }
}

/// A populated agent specification ready for execution
#[derive(Debug, Clone)]
pub struct AgentSpecification {
    pub system_prompt: String,
}

/// The result of executing an agent
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub output: String,
}

/// A model that can execute an agent
#[derive(Debug, Clone)]
pub struct Model {
    pub name: String,
}

/// Trait for loading agent specifications by name
pub trait AgentRegistry {
    /// Load an agent specification template by agent name
    fn get_specification(&self, agent_name: &str) -> Result<String, PopulateError>;
}

/// Trait for resolving execution models by agent name
pub trait AgentModelRegistry {
    /// Get the model to use for a given agent
    fn get_model(&self, agent_name: &str) -> Result<Model, ExecutionError>;
}

/// Agent Runner context manages execution of agents with templating and caching
pub struct AgentRunner<T, R, M>
where
    T: Serialize,
    R: AgentRegistry,
    M: AgentModelRegistry,
{
    /// The agent name (role player)
    agent: String,
    /// Input data for template population
    input: T,
    /// Registry for loading agent specifications
    agent_registry: R,
    /// Registry for resolving execution models
    agent_model_registry: M,
}

impl<T, R, M> AgentRunner<T, R, M>
where
    T: Serialize,
    R: AgentRegistry,
    M: AgentModelRegistry,
{
    /// Creates a new AgentRunner context
    ///
    /// # Arguments
    /// * `agent` - The agent name
    /// * `input` - The input data for template population
    /// * `agent_registry` - Registry for loading agent specifications
    /// * `agent_model_registry` - Registry for resolving execution models
    pub fn new(agent: String, input: T, agent_registry: R, agent_model_registry: M) -> Self {
        Self {
            agent,
            input,
            agent_registry,
            agent_model_registry,
        }
    }

    /// Role method: agent.populate
    ///
    /// Runs the templating engine using the agent specifications from the registry
    /// with values from the input prop.
    fn populate(&self) -> Result<AgentSpecification, PopulateError> {
        // Load the specification template from the registry
        let template = self.agent_registry.get_specification(&self.agent)?;

        // For now, we implement basic placeholder replacement
        // A full implementation would need a proper template engine
        let populated = self.replace_placeholders(&template)?;

        Ok(AgentSpecification {
            system_prompt: populated,
        })
    }

    /// Role method: agent.execute
    ///
    /// Executes the agent using the populated specification and resolved model.
    /// Can execute in Rust or via Python runner using stdio.
    ///
    /// Note: This is a single-shot execution. For conversational agents,
    /// the conversation handling is managed at a higher level (in agent_executor.rs)
    /// which calls this method multiple times as part of the conversation flow.
    fn execute(
        &self,
        specification: &AgentSpecification,
        model: &Model,
    ) -> Result<ExecutionResult, ExecutionError> {
        // Execute via Python runner using stdio
        self.execute_via_python(specification, model)
    }

    /// Executes the agent via Python runner using stdio communication
    fn execute_via_python(
        &self,
        specification: &AgentSpecification,
        model: &Model,
    ) -> Result<ExecutionResult, ExecutionError> {
        use std::process::{Command, Stdio};
        use std::io::Write;

        // Prepare the request JSON
        let request = serde_json::json!({
            "model": model.name,
            "system_prompt": specification.system_prompt
        });

        let request_json = serde_json::to_string(&request)
            .map_err(|e| ExecutionError::PythonRunnerError(format!("Failed to serialize request: {}", e)))?;

        // Spawn the Python runner
        let mut child = Command::new("python3")
            .arg("runner.py")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| ExecutionError::PythonRunnerError(format!("Failed to spawn Python runner: {}", e)))?;

        // Write the request to stdin
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(request_json.as_bytes())
                .map_err(|e| ExecutionError::PythonRunnerError(format!("Failed to write to Python runner stdin: {}", e)))?;
        }

        // Wait for the process to complete and capture output
        let output = child.wait_with_output()
            .map_err(|e| ExecutionError::PythonRunnerError(format!("Failed to read Python runner output: {}", e)))?;

        // Check if the process succeeded
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(ExecutionError::PythonRunnerError(format!(
                "Python runner failed. Stdout: {} Stderr: {}",
                stdout, stderr
            )));
        }

        // Parse the response JSON
        let response_json = String::from_utf8(output.stdout)
            .map_err(|e| ExecutionError::PythonRunnerError(format!("Invalid UTF-8 in response: {}", e)))?;

        let response: serde_json::Value = serde_json::from_str(&response_json)
            .map_err(|e| ExecutionError::PythonRunnerError(format!("Failed to parse response JSON: {}", e)))?;

        // Check if execution was successful
        if !response["success"].as_bool().unwrap_or(false) {
            let error = response["error"].as_str().unwrap_or("Unknown error");
            return Err(ExecutionError::ExecutionFailed(error.to_string()));
        }

        // Extract the output
        let output_text = response["output"].as_str()
            .ok_or_else(|| ExecutionError::PythonRunnerError("No output in response".to_string()))?
            .to_string();

        Ok(ExecutionResult {
            output: output_text,
        })
    }

    /// Generates a hash of agent instructions + model name for folder structure
    ///
    /// This hash is used to create a folder that groups all cache entries
    /// for a specific agent instruction set and model combination.
    fn generate_instructions_model_hash(&self, agent_instructions: &str, model_name: &str) -> String {
        let composite = format!("{}:{}", agent_instructions, model_name);
        let mut hasher = Sha256::new();
        hasher.update(composite.as_bytes());
        let result = hasher.finalize();
        hex::encode(result)
    }

    /// Generates a cache key based on agent instructions and input values
    ///
    /// The cache key is a hash of agent_instructions + input_json, ensuring that
    /// changes to either the instructions or input will result in a cache miss.
    fn generate_cache_key(&self, agent_instructions: &str) -> String {
        // Serialize the input to JSON to get a stable representation
        let input_json = serde_json::to_string(&self.input).unwrap_or_else(|_| "{}".to_string());

        // Create a composite key from agent instructions and input
        let composite = format!("{}:{}", agent_instructions, input_json);

        // Hash the composite key to get a fixed-size key
        let mut hasher = Sha256::new();
        hasher.update(composite.as_bytes());
        let result = hasher.finalize();

        // Return hex-encoded hash
        hex::encode(result)
    }

    /// Role method: cache.get_cached_artefact
    ///
    /// Creates and returns a FileCache instance configured for this agent and model.
    /// The cache folder is based on hash(agent_instructions + model_name).
    fn get_cached_artefact(&self, agent_instructions: &str, model_name: &str) -> Result<FileCache, ExecutionError> {
        let instructions_model_hash = self.generate_instructions_model_hash(agent_instructions, model_name);
        Ok(FileCache::new(None, instructions_model_hash))
    }

    /// Helper: Replace placeholders in a template
    ///
    /// Supports:
    /// - Mandatory: {{input.prop_name}}
    /// - Optional: {{input.prop_name?}}
    /// - Nested: {{input.prop1.prop2}}
    fn replace_placeholders(&self, template: &str) -> Result<String, PopulateError> {
        let input_json = serde_json::to_value(&self.input)
            .map_err(|e| PopulateError::InvalidSpecification(e.to_string()))?;

        let mut result = template.to_string();
        let mut offset = 0;

        // Find all placeholders in the template
        while let Some(start) = result[offset..].find("{{") {
            let start = offset + start;
            if let Some(end_pos) = result[start..].find("}}") {
                let end = start + end_pos;

                // Extract placeholder content between {{ and }}
                let placeholder = &result[start + 2..end];

                // Check if it's optional (ends with ?)
                let (path, is_optional) = if placeholder.ends_with('?') {
                    (&placeholder[..placeholder.len() - 1], true)
                } else {
                    (placeholder, false)
                };

                // Resolve the path in the input JSON
                let value = self.resolve_path(&input_json, path)?;

                match value {
                    Some(v) => {
                        // Convert value to string
                        let replacement = match v {
                            serde_json::Value::String(s) => s.clone(),
                            serde_json::Value::Number(n) => n.to_string(),
                            serde_json::Value::Bool(b) => b.to_string(),
                            serde_json::Value::Null => String::new(),
                            _ => serde_json::to_string(&v)
                                .map_err(|e| PopulateError::InvalidSpecification(e.to_string()))?,
                        };

                        // Replace the placeholder (including the {{ and }})
                        result.replace_range(start..end + 2, &replacement);
                        offset = start + replacement.len();
                    }
                    None => {
                        if is_optional {
                            // Remove optional placeholder (including the {{ and }})
                            result.replace_range(start..end + 2, "");
                            offset = start;
                        } else {
                            // Mandatory placeholder not found
                            return Err(PopulateError::MissingMandatoryPlaceholder(
                                path.to_string(),
                            ));
                        }
                    }
                }
            } else {
                break;
            }
        }

        Ok(result)
    }

    /// Helper: Resolve a dotted path in a JSON value
    ///
    /// Supports paths like "input.prop1.prop2"
    fn resolve_path<'a>(&self, value: &'a serde_json::Value, path: &str) -> Result<Option<&'a serde_json::Value>, PopulateError> {
        let parts: Vec<&str> = path.split('.').collect();

        // First part should be "input"
        if parts.is_empty() || parts[0] != "input" {
            return Err(PopulateError::InvalidPlaceholderPath(path.to_string()));
        }

        let mut current = value;

        // Navigate through the path (skip "input" as we start from the input value)
        for part in &parts[1..] {
            match current.get(part) {
                Some(v) => current = v,
                None => return Ok(None),
            }
        }

        Ok(Some(current))
    }

    /// Public function: run
    ///
    /// Activates the agent by orchestrating the complete execution lifecycle
    /// with persistent caching.
    pub fn run(self) -> Result<ExecutionResult, AgentRunnerError> {
        // Step 1: Load agent template (instructions) before populating
        // This is needed to generate the cache folder hash
        let agent_template = self.agent_registry.get_specification(&self.agent)
            .map_err(|e| AgentRunnerError::Populate(e))?;

        // Step 2: Resolve model
        let model = self.agent_model_registry.get_model(&self.agent)?;

        // Step 3: Generate cache key based on agent instructions + input
        let cache_key = self.generate_cache_key(&agent_template);

        // Step 4: Get cache instance (folder based on hash(instructions + model))
        let cache = self.get_cached_artefact(&agent_template, &model.name)?;

        // Step 5: Check cache for existing result
        if let Some(cached_value) = cache.get(&cache_key) {
            // Cache hit - return immediately
            return Ok(ExecutionResult {
                output: cached_value,
            });
        }

        // Cache miss - proceed with execution
        // Step 6: Populate the specification (replace placeholders in template)
        let specification = self.populate()?;

        // Step 7: Execute the agent
        let result = self.execute(&specification, &model)?;

        // Step 8: Store result in cache (background operation)
        // Note: In a real implementation, this would be done in a background thread
        // to ensure it doesn't block returning the result
        let cache_value = result.output.clone();
        std::thread::spawn(move || {
            cache.set(&cache_key, &cache_value);
        });

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Serialize)]
    struct TestInput {
        name: String,
        value: i32,
    }

    #[derive(Serialize)]
    struct NestedData {
        city: String,
        country: String,
    }

    #[derive(Serialize)]
    struct NestedTestInput {
        name: String,
        location: NestedData,
    }

    struct TestRegistry;

    impl AgentRegistry for TestRegistry {
        fn get_specification(&self, agent_name: &str) -> Result<String, PopulateError> {
            Ok(format!("Test specification for {}", agent_name))
        }
    }

    struct TestModelRegistry;

    impl AgentModelRegistry for TestModelRegistry {
        fn get_model(&self, _agent_name: &str) -> Result<Model, ExecutionError> {
            Ok(Model {
                name: "test-model".to_string(),
            })
        }
    }

    #[test]
    fn test_agent_runner_creation() {
        let input = TestInput {
            name: "test".to_string(),
            value: 42,
        };
        let runner = AgentRunner::new(
            "test_agent".to_string(),
            input,
            TestRegistry,
            TestModelRegistry,
        );

        assert_eq!(runner.agent, "test_agent");
    }

    #[test]
    fn test_cache_key_generation() {
        let input = TestInput {
            name: "test".to_string(),
            value: 42,
        };
        let runner = AgentRunner::new(
            "test_agent".to_string(),
            input,
            TestRegistry,
            TestModelRegistry,
        );

        let key1 = runner.generate_cache_key("model1");
        let key2 = runner.generate_cache_key("model1");
        let key3 = runner.generate_cache_key("model2");

        // Same inputs should produce same key
        assert_eq!(key1, key2);
        // Different model should produce different key
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_agent_runner_execution() {
        let input = TestInput {
            name: "test".to_string(),
            value: 42,
        };
        let runner = AgentRunner::new(
            "test_agent".to_string(),
            input,
            TestRegistry,
            TestModelRegistry,
        );

        let result = runner.run();
        assert!(result.is_ok());
    }

    #[test]
    fn test_placeholder_replacement_mandatory() {
        let input = TestInput {
            name: "Alice".to_string(),
            value: 100,
        };
        let runner = AgentRunner::new(
            "test_agent".to_string(),
            input,
            TestRegistry,
            TestModelRegistry,
        );

        let template = "Hello {{input.name}}, your value is {{input.value}}!";
        let result = runner.replace_placeholders(template);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Hello Alice, your value is 100!");
    }

    #[test]
    fn test_placeholder_replacement_optional_present() {
        let input = TestInput {
            name: "Bob".to_string(),
            value: 200,
        };
        let runner = AgentRunner::new(
            "test_agent".to_string(),
            input,
            TestRegistry,
            TestModelRegistry,
        );

        let template = "Name: {{input.name?}}";
        let result = runner.replace_placeholders(template);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Name: Bob");
    }

    #[test]
    fn test_placeholder_replacement_optional_missing() {
        let input = TestInput {
            name: "Charlie".to_string(),
            value: 300,
        };
        let runner = AgentRunner::new(
            "test_agent".to_string(),
            input,
            TestRegistry,
            TestModelRegistry,
        );

        let template = "Age: {{input.age?}}";
        let result = runner.replace_placeholders(template);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Age: ");
    }

    #[test]
    fn test_placeholder_replacement_mandatory_missing() {
        let input = TestInput {
            name: "Dave".to_string(),
            value: 400,
        };
        let runner = AgentRunner::new(
            "test_agent".to_string(),
            input,
            TestRegistry,
            TestModelRegistry,
        );

        let template = "Missing: {{input.missing_field}}";
        let result = runner.replace_placeholders(template);

        assert!(result.is_err());
        match result {
            Err(PopulateError::MissingMandatoryPlaceholder(field)) => {
                assert_eq!(field, "input.missing_field");
            }
            _ => panic!("Expected MissingMandatoryPlaceholder error"),
        }
    }

    #[test]
    fn test_placeholder_replacement_invalid_path() {
        let input = TestInput {
            name: "Eve".to_string(),
            value: 500,
        };
        let runner = AgentRunner::new(
            "test_agent".to_string(),
            input,
            TestRegistry,
            TestModelRegistry,
        );

        let template = "Invalid: {{output.field}}";
        let result = runner.replace_placeholders(template);

        assert!(result.is_err());
        match result {
            Err(PopulateError::InvalidPlaceholderPath(path)) => {
                assert_eq!(path, "output.field");
            }
            _ => panic!("Expected InvalidPlaceholderPath error"),
        }
    }

    #[test]
    fn test_placeholder_replacement_nested() {
        let input = NestedTestInput {
            name: "Frank".to_string(),
            location: NestedData {
                city: "Paris".to_string(),
                country: "France".to_string(),
            },
        };
        let runner = AgentRunner::new(
            "test_agent".to_string(),
            input,
            TestRegistry,
            TestModelRegistry,
        );

        let template = "{{input.name}} lives in {{input.location.city}}, {{input.location.country}}";
        let result = runner.replace_placeholders(template);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Frank lives in Paris, France");
    }
}
