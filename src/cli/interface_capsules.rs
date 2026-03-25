use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;

use super::contracts::ContractArtifact;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct CapsuleSnippet {
    pub(crate) label: String,
    pub(crate) content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct InterfaceCapsule {
    pub(crate) name: String,
    pub(crate) spec_path: String,
    pub(crate) source_path: Option<String>,
    pub(crate) artifact_kind: String,
    pub(crate) public_types: Vec<String>,
    pub(crate) public_methods: Vec<String>,
    pub(crate) relevant_role_methods: Vec<String>,
    pub(crate) important_fields: Vec<String>,
    pub(crate) ownership_notes: Vec<String>,
    pub(crate) sharing_notes: Vec<String>,
    pub(crate) call_edge_exports: Vec<String>,
    pub(crate) verification_notes: Vec<String>,
    pub(crate) selected_snippets: Vec<CapsuleSnippet>,
}

pub(crate) fn build_interface_capsule(
    contract: &ContractArtifact,
    source_path: Option<&Path>,
    source_content: Option<&str>,
) -> InterfaceCapsule {
    let public_types = source_content
        .map(extract_public_types)
        .unwrap_or_else(|| infer_public_types_from_contract(contract));
    let public_methods = source_content
        .map(extract_public_methods)
        .unwrap_or_else(|| {
            contract
                .public_functionalities
                .iter()
                .map(|item| item.name.clone())
                .collect()
        });
    let relevant_role_methods = contract
        .role_methods
        .iter()
        .map(|item| format!("{}.{}", item.role, item.method_name))
        .collect::<Vec<_>>();
    let important_fields = contract
        .props
        .iter()
        .map(|prop| prop.name.clone())
        .collect::<Vec<_>>();
    let ownership_notes = contract
        .roles
        .iter()
        .map(|role| format!("{}: {}", role.name, role.mutation_semantics))
        .collect::<Vec<_>>();
    let sharing_notes = if contract.shared_identity_constraints.is_empty() {
        contract
            .roles
            .iter()
            .filter(|role| role.identity_semantics != "infer_from_behavior")
            .map(|role| format!("{}: {}", role.name, role.identity_semantics))
            .collect::<Vec<_>>()
    } else {
        contract.shared_identity_constraints.clone()
    };
    let call_edge_exports = contract
        .required_call_edges
        .iter()
        .map(|edge| format!("{} -> {}.{}", edge.caller_surface, edge.callee_role, edge.callee_method))
        .collect::<Vec<_>>();
    let verification_notes = contract.verification_targets.clone();
    let selected_snippets = source_content
        .map(|content| select_snippets(contract, content))
        .unwrap_or_default();

    InterfaceCapsule {
        name: contract.title.clone(),
        spec_path: contract.source_spec_path.clone(),
        source_path: source_path.map(|path| path.display().to_string()),
        artifact_kind: contract.target_artifact_kind.clone(),
        public_types,
        public_methods,
        relevant_role_methods,
        important_fields,
        ownership_notes,
        sharing_notes,
        call_edge_exports,
        verification_notes,
        selected_snippets,
    }
}

pub(crate) fn compact_interface_capsules(
    capsules: &[InterfaceCapsule],
    max_entries: usize,
) -> serde_json::Value {
    let compacted = capsules
        .iter()
        .take(max_entries)
        .map(compact_interface_capsule_value)
        .collect::<Vec<_>>();
    json!(compacted)
}

pub(crate) fn compact_interface_capsule_value(capsule: &InterfaceCapsule) -> serde_json::Value {
    json!({
        "name": capsule.name,
        "spec_path": capsule.spec_path,
        "source_path": capsule.source_path,
        "artifact_kind": capsule.artifact_kind,
        "public_types": capsule.public_types,
        "public_methods": capsule.public_methods,
        "relevant_role_methods": capsule.relevant_role_methods,
        "important_fields": capsule.important_fields,
        "ownership_notes": capsule.ownership_notes,
        "sharing_notes": capsule.sharing_notes,
        "call_edge_exports": capsule.call_edge_exports,
        "verification_notes": capsule.verification_notes,
        "selected_snippets": capsule
            .selected_snippets
            .iter()
            .take(4)
            .map(|snippet| {
                json!({
                    "label": snippet.label,
                    "content": truncate_snippet(&snippet.content, 600),
                })
            })
            .collect::<Vec<_>>(),
    })
}

fn infer_public_types_from_contract(contract: &ContractArtifact) -> Vec<String> {
    if contract.title.is_empty() {
        Vec::new()
    } else {
        vec![contract.title.clone()]
    }
}

fn extract_public_types(content: &str) -> Vec<String> {
    let re = Regex::new(r"\bpub\s+(?:struct|enum|trait|type)\s+([A-Za-z0-9_]+)").unwrap();
    re.captures_iter(content)
        .filter_map(|captures| captures.get(1).map(|matched| matched.as_str().to_string()))
        .collect()
}

fn extract_public_methods(content: &str) -> Vec<String> {
    let re = Regex::new(r"\bpub\s+fn\s+([A-Za-z0-9_]+)").unwrap();
    re.captures_iter(content)
        .filter_map(|captures| captures.get(1).map(|matched| matched.as_str().to_string()))
        .collect()
}

fn select_snippets(contract: &ContractArtifact, content: &str) -> Vec<CapsuleSnippet> {
    let mut names = contract
        .public_functionalities
        .iter()
        .map(|item| item.name.clone())
        .collect::<Vec<_>>();
    names.extend(contract.role_methods.iter().map(|method| method.method_name.clone()));
    names.extend(contract.props.iter().map(|prop| prop.name.clone()));
    dedupe_preserve(&mut names);

    let mut snippets = Vec::new();
    for name in names {
        if let Some(content) = extract_named_block(content, &name) {
            snippets.push(CapsuleSnippet {
                label: name,
                content: truncate_snippet(&content, 1200),
            });
        }
    }

    if snippets.is_empty() {
        let first = content.lines().take(40).collect::<Vec<_>>().join("\n");
        if !first.trim().is_empty() {
            snippets.push(CapsuleSnippet {
                label: "module_head".to_string(),
                content: first,
            });
        }
    }

    snippets
}

fn extract_named_block(content: &str, name: &str) -> Option<String> {
    let escaped = regex::escape(name);
    let patterns = [
        format!(r"(?s)\bpub\s+fn\s+{escaped}\b[^\{{]*\{{.*?\n\}}"),
        format!(r"(?s)\bfn\s+[A-Za-z0-9_]*{escaped}[A-Za-z0-9_]*\b[^\{{]*\{{.*?\n\}}"),
        format!(r"(?s)\bpub\s+(?:struct|enum)\s+{escaped}\b.*?\n\}}"),
    ];

    for pattern in patterns {
        if let Ok(re) = Regex::new(&pattern) {
            if let Some(matched) = re.find(content) {
                return Some(matched.as_str().trim().to_string());
            }
        }
    }

    let line_index = content
        .lines()
        .enumerate()
        .find_map(|(idx, line)| line.contains(name).then_some(idx));
    let Some(index) = line_index else {
        return None;
    };
    let start = index.saturating_sub(4);
    let end = (index + 12).min(content.lines().count());
    let snippet = content
        .lines()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect::<Vec<_>>()
        .join("\n");
    Some(snippet)
}

fn truncate_snippet(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    format!(
        "{}\n...[truncated by reen]",
        value.chars().take(max_chars).collect::<String>()
    )
}

fn dedupe_preserve(values: &mut Vec<String>) {
    let mut seen = std::collections::HashMap::new();
    values.retain(|value| seen.insert(value.clone(), ()).is_none());
}

#[cfg(test)]
mod tests {
    use super::build_interface_capsule;
    use crate::cli::contracts::build_contract_artifact;
    use std::path::Path;

    #[test]
    fn builds_capsule_from_contract_and_source() {
        let spec = r#"# CommandInputContext

## Roles
- **stdin_source**
  Provides input.

## Props
- **buffer**
  Queue of chars.

## Role Methods
### stdin_source
- **read_available**
  Returns chars.

## Functionalities
- **capture()**
  Captures chars.
"#;
        let source = r#"
pub struct CommandInputContext {
    buffer: Vec<char>,
}

impl CommandInputContext {
    pub fn capture(&mut self) {}

    fn stdin_source_read_available(&self) -> Vec<char> {
        Vec::new()
    }
}
"#;
        let contract = build_contract_artifact(
            Path::new("specifications/contexts/command_input.md"),
            spec,
            Some(Path::new("src/contexts/command_input.rs")),
            None,
        );
        let capsule = build_interface_capsule(
            &contract,
            Some(Path::new("src/contexts/command_input.rs")),
            Some(source),
        );
        assert!(capsule.public_types.contains(&"CommandInputContext".to_string()));
        assert!(capsule.public_methods.contains(&"capture".to_string()));
        assert!(capsule
            .relevant_role_methods
            .contains(&"stdin_source.read_available".to_string()));
        assert!(!capsule.selected_snippets.is_empty());
    }
}
