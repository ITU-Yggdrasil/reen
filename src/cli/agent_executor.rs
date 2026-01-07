use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;

use reen::contexts::{AgentRunner, AgentRunnerError};
use reen::registries::{FileAgentModelRegistry, FileAgentRegistry};

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

    /// Executes the agent with additional context (for conversational interactions)
    pub async fn execute_with_context(
        &self,
        input: &str,
        additional_context: HashMap<String, serde_json::Value>,
    ) -> Result<AgentResponse> {
        if self.verbose {
            println!("Executing agent: {}", self.agent_name);
        }

        // Prepare input based on agent type
        let agent_input = match self.agent_name.as_str() {
            "create_specifications" | "create_specifications_context" | "create_specifications_data" | "create_specifications_main" => AgentInput {
                draft_content: Some(input.to_string()),
                context_content: None,
                additional: additional_context,
            },
            "create_implementation" | "create_test" => AgentInput {
                draft_content: None,
                context_content: Some(input.to_string()),
                additional: additional_context,
            },
            _ => AgentInput {
                draft_content: Some(input.to_string()),
                context_content: None,
                additional: additional_context,
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

    /// Detects if the agent output contains questions
    fn contains_questions(&self, output: &str) -> bool {
        // Simple heuristic: check for question markers
        // A more sophisticated implementation might parse structured output
        let question_markers = ["?", "## Questions", "# Questions", "**Questions**"];

        question_markers.iter().any(|marker| output.contains(marker))
            && (output.contains("clarification") || output.contains("answer") || output.contains("question"))
    }

    /// Handles the full conversational loop with question/answer cycles
    pub async fn execute_with_conversation(&self, input: &str, context_name: &str) -> Result<String> {
        let mut context: HashMap<String, serde_json::Value> = HashMap::new();
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
                    println!("Questions have been written to: {}", questions_file.display());
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
            fs::create_dir_all(&questions_dir)
                .context("Failed to create questions directory")?;
        }

        let questions_file = questions_dir.join(format!("{}.md", context_name));

        fs::write(&questions_file, questions)
            .context("Failed to write questions file")?;

        Ok(questions_file)
    }

    /// Gets a reference to the model registry
    pub fn model_registry(&self) -> &FileAgentModelRegistry {
        &self.model_registry
    }
}

fn validate_agent_exists(agent_name: &str) -> Result<()> {
    let agent_path = PathBuf::from("agents").join(format!("{}.yml", agent_name));

    if !agent_path.exists() {
        anyhow::bail!(
            "Agent '{}' not found. Expected file: {}",
            agent_name,
            agent_path.display()
        );
    }

    Ok(())
}
