use super::agent_spec_resolver::candidate_agent_spec_filenames;
use crate::execution::{
    AgentModelRegistry, AgentRegistry, AgentSpecificationTemplate, ExecutionError, PopulateError,
};
use crate::registries::{FileAgentModelRegistry, embedded_agent_spec};

/// Shared preamble for split implementation agents ([`compose_agent_specification`]).
const IMPLEMENTATION_SHARED_RULES_PREFIX: &str =
    include_str!("../../agents/implementation_shared_static.inc.md");

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
        extract_specification(agent_content).map(|template| compose_agent_specification(agent_name, template))
    }
}

fn compose_agent_specification(
    agent_name: &str,
    template: AgentSpecificationTemplate,
) -> AgentSpecificationTemplate {
    match template {
        AgentSpecificationTemplate::Split {
            static_prompt,
            variable_prompt,
        } => {
            let mut static_prompt = if uses_shared_implementation_rule_prefix(agent_name) {
                format!(
                    "{}\n{}",
                    IMPLEMENTATION_SHARED_RULES_PREFIX.trim_end(),
                    static_prompt.trim_start()
                )
            } else {
                static_prompt
            };
            if let Some(shared_suffix) = shared_implementation_static_suffix(agent_name) {
                static_prompt = format!(
                    "{}\n\n{}",
                    static_prompt.trim_end(),
                    shared_suffix.trim_start()
                );
            }
            AgentSpecificationTemplate::Split {
                static_prompt,
                variable_prompt,
            }
        }
    }
}

fn uses_shared_implementation_rule_prefix(agent_name: &str) -> bool {
    matches!(
        agent_name,
        "create_implementation_data"
            | "create_implementation_projection"
            | "create_implementation_context"
    )
}

fn shared_implementation_static_suffix(agent_name: &str) -> Option<String> {
    let (written_where, extra_dependency_lines) = match agent_name {
        "create_implementation_data" => (
            "- `specifications/data/X.md` → `src/data/X.rs` (your output goes here)\n- `specifications/data/group/X.md` → `src/data/group/X.rs` (your output goes here)",
            "",
        ),
        "create_implementation_projection" => (
            "- `specifications/projections/X.md` → `src/projections/X.rs` (your output goes here)\n- `specifications/projections/group/X.md` → `src/projections/group/X.rs`",
            "- Prefer `input.direct_dependency_contracts` over broad file scraping when resolving role APIs\n",
        ),
        "create_implementation_context" => (
            "- `specifications/contexts/X.md` → `src/contexts/X.rs` (your output goes here)\n- `specifications/contexts/group/X.md` → `src/contexts/group/X.rs`\n- `specifications/app.md` → `src/main.rs`",
            "- Prefer `input.direct_dependency_contracts` over broad file scraping when resolving role APIs\n- Treat transitive dependency entries in the manifest as equally eligible lookup targets\n",
        ),
        _ => return None,
    };

    Some(format!(
        r#"
## CRITICAL: Output Format

**Your output must be ONLY the Rust code for the single .rs file being generated.**

### What to Output:

- **ONLY Rust code** for the specific module/type being implemented
- Start directly with `use` statements, doc comments, or the primary type definition
- NO preamble, NO file listings, NO Cargo.toml content, NO markdown, NO explanations

### What Gets Written Where:

{written_where}

The Cargo.toml and src/lib.rs are managed separately. You ONLY generate the specific .rs file.

## Direct Dependency Context (Authoritative)

If `input.direct_dependencies` is present, it is authoritative project context.

You MUST:
- Treat `input.resolved_dependency_plan` as the authoritative external-crate plan when present
- Treat `input.scaffold_dependencies` as the authoritative list of scaffold-selected external crates when present
- Treat `input.direct_dependencies` as an authoritative dependency manifest
- Treat `input.contract_artifact` as the authoritative machine-readable contract when present
- Prefer provided dependency artifacts over inference
- Reuse existing definitions from dependency context instead of creating duplicates
{extra_dependency_lines}- If `input.tooling_symbols` is present, treat it as the authoritative crate/type inventory
- Do NOT introduce new external crates outside `input.resolved_dependency_plan` / `input.scaffold_dependencies` when those are present
- Prefer standard-library or already-authorized crate features over adding a new dependency
- If you need a custom error type and `thiserror` is not explicitly authorized by the dependency plan/scaffold list, implement `Display` / `std::error::Error` manually instead of using `thiserror`

## Revision Mode (When Present)

- If `input.previous_output` is present, revise that Rust file instead of starting over
- If `input.verifier_feedback` is present, treat every listed verifier issue as a required fix before returning
- Preserve compliant parts of the previous output unless they conflict with the verifier feedback
"#
    ))
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
            .get_specification("create_implementation_data")
            .expect("embedded spec should load");
        assert!(
            template
                .canonical_for_cache()
                .contains("code implementation agent for Data kinds")
        );
    }

    #[test]
    fn falls_back_to_default_embedded_spec_for_variant_model() {
        let registry = FileAgentRegistry::new(None);
        let template = registry
            .get_specification("create_implementation_context")
            .expect("spec load");
        assert!(
            template
                .canonical_for_cache()
                .contains("ONLY functions listed in \"Functionalities\" section can be public")
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

    #[test]
    fn split_implementation_agents_share_static_rule_prefix() {
        let registry = FileAgentRegistry::new(None);
        let prefix = IMPLEMENTATION_SHARED_RULES_PREFIX.trim();
        for agent in [
            "create_implementation_data",
            "create_implementation_projection",
            "create_implementation_context",
        ] {
            let template = registry
                .get_specification(agent)
                .unwrap_or_else(|e| panic!("{agent}: {e:?}"));
            let AgentSpecificationTemplate::Split { static_prompt, .. } = template;
            assert!(
                static_prompt.starts_with(prefix),
                "{agent}: static prompt should start with shared rules prefix"
            );
        }
    }
}
