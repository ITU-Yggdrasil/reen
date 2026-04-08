use anyhow::Result;
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;

use super::agent_executor::AgentExecutor;
use super::contracts::{
    ContractArtifact, compact_contract_artifact_value, compact_contract_artifacts,
};
use super::draft_schema::DraftDocument;
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
            for key in ["name", "path", "spec_path"] {
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
    let manifest_sources = [
        "direct_dependencies",
        "dependency_closure",
        "mcp_context",
        "implemented_dependencies",
        "implemented_direct_dependencies",
    ];

    // Preferred default: proactively compact manifest-like structures and strip the raw
    // dependency tool payload before estimating or executing agent requests (unless the user
    // opts into full dependency payloads via REEN_FULL_IMPLEMENTATION_DEPS).
    let mut preferred = base_context.clone();
    if !super::full_implementation_dependency_context_enabled() {
        preferred.remove("dependency_tool_context");
    }
    for key in manifest_sources {
        if let Some(value) = preferred.get(key).cloned() {
            preferred.insert(key.to_string(), compact_dependency_manifest_entries(&value));
        }
    }
    if let Some(value) = preferred.get("contract_artifact").cloned() {
        preferred.insert(
            "contract_artifact".to_string(),
            compact_single_contract_value(&value),
        );
    }
    for (key, max_entries) in [
        ("dependency_contracts", 8usize),
        ("direct_dependency_contracts", 8usize),
        ("implemented_role_capsules", 6usize),
        ("implemented_direct_role_capsules", 8usize),
    ] {
        if let Some(value) = preferred.get(key).cloned() {
            let reduced = if key.contains("capsules") {
                compact_capsule_entries(&value, max_entries)
            } else {
                compact_contract_entries(&value, max_entries)
            };
            preferred.insert(key.to_string(), reduced);
        }
    }

    let mut variants = vec![preferred.clone()];

    if preferred.contains_key("implemented_dependencies") {
        let mut without_impl = preferred.clone();
        without_impl.remove("implemented_dependencies");
        variants.push(without_impl);
    }

    if let Some(openapi_content) = preferred.get("openapi_content").and_then(|v| v.as_str()) {
        let mut compact_openapi = preferred.clone();
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

    let mut compact_manifest = preferred.clone();
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

    let mut contract_compact = preferred.clone();
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
    let mut compact = preferred.clone();
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

    if let Some(direct_only) = preferred.get("direct_dependencies_only") {
        let mut direct_only_ctx = preferred.clone();
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
    parsed_draft: Option<&DraftDocument>,
) -> Result<HashMap<String, serde_json::Value>> {
    let drafts_root = Path::new(drafts_dir);
    let relative_path = draft_file.strip_prefix(drafts_root).unwrap_or(draft_file);
    if relative_path == Path::new("app.md") {
        context.insert("specification_kind".to_string(), json!("app"));
    }
    if let Some(parsed) = parsed_draft {
        context.insert("draft_summary".to_string(), json!(parsed.summary));
        context.insert("draft_kind".to_string(), json!(parsed.kind));
    }

    if !is_external_api_draft_path(draft_file, drafts_dir) {
        return Ok(context);
    }

    let metadata = parse_external_api_draft(draft_content)?;
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
    let mut variants = build_context_variants(&base_context).into_iter();
    let preferred = variants.next().unwrap_or(base_context);
    let preferred_estimate = estimate_agent_request_tokens(executor, input, &preferred);
    let Some(limit) = token_limit else {
        return Ok((preferred, preferred_estimate));
    };
    if preferred_estimate as f64 <= limit {
        return Ok((preferred, preferred_estimate));
    }

    for candidate in variants {
        let candidate_estimate = estimate_agent_request_tokens(executor, input, &candidate);
        if candidate_estimate as f64 <= limit {
            return Ok((candidate, candidate_estimate));
        }
    }

    Ok((preferred, preferred_estimate))
}

pub(super) fn find_cached_context_variant(
    executor: &AgentExecutor,
    input: &str,
    base_context: HashMap<String, serde_json::Value>,
) -> Result<Option<(HashMap<String, serde_json::Value>, usize)>> {
    for candidate in build_context_variants(&base_context) {
        if executor.is_cache_hit(input, candidate.clone())? {
            let estimated = estimate_agent_request_tokens(executor, input, &candidate);
            return Ok(Some((candidate, estimated)));
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::{build_context_variants, build_specification_context, fit_context_to_token_limit};
    use crate::cli::{Config, agent_executor::AgentExecutor};
    use serde_json::json;
    use std::collections::HashMap;
    use std::path::Path;

    #[test]
    fn preferred_context_variant_is_proactively_compacted() {
        let base = HashMap::from([
            (
                "direct_dependencies".to_string(),
                json!([
                    {
                        "name": "amount",
                        "path": "drafts/data/amount.md",
                        "dependency_kind": "direct",
                        "artifact_type": "draft_or_spec",
                        "sha256": "a",
                        "content": "# Amount\nA very long dependency body"
                    }
                ]),
            ),
            (
                "dependency_tool_context".to_string(),
                json!({
                    "dependency_artifacts": [
                        {
                            "path": "drafts/data/amount.md",
                            "content": "full dependency draft"
                        }
                    ]
                }),
            ),
        ]);

        let preferred = build_context_variants(&base)
            .into_iter()
            .next()
            .expect("preferred variant");

        assert!(!preferred.contains_key("dependency_tool_context"));
        let item = &preferred["direct_dependencies"][0];
        assert_eq!(item["name"], json!("amount"));
        assert_eq!(item["dependency_kind"], json!("direct"));
        assert!(item.get("sha256").is_none());
        assert!(item.get("content").is_none());
    }

    #[test]
    fn context_variants_prefer_full_dependency_closure_before_direct_only_manifest() {
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
        let retains_transitive_closure_index = variants
            .iter()
            .position(|variant| {
                variant
                    .get("dependency_closure")
                    .and_then(|value| value.as_array())
                    .map(|items| items.len() == 2)
                    .unwrap_or(false)
                    && variant
                        .get("dependency_closure")
                        .and_then(|value| value.as_array())
                        .and_then(|items| items.get(1))
                        .and_then(|item| item.get("dependency_kind"))
                        .and_then(|value| value.as_str())
                        == Some("transitive")
                    && variant
                        .get("dependency_closure")
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
                    .get("dependency_closure")
                    .and_then(|value| value.as_array())
                    .map(|items| items.len() == 1)
                    .unwrap_or(false)
                    && variant
                        .get("direct_dependencies")
                        .and_then(|value| value.as_array())
                        .map(|items| items.len() == 1)
                        .unwrap_or(false)
            })
            .expect("expected a direct-only fallback");

        assert!(
            retains_transitive_closure_index < direct_only_index,
            "full dependency_closure compact variant should be tried before direct-only fallback"
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
                    "contract_version": "reen.contract/v2",
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
                        "contract_version": "reen.contract/v2",
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
        let built = build_specification_context(
            Path::new("drafts/app.md"),
            "# App",
            context,
            "drafts",
            None,
        )
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
            None,
        )
        .expect("build spec context");

        assert!(!built.contains_key("specification_kind"));
    }

    #[test]
    fn context_variants_drop_heavy_dependency_content_in_preferred_variant() {
        let base = HashMap::from([(
            "direct_dependencies".to_string(),
            json!([
                {
                    "name": "command_input",
                    "path": "drafts/contexts/command_input.md",
                    "dependency_kind": "direct",
                    "artifact_type": "draft_or_spec",
                    "sha256": "a",
                    "content": "# Command Input\nA".repeat(200)
                }
            ]),
        )]);

        let variants = build_context_variants(&base);
        let preferred = variants.first().expect("preferred variant");
        let item = &preferred["direct_dependencies"][0];
        assert_eq!(item["name"], json!("command_input"));
        assert_eq!(item["path"], json!("drafts/contexts/command_input.md"));
        assert!(item.get("sha256").is_none());
        assert!(item.get("content").is_none());
    }

    #[test]
    fn fit_context_without_limit_uses_preferred_compact_variant() {
        let executor = AgentExecutor::new(
            "create_specifications_data",
            &Config {
                verbose: false,
                dry_run: false,
                github_repo: None,
            },
        )
        .expect("executor");

        let base = HashMap::from([
            (
                "direct_dependencies".to_string(),
                json!([
                    {
                        "name": "amount",
                        "path": "drafts/data/amount.md",
                        "dependency_kind": "direct",
                        "artifact_type": "draft_or_spec",
                        "sha256": "a",
                        "content": "# Amount\nA very long dependency body"
                    }
                ]),
            ),
            (
                "dependency_tool_context".to_string(),
                json!({
                    "dependency_artifacts": [
                        {
                            "path": "drafts/data/amount.md",
                            "content": "full dependency draft"
                        }
                    ]
                }),
            ),
        ]);

        let (selected, estimated) = fit_context_to_token_limit(&executor, "# Amount", base, None)
            .expect("fit context");

        assert!(estimated > 0);
        assert!(!selected.contains_key("dependency_tool_context"));
        assert!(selected["direct_dependencies"][0].get("sha256").is_none());
        assert!(selected["direct_dependencies"][0].get("content").is_none());
    }
}
