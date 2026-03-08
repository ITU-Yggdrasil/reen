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
        "create_specifications_main.yml" => {
            Some(include_str!("../../agents/create_specifications_main.yml"))
        }
        "create_implementation.yml" => Some(include_str!("../../agents/create_implementation.yml")),
        "create_test.yml" => Some(include_str!("../../agents/create_test.yml")),
        "resolve_compilation_errors.yml" => {
            Some(include_str!("../../agents/resolve_compilation_errors.yml"))
        }
        "review_draft_errors.yml" => Some(include_str!("../../agents/review_draft_errors.yml")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{embedded_agent_spec, embedded_default_model_registry};

    #[test]
    fn embedded_model_registry_is_available() {
        let content = embedded_default_model_registry();
        assert!(content.contains("create_implementation"));
    }

    #[test]
    fn embedded_agent_specs_are_available() {
        let content = embedded_agent_spec("create_implementation.yml").expect("embedded spec");
        assert!(content.contains("system_prompt"));
    }
}
