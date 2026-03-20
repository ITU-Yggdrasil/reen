use anyhow::Result;
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;

use super::agent_executor::AgentExecutor;
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

fn build_context_variants(
    base_context: &HashMap<String, serde_json::Value>,
) -> Vec<HashMap<String, serde_json::Value>> {
    let mut variants = vec![base_context.clone()];

    if base_context.contains_key("implemented_dependencies") {
        let mut without_impl = base_context.clone();
        without_impl.remove("implemented_dependencies");
        variants.push(without_impl);
    }

    if let Some(direct_only) = base_context.get("direct_dependencies_only") {
        let mut direct_only_ctx = base_context.clone();
        direct_only_ctx.insert("direct_dependencies".to_string(), direct_only.clone());
        direct_only_ctx.insert("dependency_closure".to_string(), direct_only.clone());
        direct_only_ctx.insert("mcp_context".to_string(), direct_only.clone());
        variants.push(direct_only_ctx.clone());

        let mut no_impl = direct_only_ctx;
        no_impl.remove("implemented_dependencies");
        variants.push(no_impl);
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

    let compact_sources = [
        ("direct_dependencies", 1200usize, 8usize),
        ("dependency_closure", 1200usize, 8usize),
        ("mcp_context", 1200usize, 8usize),
        ("implemented_dependencies", 800usize, 6usize),
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

    variants
}

pub(super) fn build_specification_context(
    draft_file: &Path,
    draft_content: &str,
    mut context: HashMap<String, serde_json::Value>,
    drafts_dir: &str,
) -> Result<HashMap<String, serde_json::Value>> {
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

    anyhow::bail!(
        "Estimated request size ({estimated} input tokens) exceeds configured token limit and could not be reduced automatically."
    )
}
