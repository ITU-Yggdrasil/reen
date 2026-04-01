use anyhow::Result;
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;

use super::agent_executor::AgentExecutor;
use super::contracts::{
    ContractArtifact, compact_contract_artifact_value, compact_contract_artifacts,
};
use super::interface_capsules::{InterfaceCapsule, compact_interface_capsules};
use super::openapi_fetcher::{
    extract_external_api_symbol_inventory, is_external_api_draft_path, load_openapi_content,
    parse_external_api_draft,
};
use super::stage_runner::estimate_agent_request_tokens;
fn compact_dependency_entries(
    value: &serde_json::Value,
    content_limit: usize,
    max_entries: usize,
) -> serde_json::Value {
    let Some(entries) = value.as_array() else {
        return value.clone();
    };
    let compacted = entries
        .iter()
        .take(max_entries)
        .map(|entry| {
            let Some(obj) = entry.as_object() else {
                return entry.clone();
            };
            let mut out = serde_json::Map::new();
            for key in ["name", "path", "spec_path", "sha256"] {
                if let Some(v) = obj.get(key) {
                    out.insert(key.to_string(), v.clone());
                }
            }
            if let Some(content) = obj.get("content").and_then(|v| v.as_str()) {
                let truncated = if content.chars().count() > content_limit {
                    format!(
                        "{}\n...[truncated by reen]",
                        content.chars().take(content_limit).collect::<String>()
                    )
                } else {
                    content.to_string()
                };
                out.insert("content".to_string(), serde_json::Value::String(truncated));
            }
            serde_json::Value::Object(out)
        })
        .collect::<Vec<_>>();
    serde_json::Value::Array(compacted)
}

fn compact_dependency_manifest_entries(value: &serde_json::Value) -> serde_json::Value {
    let Some(entries) = value.as_array() else {
        return value.clone();
    };
    let compacted = entries
        .iter()
        .map(|entry| {
            let Some(obj) = entry.as_object() else {
                return entry.clone();
            };
            let mut out = serde_json::Map::new();
            for key in [
                "name",
                "path",
                "spec_path",
                "artifact_type",
                "dependency_kind",
            ] {
                if let Some(v) = obj.get(key) {
                    out.insert(key.to_string(), v.clone());
                }
            }
            serde_json::Value::Object(out)
        })
        .collect::<Vec<_>>();
    serde_json::Value::Array(compacted)
}

fn compact_contract_entries(value: &serde_json::Value, max_entries: usize) -> serde_json::Value {
    let Some(entries) = value.as_array() else {
        return value.clone();
    };
    let parsed = entries
        .iter()
        .filter_map(|entry| serde_json::from_value::<ContractArtifact>(entry.clone()).ok())
        .collect::<Vec<_>>();
    compact_contract_artifacts(&parsed, max_entries)
}

fn compact_capsule_entries(value: &serde_json::Value, max_entries: usize) -> serde_json::Value {
    let Some(entries) = value.as_array() else {
        return value.clone();
    };
    let parsed = entries
        .iter()
        .filter_map(|entry| serde_json::from_value::<InterfaceCapsule>(entry.clone()).ok())
        .collect::<Vec<_>>();
    compact_interface_capsules(&parsed, max_entries)
}

fn compact_single_contract_value(value: &serde_json::Value) -> serde_json::Value {
    serde_json::from_value::<ContractArtifact>(value.clone())
        .map(|artifact| compact_contract_artifact_value(&artifact))
        .unwrap_or_else(|_| value.clone())
}

fn build_context_variants(
    base_context: &HashMap<String, serde_json::Value>,
) -> Vec<HashMap<String, serde_json::Value>> {
    let mut variants = vec![base_context.clone()];

    if base_context.contains_key("implemented_dependencies") {
        let mut without_impl = base_context.clone();
        without_impl.remove("implemented_dependencies");
        variants.push(without_impl);
    }

    if let Some(openapi_content) = base_context.get("openapi_content").and_then(|v| v.as_str()) {
        let mut compact_openapi = base_context.clone();
        let truncated = if openapi_content.chars().count() > 12000 {
            format!(
                "{}\n...[truncated OpenAPI by reen]",
                openapi_content.chars().take(12000).collect::<String>()
            )
        } else {
            openapi_content.to_string()
        };
        compact_openapi.insert("openapi_content".to_string(), json!(truncated));
        variants.push(compact_openapi);
    }

    let manifest_sources = [
        "direct_dependencies",
        "dependency_closure",
        "mcp_context",
        "implemented_dependencies",
        "implemented_direct_dependencies",
    ];
    let mut compact_manifest = base_context.clone();
    let mut compact_manifest_changed = false;
    for key in manifest_sources {
        if let Some(value) = compact_manifest.get(key).cloned() {
            let reduced = compact_dependency_manifest_entries(&value);
            if reduced != value {
                compact_manifest.insert(key.to_string(), reduced);
                compact_manifest_changed = true;
            }
        }
    }
    if compact_manifest_changed {
        variants.push(compact_manifest.clone());
        if compact_manifest.contains_key("implemented_dependencies") {
            let mut no_impl = compact_manifest;
            no_impl.remove("implemented_dependencies");
            variants.push(no_impl);
        }
    }

    let mut contract_compact = base_context.clone();
    let mut contract_changed = false;
    for (key, max_entries) in [
        ("dependency_contracts", 8usize),
        ("direct_dependency_contracts", 8usize),
        ("implemented_role_capsules", 6usize),
        ("implemented_direct_role_capsules", 8usize),
    ] {
        if let Some(value) = contract_compact.get(key).cloned() {
            let reduced = if key.contains("capsules") {
                compact_capsule_entries(&value, max_entries)
            } else {
                compact_contract_entries(&value, max_entries)
            };
            if reduced != value {
                contract_compact.insert(key.to_string(), reduced);
                contract_changed = true;
            }
        }
    }
    if let Some(value) = contract_compact.get("contract_artifact").cloned() {
        let reduced = compact_single_contract_value(&value);
        if reduced != value {
            contract_compact.insert("contract_artifact".to_string(), reduced);
            contract_changed = true;
        }
    }
    if contract_changed {
        variants.push(contract_compact);
    }

    let compact_sources = [
        ("direct_dependencies", 1200usize, 8usize),
        ("dependency_closure", 1200usize, 8usize),
        ("mcp_context", 1200usize, 8usize),
        ("implemented_dependencies", 800usize, 6usize),
        ("implemented_direct_dependencies", 1600usize, 8usize),
    ];
    let mut compact = base_context.clone();
    let mut changed = false;
    for (key, content_limit, max_entries) in compact_sources {
        if let Some(value) = compact.get(key).cloned() {
            compact.insert(
                key.to_string(),
                compact_dependency_entries(&value, content_limit, max_entries),
            );
            changed = true;
        }
    }
    if changed {
        variants.push(compact);
    }

    if let Some(direct_only) = base_context.get("direct_dependencies_only") {
        let mut direct_only_ctx = base_context.clone();
        direct_only_ctx.insert(
            "direct_dependencies".to_string(),
            compact_dependency_manifest_entries(direct_only),
        );
        direct_only_ctx.insert(
            "dependency_closure".to_string(),
            compact_dependency_manifest_entries(direct_only),
        );
        direct_only_ctx.insert(
            "mcp_context".to_string(),
            compact_dependency_manifest_entries(direct_only),
        );
        variants.push(direct_only_ctx.clone());

        let mut no_impl = direct_only_ctx;
        no_impl.remove("implemented_dependencies");
        variants.push(no_impl);
    }

    variants
}

pub(super) fn build_specification_context(
    draft_file: &Path,
    draft_content: &str,
    mut context: HashMap<String, serde_json::Value>,
    drafts_dir: &str,
) -> Result<HashMap<String, serde_json::Value>> {
    let drafts_root = Path::new(drafts_dir);
    let relative_path = draft_file.strip_prefix(drafts_root).unwrap_or(draft_file);
    if relative_path == Path::new("app.md") {
        context.insert("specification_kind".to_string(), json!("app"));
    }

    if !is_external_api_draft_path(draft_file, drafts_dir) {
        return Ok(context);
    }

    let metadata = parse_external_api_draft(draft_content);
    let openapi_content = load_openapi_content(draft_file, &metadata)?;
    let symbol_inventory = extract_external_api_symbol_inventory(draft_file, draft_content)?;
    context.insert("openapi_content".to_string(), json!(openapi_content));
    context.insert(
        "external_symbol_inventory".to_string(),
        json!({
            "operation_symbols": symbol_inventory.operation_symbols,
            "boundary_type_symbols": symbol_inventory.boundary_type_symbols,
        }),
    );
    if !metadata.documentation_urls.is_empty() {
        context.insert(
            "documentation_urls".to_string(),
            json!(metadata.documentation_urls),
        );
    }
    if !metadata.endpoint_scope.is_empty() {
        context.insert("openapi_scope".to_string(), json!(metadata.endpoint_scope));
    }

    Ok(context)
}

pub(super) fn fit_context_to_token_limit(
    executor: &AgentExecutor,
    input: &str,
    base_context: HashMap<String, serde_json::Value>,
    token_limit: Option<f64>,
) -> Result<(HashMap<String, serde_json::Value>, usize)> {
    let estimated = estimate_agent_request_tokens(executor, input, &base_context);
    let Some(limit) = token_limit else {
        return Ok((base_context, estimated));
    };
    if estimated as f64 <= limit {
        return Ok((base_context, estimated));
    }

    for candidate in build_context_variants(&base_context) {
        let candidate_estimate = estimate_agent_request_tokens(executor, input, &candidate);
        if candidate_estimate as f64 <= limit {
            return Ok((candidate, candidate_estimate));
        }
    }

    Ok((base_context, estimated))
}

#[cfg(test)]
mod tests {
    use super::{build_context_variants, build_specification_context};
    use serde_json::json;
    use std::collections::HashMap;
    use std::path::Path;

    #[test]
    fn context_variants_prefer_full_closure_manifest_before_direct_only() {
        let base = HashMap::from([
            (
                "direct_dependencies".to_string(),
                json!([
                    {
                        "name": "command_input",
                        "path": "drafts/contexts/command_input.md",
                        "dependency_kind": "direct",
                        "artifact_type": "draft_or_spec",
                        "sha256": "a"
                    },
                    {
                        "name": "Snake",
                        "path": "drafts/data/Snake.md",
                        "dependency_kind": "transitive",
                        "artifact_type": "draft_or_spec",
                        "sha256": "b"
                    }
                ]),
            ),
            (
                "dependency_closure".to_string(),
                json!([
                    {
                        "name": "command_input",
                        "path": "drafts/contexts/command_input.md",
                        "dependency_kind": "direct",
                        "artifact_type": "draft_or_spec",
                        "sha256": "a"
                    },
                    {
                        "name": "Snake",
                        "path": "drafts/data/Snake.md",
                        "dependency_kind": "transitive",
                        "artifact_type": "draft_or_spec",
                        "sha256": "b"
                    }
                ]),
            ),
            (
                "mcp_context".to_string(),
                json!([
                    {
                        "name": "command_input",
                        "path": "drafts/contexts/command_input.md",
                        "dependency_kind": "direct",
                        "artifact_type": "draft_or_spec",
                        "sha256": "a"
                    },
                    {
                        "name": "Snake",
                        "path": "drafts/data/Snake.md",
                        "dependency_kind": "transitive",
                        "artifact_type": "draft_or_spec",
                        "sha256": "b"
                    }
                ]),
            ),
            (
                "direct_dependencies_only".to_string(),
                json!([
                    {
                        "name": "command_input",
                        "path": "drafts/contexts/command_input.md",
                        "dependency_kind": "direct",
                        "artifact_type": "draft_or_spec",
                        "sha256": "a"
                    }
                ]),
            ),
            (
                "implemented_dependencies".to_string(),
                json!([
                    {
                        "name": "CommandInputContext",
                        "path": "src/contexts/command_input.rs",
                        "spec_path": "specifications/contexts/command_input.md",
                        "dependency_kind": "direct",
                        "artifact_type": "implementation_source",
                        "sha256": "impl"
                    }
                ]),
            ),
            (
                "implemented_direct_dependencies".to_string(),
                json!([
                    {
                        "name": "CommandInputContext",
                        "path": "src/contexts/command_input.rs",
                        "spec_path": "drafts/contexts/command_input.md",
                        "dependency_kind": "direct",
                        "artifact_type": "implementation_source",
                        "sha256": "impl"
                    }
                ]),
            ),
        ]);

        let variants = build_context_variants(&base);
        let compact_manifest_index = variants
            .iter()
            .position(|variant| {
                variant
                    .get("direct_dependencies")
                    .and_then(|value| value.as_array())
                    .map(|items| items.len() == 2)
                    .unwrap_or(false)
                    && variant
                        .get("direct_dependencies")
                        .and_then(|value| value.as_array())
                        .and_then(|items| items.get(1))
                        .and_then(|item| item.get("dependency_kind"))
                        .and_then(|value| value.as_str())
                        == Some("transitive")
                    && variant
                        .get("direct_dependencies")
                        .and_then(|value| value.as_array())
                        .and_then(|items| items.first())
                        .and_then(|item| item.get("sha256"))
                        .is_none()
            })
            .expect("expected a compact full-closure variant");

        let direct_only_index = variants
            .iter()
            .position(|variant| {
                variant
                    .get("direct_dependencies")
                    .and_then(|value| value.as_array())
                    .map(|items| items.len() == 1)
                    .unwrap_or(false)
            })
            .expect("expected a direct-only fallback");

        assert!(
            compact_manifest_index < direct_only_index,
            "full-closure compact variant should be tried before direct-only fallback"
        );

        assert!(
            variants.iter().any(|variant| variant
                .get("implemented_direct_dependencies")
                .and_then(|value| value.as_array())
                .map(|items| items.len() == 1)
                .unwrap_or(false)),
            "direct implemented dependencies should be preserved in context variants"
        );
    }

    #[test]
    fn context_variants_compact_contracts_and_role_capsules() {
        let base = HashMap::from([
            (
                "contract_artifact".to_string(),
                json!({
                    "contract_version": "reen.contract/v1",
                    "source_spec_path": "specifications/contexts/command_input.md",
                    "title": "CommandInputContext",
                    "specification_kind": "context",
                    "target_artifact_kind": "context_module",
                    "primary_output_path_hint": "src/contexts/command_input.rs",
                    "public_functionalities": [{"name": "capture", "signature_hint": null, "behavior_summary": ["Reads input"], "required_inputs": [], "required_outputs": []}],
                    "props": [{"name": "buffer", "description": "Queue", "type_hint": null, "notes": []}],
                    "roles": [{"name": "stdin_source", "kind": "role", "required": true, "capabilities": ["read"], "dependency_hint": null, "identity_semantics": "shared_identity", "mutation_semantics": "infer_from_behavior", "notes": []}],
                    "role_methods": [{"role": "stdin_source", "method_name": "read_available", "signature_hint": null, "behavior_summary": ["Returns chars"], "required_inputs": [], "required_outputs": []}],
                    "required_call_edges": [],
                    "shared_identity_constraints": ["same shared input stream"],
                    "mutation_constraints": [],
                    "output_obligations": [],
                    "env_config_obligations": [],
                    "lifecycle_obligations": [],
                    "allowed_freedoms": [],
                    "verification_targets": ["role_method: stdin_source.read_available"]
                }),
            ),
            (
                "direct_dependency_contracts".to_string(),
                json!([
                    {
                        "contract_version": "reen.contract/v1",
                        "source_spec_path": "specifications/contexts/string_renderer.md",
                        "title": "StringRenderer",
                        "specification_kind": "context",
                        "target_artifact_kind": "context_module",
                        "primary_output_path_hint": "src/contexts/string_renderer.rs",
                        "public_functionalities": [{"name": "render", "signature_hint": null, "behavior_summary": [], "required_inputs": [], "required_outputs": []}],
                        "props": [],
                        "roles": [{"name": "board", "kind": "role", "required": true, "capabilities": ["format"], "dependency_hint": null, "identity_semantics": "infer_from_behavior", "mutation_semantics": "infer_from_behavior", "notes": []}],
                        "role_methods": [],
                        "required_call_edges": [],
                        "shared_identity_constraints": [],
                        "mutation_constraints": [],
                        "output_obligations": ["Score: <score>"],
                        "env_config_obligations": [],
                        "lifecycle_obligations": [],
                        "allowed_freedoms": [],
                        "verification_targets": ["functionality: render"]
                    }
                ]),
            ),
            (
                "implemented_direct_role_capsules".to_string(),
                json!([
                    {
                        "name": "StringRenderer",
                        "spec_path": "specifications/contexts/string_renderer.md",
                        "source_path": "src/contexts/string_renderer.rs",
                        "artifact_kind": "context_module",
                        "public_types": ["StringRenderer"],
                        "public_methods": ["render"],
                        "relevant_role_methods": ["formatter.render"],
                        "important_fields": ["buffer"],
                        "ownership_notes": ["formatter: infer_from_behavior"],
                        "sharing_notes": ["same shared input stream"],
                        "call_edge_exports": ["render -> formatter.render"],
                        "verification_notes": ["functionality: render"],
                        "selected_snippets": [{"label": "render", "content": "pub fn render(...) { ... very long snippet ... }"}]
                    }
                ]),
            ),
        ]);

        let variants = build_context_variants(&base);
        let compacted = variants
            .iter()
            .find(|variant| {
                variant
                    .get("implemented_direct_role_capsules")
                    .and_then(|value| value.as_array())
                    .and_then(|items| items.first())
                    .and_then(|item| item.get("selected_snippets"))
                    .and_then(|value| value.as_array())
                    .and_then(|items| items.first())
                    .and_then(|item| item.get("content"))
                    .and_then(|value| value.as_str())
                    .map(|content| {
                        content.contains("truncated by reen") || content.contains("pub fn render")
                    })
                    .unwrap_or(false)
                    && variant
                        .get("contract_artifact")
                        .and_then(|value| value.get("roles"))
                        .is_some()
            })
            .expect("expected a compact contract/capsule variant");

        assert!(compacted.get("contract_artifact").is_some());
        assert!(compacted.get("direct_dependency_contracts").is_some());
        assert!(compacted.get("implemented_direct_role_capsules").is_some());
    }

    #[test]
    fn build_specification_context_marks_root_app_drafts() {
        let context = HashMap::new();
        let built =
            build_specification_context(Path::new("drafts/app.md"), "# App", context, "drafts")
                .expect("build spec context");

        assert_eq!(built.get("specification_kind"), Some(&json!("app")));
    }

    #[test]
    fn build_specification_context_leaves_non_app_drafts_unchanged() {
        let context = HashMap::new();
        let built = build_specification_context(
            Path::new("drafts/contexts/game_loop.md"),
            "# Game Loop",
            context,
            "drafts",
        )
        .expect("build spec context");

        assert!(!built.contains_key("specification_kind"));
    }
}
