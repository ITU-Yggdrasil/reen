use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use reen::contexts::{
    AgentModelRegistry, AgentRunner, AgentRunnerError, PreparedExecution, PreparedExecutionState,
};
use reen::registries::{
    candidate_agent_spec_filenames, embedded_agent_spec, embedded_runner_py,
    FileAgentModelRegistry, FileAgentRegistry,
};

use super::Config;

/// Input structure for agent execution
#[derive(Serialize)]
struct AgentInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    draft_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    openapi_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    documentation_urls: Option<Vec<String>>,
    #[serde(flatten)]
    additional: HashMap<String, serde_json::Value>,
}

fn json_value_to_string(value: serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s),
        serde_json::Value::Null => None,
        other => Some(other.to_string()),
    }
}

fn json_value_to_string_vec(value: serde_json::Value) -> Option<Vec<String>> {
    match value {
        serde_json::Value::Array(items) => {
            let values = items
                .into_iter()
                .filter_map(json_value_to_string)
                .collect::<Vec<_>>();
            if values.is_empty() {
                None
            } else {
                Some(values)
            }
        }
        serde_json::Value::String(s) => Some(vec![s]),
        serde_json::Value::Null => None,
        other => Some(vec![other.to_string()]),
    }
}

fn build_agent_input(
    agent_name: &str,
    input: &str,
    mut additional_context: HashMap<String, serde_json::Value>,
) -> AgentInput {
    let openapi_content = additional_context
        .remove("openapi_content")
        .and_then(json_value_to_string);
    let documentation_urls = additional_context
        .remove("documentation_urls")
        .and_then(json_value_to_string_vec);

    match agent_name {
        "create_specifications"
        | "create_specifications_context"
        | "create_specifications_data"
        | "create_specifications_main"
        | "create_specifications_external_api" => AgentInput {
            draft_content: Some(input.to_string()),
            context_content: None,
            openapi_content,
            documentation_urls,
            additional: additional_context,
        },
        "create_implementation" | "create_test" => AgentInput {
            draft_content: None,
            context_content: Some(input.to_string()),
            openapi_content: None,
            documentation_urls: None,
            additional: additional_context,
        },
        "fix_draft_blockers" => AgentInput {
            draft_content: None,
            context_content: None,
            openapi_content,
            documentation_urls,
            additional: additional_context,
        },
        _ => AgentInput {
            draft_content: Some(input.to_string()),
            context_content: None,
            openapi_content,
            documentation_urls,
            additional: additional_context,
        },
    }
}

/// Response types from agent execution
pub enum AgentResponse {
    /// Final result from the agent
    Final(String),
    /// Agent has questions that need answers
    Questions(String),
}

pub struct AgentExecutor {
    agent_name: String,
    verbose: bool,
    agent_registry: FileAgentRegistry,
    model_registry: FileAgentModelRegistry,
}

impl AgentExecutor {
    pub fn new(agent_name: &str, config: &Config) -> Result<Self> {
        validate_agent_exists(agent_name)?;

        Ok(Self {
            agent_name: agent_name.to_string(),
            verbose: config.verbose,
            agent_registry: FileAgentRegistry::new(None),
            model_registry: FileAgentModelRegistry::new(None, None, None),
        })
    }

    /// Whether this agent is configured to run in parallel.
    pub fn can_run_parallel(&self) -> Result<bool> {
        self.model_registry
            .can_run_parallel(&self.agent_name)
            .map_err(|e| anyhow::anyhow!("Failed to read model registry: {}", e))
    }

    /// Whether this agent is configured to use provider-side batch execution.
    pub fn can_use_batch(&self) -> Result<bool> {
        self.model_registry
            .can_use_batch(&self.agent_name)
            .map_err(|e| anyhow::anyhow!("Failed to read model registry: {}", e))
    }

    /// Reference to the model registry (for diagnostics).
    pub fn model_registry(&self) -> &FileAgentModelRegistry {
        &self.model_registry
    }

    /// Executes the agent with additional context (for conversational interactions)
    pub async fn execute_with_context(
        &self,
        input: &str,
        additional_context: HashMap<String, serde_json::Value>,
    ) -> Result<AgentResponse> {
        if self.verbose {
            println!("Executing agent: {}", self.agent_name);
        }

        let agent_input = build_agent_input(&self.agent_name, input, additional_context);

        // Create and run the AgentRunner
        let runner = AgentRunner::new(
            self.agent_name.clone(),
            agent_input,
            self.agent_registry.clone(),
            self.model_registry.clone(),
        );

        let result = runner.run().map_err(|e| match e {
            AgentRunnerError::Populate(populate_err) => {
                anyhow::anyhow!("Failed to populate agent: {}", populate_err)
            }
            AgentRunnerError::Execution(exec_err) => {
                anyhow::anyhow!("Failed to execute agent: {}", exec_err)
            }
        })?;

        // Check if the result contains questions
        if self.contains_questions(&result.output) {
            Ok(AgentResponse::Questions(result.output))
        } else {
            Ok(AgentResponse::Final(result.output))
        }
    }

    /// Returns true if the agent call would be served from the on-disk cache.
    pub fn is_cache_hit(
        &self,
        input: &str,
        additional_context: HashMap<String, serde_json::Value>,
    ) -> Result<bool> {
        let agent_input = build_agent_input(&self.agent_name, input, additional_context);

        let runner = AgentRunner::new(
            self.agent_name.clone(),
            agent_input,
            self.agent_registry.clone(),
            self.model_registry.clone(),
        );

        runner.is_cache_hit().map_err(|e| match e {
            AgentRunnerError::Populate(populate_err) => {
                anyhow::anyhow!("Failed to populate agent: {}", populate_err)
            }
            AgentRunnerError::Execution(exec_err) => {
                anyhow::anyhow!("Failed to execute agent: {}", exec_err)
            }
        })
    }

    /// Estimates the populated request input tokens for this agent invocation.
    pub fn estimate_request_tokens(
        &self,
        input: &str,
        additional_context: HashMap<String, serde_json::Value>,
    ) -> Result<usize> {
        let agent_input = build_agent_input(&self.agent_name, input, additional_context);

        let runner = AgentRunner::new(
            self.agent_name.clone(),
            agent_input,
            self.agent_registry.clone(),
            self.model_registry.clone(),
        );

        runner.estimate_input_tokens().map_err(|e| match e {
            AgentRunnerError::Populate(populate_err) => {
                anyhow::anyhow!("Failed to populate agent: {}", populate_err)
            }
            AgentRunnerError::Execution(exec_err) => {
                anyhow::anyhow!("Failed to execute agent: {}", exec_err)
            }
        })
    }

    /// Prepares a request and cache metadata without executing the model.
    pub fn prepare_execution(
        &self,
        input: &str,
        additional_context: HashMap<String, serde_json::Value>,
    ) -> Result<PreparedExecutionState> {
        let agent_input = build_agent_input(&self.agent_name, input, additional_context);

        let runner = AgentRunner::new(
            self.agent_name.clone(),
            agent_input,
            self.agent_registry.clone(),
            self.model_registry.clone(),
        );

        runner.prepare_execution().map_err(|e| match e {
            AgentRunnerError::Populate(populate_err) => {
                anyhow::anyhow!("Failed to populate agent: {}", populate_err)
            }
            AgentRunnerError::Execution(exec_err) => {
                anyhow::anyhow!("Failed to execute agent: {}", exec_err)
            }
        })
    }

    /// Executes a batch of prepared requests in a single Python runner process.
    pub fn execute_batch(
        &self,
        prepared: Vec<(String, PreparedExecution)>,
    ) -> Result<HashMap<String, String>> {
        if prepared.is_empty() {
            return Ok(HashMap::new());
        }

        let runner_path = std::env::temp_dir().join("reen_runner.py");
        fs::write(&runner_path, embedded_runner_py())
            .context("Failed to write embedded Python runner")?;

        let batch_requests = prepared
            .iter()
            .map(|(custom_id, item)| {
                serde_json::json!({
                    "custom_id": custom_id,
                    "request": item.request,
                })
            })
            .collect::<Vec<_>>();

        let request_json = serde_json::to_string(&serde_json::json!({
            "batch_requests": batch_requests
        }))
        .context("Failed to serialize batch request")?;

        let mut child = Command::new("python3")
            .arg(&runner_path)
            .env(
                "REEN_PROJECT_DIR",
                std::env::current_dir().unwrap_or_default(),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn Python runner")?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(request_json.as_bytes())
                .context("Failed to write batch request to Python runner stdin")?;
        }

        let output = child
            .wait_with_output()
            .context("Failed to read Python runner batch output")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            anyhow::bail!(
                "Python runner batch failed. Stdout: {} Stderr: {}",
                stdout,
                stderr
            );
        }

        let response_json =
            String::from_utf8(output.stdout).context("Invalid UTF-8 in Python batch response")?;
        let response: serde_json::Value = serde_json::from_str(&response_json)
            .context("Failed to parse Python batch response")?;

        if !response["success"].as_bool().unwrap_or(false) {
            let error = response["error"].as_str().unwrap_or("Unknown batch error");
            anyhow::bail!("{}", error);
        }

        let outputs = response["outputs"]
            .as_array()
            .context("Batch response missing outputs")?;
        let mut results = HashMap::new();
        for item in outputs {
            let custom_id = item["custom_id"]
                .as_str()
                .context("Batch output missing custom_id")?;
            let output = item["output"]
                .as_str()
                .context("Batch output missing output")?;
            results.insert(custom_id.to_string(), output.to_string());
        }

        for (custom_id, item) in prepared {
            if let Some(output) = results.get(&custom_id) {
                item.store_output(output);
            }
        }

        Ok(results)
    }

    /// Detects if the agent output contains questions
    fn contains_questions(&self, output: &str) -> bool {
        // Simple heuristic: check for question markers
        // A more sophisticated implementation might parse structured output
        let question_markers = ["?", "## Questions", "# Questions", "**Questions**"];

        question_markers
            .iter()
            .any(|marker| output.contains(marker))
            && (output.contains("clarification")
                || output.contains("answer")
                || output.contains("question"))
    }

    /// Handles the full conversational loop with question/answer cycles and caller-provided seed context
    pub async fn execute_with_conversation_with_seed(
        &self,
        input: &str,
        context_name: &str,
        mut context: HashMap<String, serde_json::Value>,
    ) -> Result<String> {
        let mut conversation_round = 0;

        loop {
            conversation_round += 1;

            if self.verbose {
                println!("Conversation round: {}", conversation_round);
            }

            match self.execute_with_context(input, context.clone()).await? {
                AgentResponse::Final(result) => {
                    return Ok(result);
                }
                AgentResponse::Questions(questions) => {
                    if conversation_round > 10 {
                        anyhow::bail!("Too many conversation rounds - possible infinite loop");
                    }

                    // Write questions to file
                    let questions_file = self.write_questions_file(context_name, &questions)?;

                    // Prompt user for answers
                    println!("\n{}", "=".repeat(60));
                    println!("The agent has questions that need answers.");
                    println!(
                        "Questions have been written to: {}",
                        questions_file.display()
                    );
                    println!("Please edit the file to provide your answers.");
                    println!("{}", "=".repeat(60));
                    println!("\nPress Enter when you're ready to continue (or type 'ready'):");

                    // Wait for user input
                    let mut user_input = String::new();
                    io::stdin()
                        .read_line(&mut user_input)
                        .context("Failed to read user input")?;

                    // Read answers from file
                    let answers = fs::read_to_string(&questions_file)
                        .context("Failed to read answers file")?;

                    // Add answers to context for next round
                    context.insert(
                        "previous_questions".to_string(),
                        serde_json::Value::String(questions),
                    );
                    context.insert(
                        "user_answers".to_string(),
                        serde_json::Value::String(answers),
                    );
                    context.insert(
                        "conversation_round".to_string(),
                        serde_json::Value::Number(conversation_round.into()),
                    );
                }
            }
        }
    }

    /// Writes questions to a file in the questions/ directory
    fn write_questions_file(&self, context_name: &str, questions: &str) -> Result<PathBuf> {
        let questions_dir = PathBuf::from("questions");

        // Create questions directory if it doesn't exist
        if !questions_dir.exists() {
            fs::create_dir_all(&questions_dir).context("Failed to create questions directory")?;
        }

        let questions_file = questions_dir.join(format!("{}.md", context_name));

        fs::write(&questions_file, questions).context("Failed to write questions file")?;

        Ok(questions_file)
    }
}

fn validate_agent_exists(agent_name: &str) -> Result<()> {
    validate_agent_exists_with_registry(agent_name, &FileAgentModelRegistry::new(None, None, None))
}

fn validate_agent_exists_with_registry(
    agent_name: &str,
    model_registry: &FileAgentModelRegistry,
) -> Result<()> {
    let model_name = model_registry
        .get_model(agent_name)
        .map(|model| model.name)
        .unwrap_or_default();
    let candidates = candidate_agent_spec_filenames(agent_name, &model_name);
    if candidates
        .iter()
        .all(|filename| embedded_agent_spec(filename).is_none())
    {
        let expected = candidates.into_iter().collect::<Vec<_>>().join(" or ");
        anyhow::bail!(
            "Agent '{}' not found. Expected file: {}",
            agent_name,
            expected
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_agent_exists_with_registry;
    use reen::registries::FileAgentModelRegistry;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("reen_agent_executor_{}_{}", prefix, nanos))
    }

    #[test]
    fn validate_uses_embedded_specs_without_agents_symlink() {
        let test_dir = unique_test_dir("embedded_only");
        fs::create_dir_all(&test_dir).expect("create temp dir");
        let registry_path = test_dir.join("agent_model_registry.yml");
        fs::write(
            &registry_path,
            "create_implementation:\n  model: claude-3-sonnet\n  parallel: false\n",
        )
        .expect("write model registry");

        let registry = FileAgentModelRegistry::new(Some(registry_path), None, None);
        validate_agent_exists_with_registry("create_implementation", &registry)
            .expect("default fallback should validate");
        fs::remove_dir_all(&test_dir).expect("cleanup");
    }
}
