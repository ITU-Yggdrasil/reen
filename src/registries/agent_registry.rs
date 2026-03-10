use super::agent_spec_resolver::candidate_agent_spec_filenames;
use crate::contexts::{AgentModelRegistry, AgentRegistry, ExecutionError, PopulateError};
use crate::registries::{embedded_agent_spec, FileAgentModelRegistry};

/// File-based implementation of AgentRegistry
/// Loads agent specifications from YAML files in the agents/ directory
#[derive(Clone)]
pub struct FileAgentRegistry;

impl FileAgentRegistry {
    /// Creates a new FileAgentRegistry
    ///
    /// # Arguments
    /// * `agents_dir` - Optional path to agents directory (defaults to "agents")
    pub fn new(_agents_dir: Option<std::path::PathBuf>) -> Self {
        Self
    }
}

impl AgentRegistry for FileAgentRegistry {
    fn get_specification(&self, agent_name: &str) -> Result<String, PopulateError> {
        let model_name = FileAgentModelRegistry::new(None, None, None)
            .get_model(agent_name)
            .map(|model| model.name)
            .map_err(|e| match e {
                ExecutionError::ModelNotFound(_) => {
                    PopulateError::AgentNotFound(agent_name.to_string())
                }
                _ => PopulateError::InvalidSpecification(format!(
                    "Failed to resolve model for '{}': {}",
                    agent_name, e
                )),
            })?;

        let candidate_names = candidate_agent_spec_filenames(agent_name, &model_name);
        let Some(agent_content) = candidate_names
            .iter()
            .find_map(|name| embedded_agent_spec(name))
        else {
            return Err(PopulateError::AgentNotFound(agent_name.to_string()));
        };

        // Extract the system_prompt from the YAML.
        // This is a simple extraction - could use a proper YAML parser.
        extract_system_prompt(agent_content)
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

    #[test]
    fn loads_embedded_agent_specification() {
        let registry = FileAgentRegistry::new(None);
        let output = registry
            .get_specification("create_implementation")
            .expect("embedded spec should load");
        assert!(output.contains("code implementation agent"));
    }

    #[test]
    fn falls_back_to_default_embedded_spec_for_variant_model() {
        let registry = FileAgentRegistry::new(None);
        let output = registry
            .get_specification("create_implementation")
            .expect("spec load");
        assert!(output.contains("Strict Specification Compliance"));
    }

    #[test]
    fn returns_agent_not_found_when_no_candidates_exist() {
        let registry = FileAgentRegistry::new(None);
        let error = registry
            .get_specification("agent_that_does_not_exist")
            .expect_err("expected missing agent");

        match error {
            PopulateError::AgentNotFound(name) => assert_eq!(name, "agent_that_does_not_exist"),
            other => panic!("expected AgentNotFound, got {:?}", other),
        }
    }
}
