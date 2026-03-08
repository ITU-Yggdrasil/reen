use std::path::{Path, PathBuf};

/// Derives the optional file variant suffix from a model name.
///
/// Examples:
/// - `gpt-5` -> `Some("gpt")`
/// - `qwen2.5:7b` -> `Some("qwen")`
/// - `claude-3-opus` -> `Some("opus")`
pub fn model_variant_suffix(model_name: &str) -> Option<&'static str> {
    let lower = model_name.to_ascii_lowercase();
    if lower.contains("sonnet") {
        return Some("sonnet");
    }
    if lower.contains("opus") {
        return Some("opus");
    }
    if lower.contains("qwen") {
        return Some("qwen");
    }
    if lower.contains("mistral") {
        return Some("mistral");
    }
    if lower.contains("gpt")
        || lower.contains("openai")
        || lower.contains("o1")
        || lower.contains("o3")
    {
        return Some("gpt");
    }
    None
}

/// Returns candidate spec files in priority order.
///
/// Preferred candidate is `<agent>.<variant>.yml` when the model maps to a known
/// variant; fallback candidate is always `<agent>.yml`.
pub fn candidate_agent_spec_paths(
    agents_dir: &Path,
    agent_name: &str,
    model_name: &str,
) -> Vec<PathBuf> {
    candidate_agent_spec_filenames(agent_name, model_name)
        .into_iter()
        .map(|name| agents_dir.join(name))
        .collect()
}

/// Returns candidate spec filenames in priority order.
pub fn candidate_agent_spec_filenames(agent_name: &str, model_name: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some(variant) = model_variant_suffix(model_name) {
        candidates.push(format!("{}.{}.yml", agent_name, variant));
    }
    candidates.push(format!("{}.yml", agent_name));
    candidates
}

/// Resolves the first candidate path that exists on disk.
pub fn resolve_existing_agent_spec_path(
    agents_dir: &Path,
    agent_name: &str,
    model_name: &str,
) -> Option<PathBuf> {
    candidate_agent_spec_paths(agents_dir, agent_name, model_name)
        .into_iter()
        .find(|path| path.exists())
}

#[cfg(test)]
mod tests {
    use super::{candidate_agent_spec_filenames, candidate_agent_spec_paths, model_variant_suffix};
    use std::path::Path;

    #[test]
    fn model_variant_mapping_supports_requested_families() {
        assert_eq!(model_variant_suffix("gpt-5"), Some("gpt"));
        assert_eq!(model_variant_suffix("qwen2.5:7b"), Some("qwen"));
        assert_eq!(model_variant_suffix("claude-3-opus"), Some("opus"));
        assert_eq!(model_variant_suffix("claude-3-7-sonnet"), Some("sonnet"));
        assert_eq!(model_variant_suffix("mistral:7b"), Some("mistral"));
        assert_eq!(model_variant_suffix("unknown-model"), None);
    }

    #[test]
    fn candidate_order_is_variant_then_default() {
        let agents_dir = Path::new("agents");
        let paths = candidate_agent_spec_paths(agents_dir, "create_implementation", "gpt-5");
        assert_eq!(paths.len(), 2);
        assert!(paths[0].ends_with("create_implementation.gpt.yml"));
        assert!(paths[1].ends_with("create_implementation.yml"));
    }

    #[test]
    fn filename_candidates_match_expected_order() {
        let names = candidate_agent_spec_filenames("create_implementation", "claude-3-sonnet");
        assert_eq!(
            names,
            vec![
                "create_implementation.sonnet.yml",
                "create_implementation.yml"
            ]
        );
    }
}
