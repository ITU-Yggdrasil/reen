/// Returns the embedded default model registry YAML content.
pub fn embedded_default_model_registry() -> &'static str {
    include_str!("../../agents/agent_model_registry.yml")
}

/// Returns embedded agent specification YAML by filename.
///
/// Expected keys are filenames like `create_implementation.yml`.
pub fn embedded_agent_spec(filename: &str) -> Option<&'static str> {
    match filename {
        "create_specifications_data.yml" => {
            Some(include_str!("../../agents/create_specifications_data.yml"))
        }
        "create_specifications_context.yml" => Some(include_str!(
            "../../agents/create_specifications_context.yml"
        )),
        "create_specifications_external_api.yml" => Some(include_str!(
            "../../agents/create_specifications_external_api.yml"
        )),
        "create_plan.yml" => Some(include_str!("../../agents/create_plan.yml")),
        "create_implementation.yml" => Some(include_str!("../../agents/create_implementation.yml")),
        "create_test.yml" => Some(include_str!("../../agents/create_test.yml")),
        "resolve_compilation_errors.yml" => {
            Some(include_str!("../../agents/resolve_compilation_errors.yml"))
        }
        "fix_draft_blockers.yml" => Some(include_str!("../../agents/fix_draft_blockers.yml")),
        _ => None,
    }
}

/// Returns the canonical set of agent names expected in the model registry.
pub fn embedded_expected_agent_names() -> &'static [&'static str] {
    &[
        "create_specifications_data",
        "create_specifications_context",
        "create_specifications_external_api",
        "create_plan",
        "create_implementation",
        "create_test",
        "resolve_compilation_errors",
        "fix_draft_blockers",
    ]
}

#[cfg(test)]
mod tests {
    use super::{
        embedded_agent_spec, embedded_default_model_registry, embedded_expected_agent_names,
    };

    #[test]
    fn embedded_model_registry_is_available() {
        let content = embedded_default_model_registry();
        assert!(content.contains("create_implementation"));
    }

    #[test]
    fn embedded_agent_specs_are_available() {
        let content = embedded_agent_spec("create_implementation.yml").expect("embedded spec");
        assert!(
            content.contains("static_prompt:") && content.contains("variable_prompt:"),
            "agent spec must define both static_prompt and variable_prompt"
        );
    }

    #[test]
    fn embedded_agent_specs_use_split_prompts_for_cacheable_prefixes() {
        for agent_name in embedded_expected_agent_names() {
            let filename = format!("{agent_name}.yml");
            let content = embedded_agent_spec(&filename).expect("embedded spec");
            assert!(
                content.contains("static_prompt:") && content.contains("variable_prompt:"),
                "{filename} should use split prompts"
            );
            assert!(
                !content.contains("\nsystem_prompt:"),
                "{filename} should not define system_prompt"
            );
        }
    }
}
