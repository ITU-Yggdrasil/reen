use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;

use super::pipeline_quality::{BehaviorContract, SpecificationKind, analyze_specification};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ContractRole {
    pub(crate) name: String,
    pub(crate) kind: String,
    pub(crate) required: bool,
    pub(crate) capabilities: Vec<String>,
    pub(crate) dependency_hint: Option<String>,
    pub(crate) identity_semantics: String,
    pub(crate) mutation_semantics: String,
    pub(crate) notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ContractProp {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) type_hint: Option<String>,
    pub(crate) notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ContractFunctionality {
    pub(crate) name: String,
    pub(crate) signature_hint: Option<String>,
    pub(crate) behavior_summary: Vec<String>,
    pub(crate) required_inputs: Vec<String>,
    pub(crate) required_outputs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ContractRoleMethod {
    pub(crate) role: String,
    pub(crate) method_name: String,
    pub(crate) signature_hint: Option<String>,
    pub(crate) behavior_summary: Vec<String>,
    pub(crate) required_inputs: Vec<String>,
    pub(crate) required_outputs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ContractCallEdge {
    pub(crate) caller_surface: String,
    pub(crate) callee_role: String,
    pub(crate) callee_method: String,
    pub(crate) obligation_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ContractArtifact {
    pub(crate) contract_version: String,
    pub(crate) source_spec_path: String,
    pub(crate) title: String,
    pub(crate) specification_kind: String,
    pub(crate) target_artifact_kind: String,
    pub(crate) primary_output_path_hint: Option<String>,
    pub(crate) public_functionalities: Vec<ContractFunctionality>,
    pub(crate) props: Vec<ContractProp>,
    pub(crate) roles: Vec<ContractRole>,
    pub(crate) role_methods: Vec<ContractRoleMethod>,
    pub(crate) required_call_edges: Vec<ContractCallEdge>,
    pub(crate) shared_identity_constraints: Vec<String>,
    pub(crate) mutation_constraints: Vec<String>,
    pub(crate) output_obligations: Vec<String>,
    pub(crate) env_config_obligations: Vec<String>,
    pub(crate) lifecycle_obligations: Vec<String>,
    pub(crate) allowed_freedoms: Vec<String>,
    pub(crate) verification_targets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ContractValidationReport {
    pub(crate) ok: bool,
    pub(crate) errors: Vec<String>,
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct Section {
    title: String,
    body: String,
}

pub(crate) fn build_contract_artifact(
    spec_path: &Path,
    spec_content: &str,
    output_path_hint: Option<&Path>,
    dependency_context: Option<&HashMap<String, serde_json::Value>>,
) -> ContractArtifact {
    let spec_report = analyze_specification(spec_path, spec_content, dependency_context);
    let summary = spec_report.contract;
    let sections = parse_markdown_sections(spec_content);
    let roles = extract_roles(&sections, &summary, dependency_context);
    let props = extract_props(&sections);
    let public_functionalities = extract_functionalities(&sections);
    let role_methods = extract_role_methods(&sections);
    let required_call_edges = summary
        .delegation_requirements
        .iter()
        .map(|requirement| ContractCallEdge {
            caller_surface: requirement.actor.clone(),
            callee_role: requirement.target.clone(),
            callee_method: "inferred".to_string(),
            obligation_reason: requirement.source_line.clone(),
        })
        .collect::<Vec<_>>();
    let lifecycle_obligations =
        extract_lifecycle_obligations(spec_content, &public_functionalities);
    let allowed_freedoms = extract_allowed_freedoms(&sections);
    let verification_targets = build_verification_targets(
        &summary,
        &public_functionalities,
        &role_methods,
        &required_call_edges,
    );

    ContractArtifact {
        contract_version: "reen.contract/v1".to_string(),
        source_spec_path: spec_path.display().to_string(),
        title: summary.title.clone(),
        specification_kind: specification_kind_name(summary.kind.clone()).to_string(),
        target_artifact_kind: infer_target_artifact_kind(&summary, output_path_hint),
        primary_output_path_hint: output_path_hint.map(|path| path.display().to_string()),
        public_functionalities,
        props,
        roles,
        role_methods,
        required_call_edges,
        shared_identity_constraints: summary.shared_state_requirements.clone(),
        mutation_constraints: extract_mutation_constraints(spec_content),
        output_obligations: summary
            .output_requirements
            .iter()
            .map(|requirement| requirement.literal.clone())
            .collect(),
        env_config_obligations: summary.env_vars.clone(),
        lifecycle_obligations,
        allowed_freedoms,
        verification_targets,
    }
}

pub(crate) fn validate_contract_artifact(
    contract: &ContractArtifact,
    spec_path: &Path,
    spec_content: &str,
    dependency_context: Option<&HashMap<String, serde_json::Value>>,
) -> ContractValidationReport {
    let spec_report = analyze_specification(spec_path, spec_content, dependency_context);
    let sections = parse_markdown_sections(spec_content);
    let mut errors = spec_report.errors;
    let mut warnings = spec_report.warnings;

    if has_section(&sections, "Roles") && contract.roles.is_empty() {
        errors
            .push("Contract stage could not extract any roles from the Roles section".to_string());
    }
    if has_section(&sections, "Props") && contract.props.is_empty() {
        warnings
            .push("Contract stage could not extract any props from the Props section".to_string());
    }
    if (has_section(&sections, "Functionality") || has_section(&sections, "Functionalities"))
        && contract.public_functionalities.is_empty()
    {
        errors.push(
            "Contract stage could not extract any public functionalities from the Functionality/Functionalities section"
                .to_string(),
        );
    }
    if has_section(&sections, "Role Methods") && contract.role_methods.is_empty() {
        errors.push(
            "Contract stage could not extract any role methods from the Role Methods section"
                .to_string(),
        );
    }

    let role_names = contract
        .roles
        .iter()
        .map(|role| role.name.to_ascii_lowercase())
        .collect::<Vec<_>>();
    for method in &contract.role_methods {
        if !role_names.is_empty() && !role_names.contains(&method.role.to_ascii_lowercase()) {
            warnings.push(format!(
                "Role method '{}.{}' does not map to an extracted role",
                method.role, method.method_name
            ));
        }
    }
    for edge in &contract.required_call_edges {
        if !role_names.is_empty() && !role_names.contains(&edge.callee_role.to_ascii_lowercase()) {
            warnings.push(format!(
                "Required call edge '{} -> {}' references a role that was not extracted",
                edge.caller_surface, edge.callee_role
            ));
        }
    }

    ContractValidationReport {
        ok: errors.is_empty(),
        errors,
        warnings,
    }
}

pub(crate) fn contract_artifact_to_context_value(contract: &ContractArtifact) -> serde_json::Value {
    json!(contract)
}

pub(crate) fn contract_validation_to_context_value(
    report: &ContractValidationReport,
) -> serde_json::Value {
    json!(report)
}

pub(crate) fn compact_contract_artifacts(
    artifacts: &[ContractArtifact],
    max_entries: usize,
) -> serde_json::Value {
    let compacted = artifacts
        .iter()
        .take(max_entries)
        .map(compact_contract_artifact_value)
        .collect::<Vec<_>>();
    json!(compacted)
}

pub(crate) fn compact_contract_artifact_value(contract: &ContractArtifact) -> serde_json::Value {
    json!({
        "source_spec_path": contract.source_spec_path,
        "title": contract.title,
        "specification_kind": contract.specification_kind,
        "target_artifact_kind": contract.target_artifact_kind,
        "primary_output_path_hint": contract.primary_output_path_hint,
        "roles": contract.roles.iter().map(|role| {
            json!({
                "name": role.name,
                "capabilities": role.capabilities,
                "identity_semantics": role.identity_semantics,
                "mutation_semantics": role.mutation_semantics,
            })
        }).collect::<Vec<_>>(),
        "props": contract.props.iter().map(|prop| prop.name.clone()).collect::<Vec<_>>(),
        "public_functionalities": contract.public_functionalities.iter().map(|functionality| functionality.name.clone()).collect::<Vec<_>>(),
        "role_methods": contract.role_methods.iter().map(|method| {
            json!({
                "role": method.role,
                "method_name": method.method_name,
            })
        }).collect::<Vec<_>>(),
        "required_call_edges": contract.required_call_edges,
        "shared_identity_constraints": contract.shared_identity_constraints,
        "output_obligations": contract.output_obligations,
        "env_config_obligations": contract.env_config_obligations,
        "verification_targets": contract.verification_targets,
    })
}

fn specification_kind_name(kind: SpecificationKind) -> &'static str {
    match kind {
        SpecificationKind::App => "app",
        SpecificationKind::Context => "context",
        SpecificationKind::Data => "data",
        SpecificationKind::Unknown => "unknown",
    }
}

fn infer_target_artifact_kind(
    summary: &BehaviorContract,
    output_path_hint: Option<&Path>,
) -> String {
    if let Some(path) = output_path_hint {
        let rendered = path.to_string_lossy();
        if rendered.ends_with("src/main.rs") {
            return "binary_main".to_string();
        }
        if rendered.contains("/src/contexts/") || rendered.starts_with("src/contexts/") {
            return "context_module".to_string();
        }
        if rendered.contains("/src/data/") || rendered.starts_with("src/data/") {
            return "data_module".to_string();
        }
        return "module".to_string();
    }

    match summary.kind {
        SpecificationKind::App => "binary_main".to_string(),
        SpecificationKind::Context => "context_module".to_string(),
        SpecificationKind::Data => "data_module".to_string(),
        SpecificationKind::Unknown => "module".to_string(),
    }
}

fn extract_roles(
    sections: &[Section],
    summary: &BehaviorContract,
    dependency_context: Option<&HashMap<String, serde_json::Value>>,
) -> Vec<ContractRole> {
    let mut roles = Vec::new();
    let Some(section) = find_section(sections, "Roles") else {
        for name in &summary.collaborators {
            roles.push(build_fallback_role(name, summary, dependency_context));
        }
        return roles;
    };

    let lines = section.body.lines().collect::<Vec<_>>();
    let uses_subheadings = lines.iter().any(|line| line.trim().starts_with("### "));
    if uses_subheadings {
        let mut current_name: Option<String> = None;
        let mut current_body = Vec::new();
        for line in lines {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("### ") {
                if let Some(name) = current_name.take() {
                    roles.push(build_role_from_block(
                        &name,
                        &current_body.join("\n"),
                        summary,
                        dependency_context,
                    ));
                    current_body.clear();
                }
                current_name = Some(normalize_symbol_name(rest));
            } else if current_name.is_some() {
                current_body.push(trimmed.to_string());
            }
        }
        if let Some(name) = current_name.take() {
            roles.push(build_role_from_block(
                &name,
                &current_body.join("\n"),
                summary,
                dependency_context,
            ));
        }
    } else {
        let table_row_re = Regex::new(r"^\|\s*\*\*([^*|`]+)\*\*\s*\|\s*(.+?)\s*\|$").unwrap();
        let mut pending_role: Option<String> = None;
        let mut pending_notes = Vec::new();
        for line in lines {
            let trimmed = line.trim();
            if trimmed.starts_with('|') && !trimmed.contains("---") {
                if let Some(captures) = table_row_re.captures(trimmed) {
                    let name = captures
                        .get(1)
                        .map(|matched| normalize_symbol_name(matched.as_str()))
                        .unwrap_or_default();
                    let description = captures
                        .get(2)
                        .map(|matched| strip_markdown(matched.as_str()))
                        .unwrap_or_default();
                    roles.push(build_role_from_block(
                        &name,
                        &description,
                        summary,
                        dependency_context,
                    ));
                }
                continue;
            }

            if let Some(name) = trimmed.strip_prefix("- **").and_then(|rest| {
                rest.find("**")
                    .map(|end| normalize_symbol_name(&rest[..end]))
            }) {
                if let Some(previous_name) = pending_role.take() {
                    roles.push(build_role_from_block(
                        &previous_name,
                        &pending_notes.join("\n"),
                        summary,
                        dependency_context,
                    ));
                    pending_notes.clear();
                }
                pending_role = Some(name);
            } else if pending_role.is_some() && !trimmed.is_empty() {
                pending_notes.push(strip_markdown(trimmed));
            }
        }
        if let Some(previous_name) = pending_role.take() {
            roles.push(build_role_from_block(
                &previous_name,
                &pending_notes.join("\n"),
                summary,
                dependency_context,
            ));
        }
    }

    if roles.is_empty() {
        for name in &summary.collaborators {
            roles.push(build_fallback_role(name, summary, dependency_context));
        }
    }

    roles
}

fn build_role_from_block(
    name: &str,
    block: &str,
    summary: &BehaviorContract,
    dependency_context: Option<&HashMap<String, serde_json::Value>>,
) -> ContractRole {
    let normalized_name = normalize_symbol_name(name);
    let capabilities = extract_capabilities_from_block(block);
    let dependency_hint = dependency_hint_for_name(&normalized_name, dependency_context);
    let related_shared_notes = summary
        .shared_state_requirements
        .iter()
        .filter(|line| {
            line.to_ascii_lowercase()
                .contains(&normalized_name.to_ascii_lowercase())
        })
        .cloned()
        .collect::<Vec<_>>();
    let identity_semantics = if !related_shared_notes.is_empty() {
        "shared_identity".to_string()
    } else {
        "infer_from_behavior".to_string()
    };

    ContractRole {
        name: normalized_name,
        kind: "role".to_string(),
        required: true,
        capabilities,
        dependency_hint,
        identity_semantics,
        mutation_semantics: "infer_from_behavior".to_string(),
        notes: non_empty_lines(block)
            .into_iter()
            .chain(related_shared_notes)
            .collect(),
    }
}

fn build_fallback_role(
    name: &str,
    summary: &BehaviorContract,
    dependency_context: Option<&HashMap<String, serde_json::Value>>,
) -> ContractRole {
    build_role_from_block(name, "", summary, dependency_context)
}

fn extract_props(sections: &[Section]) -> Vec<ContractProp> {
    let Some(section) = find_section(sections, "Props") else {
        return Vec::new();
    };
    let mut props = Vec::new();
    let table_row_re = Regex::new(r"^\|\s*\*\*([^*|`]+)\*\*\s*\|\s*(.+?)\s*\|$").unwrap();
    let mut pending_name: Option<String> = None;
    let mut pending_notes = Vec::new();

    for line in section.body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('|') && !trimmed.contains("---") {
            if let Some(captures) = table_row_re.captures(trimmed) {
                let name = captures
                    .get(1)
                    .map(|matched| normalize_symbol_name(matched.as_str()))
                    .unwrap_or_default();
                let description = captures
                    .get(2)
                    .map(|matched| strip_markdown(matched.as_str()))
                    .unwrap_or_default();
                props.push(ContractProp {
                    name,
                    description,
                    type_hint: None,
                    notes: Vec::new(),
                });
            }
            continue;
        }

        if let Some(name) = trimmed.strip_prefix("- **").and_then(|rest| {
            rest.find("**")
                .map(|end| normalize_symbol_name(&rest[..end]))
        }) {
            if let Some(previous_name) = pending_name.take() {
                props.push(ContractProp {
                    name: previous_name,
                    description: pending_notes.first().cloned().unwrap_or_default(),
                    type_hint: None,
                    notes: pending_notes.clone(),
                });
                pending_notes.clear();
            }
            pending_name = Some(name);
        } else if pending_name.is_some() && !trimmed.is_empty() {
            pending_notes.push(strip_markdown(trimmed));
        }
    }

    if let Some(previous_name) = pending_name.take() {
        props.push(ContractProp {
            name: previous_name,
            description: pending_notes.first().cloned().unwrap_or_default(),
            type_hint: None,
            notes: pending_notes,
        });
    }

    props
}

fn extract_functionalities(sections: &[Section]) -> Vec<ContractFunctionality> {
    let Some(section) = find_section(sections, "Functionality")
        .or_else(|| find_section(sections, "Functionalities"))
    else {
        return Vec::new();
    };

    let mut items = Vec::new();
    let table_row_re = Regex::new(r"^\|\s*\*\*?([^*|`]+)\*{0,2}\s*\|\s*(.+?)\s*\|$").unwrap();
    let mut current_name: Option<String> = None;
    let mut current_body = Vec::new();

    for line in section.body.lines() {
        let trimmed = line.trim();
        let heading_name = trimmed
            .strip_prefix("### ")
            .map(normalize_symbol_name)
            .filter(|value| !value.is_empty());
        let bullet_name = trimmed.strip_prefix("- **").and_then(|rest| {
            rest.find("**")
                .map(|end| normalize_symbol_name(&rest[..end]))
        });
        let table_name = if trimmed.starts_with('|') && !trimmed.contains("---") {
            table_row_re.captures(trimmed).and_then(|captures| {
                captures
                    .get(1)
                    .map(|matched| normalize_symbol_name(matched.as_str()))
            })
        } else {
            None
        };
        let next_name = heading_name.or(bullet_name).or(table_name);

        if let Some(name) = next_name {
            if let Some(existing) = current_name.take() {
                items.push(build_functionality(&existing, &current_body.join("\n")));
                current_body.clear();
            }
            current_name = Some(name);
        } else if current_name.is_some() && !trimmed.is_empty() {
            current_body.push(strip_markdown(trimmed));
        }
    }

    if let Some(existing) = current_name.take() {
        items.push(build_functionality(&existing, &current_body.join("\n")));
    }

    items
}

fn build_functionality(name: &str, body: &str) -> ContractFunctionality {
    ContractFunctionality {
        name: normalize_symbol_name(name),
        signature_hint: if name.contains('(') {
            Some(name.trim().trim_matches('`').to_string())
        } else {
            None
        },
        behavior_summary: non_empty_lines(body),
        required_inputs: extract_io_lines(body, "input"),
        required_outputs: extract_io_lines(body, "output"),
    }
}

fn extract_role_methods(sections: &[Section]) -> Vec<ContractRoleMethod> {
    let Some(section) = find_section(sections, "Role Methods") else {
        return Vec::new();
    };

    let mut methods = Vec::new();
    let table_row_re = Regex::new(r"^\|\s*\*\*([^*|`]+)\*\*\s*\|\s*(.+?)\s*\|$").unwrap();
    let mut current_role: Option<String> = None;
    let mut current_method_name: Option<String> = None;
    let mut current_body = Vec::new();

    let flush_method = |methods: &mut Vec<ContractRoleMethod>,
                        current_role: &Option<String>,
                        current_method_name: &mut Option<String>,
                        current_body: &mut Vec<String>| {
        if let (Some(role), Some(method_name)) = (current_role.clone(), current_method_name.take())
        {
            methods.push(ContractRoleMethod {
                role,
                method_name: normalize_symbol_name(&method_name),
                signature_hint: if method_name.contains('(') {
                    Some(method_name.trim().trim_matches('`').to_string())
                } else {
                    None
                },
                behavior_summary: non_empty_lines(&current_body.join("\n")),
                required_inputs: extract_io_lines(&current_body.join("\n"), "input"),
                required_outputs: extract_io_lines(&current_body.join("\n"), "output"),
            });
            current_body.clear();
        }
    };

    for line in section.body.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("### ") {
            flush_method(
                &mut methods,
                &current_role,
                &mut current_method_name,
                &mut current_body,
            );
            current_role = Some(normalize_symbol_name(rest));
            continue;
        }

        let next_method = trimmed
            .strip_prefix("- **")
            .and_then(|rest| rest.find("**").map(|end| rest[..end].trim().to_string()))
            .or_else(|| {
                if trimmed.starts_with('|') && !trimmed.contains("---") {
                    table_row_re.captures(trimmed).and_then(|captures| {
                        captures
                            .get(1)
                            .map(|matched| matched.as_str().trim().to_string())
                    })
                } else {
                    None
                }
            });

        if let Some(method_name) = next_method {
            flush_method(
                &mut methods,
                &current_role,
                &mut current_method_name,
                &mut current_body,
            );
            current_method_name = Some(method_name);
        } else if current_method_name.is_some() && !trimmed.is_empty() {
            current_body.push(strip_markdown(trimmed));
        }
    }

    flush_method(
        &mut methods,
        &current_role,
        &mut current_method_name,
        &mut current_body,
    );
    methods
}

fn extract_lifecycle_obligations(
    spec_content: &str,
    functionalities: &[ContractFunctionality],
) -> Vec<String> {
    let mut obligations = Vec::new();
    for functionality in functionalities {
        let name = functionality.name.to_ascii_lowercase();
        if matches!(
            name.as_str(),
            "new" | "run" | "main" | "init" | "initialize"
        ) {
            obligations.push(format!("functionality {}", functionality.name));
        }
    }
    for line in spec_content.lines() {
        let lowered = line.to_ascii_lowercase();
        if lowered.contains("initialize")
            || lowered.contains("cleanup")
            || lowered.contains("restore")
            || lowered.contains("teardown")
        {
            obligations.push(strip_markdown(line.trim()));
        }
    }
    dedupe_preserve(&mut obligations);
    obligations
}

fn extract_allowed_freedoms(sections: &[Section]) -> Vec<String> {
    let Some(section) = find_section(sections, "Implementation Choices Left Open") else {
        return Vec::new();
    };
    non_empty_lines(&section.body)
}

fn build_verification_targets(
    summary: &BehaviorContract,
    functionalities: &[ContractFunctionality],
    role_methods: &[ContractRoleMethod],
    call_edges: &[ContractCallEdge],
) -> Vec<String> {
    let mut targets = Vec::new();
    targets.extend(
        functionalities
            .iter()
            .map(|functionality| format!("functionality: {}", functionality.name)),
    );
    targets.extend(
        role_methods
            .iter()
            .map(|method| format!("role_method: {}.{}", method.role, method.method_name)),
    );
    targets.extend(
        call_edges
            .iter()
            .map(|edge| format!("call_edge: {} -> {}", edge.caller_surface, edge.callee_role)),
    );
    targets.extend(
        summary
            .output_requirements
            .iter()
            .map(|requirement| format!("output: {}", requirement.literal)),
    );
    targets.extend(summary.env_vars.iter().map(|name| format!("env: {}", name)));
    targets.extend(
        summary
            .shared_state_requirements
            .iter()
            .map(|value| format!("shared_identity: {}", strip_markdown(value))),
    );
    dedupe_preserve(&mut targets);
    targets
}

fn extract_mutation_constraints(content: &str) -> Vec<String> {
    let mut constraints = Vec::new();
    for line in content.lines() {
        let lowered = line.to_ascii_lowercase();
        if lowered.contains("mutable")
            || lowered.contains("mutability")
            || lowered.contains("ownership")
            || lowered.contains("without resetting or replacing")
            || lowered.contains("same shared")
            || lowered.contains("one shared")
        {
            constraints.push(strip_markdown(line.trim()));
        }
    }
    dedupe_preserve(&mut constraints);
    constraints
}

fn dependency_hint_for_name(
    name: &str,
    dependency_context: Option<&HashMap<String, serde_json::Value>>,
) -> Option<String> {
    let Some(context) = dependency_context else {
        return None;
    };
    for key in [
        "direct_dependencies",
        "dependency_closure",
        "implemented_dependencies",
        "implemented_direct_dependencies",
    ] {
        if let Some(entries) = context.get(key).and_then(|value| value.as_array()) {
            for entry in entries {
                let entry_name = entry
                    .get("name")
                    .and_then(|value| value.as_str())
                    .unwrap_or("");
                if normalize_symbol_name(entry_name).eq_ignore_ascii_case(name) {
                    return entry
                        .get("path")
                        .or_else(|| entry.get("spec_path"))
                        .and_then(|value| value.as_str())
                        .map(ToOwned::to_owned);
                }
            }
        }
    }
    None
}

fn extract_capabilities_from_block(block: &str) -> Vec<String> {
    let mut capabilities = Vec::new();
    for line in block.lines() {
        let trimmed = strip_markdown(line.trim());
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.to_ascii_lowercase().starts_with("purpose") {
            continue;
        }
        capabilities.push(trimmed);
    }
    dedupe_preserve(&mut capabilities);
    capabilities
}

fn extract_io_lines(body: &str, label: &str) -> Vec<String> {
    body.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let lowered = trimmed.to_ascii_lowercase();
            if lowered.contains(label) {
                Some(strip_markdown(trimmed))
            } else {
                None
            }
        })
        .collect()
}

fn parse_markdown_sections(content: &str) -> Vec<Section> {
    let mut sections = Vec::new();
    let mut current_title: Option<String> = None;
    let mut current_body = String::new();

    for line in content.lines() {
        if let Some(title) = line.trim().strip_prefix("## ") {
            if let Some(existing) = current_title.take() {
                sections.push(Section {
                    title: existing,
                    body: current_body.trim().to_string(),
                });
                current_body.clear();
            }
            current_title = Some(title.trim().to_string());
            continue;
        }

        if current_title.is_some() {
            current_body.push_str(line);
            current_body.push('\n');
        }
    }

    if let Some(existing) = current_title {
        sections.push(Section {
            title: existing,
            body: current_body.trim().to_string(),
        });
    }

    sections
}

fn find_section<'a>(sections: &'a [Section], title: &str) -> Option<&'a Section> {
    let expected = normalize_section_title(title);
    sections
        .iter()
        .find(|section| normalize_section_title(&section.title) == expected)
}

fn has_section(sections: &[Section], title: &str) -> bool {
    find_section(sections, title).is_some()
}

fn normalize_section_title(title: &str) -> String {
    let trimmed = title.trim().trim_end_matches(':').trim();
    let normalized = if trimmed
        .chars()
        .next()
        .map(|ch| ch.is_ascii_digit())
        .unwrap_or(false)
    {
        trimmed.trim_start_matches(|ch: char| {
            ch.is_ascii_digit() || matches!(ch, '.' | ')' | ':' | '-' | ' ')
        })
    } else {
        trimmed
    };
    normalized.to_ascii_lowercase()
}

fn normalize_symbol_name(value: &str) -> String {
    let trimmed = value
        .trim()
        .trim_matches('`')
        .trim_matches('*')
        .trim_matches('|');
    if trimmed.is_empty() {
        return String::new();
    }
    trimmed
        .split_whitespace()
        .next()
        .unwrap_or(trimmed)
        .trim_matches(|c: char| matches!(c, '(' | ')' | ',' | '.' | ':'))
        .to_string()
}

fn strip_markdown(value: &str) -> String {
    value
        .trim()
        .trim_matches('-')
        .trim()
        .trim_matches('`')
        .trim_matches('*')
        .trim()
        .to_string()
}

fn non_empty_lines(value: &str) -> Vec<String> {
    value
        .lines()
        .map(|line| strip_markdown(line.trim()))
        .filter(|line| !line.is_empty())
        .collect()
}

fn dedupe_preserve(values: &mut Vec<String>) {
    let mut seen = HashMap::new();
    values.retain(|value| seen.insert(value.clone(), ()).is_none());
}

#[cfg(test)]
mod tests {
    use super::{build_contract_artifact, validate_contract_artifact};
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    #[test]
    fn builds_context_contract_with_roles_props_and_methods() {
        let content = r#"# CommandInputContext

## Roles
- **stdin_source**
  Provides non-blocking reads from standard input.

## Props
- **buffer**
  FIFO queue of captured keystrokes.

## Role Methods
### stdin_source
- **read_available**
  Returns all currently available keystrokes in arrival order without blocking.

## Functionalities
- **new()**
  Starts with an empty input buffer.
- **capture()**
  Reads currently available key presses.
"#;

        let contract = build_contract_artifact(
            Path::new("specifications/contexts/command_input.md"),
            content,
            Some(Path::new("src/contexts/command_input.rs")),
            None,
        );

        assert_eq!(contract.roles.len(), 1);
        assert_eq!(contract.roles[0].name, "stdin_source");
        assert_eq!(contract.props.len(), 1);
        assert_eq!(contract.props[0].name, "buffer");
        assert_eq!(contract.role_methods.len(), 1);
        assert_eq!(contract.role_methods[0].method_name, "read_available");
        assert_eq!(contract.public_functionalities.len(), 2);
    }

    #[test]
    fn contract_validation_respects_numbered_sections() {
        let content = r#"# GameLoopContext

## 2. Roles
| Role | Description |
|------|-------------|
| **command** | Provides player input. |

## 3. Props:
| Prop | Description |
|------|-------------|
| **board** | Board dimensions. |

## 4. Role methods:
### command
| Method | Description |
|--------|-------------|
| **next** | Returns the next direction. |

## 5. Functionalities
### `tick()`
- Reads the next movement direction.
"#;

        let contract = build_contract_artifact(
            Path::new("specifications/contexts/game_loop.md"),
            content,
            Some(PathBuf::from("src/contexts/game_loop.rs").as_path()),
            Some(&HashMap::new()),
        );
        let report = validate_contract_artifact(
            &contract,
            Path::new("specifications/contexts/game_loop.md"),
            content,
            Some(&HashMap::new()),
        );
        assert!(report.ok);
        assert_eq!(contract.roles[0].name, "command");
        assert_eq!(contract.role_methods[0].method_name, "next");
        assert_eq!(contract.public_functionalities[0].name, "tick");
    }
}
