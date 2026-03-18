use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use sha2::Digest;

#[derive(Debug, Clone)]
pub struct FileCache {
    folder: Option<PathBuf>,
    cache: Mutex<HashMap<String, PathBuf>>,
}

impl FileCache {
    pub fn new(folder: Option<String>) -> Self {
        FileCache {
            folder: folder.map(PathBuf::from),
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn add(&self, key: String, value: PathBuf) -> Result<(), String> {
        let mut cache = self.cache.lock().unwrap();
        cache.insert(key, value);
        Ok(())
    }

    pub fn get(&self, key: String) -> Result<PathBuf, String> {
        let cache = self.cache.lock().unwrap();
        cache.get(&key).cloned().ok_or_else(|| format!("Key not found: {}", key))
    }

    pub fn remove(&self, key: String) -> Result<(), String> {
        let mut cache = self.cache.lock().unwrap();
        cache.remove(&key).map_err(|_| format!("Key not found: {}", key))
    }

    pub fn flush(&self) {
        self.cache.lock().unwrap().clear();
    }
}

#[derive(Debug, Clone)]
pub struct AgentInstructions {
    // Define the structure of agent instructions
    // For example:
    // pub agent_id: u32,
    // pub model_name: String,
    // pub input_json: serde_json::Value,
    // ...
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
        use crate::registries::embedded_runner_py;
        use std::io::Write;
        use std::process::{Command, Stdio};

        // Write the embedded runner script to a temp file
        let runner_path = std::env::temp_dir().join("reen_runner.py");
        std::fs::write(&runner_path, embedded_runner_py()).map_err(|e| {
            ExecutionError::PythonRunnerError(format!("Failed to write embedded runner: {}", e))
        })?;

        // Prepare the request JSON
        let request = serde_json::json!({
            "model": model.name,
            "system_prompt": specification.system_prompt
        });

        let request_json = serde_json::to_string(&request).map_err(|e| {
            ExecutionError::PythonRunnerError(format!("Failed to serialize request: {}", e))
        })?;

        // Spawn the Python runner from the embedded temp file.
        // Pass the current working directory so the script can find .env and .venv
        // even though it runs from a temp location.
        let mut child = Command::new("python3")
            .arg(&runner_path)
            .env("REEN_PROJECT_DIR", std::env::current_dir().unwrap_or_default())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                ExecutionError::PythonRunnerError(format!("Failed to spawn Python runner: {}", e))
            })?;

        // Write the request to stdin
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(request_json.as_bytes()).map_err(|e| {
                ExecutionError::PythonRunnerError(format!(
                    "Failed to write to Python runner stdin: {}",
                    e
                ))
            })?;
        }

        // Wait for the process to complete and capture output
        let output = child.wait_with_output().map_err(|e| {
            ExecutionError::PythonRunnerError(format!("Failed to read Python runner output: {}", e))
        })?;

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
        let response_json = String::from_utf8(output.stdout).map_err(|e| {
            ExecutionError::PythonRunnerError(format!("Invalid UTF-8 in response: {}", e))
        })?;

        let response: serde_json::Value = serde_json::from_str(&response_json).map_err(|e| {
            ExecutionError::PythonRunnerError(format!("Failed to parse response JSON: {}", e))
        })?;

        // Check if execution was successful
        if !response["success"].as_bool().unwrap_or(false) {
            let error = response["error"].as_str().unwrap_or("Unknown error");
            return Err(ExecutionError::ExecutionFailed(error.to_string()));
        }

        // Extract the output
        let output_text = response["output"]
            .as_str()
            .ok_or_else(|| ExecutionError::PythonRunnerError("No output in response".to_string()))?
            .to_string();

        let _ = std::fs::remove_file(&runner_path);

        Ok(ExecutionResult {
            output: output_text,
        })
    }

    /// Generates a hash of agent instructions + model name for folder structure
    ///
    /// This hash is used to create a folder that groups all cache entries
    /// for a specific agent instruction set and model combination.
    fn generate_instructions_model_hash(
        &self,
        agent_instructions: &str,
        model_name: &str,
    ) -> String {
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
    fn get_cached_artefact(
        &self,
        agent_instructions: &str,
        model_name: &str,
    ) -> Result<FileCache, ExecutionError> {
        let instructions_model_hash =
            self.generate_instructions_model_hash(agent_instructions, model_name);
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
    fn resolve_path<'a>(
        &self,
        value: &'a serde_json::Value,
        path: &str,
    ) -> Result<Option<&'a serde_json::Value>, PopulateError> {
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
        let agent_template = self
            .agent_registry
            .get_specification(&self.agent)
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

impl FileCache {
    pub fn cache_path(&self, instructions: &AgentInstructions, input_json: &serde_json::Value) -> PathBuf {
        let instructions_model_hash = format!("{:?}", instructions);
        let input_hash = serde_json::json(input_json).to_string();
        let hash = format!("{}/{}", instructions_model_hash, input_hash);
        let mut hasher = sha2::Sha256::new();
        hasher.update(&hash);
        let instructions_model_hash = hasher.finalize().to_vec();
        let instructions_model_hash_str = hex::encode(instructions_model_hash);

        let mut hasher = sha2::Sha256::new();
        hasher.update(&serde_json::json(input_json).to_string());
        let input_hash = hasher.finalize().to_vec();
        let input_hash_str = hex::encode(input_hash);

        let folder_path = self.folder.as_ref().unwrap_or(&Path::new(".reen"));
        let file_path = folder_path.join(format!("{}/{}", instructions_model_hash_str, input_hash_str)).with_extension("cache");
        file_path
    }

    pub fn cache_key(&self, instructions: &AgentInstructions, input_json: &serde_json::Value) -> String {
        format!("{}/{}", FileCache::hash(instructions), serde_json::json(input_json).to_string())
    }

    pub fn instructions_model_hash(&self, instructions: &AgentInstructions) -> String {
        FileCache::hash(instructions)
    }

    pub fn input_hash(&self, input_json: &serde_json::Value) -> String {
        serde_json::json(input_json).to_string()
    }
}