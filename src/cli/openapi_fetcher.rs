use anyhow::{Context, Result};
use regex::Regex;
use serde::Serialize;
use serde_json::Value as JsonValue;
use sha2::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExternalApiOperationSymbol {
    pub name: String,
    pub method: String,
    pub path: String,
    pub operation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExternalApiBoundaryTypeSymbol {
    pub name: String,
    pub source: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ExternalApiSymbolInventory {
    pub operation_symbols: Vec<ExternalApiOperationSymbol>,
    pub boundary_type_symbols: Vec<ExternalApiBoundaryTypeSymbol>,
}

impl ExternalApiSymbolInventory {
    pub fn published_symbol_names(&self) -> Vec<String> {
        let mut names = BTreeSet::new();
        for operation in &self.operation_symbols {
            names.insert(operation.name.clone());
        }
        for boundary in &self.boundary_type_symbols {
            names.insert(boundary.name.clone());
        }
        names.into_iter().collect()
    }
}

fn is_external_api_folder(component: &str) -> bool {
    matches!(component, "external_apis" | "apis")
}

pub fn is_external_api_draft_path(draft_file: &Path, drafts_dir: &str) -> bool {
    let drafts_root = PathBuf::from(drafts_dir);
    let relative = draft_file.strip_prefix(&drafts_root).unwrap_or(draft_file);
    relative
        .components()
        .next()
        .and_then(|component| component.as_os_str().to_str())
        .map(is_external_api_folder)
        .unwrap_or(false)
}

pub fn parse_external_api_draft(draft_content: &str) -> ExternalApiDraftMetadata {
    let openapi_section =
        extract_markdown_section(draft_content, &["OpenAPI", "API Specification"]);
    let documentation_section = extract_markdown_section(draft_content, &["Documentation"]);
    let scope_section = extract_markdown_section(draft_content, &["Scope"]);

    ExternalApiDraftMetadata {
        openapi_url: extract_labeled_value(&openapi_section, "URL")
            .or_else(|| extract_first_url(&openapi_section)),
        openapi_local: extract_labeled_value(&openapi_section, "Local"),
        documentation_urls: extract_urls(&documentation_section),
        endpoint_scope: extract_endpoint_scope(&scope_section),
    }
}

pub fn load_openapi_content(
    draft_file: &Path,
    metadata: &ExternalApiDraftMetadata,
) -> Result<String> {
    let filtered = load_openapi_document(draft_file, metadata)?;
    serde_json::to_string_pretty(&filtered).context("failed to serialize normalized OpenAPI")
}

pub fn load_openapi_document(
    draft_file: &Path,
    metadata: &ExternalApiDraftMetadata,
) -> Result<JsonValue> {
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
    Ok(filter_openapi_to_scope(parsed, &metadata.endpoint_scope))
}

pub fn extract_external_api_symbol_inventory(
    draft_file: &Path,
    draft_content: &str,
) -> Result<ExternalApiSymbolInventory> {
    let metadata = parse_external_api_draft(draft_content);
    let document = load_openapi_document(draft_file, &metadata)?;
    Ok(extract_symbol_inventory_from_openapi(&document))
}

pub fn fallback_operation_name(method: &str, path: &str) -> String {
    let normalized_path = normalize_operation_path(path);
    format!(
        "{}_{}",
        method.trim().to_ascii_lowercase(),
        if normalized_path.is_empty() {
            "root".to_string()
        } else {
            normalized_path
        }
    )
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
    let url = normalize_openapi_source_url(url);
    thread::spawn(move || {
        let client = reqwest::blocking::Client::builder()
            .build()
            .context("failed to create HTTP client")?;
        client
            .get(&url)
            .send()
            .with_context(|| format!("failed to fetch OpenAPI URL: {url}"))?
            .error_for_status()
            .with_context(|| format!("OpenAPI URL returned error status: {url}"))?
            .text()
            .context("failed to read OpenAPI response body")
    })
    .join()
    .map_err(|_| anyhow::anyhow!("OpenAPI fetch thread panicked"))?
}

fn normalize_openapi_source_url(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("https://github.com/") {
        let parts = rest.split('/').collect::<Vec<_>>();
        if parts.len() >= 5 && parts[2] == "blob" {
            let owner = parts[0];
            let repo = parts[1];
            let branch = parts[3];
            let path = parts[4..].join("/");
            return format!("https://raw.githubusercontent.com/{owner}/{repo}/{branch}/{path}");
        }
    }
    url.to_string()
}

fn parse_openapi_document(raw: &str) -> Result<JsonValue> {
    if let Ok(json_value) = serde_json::from_str::<JsonValue>(raw) {
        return Ok(json_value);
    }

    let yaml_value = serde_yaml::from_str::<serde_yaml::Value>(raw)
        .context("OpenAPI content is neither valid JSON nor YAML")?;
    serde_json::to_value(yaml_value).context("failed to normalize OpenAPI YAML")
}

fn extract_symbol_inventory_from_openapi(document: &JsonValue) -> ExternalApiSymbolInventory {
    let mut operations = Vec::new();
    let mut published_type_names = BTreeSet::new();
    let Some(root) = document.as_object() else {
        return ExternalApiSymbolInventory::default();
    };

    let global_security = root
        .get("security")
        .map(extract_security_scheme_names)
        .unwrap_or_default();

    let mut proposed_names: Vec<(String, String, String, Option<String>)> = Vec::new();
    let mut collision_counts = BTreeMap::new();

    if let Some(paths) = root.get("paths").and_then(|value| value.as_object()) {
        let mut sorted_paths: Vec<_> = paths.iter().collect();
        sorted_paths.sort_by(|(left, _), (right, _)| left.cmp(right));

        for (path, item) in sorted_paths {
            let Some(path_item) = item.as_object() else {
                continue;
            };
            for method in [
                "get", "post", "put", "delete", "patch", "options", "head", "trace",
            ] {
                let Some(operation) = path_item.get(method).and_then(|value| value.as_object())
                else {
                    continue;
                };

                let operation_id = operation
                    .get("operationId")
                    .and_then(|value| value.as_str())
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned);
                let proposed_name = operation_id
                    .clone()
                    .unwrap_or_else(|| fallback_operation_name(method, path));
                *collision_counts
                    .entry(proposed_name.clone())
                    .or_insert(0usize) += 1;
                proposed_names.push((
                    proposed_name,
                    method.to_string(),
                    path.to_string(),
                    operation_id.clone(),
                ));

                let mut refs = BTreeSet::new();
                collect_json_refs(&JsonValue::Object(operation.clone()), &mut refs);
                for reference in refs {
                    if let Some((section, name)) = parse_component_reference(&reference) {
                        published_type_names.insert((name.to_string(), section.to_string()));
                    }
                }

                let mut security_names = global_security.clone();
                security_names.extend(extract_security_scheme_names(
                    operation.get("security").unwrap_or(&JsonValue::Null),
                ));
                for name in security_names {
                    published_type_names.insert((name, "securitySchemes".to_string()));
                }
            }
        }
    }

    for (proposed_name, method, path, operation_id) in proposed_names {
        let name = if collision_counts.get(&proposed_name).copied().unwrap_or(0) > 1 {
            format!(
                "{}_{}",
                proposed_name,
                short_symbol_hash(&format!("{}:{}", method, path))
            )
        } else {
            proposed_name
        };
        operations.push(ExternalApiOperationSymbol {
            name,
            method,
            path,
            operation_id,
        });
    }

    let boundary_type_symbols = published_type_names
        .into_iter()
        .map(|(name, source)| ExternalApiBoundaryTypeSymbol { name, source })
        .collect();

    ExternalApiSymbolInventory {
        operation_symbols: operations,
        boundary_type_symbols,
    }
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

fn parse_component_reference(reference: &str) -> Option<(&str, &str)> {
    reference
        .strip_prefix("#/components/")
        .and_then(|path| path.split_once('/'))
}

fn extract_security_scheme_names(value: &JsonValue) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    if let Some(requirements) = value.as_array() {
        for requirement in requirements {
            if let Some(map) = requirement.as_object() {
                for name in map.keys() {
                    if !name.trim().is_empty() {
                        names.insert(name.trim().to_string());
                    }
                }
            }
        }
    }
    names
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

fn normalize_operation_path(path: &str) -> String {
    let param_re = Regex::new(r"\{([^}]+)\}").expect("valid path parameter regex");
    let mut normalized_segments = Vec::new();
    for raw_segment in path.trim_matches('/').split('/') {
        if raw_segment.is_empty() {
            continue;
        }

        let replaced = param_re.replace_all(raw_segment, "by_$1");
        let normalized = normalize_symbol_fragment(replaced.as_ref());
        if !normalized.is_empty() {
            normalized_segments.push(normalized);
        }
    }

    normalized_segments.join("_")
}

fn normalize_symbol_fragment(raw: &str) -> String {
    let mut out = String::new();
    let mut last_was_underscore = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_was_underscore = false;
        } else if !last_was_underscore {
            out.push('_');
            last_was_underscore = true;
        }
    }
    out.trim_matches('_').to_string()
}

fn short_symbol_hash(value: &str) -> String {
    let mut hasher = sha2::Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())[..8].to_string()
}

fn extract_markdown_section(content: &str, titles: &[&str]) -> String {
    let headings = titles
        .into_iter()
        .map(|title| format!("## {}", title))
        .collect::<Vec<_>>();
    let mut lines = Vec::new();
    let mut in_section = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            if headings.iter().any(|heading| trimmed == heading) {
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

fn extract_first_url(section: &str) -> Option<String> {
    extract_urls(section).into_iter().next()
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
        ExternalApiDraftMetadata, extract_external_api_symbol_inventory, fallback_operation_name,
        filter_openapi_to_scope, is_external_api_draft_path, load_openapi_content,
        normalize_openapi_source_url, parse_external_api_draft,
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

## API Specification
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
        assert!(is_external_api_draft_path(
            Path::new("drafts/apis/stripe.md"),
            "drafts"
        ));
        assert!(!is_external_api_draft_path(
            Path::new("drafts/contexts/stripe.md"),
            "drafts"
        ));
    }

    #[test]
    fn parses_external_api_draft_metadata_from_bare_api_specification_url() {
        let draft = r#"# AISStream

## API Specification
- https://example.com/openapi.yaml

## Documentation
- https://docs.example.com/aisstream
"#;

        let metadata = parse_external_api_draft(draft);
        assert_eq!(
            metadata.openapi_url.as_deref(),
            Some("https://example.com/openapi.yaml")
        );
        assert_eq!(
            metadata.documentation_urls,
            vec!["https://docs.example.com/aisstream"]
        );
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

    #[test]
    fn uses_operation_id_when_present_and_extracts_boundary_types() {
        let root = temp_dir("symbols_operation_id");
        let draft_dir = root.join("drafts/external_apis");
        let spec_dir = draft_dir.join("specs");
        fs::create_dir_all(&spec_dir).expect("create dirs");
        let draft_file = draft_dir.join("stripe.md");
        fs::write(
            &draft_file,
            "# Stripe\n\n## OpenAPI\n- Local: specs/stripe.yaml\n",
        )
        .expect("write draft");
        fs::write(
            spec_dir.join("stripe.yaml"),
            r##"
openapi: 3.1.0
paths:
  /v1/charges:
    post:
      operationId: CreateCharge
      security:
        - bearerAuth: []
      requestBody:
        required: true
        content:
          application/json:
            schema:
              $ref: "#/components/schemas/CreateChargeRequest"
      responses:
        "200":
          description: ok
          content:
            application/json:
              schema:
                $ref: "#/components/schemas/Charge"
components:
  schemas:
    CreateChargeRequest:
      type: object
    Charge:
      type: object
  securitySchemes:
    bearerAuth:
      type: http
      scheme: bearer
"##,
        )
        .expect("write spec");

        let inventory = extract_external_api_symbol_inventory(
            &draft_file,
            &fs::read_to_string(&draft_file).unwrap(),
        )
        .expect("inventory");

        assert_eq!(inventory.operation_symbols.len(), 1);
        assert_eq!(inventory.operation_symbols[0].name, "CreateCharge");
        let published = inventory.published_symbol_names();
        assert!(published.iter().any(|name| name == "CreateChargeRequest"));
        assert!(published.iter().any(|name| name == "Charge"));
        assert!(published.iter().any(|name| name == "bearerAuth"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn fallback_operation_name_is_deterministic_and_normalized() {
        assert_eq!(
            fallback_operation_name("POST", "/v1/payment-intents/{intent_id}.json"),
            "post_v1_payment_intents_by_intent_id_json"
        );
    }

    #[test]
    fn normalizes_github_blob_urls_to_raw_content() {
        assert_eq!(
            normalize_openapi_source_url(
                "https://github.com/aisstream/ais-message-models/blob/master/type-definition.yaml"
            ),
            "https://raw.githubusercontent.com/aisstream/ais-message-models/master/type-definition.yaml"
        );
    }

    #[test]
    fn colliding_fallback_names_receive_stable_suffixes() {
        let root = temp_dir("symbols_collision");
        let draft_dir = root.join("drafts/external_apis");
        let spec_dir = draft_dir.join("specs");
        fs::create_dir_all(&spec_dir).expect("create dirs");
        let draft_file = draft_dir.join("demo.md");
        fs::write(
            &draft_file,
            "# Demo\n\n## OpenAPI\n- Local: specs/demo.yaml\n",
        )
        .expect("write draft");
        fs::write(
            spec_dir.join("demo.yaml"),
            r##"
openapi: 3.1.0
paths:
  /users/{id}:
    get:
      responses:
        "200": { description: ok }
  /users/by-id:
    get:
      responses:
        "200": { description: ok }
"##,
        )
        .expect("write spec");

        let inventory = extract_external_api_symbol_inventory(
            &draft_file,
            &fs::read_to_string(&draft_file).unwrap(),
        )
        .expect("inventory");
        let names: Vec<String> = inventory
            .operation_symbols
            .iter()
            .map(|operation| operation.name.clone())
            .collect();

        assert_eq!(names.len(), 2);
        assert!(
            names
                .iter()
                .all(|name| name.starts_with("get_users_by_id_"))
        );
        assert_ne!(names[0], names[1]);

        let _ = fs::remove_dir_all(root);
    }
}
