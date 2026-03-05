use crate::contexts::{AgentRegistry, PopulateError};
use std::fs;
use std::path::PathBuf;

/// File-based implementation of AgentRegistry
/// Loads agent specifications from YAML files in the agents/ directory
#[derive(Clone)]
pub struct FileAgentRegistry {
    agents_dir: PathBuf,
}

impl FileAgentRegistry {
    /// Creates a new FileAgentRegistry
    ///
    /// # Arguments
    /// * `agents_dir` - Optional path to agents directory (defaults to "agents")
    pub fn new(agents_dir: Option<PathBuf>) -> Self {
        Self {
            agents_dir: agents_dir.unwrap_or_else(|| PathBuf::from("agents")),
        }
    }
}

impl AgentRegistry for FileAgentRegistry {
    fn get_specification(&self, agent_name: &str) -> Result<String, PopulateError> {
        let agent_path = self.agents_dir.join(format!("{}.yml", agent_name));

        if !agent_path.exists() {
            return Err(PopulateError::AgentNotFound(agent_name.to_string()));
        }

        fs::read_to_string(&agent_path)
            .map_err(|e| {
                PopulateError::InvalidSpecification(format!(
                    "Failed to read agent specification {}: {}",
                    agent_path.display(),
                    e
                ))
            })
            .and_then(|content| {
                // Extract the system_prompt from the YAML
                // This is a simple extraction - could use a proper YAML parser
                extract_system_prompt(&content)
            })
    }
}

/// Extracts the system_prompt field from a YAML agent specification
fn extract_system_prompt(yaml_content: &str) -> Result<String, PopulateError> {
    use yaml_rust::YamlLoader;

    let docs = YamlLoader::load_from_str(yaml_content)
        .map_err(|e| PopulateError::InvalidSpecification(format!("Invalid YAML: {}", e)))?;

    if docs.is_empty() {
        return Err(PopulateError::InvalidSpecification(
            "Empty YAML document".to_string(),
        ));
    }

    let doc = &docs[0];

    // Extract the system_prompt field
    if let Some(system_prompt) = doc["system_prompt"].as_str() {
        Ok(system_prompt.to_string())
    } else {
        Err(PopulateError::InvalidSpecification(
            "No system_prompt field found in agent specification".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_system_prompt() {
        let yaml = r#"
name: test_agent
description: Test agent
system_prompt: |
  This is a test prompt.
  It has multiple lines.
"#;

        let result = extract_system_prompt(yaml);
        assert!(result.is_ok());
        let prompt = result.unwrap();
        assert!(prompt.contains("This is a test prompt"));
    }

    #[test]
    fn test_extract_system_prompt_missing() {
        let yaml = r#"
name: test_agent
description: Test agent
"#;

        let result = extract_system_prompt(yaml);
        assert!(result.is_err());
    }
}
