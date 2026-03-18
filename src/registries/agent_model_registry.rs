use crate::contexts::{AgentModelRegistry, ExecutionError, Model};
use crate::registries::embedded_default_model_registry;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Resolves the agent model registry path by searching upward from the current
/// directory for a directory that contains `agents/agent_model_registry.yml`.
/// This ensures the same registry is used when running from project subdirectories
/// (e.g. `tests/snake`). Falls back to `agents/agent_model_registry.yml` relative
/// to the current directory if no project root is found.
pub fn resolve_registry_path() -> PathBuf {
    const REGISTRY_REL: &str = "agents/agent_model_registry.yml";
    let cwd = match std::env::current_dir() {
        Ok(p) => p,
        Err(_) => return PathBuf::from(REGISTRY_REL),
    };
    let mut dir = cwd.as_path();
    loop {
        let candidate = dir.join(REGISTRY_REL);
        if candidate.exists() {
            return candidate;
        }
        if let Some(parent) = dir.parent() {
            dir = parent;
        } else {
            break;
        }
    }
    PathBuf::from(REGISTRY_REL)
}

/// Agent configuration from the registry
#[derive(Clone, Debug)]
pub struct AgentConfig {
    pub model: String,
    pub parallel: bool,
}

/// File-based implementation of AgentModelRegistry
/// Loads agent-to-model mappings from a YAML file
#[derive(Clone)]
pub struct FileAgentModelRegistry {
    registry_path: PathBuf,
    default_model: String,
    default_parallel: bool,
}

impl FileAgentModelRegistry {
    /// Creates a new FileAgentModelRegistry
    ///
    /// # Arguments
    /// * `registry_path` - Optional path to registry file (defaults to "agents/agent_model_registry.yml")
    /// * `default_model` - Default model to use if agent not found in registry
    /// * `default_parallel` - Default parallel execution setting if agent not found in registry
    pub fn new(
        registry_path: Option<PathBuf>,
        default_model: Option<String>,
        default_parallel: Option<bool>,
    ) -> Self {
        Self {
            registry_path: registry_path.unwrap_or_else(resolve_registry_path),
            default_model: default_model.unwrap_or_else(|| "default".to_string()),
            default_parallel: default_parallel.unwrap_or(false),
        }
    }

    /// Loads the registry from the file
    fn load_registry(&self) -> Result<HashMap<String, AgentConfig>, ExecutionError> {
        let content = if self.registry_path.exists() {
            fs::read_to_string(&self.registry_path).map_err(|e| {
                ExecutionError::ExecutionFailed(format!(
                    "Failed to read agent model registry: {}",
                    e
                ))
            })?
        } else {
            embedded_default_model_registry().to_string()
        };

        parse_registry(&content, &self.default_model, self.default_parallel)
    }

    /// Checks if an agent can run in parallel
    pub fn can_run_parallel(&self, agent_name: &str) -> Result<bool, ExecutionError> {
        let registry = self.load_registry()?;

        if let Some(config) = registry.get(agent_name) {
            Ok(config.parallel)
        } else {
            Ok(self.default_parallel)
        }
    }

    /// Path to the registry file (for diagnostics).
    pub fn registry_path(&self) -> &Path {
        &self.registry_path
    }
}

impl AgentModelRegistry for FileAgentModelRegistry {
    fn get_model(&self, agent_name: &str) -> Result<Model, ExecutionError> {
        let registry = self.load_registry()?;

        let model_name = registry
            .get(agent_name)
            .map(|config| config.model.clone())
            .unwrap_or_else(|| self.default_model.clone());

        Ok(Model { name: model_name })
    }
}

/// Parses the YAML registry file into a HashMap
/// Supports both old format (string) and new format (object with model and parallel)
fn parse_registry(
    yaml_content: &str,
    default_model: &str,
    default_parallel: bool,
) -> Result<HashMap<String, AgentConfig>, ExecutionError> {
    use yaml_rust::YamlLoader;

    let docs = YamlLoader::load_from_str(yaml_content)
        .map_err(|e| ExecutionError::ExecutionFailed(format!("Invalid registry YAML: {}", e)))?;

    if docs.is_empty() {
        return Ok(HashMap::new());
    }

    let doc = &docs[0];
    let mut registry = HashMap::new();

    // Extract key-value pairs from the YAML
    if let Some(hash) = doc.as_hash() {
        for (key, value) in hash {
            if let Some(k) = key.as_str() {
                let config = if let Some(v_str) = value.as_str() {
                    // Old format: simple string value (model name)
                    AgentConfig {
                        model: v_str.to_string(),
                        parallel: default_parallel,
                    }
                } else if let Some(v_hash) = value.as_hash() {
                    // New format: object with model and parallel
                    let model = v_hash
                        .get(&yaml_rust::Yaml::String("model".to_string()))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| default_model.to_string());

                    let parallel = v_hash
                        .get(&yaml_rust::Yaml::String("parallel".to_string()))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(default_parallel);

                    AgentConfig { model, parallel }
                } else {
                    // Fallback to defaults
                    AgentConfig {
                        model: default_model.to_string(),
                        parallel: default_parallel,
                    }
                };

                registry.insert(k.to_string(), config);
            }
        }
    }

    Ok(registry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("reen_agent_model_registry_{}_{}", prefix, nanos))
    }

    #[test]
    fn test_parse_registry_old_format() {
        let yaml = r#"
create_specifications: gpt-4
create_implementation: claude-3-opus
create_test: gpt-4
"#;

        let result = parse_registry(yaml, "default", false);
        assert!(result.is_ok());

        let registry = result.unwrap();
        assert_eq!(
            registry.get("create_specifications").map(|c| &c.model),
            Some(&"gpt-4".to_string())
        );
        assert_eq!(
            registry.get("create_implementation").map(|c| &c.model),
            Some(&"claude-3-opus".to_string())
        );
    }

    #[test]
    fn test_parse_registry_new_format() {
        let yaml = r#"
create_specifications:
  model: gpt-4
  parallel: true
create_implementation:
  model: claude-3-opus
  parallel: false
"#;

        let result = parse_registry(yaml, "default", false);
        assert!(result.is_ok());

        let registry = result.unwrap();
        let spec_config = registry.get("create_specifications").unwrap();
        assert_eq!(spec_config.model, "gpt-4");
        assert_eq!(spec_config.parallel, true);

        let impl_config = registry.get("create_implementation").unwrap();
        assert_eq!(impl_config.model, "claude-3-opus");
        assert_eq!(impl_config.parallel, false);
    }

    #[test]
    fn test_parse_empty_registry() {
        let yaml = "";
        let result = parse_registry(yaml, "default", false);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn uses_embedded_default_when_local_registry_missing() {
        let registry = FileAgentModelRegistry::new(
            Some(PathBuf::from(
                "__definitely_missing__/agent_model_registry.yml",
            )),
            Some("default".to_string()),
            Some(false),
        );
        let model = registry
            .get_model("create_implementation")
            .expect("embedded default registry should resolve model");
        assert_eq!(model.name, "qwen2.5:7b");
    }

    #[test]
    fn local_registry_overrides_embedded_default() {
        let test_dir = unique_test_dir("override");
        fs::create_dir_all(&test_dir).expect("create temp dir");
        let registry_path = test_dir.join("agent_model_registry.yml");
        fs::write(
            &registry_path,
            "create_implementation:\n  model: gpt-5\n  parallel: false\n",
        )
        .expect("write local registry");

        let registry = FileAgentModelRegistry::new(Some(registry_path), None, None);
        let model = registry
            .get_model("create_implementation")
            .expect("local override should resolve");
        assert_eq!(model.name, "gpt-5");

        fs::remove_dir_all(&test_dir).expect("cleanup");
    }
}
