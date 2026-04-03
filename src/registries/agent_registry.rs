use super::agent_spec_resolver::candidate_agent_spec_filenames;
use crate::execution::{
    AgentModelRegistry, AgentRegistry, AgentSpecificationTemplate, ExecutionError, PopulateError,
};
use crate::registries::{FileAgentModelRegistry, embedded_agent_spec};

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
    fn get_specification(
        &self,
        agent_name: &str,
    ) -> Result<AgentSpecificationTemplate, PopulateError> {
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

        // Extract the specification from the YAML.
        extract_specification(agent_content)
    }
}

/// Extracts the agent specification from YAML.
fn extract_specification(yaml_content: &str) -> Result<AgentSpecificationTemplate, PopulateError> {
    use yaml_rust::YamlLoader;

    let docs = YamlLoader::load_from_str(yaml_content)
        .map_err(|e| PopulateError::InvalidSpecification(format!("Invalid YAML: {}", e)))?;

    if docs.is_empty() {
        return Err(PopulateError::InvalidSpecification(
            "Empty YAML document".to_string(),
        ));
    }

    let doc = &docs[0];

    let static_p = doc["static_prompt"].as_str();
    let variable_p = doc["variable_prompt"].as_str();
    if let (Some(static_prompt), Some(variable_prompt)) = (static_p, variable_p) {
        return Ok(AgentSpecificationTemplate::Split {
            static_prompt: static_prompt.to_string(),
            variable_prompt: variable_prompt.to_string(),
        });
    }

    Err(PopulateError::InvalidSpecification(
        "Agent specification must define both static_prompt and variable_prompt".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::AgentSpecificationTemplate;

    #[test]
    fn test_extract_specification_split() {
        let yaml = r#"
name: test_agent
description: Test agent
static_prompt: |
  Static instructions here.
variable_prompt: |
  Variable {{input.foo}} here.
"#;

        let result = extract_specification(yaml);
        assert!(result.is_ok());
        let AgentSpecificationTemplate::Split {
            static_prompt,
            variable_prompt,
        } = result.unwrap();
        assert!(static_prompt.contains("Static instructions"));
        assert!(variable_prompt.contains("{{input.foo}}"));
    }

    #[test]
    fn test_extract_specification_missing() {
        let yaml = r#"
name: test_agent
description: Test agent
"#;

        let result = extract_specification(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn loads_embedded_agent_specification() {
        let registry = FileAgentRegistry::new(None);
        let template = registry
            .get_specification("create_implementation")
            .expect("embedded spec should load");
        assert!(
            template
                .canonical_for_cache()
                .contains("code implementation agent")
        );
    }

    #[test]
    fn falls_back_to_default_embedded_spec_for_variant_model() {
        let registry = FileAgentRegistry::new(None);
        let template = registry
            .get_specification("create_implementation")
            .expect("spec load");
        assert!(
            template
                .canonical_for_cache()
                .contains("Strict Specification Compliance")
        );
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
