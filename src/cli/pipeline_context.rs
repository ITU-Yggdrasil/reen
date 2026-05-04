use anyhow::Result;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use super::agent_executor::AgentExecutor;
use super::openapi_fetcher::{
    is_external_api_draft_path, load_openapi_content, parse_external_api_draft,
};
use super::stage_runner::estimate_agent_request_tokens;
use super::DRAFTS_DIR;

fn is_component_draft_path(draft_file: &Path, drafts_dir: &str) -> bool {
    draft_file
        .strip_prefix(drafts_dir)
        .ok()
        .and_then(|relative| relative.components().next())
        .and_then(|component| component.as_os_str().to_str())
        == Some("components")
}

fn draft_category(draft_file: &Path, drafts_dir: &str) -> Option<String> {
    draft_file
        .strip_prefix(drafts_dir)
        .ok()
        .and_then(|relative| relative.components().next())
        .and_then(|component| component.as_os_str().to_str())
        .map(|category| category.to_string())
}

fn collect_markdown_artifacts(root: &Path) -> Result<Vec<serde_json::Value>> {
    fn visit(dir: &Path, out: &mut Vec<serde_json::Value>) -> Result<()> {
        if !dir.exists() {
            return Ok(());
        }

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                visit(&path, out)?;
                continue;
            }

            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }

            let content = fs::read_to_string(&path)?;
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            out.push(json!({
                "name": name,
                "path": path.to_string_lossy().to_string(),
                "content": content,
            }));
        }

        Ok(())
    }

    let mut artifacts = Vec::new();
    visit(root, &mut artifacts)?;
    artifacts.sort_by(|a, b| {
        let a_path = a.get("path").and_then(|v| v.as_str()).unwrap_or_default();
        let b_path = b.get("path").and_then(|v| v.as_str()).unwrap_or_default();
        a_path.cmp(b_path)
    });
    Ok(artifacts)
}

fn declared_component_name(content: &str, fallback: &str) -> String {
    let heading = content
        .lines()
        .find(|line| line.trim_start().starts_with('#'))
        .map(|line| line.trim_start().trim_start_matches('#').trim())
        .unwrap_or(fallback);

    heading
        .strip_suffix(" - Component Specification")
        .unwrap_or(heading)
        .trim()
        .to_string()
}

fn collect_component_artifacts(root: &Path) -> Result<Vec<serde_json::Value>> {
    fn visit(dir: &Path, out: &mut Vec<serde_json::Value>) -> Result<()> {
        if !dir.exists() {
            return Ok(());
        }

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                visit(&path, out)?;
                continue;
            }

            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }

            let content = fs::read_to_string(&path)?;
            let fallback_name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            let name = declared_component_name(&content, fallback_name);
            out.push(json!({
                "name": name,
                "path": path.to_string_lossy().to_string(),
                "content": content,
            }));
        }

        Ok(())
    }

    let mut artifacts = Vec::new();
    visit(root, &mut artifacts)?;
    artifacts.sort_by(|a, b| {
        let a_path = a.get("path").and_then(|v| v.as_str()).unwrap_or_default();
        let b_path = b.get("path").and_then(|v| v.as_str()).unwrap_or_default();
        a_path.cmp(b_path)
    });
    Ok(artifacts)
}

fn collect_brand_identity_artifacts(specifications_root: &Path) -> Result<Vec<serde_json::Value>> {
    let mut artifacts = Vec::new();
    for folder in ["brands", "visuals"] {
        let mut folder_artifacts = collect_markdown_artifacts(&specifications_root.join(folder))?;
        artifacts.append(&mut folder_artifacts);
    }
    artifacts.sort_by(|a, b| {
        let a_path = a.get("path").and_then(|v| v.as_str()).unwrap_or_default();
        let b_path = b.get("path").and_then(|v| v.as_str()).unwrap_or_default();
        a_path.cmp(b_path)
    });
    Ok(artifacts)
}

fn detect_duplicate_component_names(draft_file: &Path) -> Result<Vec<String>> {
    let Some(components_dir) = draft_file.parent() else {
        return Ok(Vec::new());
    };
    if components_dir.file_name().and_then(|s| s.to_str()) != Some("components") {
        return Ok(Vec::new());
    }

    let current_name = draft_file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    let mut seen = Vec::new();
    for entry in fs::read_dir(components_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }

        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if stem == current_name {
            seen.push(path);
        }
    }

    if seen.len() <= 1 {
        return Ok(Vec::new());
    }

    Ok(seen
        .into_iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect())
}

fn add_component_context(
    draft_file: &Path,
    context: &mut HashMap<String, serde_json::Value>,
) -> Result<()> {
    let Some(drafts_root) = draft_file
        .ancestors()
        .find(|ancestor| ancestor.file_name().and_then(|s| s.to_str()) == Some("drafts"))
    else {
        return Ok(());
    };
    let Some(project_root) = drafts_root.parent() else {
        return Ok(());
    };

    let existing_specs = collect_markdown_artifacts(&project_root.join("specifications"))?;
    if !existing_specs.is_empty() {
        context.insert("existing_specifications".to_string(), json!(existing_specs));
    }

    let brand_identity_specs =
        collect_brand_identity_artifacts(&project_root.join("specifications"))?;
    if !brand_identity_specs.is_empty() {
        context.insert(
            "brand_identity_specifications".to_string(),
            json!(brand_identity_specs),
        );
    }

    let draft_component_refs =
        collect_component_artifacts(&project_root.join("drafts/components"))?;
    if !draft_component_refs.is_empty() {
        let draft_component_names = draft_component_refs
            .iter()
            .filter_map(|artifact| artifact.get("name").and_then(|v| v.as_str()))
            .map(|name| name.to_string())
            .collect::<Vec<_>>();
        context.insert(
            "draft_component_references".to_string(),
            json!(draft_component_refs),
        );
        context.insert(
            "draft_component_names".to_string(),
            json!(draft_component_names),
        );
    }

    let draft_component_name_set = context
        .get("draft_component_names")
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str())
                .map(|name| name.to_string())
                .collect::<std::collections::HashSet<_>>()
        })
        .unwrap_or_default();

    let library_refs = collect_component_artifacts(&project_root.join("component_drafts"))?;
    let filtered_library_refs: Vec<serde_json::Value> = library_refs
        .into_iter()
        .filter(|artifact| {
            artifact
                .get("name")
                .and_then(|v| v.as_str())
                .map(|name| !draft_component_name_set.contains(name))
                .unwrap_or(true)
        })
        .collect();
    if !filtered_library_refs.is_empty() {
        let component_library_names = filtered_library_refs
            .iter()
            .filter_map(|artifact| artifact.get("name").and_then(|v| v.as_str()))
            .map(|name| name.to_string())
            .collect::<Vec<_>>();
        context.insert(
            "component_library_references".to_string(),
            json!(filtered_library_refs),
        );
        context.insert(
            "component_library_names".to_string(),
            json!(component_library_names),
        );
    }

    let duplicate_names = detect_duplicate_component_names(draft_file)?;
    if !duplicate_names.is_empty() {
        context.insert(
            "duplicate_component_names".to_string(),
            json!(duplicate_names),
        );
    }

    Ok(())
}

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
        ("brand_identity_specifications", 1200usize, 8usize),
        ("draft_component_references", 1200usize, 8usize),
        ("component_library_references", 1200usize, 8usize),
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
) -> Result<HashMap<String, serde_json::Value>> {
    context.insert(
        "draft_path".to_string(),
        json!(draft_file.to_string_lossy().to_string()),
    );
    if let Some(category) = draft_category(draft_file, DRAFTS_DIR) {
        context.insert("draft_category".to_string(), json!(category));
    }

    if is_component_draft_path(draft_file, DRAFTS_DIR) {
        add_component_context(draft_file, &mut context)?;
    }

    if !is_external_api_draft_path(draft_file, DRAFTS_DIR) {
        return Ok(context);
    }

    let metadata = parse_external_api_draft(draft_content);
    let openapi_content = load_openapi_content(draft_file, &metadata)?;
    context.insert("openapi_content".to_string(), json!(openapi_content));
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
