use anyhow::{Context, Result};
use regex::Regex;
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExternalApiDraftMetadata {
    pub openapi_url: Option<String>,
    pub openapi_local: Option<String>,
    pub documentation_urls: Vec<String>,
    pub endpoint_scope: Vec<String>,
}

impl ExternalApiDraftMetadata {
    pub fn preferred_openapi_source(&self) -> Option<&str> {
        self.openapi_local
            .as_deref()
            .or(self.openapi_url.as_deref())
    }
}

pub fn is_external_api_draft_path(draft_file: &Path, drafts_dir: &str) -> bool {
    let drafts_root = PathBuf::from(drafts_dir);
    let relative = draft_file.strip_prefix(&drafts_root).unwrap_or(draft_file);
    relative
        .components()
        .next()
        .and_then(|component| component.as_os_str().to_str())
        .map(|component| component == "external_apis")
        .unwrap_or(false)
}

pub fn parse_external_api_draft(draft_content: &str) -> ExternalApiDraftMetadata {
    let openapi_section = extract_markdown_section(draft_content, "OpenAPI");
    let documentation_section = extract_markdown_section(draft_content, "Documentation");
    let scope_section = extract_markdown_section(draft_content, "Scope");

    ExternalApiDraftMetadata {
        openapi_url: extract_labeled_value(&openapi_section, "URL"),
        openapi_local: extract_labeled_value(&openapi_section, "Local"),
        documentation_urls: extract_urls(&documentation_section),
        endpoint_scope: extract_endpoint_scope(&scope_section),
    }
}

pub fn load_openapi_content(
    draft_file: &Path,
    metadata: &ExternalApiDraftMetadata,
) -> Result<String> {
    metadata
        .preferred_openapi_source()
        .context("external API draft is missing an OpenAPI URL or Local entry")?;

    let raw = if let Some(local) = metadata.openapi_local.as_deref() {
        read_local_openapi(draft_file, local)?
    } else {
        fetch_remote_text(
            metadata
                .openapi_url
                .as_deref()
                .context("missing OpenAPI URL for external API draft")?,
        )?
    };

    let parsed = parse_openapi_document(&raw)?;
    let filtered = filter_openapi_to_scope(parsed, &metadata.endpoint_scope);

    serde_json::to_string_pretty(&filtered).context("failed to serialize normalized OpenAPI")
}

fn read_local_openapi(draft_file: &Path, relative_or_absolute: &str) -> Result<String> {
    let candidate = PathBuf::from(relative_or_absolute);
    let resolved = if candidate.is_absolute() {
        candidate
    } else {
        draft_file
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(candidate)
    };

    fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read OpenAPI file: {}", resolved.display()))
}

fn fetch_remote_text(url: &str) -> Result<String> {
    let client = reqwest::blocking::Client::builder()
        .build()
        .context("failed to create HTTP client")?;
    client
        .get(url)
        .send()
        .with_context(|| format!("failed to fetch OpenAPI URL: {url}"))?
        .error_for_status()
        .with_context(|| format!("OpenAPI URL returned error status: {url}"))?
        .text()
        .context("failed to read OpenAPI response body")
}

fn parse_openapi_document(raw: &str) -> Result<JsonValue> {
    if let Ok(json_value) = serde_json::from_str::<JsonValue>(raw) {
        return Ok(json_value);
    }

    let yaml_value = serde_yaml::from_str::<serde_yaml::Value>(raw)
        .context("OpenAPI content is neither valid JSON nor YAML")?;
    serde_json::to_value(yaml_value).context("failed to normalize OpenAPI YAML")
}

fn filter_openapi_to_scope(document: JsonValue, endpoint_scope: &[String]) -> JsonValue {
    if endpoint_scope.is_empty() {
        return document;
    }

    let Some(root) = document.as_object() else {
        return document;
    };
    let Some(paths_value) = root.get("paths") else {
        return JsonValue::Object(root.clone());
    };
    let Some(paths_map) = paths_value.as_object() else {
        return JsonValue::Object(root.clone());
    };

    let mut selected_paths = serde_json::Map::new();
    for endpoint in endpoint_scope {
        if let Some(value) = paths_map.get(endpoint) {
            selected_paths.insert(endpoint.clone(), value.clone());
        }
    }

    if selected_paths.is_empty() {
        return JsonValue::Object(root.clone());
    }

    let mut needed_refs = BTreeSet::new();
    for value in selected_paths.values() {
        collect_json_refs(value, &mut needed_refs);
    }

    let mut output = root.clone();
    output.insert("paths".to_string(), JsonValue::Object(selected_paths));

    if let Some(components) = root.get("components").and_then(|value| value.as_object()) {
        let filtered_components = filter_components(components, &mut needed_refs);
        if !filtered_components.is_empty() {
            output.insert(
                "components".to_string(),
                JsonValue::Object(filtered_components),
            );
        }
    }

    JsonValue::Object(output)
}

fn filter_components(
    components: &serde_json::Map<String, JsonValue>,
    needed_refs: &mut BTreeSet<String>,
) -> serde_json::Map<String, JsonValue> {
    let mut filtered_components = serde_json::Map::new();
    let mut queue: Vec<String> = needed_refs.iter().cloned().collect();
    let mut seen = BTreeSet::new();
    let mut grouped: BTreeMap<String, serde_json::Map<String, JsonValue>> = BTreeMap::new();

    while let Some(reference) = queue.pop() {
        if !seen.insert(reference.clone()) {
            continue;
        }
        let Some((section, name)) = reference
            .strip_prefix("#/components/")
            .and_then(|path| path.split_once('/'))
        else {
            continue;
        };
        let Some(section_map) = components.get(section).and_then(|value| value.as_object()) else {
            continue;
        };
        let Some(component_value) = section_map.get(name) else {
            continue;
        };

        collect_json_refs(component_value, needed_refs);
        for nested in needed_refs.iter() {
            if !seen.contains(nested) {
                queue.push(nested.clone());
            }
        }

        grouped
            .entry(section.to_string())
            .or_default()
            .insert(name.to_string(), component_value.clone());
    }

    for (section, values) in grouped {
        filtered_components.insert(section, JsonValue::Object(values));
    }

    filtered_components
}

fn collect_json_refs(value: &JsonValue, refs: &mut BTreeSet<String>) {
    match value {
        JsonValue::Object(map) => {
            for (key, nested) in map {
                if key == "$ref" {
                    if let Some(reference) = nested.as_str() {
                        refs.insert(reference.to_string());
                    }
                } else {
                    collect_json_refs(nested, refs);
                }
            }
        }
        JsonValue::Array(items) => {
            for item in items {
                collect_json_refs(item, refs);
            }
        }
        _ => {}
    }
}

fn extract_markdown_section(content: &str, title: &str) -> String {
    let heading = format!("## {}", title);
    let mut lines = Vec::new();
    let mut in_section = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            if trimmed == heading {
                in_section = true;
                continue;
            }
            if in_section {
                break;
            }
        }
        if in_section {
            lines.push(line);
        }
    }

    lines.join("\n")
}

fn extract_labeled_value(section: &str, label: &str) -> Option<String> {
    let pattern = format!(
        r"(?im)^\s*[-*]?\s*(?:\*\*)?{}(?:\*\*)?\s*:\s*(\S.+?)\s*$",
        regex::escape(label)
    );
    let re = Regex::new(&pattern).ok()?;
    re.captures(section)
        .and_then(|captures| captures.get(1))
        .map(|capture| capture.as_str().trim().to_string())
}

fn extract_urls(section: &str) -> Vec<String> {
    let url_re = Regex::new(r"https?://\S+").expect("valid URL regex");
    let mut urls = Vec::new();
    for capture in url_re.find_iter(section) {
        let url = capture.as_str().trim_end_matches([',', ')', ']']);
        if !urls.iter().any(|existing| existing == url) {
            urls.push(url.to_string());
        }
    }
    urls
}

fn extract_endpoint_scope(section: &str) -> Vec<String> {
    let endpoint_re = Regex::new(r"/[A-Za-z0-9_{}./:-]*").expect("valid endpoint regex");
    let mut endpoints = Vec::new();
    for capture in endpoint_re.find_iter(section) {
        let endpoint = capture.as_str().trim_end_matches([',', '.', ';']);
        if endpoint.len() > 1 && !endpoints.iter().any(|existing| existing == endpoint) {
            endpoints.push(endpoint.to_string());
        }
    }
    endpoints
}

#[cfg(test)]
mod tests {
    use super::{
        filter_openapi_to_scope, is_external_api_draft_path, load_openapi_content,
        parse_external_api_draft, ExternalApiDraftMetadata,
    };
    use serde_json::json;
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("reen_openapi_{}_{}", prefix, nanos))
    }

    #[test]
    fn parses_external_api_draft_metadata() {
        let draft = r#"# Stripe API Client

## OpenAPI
- **URL**: https://example.com/openapi.json
- **Local**: specs/stripe.yaml

## Documentation
- **URL**: https://docs.example.com/stripe

## Scope
- Endpoints to include: /v1/charges, /v1/customers
"#;

        let metadata = parse_external_api_draft(draft);
        assert_eq!(
            metadata.openapi_url.as_deref(),
            Some("https://example.com/openapi.json")
        );
        assert_eq!(metadata.openapi_local.as_deref(), Some("specs/stripe.yaml"));
        assert_eq!(
            metadata.documentation_urls,
            vec!["https://docs.example.com/stripe"]
        );
        assert_eq!(
            metadata.endpoint_scope,
            vec!["/v1/charges", "/v1/customers"]
        );
    }

    #[test]
    fn external_api_draft_path_detection_matches_expected_folder() {
        assert!(is_external_api_draft_path(
            Path::new("drafts/external_apis/stripe.md"),
            "drafts"
        ));
        assert!(!is_external_api_draft_path(
            Path::new("drafts/contexts/stripe.md"),
            "drafts"
        ));
    }

    #[test]
    fn filters_openapi_to_selected_paths_and_components() {
        let original = json!({
            "openapi": "3.1.0",
            "paths": {
                "/v1/charges": {
                    "post": {
                        "responses": {
                            "200": {
                                "content": {
                                    "application/json": {
                                        "schema": { "$ref": "#/components/schemas/Charge" }
                                    }
                                }
                            }
                        }
                    }
                },
                "/v1/customers": {
                    "get": {}
                }
            },
            "components": {
                "schemas": {
                    "Charge": {
                        "type": "object",
                        "properties": {
                            "customer": { "$ref": "#/components/schemas/Customer" }
                        }
                    },
                    "Customer": {
                        "type": "object"
                    },
                    "Unused": {
                        "type": "object"
                    }
                }
            }
        });

        let filtered = filter_openapi_to_scope(original, &["/v1/charges".to_string()]);
        let paths = filtered["paths"].as_object().expect("paths map");
        assert!(paths.contains_key("/v1/charges"));
        assert!(!paths.contains_key("/v1/customers"));
        let schemas = filtered["components"]["schemas"]
            .as_object()
            .expect("schemas map");
        assert!(schemas.contains_key("Charge"));
        assert!(schemas.contains_key("Customer"));
        assert!(!schemas.contains_key("Unused"));
    }

    #[test]
    fn loads_local_openapi_and_normalizes_to_json() {
        let root = temp_dir("local");
        let draft_dir = root.join("drafts/external_apis");
        let spec_dir = root.join("drafts/external_apis/specs");
        fs::create_dir_all(&spec_dir).expect("create dirs");
        let draft_file = draft_dir.join("stripe.md");
        fs::write(&draft_file, "# Draft").expect("write draft");
        fs::write(
            spec_dir.join("stripe.yaml"),
            "openapi: 3.1.0\npaths:\n  /v1/charges:\n    get: {}\n",
        )
        .expect("write spec");

        let metadata = ExternalApiDraftMetadata {
            openapi_local: Some("specs/stripe.yaml".to_string()),
            endpoint_scope: vec!["/v1/charges".to_string()],
            ..ExternalApiDraftMetadata::default()
        };
        let content = load_openapi_content(&draft_file, &metadata).expect("load openapi");
        assert!(content.contains("/v1/charges"));

        let _ = fs::remove_dir_all(root);
    }
}
