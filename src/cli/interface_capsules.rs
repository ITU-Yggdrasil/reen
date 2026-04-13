use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;

use super::contract_store::{
    InterfaceMethod, InterfaceParameter, InterfaceType, NameBinding, ResolvedInterface,
};
use super::contracts::ContractArtifact;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct CapsuleSnippet {
    pub(crate) label: String,
    pub(crate) content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct InterfaceCapsule {
    pub(crate) version: String,
    pub(crate) name: String,
    pub(crate) semantic_name: String,
    pub(crate) export_name: String,
    pub(crate) spec_path: String,
    pub(crate) source_path: Option<String>,
    pub(crate) artifact_kind: String,
    pub(crate) interface_fingerprint: String,
    pub(crate) public_types: Vec<String>,
    pub(crate) public_methods: Vec<String>,
    pub(crate) exported_types: Vec<InterfaceType>,
    pub(crate) exported_methods: Vec<InterfaceMethod>,
    pub(crate) name_bindings: Vec<NameBinding>,
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
    resolved_export: Option<&ResolvedInterface>,
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
    let exported_types = source_content
        .map(extract_exported_types)
        .unwrap_or_else(|| infer_exported_types_from_contract(contract));
    let exported_methods = source_content
        .map(extract_exported_methods)
        .unwrap_or_else(|| infer_exported_methods_from_contract(contract));
    let name_bindings = build_name_bindings(contract, &exported_types, &exported_methods);
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
        .map(|edge| {
            format!(
                "{} -> {}.{}",
                edge.caller_surface, edge.callee_role, edge.callee_method
            )
        })
        .collect::<Vec<_>>();
    let verification_notes = contract.verification_targets.clone();
    let selected_snippets = source_content
        .map(|content| select_snippets(contract, content))
        .unwrap_or_default();
    let mut interface_fingerprint =
        compute_interface_fingerprint(&exported_types, &exported_methods, &name_bindings);
    let mut exported_types = exported_types;
    let mut exported_methods = exported_methods;
    let mut name_bindings = name_bindings;
    if let Some(r) = resolved_export {
        let inferred_methods = exported_methods.clone();
        interface_fingerprint = r.interface_fingerprint.clone();
        exported_types = r.exported_types.clone();
        let merged = r
            .exported_methods
            .iter()
            .chain(r.role_method_exports.iter())
            .cloned()
            .collect::<Vec<_>>();
        exported_methods = if merged.is_empty() {
            inferred_methods
        } else {
            merged
        };
        name_bindings = r.name_bindings.clone();
    }
    let export_name = exported_types
        .first()
        .map(|item| item.export_name.clone())
        .unwrap_or_else(|| contract.title.clone());

    InterfaceCapsule {
        version: "reen.interface/v2".to_string(),
        name: contract.title.clone(),
        semantic_name: contract.title.clone(),
        export_name,
        spec_path: contract.source_spec_path.clone(),
        source_path: source_path.map(|path| path.display().to_string()),
        artifact_kind: contract.target_artifact_kind.clone(),
        interface_fingerprint,
        public_types,
        public_methods,
        exported_types,
        exported_methods,
        name_bindings,
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
        "version": capsule.version,
        "name": capsule.name,
        "semantic_name": capsule.semantic_name,
        "export_name": capsule.export_name,
        "spec_path": capsule.spec_path,
        "source_path": capsule.source_path,
        "artifact_kind": capsule.artifact_kind,
        "interface_fingerprint": capsule.interface_fingerprint,
        "public_types": capsule.public_types,
        "public_methods": capsule.public_methods,
        "exported_types": capsule.exported_types,
        "exported_methods": capsule.exported_methods,
        "name_bindings": capsule.name_bindings,
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

fn extract_exported_types(content: &str) -> Vec<InterfaceType> {
    let re = Regex::new(r"\bpub\s+(struct|enum|trait|type)\s+([A-Za-z0-9_]+)").unwrap();
    re.captures_iter(content)
        .filter_map(|captures| {
            let kind = captures.get(1)?.as_str().to_string();
            let rust_name = captures.get(2)?.as_str().to_string();
            let export_name = semantic_name_for_identifier(&rust_name);
            Some(InterfaceType {
                semantic_name: export_name.clone(),
                rust_name,
                export_name,
                kind,
                fields: Vec::new(),
            })
        })
        .collect()
}

fn infer_exported_types_from_contract(contract: &ContractArtifact) -> Vec<InterfaceType> {
    infer_public_types_from_contract(contract)
        .into_iter()
        .map(|name| InterfaceType {
            semantic_name: name.clone(),
            rust_name: safe_rust_identifier(&name),
            export_name: name,
            kind: "struct".to_string(),
            fields: Vec::new(),
        })
        .collect()
}

fn extract_exported_methods(content: &str) -> Vec<InterfaceMethod> {
    let mut methods = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(method) = parse_public_method_signature(trimmed) {
            methods.push(method);
        }
    }
    methods
}

fn infer_exported_methods_from_contract(contract: &ContractArtifact) -> Vec<InterfaceMethod> {
    contract
        .public_functionalities
        .iter()
        .map(|item| {
            let rust_name = safe_rust_identifier(&item.name);
            InterfaceMethod {
                semantic_name: item.name.clone(),
                rust_name: rust_name.clone(),
                export_name: item.name.clone(),
                receiver: if item.name == "new" {
                    "associated".to_string()
                } else {
                    "&self".to_string()
                },
                parameters: Vec::new(),
                return_type: if item.name == "new" {
                    "Self".to_string()
                } else if item
                    .behavior_summary
                    .iter()
                    .any(|line| line.to_ascii_lowercase().contains("text frame"))
                {
                    "String".to_string()
                } else {
                    "unknown".to_string()
                },
                failure_shape: "plain".to_string(),
                signature: if item.name == "new" {
                    format!("pub fn {rust_name}(...) -> Self")
                } else {
                    format!("pub fn {rust_name}(...) -> unknown")
                },
            }
        })
        .collect()
}

fn build_name_bindings(
    contract: &ContractArtifact,
    exported_types: &[InterfaceType],
    exported_methods: &[InterfaceMethod],
) -> Vec<NameBinding> {
    let mut bindings = Vec::new();
    for name in infer_public_types_from_contract(contract) {
        let rust_identifier = exported_types
            .iter()
            .find(|item| item.export_name == name)
            .map(|item| item.rust_name.clone())
            .unwrap_or_else(|| safe_rust_identifier(&name));
        bindings.push(NameBinding {
            semantic_name: name.clone(),
            rust_identifier: rust_identifier.clone(),
            export_name: semantic_name_for_identifier(&rust_identifier),
            reason: binding_reason(&name, &rust_identifier),
        });
    }
    for item in exported_methods {
        bindings.push(NameBinding {
            semantic_name: item.semantic_name.clone(),
            rust_identifier: item.rust_name.clone(),
            export_name: item.export_name.clone(),
            reason: binding_reason(&item.semantic_name, &item.rust_name),
        });
    }
    dedupe_name_bindings(&mut bindings);
    bindings
}

fn parse_public_method_signature(line: &str) -> Option<InterfaceMethod> {
    let trimmed = line.trim();
    let re = Regex::new(r"^pub\s+fn\s+([A-Za-z0-9_#]+)\s*\(([^)]*)\)\s*(?:->\s*([^{]+))?").ok()?;
    let captures = re.captures(trimmed)?;
    let rust_name = captures.get(1)?.as_str().trim().to_string();
    let params_raw = captures
        .get(2)
        .map(|value| value.as_str())
        .unwrap_or("")
        .trim();
    let return_type = captures
        .get(3)
        .map(|value| {
            value
                .as_str()
                .trim()
                .trim_end_matches('{')
                .trim()
                .to_string()
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "()".to_string());
    let mut parameters = Vec::new();
    let mut receiver = "associated".to_string();
    if !params_raw.is_empty() {
        for (index, param) in params_raw.split(',').map(str::trim).enumerate() {
            if index == 0 && matches!(param, "&self" | "&mut self" | "self" | "mut self") {
                receiver = param.to_string();
                continue;
            }
            let (raw_name, type_ref) = param
                .split_once(':')
                .map(|(name, ty)| (name.trim(), ty.trim()))
                .unwrap_or((param, "unknown"));
            if raw_name.is_empty() {
                continue;
            }
            parameters.push(InterfaceParameter {
                semantic_name: semantic_name_for_identifier(raw_name),
                rust_name: raw_name.to_string(),
                type_ref: type_ref.to_string(),
            });
        }
        if receiver == "associated" && !parameters.is_empty() {
            receiver = "value".to_string();
        }
    }

    let semantic_name = semantic_name_for_identifier(&rust_name);
    let failure_shape = if is_result_like_type_expr(&return_type) {
        "result".to_string()
    } else if is_option_like_type_expr(&return_type) {
        "option".to_string()
    } else {
        "plain".to_string()
    };

    Some(InterfaceMethod {
        semantic_name: semantic_name.clone(),
        rust_name,
        export_name: semantic_name,
        receiver,
        parameters,
        return_type: return_type.clone(),
        failure_shape,
        signature: trimmed.trim_end_matches('{').trim().to_string(),
    })
}

fn compute_interface_fingerprint(
    exported_types: &[InterfaceType],
    exported_methods: &[InterfaceMethod],
    name_bindings: &[NameBinding],
) -> String {
    let value = json!({
        "exported_types": exported_types,
        "exported_methods": exported_methods,
        "name_bindings": name_bindings,
    });
    let encoded = serde_json::to_string(&value).unwrap_or_default();
    let mut hasher = sha2::Sha256::new();
    use sha2::Digest;
    hasher.update(encoded.as_bytes());
    hex::encode(hasher.finalize())
}

fn semantic_name_for_identifier(identifier: &str) -> String {
    identifier.trim_start_matches("r#").to_string()
}

fn is_result_like_type_expr(type_ref: &str) -> bool {
    matches!(
        type_ref.trim(),
        value if value.starts_with("Result<")
            || value.starts_with("core::result::Result<")
            || value.starts_with("std::result::Result<")
            || value.starts_with("anyhow::Result<")
    )
}

fn is_option_like_type_expr(type_ref: &str) -> bool {
    matches!(
        type_ref.trim(),
        value
            if value.starts_with("Option<")
                || value.starts_with("core::option::Option<")
                || value.starts_with("std::option::Option<")
    )
}

fn safe_rust_identifier(name: &str) -> String {
    if rust_keyword_names().contains(&name) {
        format!("r#{name}")
    } else {
        name.to_string()
    }
}

fn binding_reason(semantic_name: &str, rust_identifier: &str) -> String {
    if semantic_name == rust_identifier {
        "identity".to_string()
    } else if rust_identifier.starts_with("r#") {
        "keyword_escape".to_string()
    } else {
        "normalized".to_string()
    }
}

fn dedupe_name_bindings(values: &mut Vec<NameBinding>) {
    let mut seen = std::collections::HashMap::new();
    values.retain(|value| {
        seen.insert(
            (
                value.semantic_name.clone(),
                value.rust_identifier.clone(),
                value.export_name.clone(),
            ),
            (),
        )
        .is_none()
    });
}

fn rust_keyword_names() -> &'static std::collections::HashSet<&'static str> {
    use std::collections::HashSet;
    use std::sync::OnceLock;

    static KEYWORDS: OnceLock<HashSet<&'static str>> = OnceLock::new();
    KEYWORDS.get_or_init(|| {
        [
            "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn",
            "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
            "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
            "unsafe", "use", "where", "while", "async", "await", "dyn",
        ]
        .into_iter()
        .collect()
    })
}

fn select_snippets(contract: &ContractArtifact, content: &str) -> Vec<CapsuleSnippet> {
    let mut names = contract
        .public_functionalities
        .iter()
        .map(|item| item.name.clone())
        .collect::<Vec<_>>();
    names.extend(
        contract
            .role_methods
            .iter()
            .map(|method| method.method_name.clone()),
    );
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
    use super::{build_interface_capsule, parse_public_method_signature};
    use crate::cli::contracts::build_contract_artifact;
    use std::path::Path;

    #[test]
    fn builds_capsule_from_contract_and_source() {
        let spec = r#"# CommandInputContext

## Purpose
Used for one shared input stream across the whole application session.

## Role Players
| Role Player | Why Involved | Expected Behaviour |
|---|---|---|
| stdin_source | Supplies keyboard input to the context | Provides input. |

## Props
| Prop | Meaning | Notes |
|---|---|---|
| buffer | Queue of chars. | Shared for the whole application session. |

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
            None,
        );
        assert!(
            capsule
                .public_types
                .contains(&"CommandInputContext".to_string())
        );
        assert!(capsule.public_methods.contains(&"capture".to_string()));
        assert!(
            capsule
                .relevant_role_methods
                .contains(&"stdin_source.read_available".to_string())
        );
        assert!(!capsule.selected_snippets.is_empty());
    }

    #[test]
    fn parses_anyhow_result_methods_as_result_failure_shape() {
        let method = parse_public_method_signature(
            "pub fn new(width: u32) -> anyhow::Result<Board> { todo!() }",
        )
        .expect("parse public fn");

        assert_eq!(method.return_type, "anyhow::Result<Board>");
        assert_eq!(method.failure_shape, "result");
    }
}
