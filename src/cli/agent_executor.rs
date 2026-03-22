use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;

use reen::execution::{
    AgentModelRegistry, AgentRunner, AgentRunnerError, NativeExecutionControl, PreparedExecution,
    PreparedExecutionState, build_agent_input, execute_native_request_with_metadata,
    output_contains_questions,
};
use reen::registries::{
    FileAgentModelRegistry, FileAgentRegistry, candidate_agent_spec_filenames, embedded_agent_spec,
};

use super::Config;

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

    fn runner(
        &self,
        input: &str,
        additional_context: HashMap<String, serde_json::Value>,
    ) -> AgentRunner<reen::execution::AgentInput, FileAgentRegistry, FileAgentModelRegistry> {
        let agent_input = build_agent_input(&self.agent_name, input, additional_context);
        AgentRunner::new(
            self.agent_name.clone(),
            agent_input,
            self.agent_registry.clone(),
            self.model_registry.clone(),
        )
    }

    fn map_runner_error(error: AgentRunnerError) -> anyhow::Error {
        match error {
            AgentRunnerError::Populate(populate_err) => {
                anyhow::anyhow!("Failed to populate agent: {}", populate_err)
            }
            AgentRunnerError::Execution(exec_err) => {
                anyhow::anyhow!("Failed to execute agent: {}", exec_err)
            }
        }
    }

    /// Executes the agent with additional context (for conversational interactions)
    pub async fn execute_with_context(
        &self,
        input: &str,
        additional_context: HashMap<String, serde_json::Value>,
        execution_control: Option<&dyn NativeExecutionControl>,
    ) -> Result<AgentResponse> {
        self.execute_with_context_options(input, additional_context, execution_control, false)
            .await
    }

    pub async fn execute_with_context_options(
        &self,
        input: &str,
        additional_context: HashMap<String, serde_json::Value>,
        execution_control: Option<&dyn NativeExecutionControl>,
        ignore_cache_reads: bool,
    ) -> Result<AgentResponse> {
        if self.verbose {
            println!("Executing agent: {}", self.agent_name);
        }

        let result = self
            .runner(input, additional_context)
            .run_with_control_options(execution_control, ignore_cache_reads)
            .map_err(Self::map_runner_error)?;

        if output_contains_questions(&result.output) {
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
        self.runner(input, additional_context)
            .is_cache_hit()
            .map_err(Self::map_runner_error)
    }

    /// Estimates the populated request input tokens for this agent invocation.
    pub fn estimate_request_tokens(
        &self,
        input: &str,
        additional_context: HashMap<String, serde_json::Value>,
    ) -> Result<usize> {
        self.runner(input, additional_context)
            .estimate_input_tokens()
            .map_err(Self::map_runner_error)
    }

    pub fn prepare_execution_options(
        &self,
        input: &str,
        additional_context: HashMap<String, serde_json::Value>,
        ignore_cache_reads: bool,
    ) -> Result<PreparedExecutionState> {
        self.runner(input, additional_context)
            .prepare_execution_options(ignore_cache_reads)
            .map_err(Self::map_runner_error)
    }

    /// Executes prepared requests sequentially via the native Rust runner.
    pub fn execute_batch(
        &self,
        prepared: Vec<(String, PreparedExecution)>,
        execution_control: Option<&dyn NativeExecutionControl>,
    ) -> Result<HashMap<String, String>> {
        if prepared.is_empty() {
            return Ok(HashMap::new());
        }
        let mut results = HashMap::new();

        for (custom_id, item) in prepared {
            let output = execute_native_request_with_metadata(&item.request, execution_control)
                .map(|result| result.output)
                .map_err(|error| {
                    anyhow::anyhow!("Native runner failed for batch item '{custom_id}': {error}")
                })?;
            item.store_output(&output);
            results.insert(custom_id, output);
        }

        Ok(results)
    }

    pub async fn execute_with_conversation_with_seed_options(
        &self,
        input: &str,
        context_name: &str,
        mut context: HashMap<String, serde_json::Value>,
        execution_control: Option<&dyn NativeExecutionControl>,
        ignore_cache_reads: bool,
    ) -> Result<String> {
        let mut conversation_round = 0;

        loop {
            conversation_round += 1;

            if self.verbose {
                println!("Conversation round: {}", conversation_round);
            }

            match self
                .execute_with_context_options(
                    input,
                    context.clone(),
                    execution_control,
                    ignore_cache_reads,
                )
                .await?
            {
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
