use super::agent_spec_resolver::resolve_existing_agent_spec_path;
use crate::contexts::{AgentModelRegistry, AgentRegistry, PopulateError};
use crate::registries::FileAgentModelRegistry;
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
        let model_name = FileAgentModelRegistry::new(None, None, None)
            .get_model(agent_name)
            .map(|model| model.name)
            .unwrap_or_default();

        let Some(agent_path) =
            resolve_existing_agent_spec_path(&self.agents_dir, agent_name, &model_name)
        else {
            return Err(PopulateError::AgentNotFound(agent_name.to_string()));
        };

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
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("reen_agent_registry_{}_{}", prefix, nanos))
    }

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
    fn prefers_model_specific_file_for_supported_variants() {
        let test_dir = unique_test_dir("prefer_variants");
        fs::create_dir_all(test_dir.join("agents")).expect("create agents dir");
        fs::write(
            test_dir.join("agents/create_implementation.yml"),
            "system_prompt: default prompt\n",
        )
        .expect("write default spec");

        let previous_dir = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(&test_dir).expect("set cwd");
        let registry = FileAgentRegistry::new(None);

        let models_and_variants = [
            ("gpt-5", "gpt"),
            ("qwen2.5:7b", "qwen"),
            ("claude-3-opus", "opus"),
            ("claude-3-7-sonnet", "sonnet"),
            ("mistral:7b", "mistral"),
        ];
        for (model_name, variant) in models_and_variants {
            fs::write(
                test_dir.join("agents/agent_model_registry.yml"),
                format!(
                    "create_implementation:\n  model: {}\n  parallel: false\n",
                    model_name
                ),
            )
            .expect("write model registry");
            fs::write(
                test_dir.join(format!("agents/create_implementation.{}.yml", variant)),
                format!("system_prompt: {} prompt\n", variant),
            )
            .expect("write variant spec");
            let output = registry
                .get_specification("create_implementation")
                .expect("spec load");
            assert_eq!(output.trim(), format!("{} prompt", variant));
            fs::remove_file(test_dir.join(format!("agents/create_implementation.{}.yml", variant)))
                .expect("cleanup variant");
        }

        std::env::set_current_dir(previous_dir).expect("restore cwd");
        fs::remove_dir_all(&test_dir).expect("cleanup");
    }

    #[test]
    fn falls_back_to_default_when_variant_missing() {
        let test_dir = unique_test_dir("fallback_default");
        fs::create_dir_all(test_dir.join("agents")).expect("create agents dir");
        fs::write(
            test_dir.join("agents/agent_model_registry.yml"),
            "create_implementation:\n  model: qwen2.5:7b\n  parallel: false\n",
        )
        .expect("write model registry");
        fs::write(
            test_dir.join("agents/create_implementation.yml"),
            "system_prompt: default prompt\n",
        )
        .expect("write default spec");

        let previous_dir = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(&test_dir).expect("set cwd");
        let registry = FileAgentRegistry::new(None);
        let output = registry
            .get_specification("create_implementation")
            .expect("spec load");
        std::env::set_current_dir(previous_dir).expect("restore cwd");
        fs::remove_dir_all(&test_dir).expect("cleanup");

        assert_eq!(output.trim(), "default prompt");
    }

    #[test]
    fn returns_agent_not_found_when_no_candidates_exist() {
        let test_dir = unique_test_dir("missing_both");
        fs::create_dir_all(test_dir.join("agents")).expect("create agents dir");
        fs::write(
            test_dir.join("agents/agent_model_registry.yml"),
            "create_implementation:\n  model: claude-3-opus\n  parallel: false\n",
        )
        .expect("write model registry");

        let previous_dir = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(&test_dir).expect("set cwd");
        let registry = FileAgentRegistry::new(None);
        let error = registry
            .get_specification("create_implementation")
            .expect_err("expected missing agent");
        std::env::set_current_dir(previous_dir).expect("restore cwd");
        fs::remove_dir_all(&test_dir).expect("cleanup");

        match error {
            PopulateError::AgentNotFound(name) => assert_eq!(name, "create_implementation"),
            other => panic!("expected AgentNotFound, got {:?}", other),
        }
    }
}
