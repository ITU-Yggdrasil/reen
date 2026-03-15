use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;

use reen::contexts::{AgentModelRegistry, AgentRunner, AgentRunnerError};
use reen::registries::{
    candidate_agent_spec_filenames, embedded_agent_spec, FileAgentModelRegistry, FileAgentRegistry,
};

use super::Config;

/// Input structure for agent execution
#[derive(Serialize)]
struct AgentInput {
    draft_content: Option<String>,
    context_content: Option<String>,
    #[serde(flatten)]
    additional: HashMap<String, serde_json::Value>,
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

        let enriched_context = additional_context;

        // Prepare input based on agent type
        let agent_input = match self.agent_name.as_str() {
            "create_specifications"
            | "create_specifications_context"
            | "create_specifications_data"
            | "create_specifications_main" => AgentInput {
                draft_content: Some(input.to_string()),
                context_content: None,
                additional: enriched_context,
            },
            "create_implementation" | "create_test" => AgentInput {
                draft_content: None,
                context_content: Some(input.to_string()),
                additional: enriched_context,
            },
            "fix_draft_blockers" => AgentInput {
                draft_content: None,
                context_content: None,
                additional: enriched_context,
            },
            _ => AgentInput {
                draft_content: Some(input.to_string()),
                context_content: None,
                additional: enriched_context,
            },
        };

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
        let agent_input = match self.agent_name.as_str() {
            "create_specifications"
            | "create_specifications_context"
            | "create_specifications_data"
            | "create_specifications_main" => AgentInput {
                draft_content: Some(input.to_string()),
                context_content: None,
                additional: additional_context,
            },
            "create_implementation" | "create_test" => AgentInput {
                draft_content: None,
                context_content: Some(input.to_string()),
                additional: additional_context,
            },
            "fix_draft_blockers" => AgentInput {
                draft_content: None,
                context_content: None,
                additional: additional_context,
            },
            _ => AgentInput {
                draft_content: Some(input.to_string()),
                context_content: None,
                additional: additional_context,
            },
        };

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
        let agent_input = match self.agent_name.as_str() {
            "create_specifications"
            | "create_specifications_context"
            | "create_specifications_data"
            | "create_specifications_main" => AgentInput {
                draft_content: Some(input.to_string()),
                context_content: None,
                additional: additional_context,
            },
            "create_implementation" | "create_test" => AgentInput {
                draft_content: None,
                context_content: Some(input.to_string()),
                additional: additional_context,
            },
            "fix_draft_blockers" => AgentInput {
                draft_content: None,
                context_content: None,
                additional: additional_context,
            },
            _ => AgentInput {
                draft_content: Some(input.to_string()),
                context_content: None,
                additional: additional_context,
            },
        };

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
