//! API ingestion: OpenAPI snapshot → generated data drafts + operation specs.
//!
//! Deterministic: same OpenAPI snapshot → byte-identical output.

use crate::draft_parser::{parse_boundary_draft, BoundaryDraft};
use crate::prepared::{
    ApiManifest, ApiManifestEntry, ApiOperationSpec, AuthSpec, IoSpec, TransportSpec,
    API_MANIFEST_SCHEMA, OPERATION_SCHEMA_VERSION,
};
use crate::workspace::Workspace;
use anyhow::{Context, Result, bail};
use serde_yaml::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write as FmtWrite;
use std::fs;
use std::path::{Path, PathBuf};

/// Ingest all boundary drafts in `drafts/apis/`, or a specific one by name.
pub fn ingest_apis(workspace: &Workspace, api_name: Option<&str>, verbose: bool) -> Result<()> {
    let apis_dir = workspace.drafts_dir.join("apis");
    if !apis_dir.is_dir() {
        bail!("No drafts/apis/ directory found at {}", apis_dir.display());
    }

    let boundary_drafts = discover_boundary_drafts(&apis_dir, api_name)?;
    if boundary_drafts.is_empty() {
        if let Some(name) = api_name {
            bail!("No boundary draft found for api `{}`", name);
        } else {
            bail!("No boundary drafts found in {}", apis_dir.display());
        }
    }

    let mut manifest_entries = Vec::new();

    for (name, md_path) in &boundary_drafts {
        let content = fs::read_to_string(md_path)
            .with_context(|| format!("Failed to read {}", md_path.display()))?;
        let draft = parse_boundary_draft(name, &content)?;

        // Load the OpenAPI snapshot
        let yaml_path = apis_dir.join(format!("{}.yaml", name));
        if !yaml_path.is_file() {
            bail!(
                "OpenAPI snapshot not found: {} (expected alongside boundary draft {})",
                yaml_path.display(),
                md_path.display()
            );
        }
        let openapi_raw = fs::read_to_string(&yaml_path)
            .with_context(|| format!("Failed to read {}", yaml_path.display()))?;

        let entry = ingest_single_api(workspace, name, &draft, &openapi_raw, verbose)?;
        manifest_entries.push(entry);
    }

    // Write the unified manifest
    let manifest = ApiManifest {
        schema: API_MANIFEST_SCHEMA.to_string(),
        apis: manifest_entries,
    };
    let manifest_dir = workspace.drafts_dir.join(".apis");
    fs::create_dir_all(&manifest_dir)
        .with_context(|| format!("Failed to create {}", manifest_dir.display()))?;
    let manifest_yaml = serde_yaml::to_string(&manifest)?;
    fs::write(manifest_dir.join("manifest.yml"), manifest_yaml)?;

    if verbose {
        println!("API manifest written to drafts/.apis/manifest.yml");
    }

    Ok(())
}

/// Discover boundary drafts: `drafts/apis/<name>.md` files.
fn discover_boundary_drafts(
    apis_dir: &Path,
    filter_name: Option<&str>,
) -> Result<Vec<(String, PathBuf)>> {
    let mut drafts = Vec::new();
    for entry in fs::read_dir(apis_dir)
        .with_context(|| format!("read_dir {}", apis_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        if let Some(filter) = filter_name {
            if name != filter {
                continue;
            }
        }
        drafts.push((name, path));
    }
    // Sort for deterministic order
    drafts.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(drafts)
}

/// Ingest a single API from its boundary draft and OpenAPI snapshot.
fn ingest_single_api(
    workspace: &Workspace,
    api_name: &str,
    draft: &BoundaryDraft,
    openapi_raw: &str,
    verbose: bool,
) -> Result<ApiManifestEntry> {
    let openapi: Value = serde_yaml::from_str(openapi_raw)
        .context("Failed to parse OpenAPI YAML")?;

    // Validate basic OpenAPI shape
    validate_openapi_shape(&openapi)?;

    // Compute hash for determinism tracking
    let mut hasher = Sha256::new();
    hasher.update(openapi_raw.as_bytes());
    let openapi_hash = format!("{:x}", hasher.finalize());

    // Extract schemas from components/schemas
    let schemas = extract_schemas(&openapi)?;

    // Determine which operations to emit and which schemas they reference
    let (operations, referenced_types) =
        build_operations(api_name, draft, &openapi, &schemas)?;

    // Validate whitelist coherence: every whitelisted name must match something
    validate_whitelist(draft, &operations, &schemas)?;

    // Create output directories
    let data_dir = workspace
        .drafts_dir
        .join(".apis/data")
        .join(api_name);
    let ops_dir = workspace
        .drafts_dir
        .join(".apis/operations")
        .join(api_name);
    fs::create_dir_all(&data_dir)
        .with_context(|| format!("Failed to create {}", data_dir.display()))?;
    fs::create_dir_all(&ops_dir)
        .with_context(|| format!("Failed to create {}", ops_dir.display()))?;

    // Clean existing generated files for determinism
    clean_dir(&data_dir)?;
    clean_dir(&ops_dir)?;

    // Emit data drafts for referenced schemas
    let mut emitted_types: Vec<String> = Vec::new();
    for type_name in &referenced_types {
        if let Some(schema) = schemas.get(type_name.as_str()) {
            let md = generate_data_draft(api_name, type_name, schema);
            let path = data_dir.join(format!("{}.md", type_name));
            fs::write(&path, &md)
                .with_context(|| format!("Failed to write {}", path.display()))?;
            emitted_types.push(type_name.clone());
            if verbose {
                println!("  data: {}/{}", api_name, type_name);
            }
        }
    }

    // Emit operation specs
    let mut emitted_ops: Vec<String> = Vec::new();
    for op in &operations {
        let yaml = serde_yaml::to_string(op)?;
        let path = ops_dir.join(format!("{}.yml", op.name));
        fs::write(&path, &yaml)
            .with_context(|| format!("Failed to write {}", path.display()))?;
        emitted_ops.push(op.name.clone());
        if verbose {
            println!("  operation: {}/{}", api_name, op.name);
        }
    }

    Ok(ApiManifestEntry {
        name: api_name.to_string(),
        openapi_hash,
        data_types: emitted_types,
        operations: emitted_ops,
    })
}

fn validate_openapi_shape(openapi: &Value) -> Result<()> {
    let mapping = openapi
        .as_mapping()
        .ok_or_else(|| anyhow::anyhow!("OpenAPI document must be a YAML mapping"))?;

    // Check for openapi version field
    let version = mapping
        .get(Value::String("openapi".to_string()))
        .and_then(Value::as_str);
    match version {
        Some(v) if v.starts_with("3.") => {}
        Some(v) => bail!("Unsupported OpenAPI version: {}", v),
        None => bail!("Missing `openapi` version field in OpenAPI document"),
    }

    Ok(())
}

/// Extract all schemas from components/schemas as a BTreeMap for deterministic order.
fn extract_schemas(openapi: &Value) -> Result<BTreeMap<String, Value>> {
    let mut schemas = BTreeMap::new();
    let components = openapi
        .as_mapping()
        .and_then(|m| m.get(Value::String("components".to_string())))
        .and_then(Value::as_mapping);
    let schema_map = components
        .and_then(|c| c.get(Value::String("schemas".to_string())))
        .and_then(Value::as_mapping);
    if let Some(map) = schema_map {
        for (key, value) in map {
            if let Some(name) = key.as_str() {
                schemas.insert(name.to_string(), value.clone());
            }
        }
    }
    Ok(schemas)
}

/// Build operation specs from the OpenAPI document and boundary draft whitelist.
///
/// For AISStream and similar WebSocket APIs with no REST paths, we synthesize
/// operations from the schema types referenced in the whitelist.
fn build_operations(
    api_name: &str,
    draft: &BoundaryDraft,
    openapi: &Value,
    schemas: &BTreeMap<String, Value>,
) -> Result<(Vec<ApiOperationSpec>, Vec<String>)> {
    let mut operations = Vec::new();
    let mut referenced_types = BTreeMap::new(); // BTreeMap for deterministic order

    // Check if there are REST paths
    let paths = openapi
        .as_mapping()
        .and_then(|m| m.get(Value::String("paths".to_string())))
        .and_then(Value::as_mapping);

    let has_rest_paths = paths.is_some_and(|p| !p.is_empty());

    if has_rest_paths {
        // Standard REST API: build operations from paths
        if let Some(paths) = paths {
            build_rest_operations(
                api_name,
                draft,
                paths,
                schemas,
                &mut operations,
                &mut referenced_types,
            )?;
        }
    } else {
        // No REST paths — likely WebSocket or similar. Build from schemas + draft hints.
        build_schema_operations(
            api_name,
            draft,
            schemas,
            &mut operations,
            &mut referenced_types,
        )?;
    }

    let type_list: Vec<String> = referenced_types.into_keys().collect();
    Ok((operations, type_list))
}

fn build_rest_operations(
    api_name: &str,
    draft: &BoundaryDraft,
    paths: &serde_yaml::Mapping,
    schemas: &BTreeMap<String, Value>,
    operations: &mut Vec<ApiOperationSpec>,
    referenced_types: &mut BTreeMap<String, ()>,
) -> Result<()> {
    let whitelist = &draft.operations_whitelist;

    for (path_key, path_item) in paths {
        let path_str = path_key.as_str().unwrap_or_default();
        let Some(path_map) = path_item.as_mapping() else {
            continue;
        };
        for method in &["get", "post", "put", "delete", "patch"] {
            let Some(op_value) = path_map.get(Value::String(method.to_string())) else {
                continue;
            };
            let Some(op_map) = op_value.as_mapping() else {
                continue;
            };
            let op_id = op_map
                .get(Value::String("operationId".to_string()))
                .and_then(Value::as_str)
                .unwrap_or_default();

            // Apply whitelist
            if !whitelist.is_empty() && !whitelist.iter().any(|w| w == op_id) {
                continue;
            }

            let (input, input_types) = extract_rest_input(op_map, schemas);
            let (output, output_types) = extract_rest_output(op_map, schemas);

            for t in &input_types {
                referenced_types.insert(t.clone(), ());
                collect_transitive_refs(t, schemas, referenced_types);
            }
            for t in &output_types {
                referenced_types.insert(t.clone(), ());
                collect_transitive_refs(t, schemas, referenced_types);
            }

            operations.push(ApiOperationSpec {
                schema: OPERATION_SCHEMA_VERSION.to_string(),
                api: api_name.to_string(),
                name: op_id.to_string(),
                transport: TransportSpec {
                    kind: "rest".to_string(),
                    url: None,
                    method: Some(method.to_ascii_uppercase()),
                    path: Some(path_str.to_string()),
                },
                auth: AuthSpec {
                    kind: "none".to_string(),
                    field: None,
                },
                input,
                output,
                errors: Vec::new(),
                notes: Vec::new(),
            });
        }
    }
    Ok(())
}

fn build_schema_operations(
    api_name: &str,
    draft: &BoundaryDraft,
    schemas: &BTreeMap<String, Value>,
    operations: &mut Vec<ApiOperationSpec>,
    referenced_types: &mut BTreeMap<String, ()>,
) -> Result<()> {
    // For WebSocket APIs, look for a SubscriptionMessage schema and build
    // Subscribe + stream operations from the whitelist.

    // Detect auth from SubscriptionMessage schema
    let auth = if let Some(sub_schema) = schemas.get("SubscriptionMessage") {
        let has_api_key = sub_schema
            .as_mapping()
            .and_then(|m| m.get(Value::String("properties".to_string())))
            .and_then(Value::as_mapping)
            .is_some_and(|props| props.contains_key(Value::String("APIKey".to_string())));
        if has_api_key {
            AuthSpec {
                kind: "api_key_body".to_string(),
                field: Some("APIKey".to_string()),
            }
        } else {
            AuthSpec {
                kind: "none".to_string(),
                field: None,
            }
        }
    } else {
        AuthSpec {
            kind: "none".to_string(),
            field: None,
        }
    };

    // Detect WebSocket URL from draft notes
    let ws_url = draft
        .notes
        .iter()
        .find(|n| n.to_ascii_lowercase().contains("websocket") || n.to_ascii_lowercase().contains("wss://"))
        .cloned();

    // Add SubscriptionMessage as a referenced type if it exists
    if schemas.contains_key("SubscriptionMessage") {
        referenced_types.insert("SubscriptionMessage".to_string(), ());
        collect_transitive_refs("SubscriptionMessage", schemas, referenced_types);
    }

    // Build a Subscribe operation
    let subscribe_input = if schemas.contains_key("SubscriptionMessage") {
        IoSpec {
            kind: "data".to_string(),
            data_type: Some("SubscriptionMessage".to_string()),
        }
    } else {
        IoSpec {
            kind: "none".to_string(),
            data_type: None,
        }
    };

    // Determine the output envelope type
    let envelope_type = if schemas.contains_key("AisStreamMessage") {
        referenced_types.insert("AisStreamMessage".to_string(), ());
        collect_transitive_refs("AisStreamMessage", schemas, referenced_types);
        Some("AisStreamMessage".to_string())
    } else {
        None
    };

    // Collect all whitelisted schema types as referenced
    for type_name in &draft.operations_whitelist {
        if schemas.contains_key(type_name.as_str()) {
            referenced_types.insert(type_name.clone(), ());
            collect_transitive_refs(type_name, schemas, referenced_types);
        }
    }

    // Also collect AisMessageTypes enum if it exists
    if schemas.contains_key("AisMessageTypes") {
        referenced_types.insert("AisMessageTypes".to_string(), ());
    }

    // Collect BoundingBox and SubscriptionFilters if referenced
    for aux_type in &["BoundingBox", "SubscriptionFilters"] {
        if schemas.contains_key(*aux_type) {
            // Check if it's transitively referenced
            if referenced_types.contains_key(*aux_type) {
                // Already collected
            }
        }
    }

    let notes = draft.notes.clone();
    if let Some(url) = &ws_url {
        if !notes.iter().any(|n| n == url) {
            // URL is already in notes from draft
        }
    }

    // Build the Subscribe operation
    operations.push(ApiOperationSpec {
        schema: OPERATION_SCHEMA_VERSION.to_string(),
        api: api_name.to_string(),
        name: "Subscribe".to_string(),
        transport: TransportSpec {
            kind: "websocket".to_string(),
            url: Some("wss://stream.aisstream.io/v0/stream".to_string()),
            method: None,
            path: None,
        },
        auth: auth.clone(),
        input: subscribe_input,
        output: IoSpec {
            kind: "stream_of".to_string(),
            data_type: envelope_type,
        },
        errors: Vec::new(),
        notes,
    });

    Ok(())
}

fn extract_rest_input(
    op_map: &serde_yaml::Mapping,
    _schemas: &BTreeMap<String, Value>,
) -> (IoSpec, Vec<String>) {
    let mut types = Vec::new();
    let body = op_map
        .get(Value::String("requestBody".to_string()))
        .and_then(Value::as_mapping)
        .and_then(|rb| rb.get(Value::String("content".to_string())))
        .and_then(Value::as_mapping)
        .and_then(|content| content.get(Value::String("application/json".to_string())))
        .and_then(Value::as_mapping)
        .and_then(|media| media.get(Value::String("schema".to_string())))
        .and_then(Value::as_mapping);

    if let Some(schema) = body {
        if let Some(ref_str) = schema
            .get(Value::String("$ref".to_string()))
            .and_then(Value::as_str)
        {
            let type_name = ref_to_type_name(ref_str);
            types.push(type_name.clone());
            return (
                IoSpec {
                    kind: "data".to_string(),
                    data_type: Some(type_name),
                },
                types,
            );
        }
    }

    (
        IoSpec {
            kind: "none".to_string(),
            data_type: None,
        },
        types,
    )
}

fn extract_rest_output(
    op_map: &serde_yaml::Mapping,
    _schemas: &BTreeMap<String, Value>,
) -> (IoSpec, Vec<String>) {
    let mut types = Vec::new();
    let responses = op_map
        .get(Value::String("responses".to_string()))
        .and_then(Value::as_mapping);
    if let Some(responses) = responses {
        // Look at 200 or 201 response
        for code in &["200", "201"] {
            let response = responses
                .get(Value::String(code.to_string()))
                .and_then(Value::as_mapping);
            if let Some(resp) = response {
                let schema = resp
                    .get(Value::String("content".to_string()))
                    .and_then(Value::as_mapping)
                    .and_then(|c| c.get(Value::String("application/json".to_string())))
                    .and_then(Value::as_mapping)
                    .and_then(|m| m.get(Value::String("schema".to_string())))
                    .and_then(Value::as_mapping);
                if let Some(schema) = schema {
                    if let Some(ref_str) = schema
                        .get(Value::String("$ref".to_string()))
                        .and_then(Value::as_str)
                    {
                        let type_name = ref_to_type_name(ref_str);
                        types.push(type_name.clone());
                        return (
                            IoSpec {
                                kind: "single".to_string(),
                                data_type: Some(type_name),
                            },
                            types,
                        );
                    }
                }
            }
        }
    }
    (
        IoSpec {
            kind: "none".to_string(),
            data_type: None,
        },
        types,
    )
}

/// Extract type name from a `$ref` like `#/components/schemas/TypeName`.
fn ref_to_type_name(ref_str: &str) -> String {
    ref_str.rsplit('/').next().unwrap_or(ref_str).to_string()
}

/// Collect all schema types transitively referenced by a given type.
fn collect_transitive_refs(
    type_name: &str,
    schemas: &BTreeMap<String, Value>,
    collected: &mut BTreeMap<String, ()>,
) {
    let Some(schema) = schemas.get(type_name) else {
        return;
    };
    // Scan for $ref values
    let refs = find_all_refs(schema);
    for ref_name in refs {
        if !collected.contains_key(&ref_name) {
            collected.insert(ref_name.clone(), ());
            collect_transitive_refs(&ref_name, schemas, collected);
        }
    }
}

/// Find all `$ref` type names in a YAML value tree.
fn find_all_refs(value: &Value) -> Vec<String> {
    let mut refs = Vec::new();
    match value {
        Value::Mapping(map) => {
            if let Some(ref_val) = map.get(Value::String("$ref".to_string())) {
                if let Some(ref_str) = ref_val.as_str() {
                    refs.push(ref_to_type_name(ref_str));
                }
            }
            for (_, v) in map {
                refs.extend(find_all_refs(v));
            }
        }
        Value::Sequence(seq) => {
            for v in seq {
                refs.extend(find_all_refs(v));
            }
        }
        _ => {}
    }
    refs
}

/// Validate that every whitelisted operation name matches something in the spec.
fn validate_whitelist(
    draft: &BoundaryDraft,
    operations: &[ApiOperationSpec],
    schemas: &BTreeMap<String, Value>,
) -> Result<()> {
    for name in &draft.operations_whitelist {
        let in_ops = operations.iter().any(|op| op.name == *name);
        let in_schemas = schemas.contains_key(name.as_str());
        if !in_ops && !in_schemas {
            bail!(
                "Boundary draft for `{}` whitelists `{}` which is not in the OpenAPI spec \
                 (not found as an operationId or schema name)",
                draft.api_name,
                name
            );
        }
    }
    Ok(())
}

/// Generate a data draft markdown for an ingested OpenAPI schema.
fn generate_data_draft(api_name: &str, type_name: &str, schema: &Value) -> String {
    let mut out = String::new();

    writeln!(out, "# Data: {}", type_name).unwrap();
    writeln!(out).unwrap();

    // Purpose cites the OpenAPI $ref path
    writeln!(out, "## Purpose").unwrap();
    writeln!(
        out,
        "Ingested from the {} API. OpenAPI schema reference: #/components/schemas/{}",
        api_name, type_name
    )
    .unwrap();
    writeln!(out).unwrap();

    // Attributes from properties
    writeln!(out, "## Attributes").unwrap();
    let properties = schema
        .as_mapping()
        .and_then(|m| m.get(Value::String("properties".to_string())))
        .and_then(Value::as_mapping);

    // Use BTreeMap to ensure deterministic order of properties
    let mut sorted_props: BTreeMap<String, &Value> = BTreeMap::new();
    if let Some(props) = properties {
        for (key, val) in props {
            if let Some(name) = key.as_str() {
                sorted_props.insert(name.to_string(), val);
            }
        }
    }

    // Also handle enum types
    let enum_values = schema
        .as_mapping()
        .and_then(|m| m.get(Value::String("enum".to_string())))
        .and_then(Value::as_sequence);

    if let Some(enum_vals) = enum_values {
        let type_str = schema
            .as_mapping()
            .and_then(|m| m.get(Value::String("type".to_string())))
            .and_then(Value::as_str)
            .unwrap_or("string");
        writeln!(
            out,
            "- value: one of {} ({})",
            enum_vals
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", "),
            type_str
        )
        .unwrap();
    } else if sorted_props.is_empty() {
        // oneOf or other complex schema
        let has_one_of = schema
            .as_mapping()
            .and_then(|m| m.get(Value::String("oneOf".to_string())))
            .and_then(Value::as_sequence);
        if let Some(one_of) = has_one_of {
            let variants: Vec<String> = one_of
                .iter()
                .filter_map(|v| {
                    v.as_mapping()
                        .and_then(|m| m.get(Value::String("$ref".to_string())))
                        .and_then(Value::as_str)
                        .map(ref_to_type_name)
                })
                .collect();
            writeln!(out, "- variant: one of {}", variants.join(", ")).unwrap();
        }

        // additionalProperties
        let additional = schema
            .as_mapping()
            .and_then(|m| m.get(Value::String("additionalProperties".to_string())));
        if additional.is_some() {
            writeln!(out, "- content: arbitrary key-value pairs").unwrap();
        }
    } else {
        for (name, val) in &sorted_props {
            let type_str = property_description(val);
            writeln!(out, "- {}: {}", name, type_str).unwrap();
        }
    }
    writeln!(out).unwrap();

    // Invariants from constraints
    writeln!(out, "## Invariants").unwrap();
    let required = schema
        .as_mapping()
        .and_then(|m| m.get(Value::String("required".to_string())))
        .and_then(Value::as_sequence);
    if let Some(req) = required {
        let names: Vec<&str> = req.iter().filter_map(Value::as_str).collect();
        if !names.is_empty() {
            writeln!(out, "- required fields: {}", names.join(", ")).unwrap();
        }
    }
    if let Some(enum_vals) = enum_values {
        let vals: Vec<&str> = enum_vals.iter().filter_map(Value::as_str).collect();
        if !vals.is_empty() {
            writeln!(out, "- value must be one of: {}", vals.join(", ")).unwrap();
        }
    }
    writeln!(out).unwrap();

    // Empty Operations and Scenarios (ingested data is vocabulary, not behavioural)
    writeln!(out, "## Operations").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "## Scenarios").unwrap();

    out
}

/// Describe a property for the data draft attributes section.
fn property_description(val: &Value) -> String {
    let map = match val.as_mapping() {
        Some(m) => m,
        None => return "unknown".to_string(),
    };
    let type_str = map
        .get(Value::String("type".to_string()))
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    // Check for $ref
    if let Some(ref_str) = map
        .get(Value::String("$ref".to_string()))
        .and_then(Value::as_str)
    {
        return format!("reference to {}", ref_to_type_name(ref_str));
    }

    // Check for array with items
    if type_str == "array" {
        let items_ref = map
            .get(Value::String("items".to_string()))
            .and_then(Value::as_mapping)
            .and_then(|m| m.get(Value::String("$ref".to_string())))
            .and_then(Value::as_str);
        if let Some(ref_str) = items_ref {
            return format!("list of {}", ref_to_type_name(ref_str));
        }
        return "list".to_string();
    }

    // Check for oneOf
    if let Some(one_of) = map
        .get(Value::String("oneOf".to_string()))
        .and_then(Value::as_sequence)
    {
        let variants: Vec<String> = one_of
            .iter()
            .filter_map(|v| {
                v.as_mapping()
                    .and_then(|m| m.get(Value::String("$ref".to_string())))
                    .and_then(Value::as_str)
                    .map(ref_to_type_name)
            })
            .collect();
        if !variants.is_empty() {
            return format!("one of {}", variants.join(", "));
        }
    }

    // Check for additionalProperties (generic object)
    if type_str == "object" {
        if map.contains_key(Value::String("additionalProperties".to_string())) {
            return "arbitrary key-value pairs".to_string();
        }
        return "structured object".to_string();
    }

    type_str.to_string()
}

/// Remove all files in a directory (but not subdirectories).
fn clean_dir(dir: &Path) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(prefix: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("reen_ingest_{prefix}_{stamp}"))
    }

    fn setup_api_project(root: &Path, boundary_md: &str, openapi_yaml: &str) {
        let apis_dir = root.join("drafts/apis");
        fs::create_dir_all(&apis_dir).unwrap();
        fs::write(apis_dir.join("testapi.md"), boundary_md).unwrap();
        fs::write(apis_dir.join("testapi.yaml"), openapi_yaml).unwrap();
    }

    const MINIMAL_OPENAPI: &str = r#"openapi: "3.1.0"
info:
  title: Test API
  version: "1.0.0"
paths: {}
components:
  schemas:
    Widget:
      type: object
      properties:
        Name:
          type: string
        Size:
          type: integer
      required:
        - Name
"#;

    const MINIMAL_BOUNDARY: &str = r#"# Test API

## Purpose
A test API for unit testing.

## Source
- OpenAPI Local: ./testapi.yaml

## Operations
- Widget

## Notes
- This is a test
"#;

    #[test]
    fn ingest_produces_deterministic_output() {
        let root = temp_root("determinism");
        setup_api_project(&root, MINIMAL_BOUNDARY, MINIMAL_OPENAPI);
        let workspace = Workspace::discover(root.clone()).unwrap();

        // First ingest
        ingest_apis(&workspace, Some("testapi"), false).unwrap();
        let manifest1 = fs::read_to_string(root.join("drafts/.apis/manifest.yml")).unwrap();
        let data1 = fs::read_to_string(root.join("drafts/.apis/data/testapi/Widget.md")).unwrap();

        // Second ingest — must be byte-identical
        ingest_apis(&workspace, Some("testapi"), false).unwrap();
        let manifest2 = fs::read_to_string(root.join("drafts/.apis/manifest.yml")).unwrap();
        let data2 = fs::read_to_string(root.join("drafts/.apis/data/testapi/Widget.md")).unwrap();

        assert_eq!(manifest1, manifest2, "manifest must be deterministic");
        assert_eq!(data1, data2, "data draft must be deterministic");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ingest_creates_expected_files() {
        let root = temp_root("files");
        setup_api_project(&root, MINIMAL_BOUNDARY, MINIMAL_OPENAPI);
        let workspace = Workspace::discover(root.clone()).unwrap();

        ingest_apis(&workspace, Some("testapi"), false).unwrap();

        assert!(root.join("drafts/.apis/manifest.yml").is_file());
        assert!(root.join("drafts/.apis/data/testapi/Widget.md").is_file());

        // Check data draft content
        let content = fs::read_to_string(root.join("drafts/.apis/data/testapi/Widget.md")).unwrap();
        assert!(content.contains("# Data: Widget"));
        assert!(content.contains("## Purpose"));
        assert!(content.contains("#/components/schemas/Widget"));
        assert!(content.contains("## Attributes"));
        assert!(content.contains("Name:"));
        assert!(content.contains("## Invariants"));
        assert!(content.contains("required fields: Name"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ingest_fails_on_missing_openapi() {
        let root = temp_root("missing_yaml");
        let apis_dir = root.join("drafts/apis");
        fs::create_dir_all(&apis_dir).unwrap();
        fs::write(apis_dir.join("testapi.md"), MINIMAL_BOUNDARY).unwrap();
        // No .yaml file
        let workspace = Workspace::discover(root.clone()).unwrap();
        let result = ingest_apis(&workspace, Some("testapi"), false);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("snapshot not found"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ingest_fails_on_bad_whitelist() {
        let root = temp_root("bad_whitelist");
        let boundary = r#"# Test API

## Purpose
A test API.

## Source
- OpenAPI Local: ./testapi.yaml

## Operations
- NonExistentType

## Notes
- test
"#;
        setup_api_project(&root, boundary, MINIMAL_OPENAPI);
        let workspace = Workspace::discover(root.clone()).unwrap();
        let result = ingest_apis(&workspace, Some("testapi"), false);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("NonExistentType"));
        let _ = fs::remove_dir_all(root);
    }
}
