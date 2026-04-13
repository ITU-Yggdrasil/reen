use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::Instant;

use reen::execution::{
    AgentModelRegistry, AgentRunner, AgentRunnerError, NativeExecutionControl, build_agent_input,
    output_contains_questions,
};
use reen::registries::{
    FileAgentModelRegistry, FileAgentRegistry, candidate_agent_spec_filenames, embedded_agent_spec,
};

use super::Config;
use super::progress::{header_text, standard_text};
use super::usage_report::{UsageReporter, UsageScope};

/// Response types from agent execution
pub enum AgentResponse {
    /// Final result from the agent
    Final(String),
    /// Agent has questions that need answers
    Questions(String),
}

#[derive(Clone)]
struct UsageTracking<'a> {
    reporter: &'a UsageReporter,
    scope: &'a UsageScope,
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

    pub async fn execute_with_context_options_tracked(
        &self,
        input: &str,
        additional_context: HashMap<String, serde_json::Value>,
        execution_control: Option<&dyn NativeExecutionControl>,
        ignore_cache_reads: bool,
        tracking: Option<(&UsageReporter, &UsageScope)>,
    ) -> Result<AgentResponse> {
        let tracking = tracking.map(|(reporter, scope)| UsageTracking { reporter, scope });
        self.execute_with_context_options_inner(
            input,
            additional_context,
            execution_control,
            ignore_cache_reads,
            tracking,
        )
        .await
    }

    async fn execute_with_context_options_inner(
        &self,
        input: &str,
        additional_context: HashMap<String, serde_json::Value>,
        execution_control: Option<&dyn NativeExecutionControl>,
        ignore_cache_reads: bool,
        tracking: Option<UsageTracking<'_>>,
    ) -> Result<AgentResponse> {
        if self.verbose {
            println!(
                "{}",
                standard_text(format!("Executing agent: {}", self.agent_name))
            );
        }

        let (provider, model) = self.resolve_provider_model()?;
        let started_at = Instant::now();
        let result = self
            .runner(input, additional_context)
            .run_with_control_options(execution_control, ignore_cache_reads)
            .map_err(Self::map_runner_error)?;
        let elapsed_ms = started_at.elapsed().as_millis();
        if let Some(tracking) = tracking {
            tracking.reporter.record(
                tracking.scope,
                &self.agent_name,
                &provider,
                &model,
                result.cached,
                elapsed_ms,
                result.usage.as_ref(),
            );
        }

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

    pub async fn execute_with_conversation_with_seed_options_tracked(
        &self,
        input: &str,
        context_name: &str,
        mut context: HashMap<String, serde_json::Value>,
        execution_control: Option<&dyn NativeExecutionControl>,
        ignore_cache_reads: bool,
        tracking: Option<(&UsageReporter, &UsageScope)>,
    ) -> Result<String> {
        let mut conversation_round = 0;

        loop {
            conversation_round += 1;

            if self.verbose {
                println!(
                    "{}",
                    standard_text(format!("Conversation round: {}", conversation_round))
                );
            }

            match self
                .execute_with_context_options_tracked(
                    input,
                    context.clone(),
                    execution_control,
                    ignore_cache_reads,
                    tracking,
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
                    println!("\n{}", header_text("=".repeat(60)));
                    println!(
                        "{}",
                        header_text("The agent has questions that need answers.")
                    );
                    println!(
                        "{}",
                        standard_text(format!(
                            "Questions have been written to: {}",
                            questions_file.display()
                        ))
                    );
                    println!(
                        "{}",
                        standard_text("Please edit the file to provide your answers.")
                    );
                    println!("{}", header_text("=".repeat(60)));
                    println!(
                        "\n{}",
                        standard_text(
                            "Press Enter when you're ready to continue (or type 'ready'):"
                        )
                    );

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
        let questions_dir = std::env::current_dir()
            .context("Failed to get current directory")?
            .join("questions");

        // Create questions directory if it doesn't exist
        if !questions_dir.exists() {
            fs::create_dir_all(&questions_dir).context("Failed to create questions directory")?;
        }

        let questions_file = questions_dir.join(format!("{}.md", context_name));

        fs::write(&questions_file, questions).context("Failed to write questions file")?;

        Ok(questions_file)
    }

    fn resolve_provider_model(&self) -> Result<(String, String)> {
        let model = self
            .model_registry
            .get_model(&self.agent_name)
            .map_err(|error| {
                anyhow::anyhow!(
                    "Failed to resolve model for agent '{}': {}",
                    self.agent_name,
                    error
                )
            })?;
        Ok((provider_from_model(&model.name), model.name))
    }
}

fn provider_from_model(model: &str) -> String {
    if let Some((provider, _)) = model.split_once('/') {
        return provider.to_lowercase();
    }

    let model_lower = model.to_lowercase();
    if model_lower.contains("claude") || model_lower.contains("anthropic") {
        "anthropic".to_string()
    } else if ["gpt", "openai", "o1", "o3"]
        .iter()
        .any(|needle| model_lower.contains(needle))
    {
        "openai".to_string()
    } else if model_lower.contains("mistral/") {
        "mistral".to_string()
    } else if [
        "ollama",
        "qwen",
        "llama",
        "mistral",
        "phi",
        "gemma",
        "codellama",
    ]
    .iter()
    .any(|needle| model_lower.contains(needle))
    {
        "ollama".to_string()
    } else {
        "ollama".to_string()
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
            "create_implementation_data:\n  model: claude-3-sonnet\n  parallel: false\n",
        )
        .expect("write model registry");

        let registry = FileAgentModelRegistry::new(Some(registry_path), None, None);
        validate_agent_exists_with_registry("create_implementation_data", &registry)
            .expect("default fallback should validate");
        fs::remove_dir_all(&test_dir).expect("cleanup");
    }
}
