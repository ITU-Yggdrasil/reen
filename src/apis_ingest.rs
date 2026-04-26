//! API ingestion (Phase A).
//!
//! Reads `drafts/apis/<name>.md` (boundary draft, config-shaped) and
//! `drafts/apis/<name>.yaml` (OpenAPI sidecar), then emits:
//!
//!   * `drafts/data/.api/<Type>.md` — one per schema referenced by an
//!     included operation.
//!   * `drafts/projections/.api/<operation>.md` — one per included
//!     operation.
//!
//! Each emitted draft is auto-generated and carries a three-line header:
//! the OpenAPI source pointer, a `DO NOT EDIT` line, and a
//! `<source-hash>` of the OpenAPI fragment that produced it. `reen
//! refine` uses the hash to detect hand-edits.
//!
//! Hard-regenerate semantics: every emit overwrites whatever was there.
//! The `.api/` subfolders are cleaned before emission so a removed
//! operation or schema doesn't leave a stale draft behind.

use crate::draft_parser::parse_boundary_draft;
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

    let data_api_dir = workspace.drafts_dir.join("data").join(".api");
    let proj_api_dir = workspace.drafts_dir.join("projections").join(".api");
    fs::create_dir_all(&data_api_dir)
        .with_context(|| format!("Failed to create {}", data_api_dir.display()))?;
    fs::create_dir_all(&proj_api_dir)
        .with_context(|| format!("Failed to create {}", proj_api_dir.display()))?;

    // Hard-regenerate: clean the `.api/` subfolders before emitting so a
    // removed operation or schema doesn't leave a stale draft on disk.
    clean_api_subfolder(&data_api_dir)?;
    clean_api_subfolder(&proj_api_dir)?;

    for (name, md_path) in &boundary_drafts {
        let content = fs::read_to_string(md_path)
            .with_context(|| format!("Failed to read {}", md_path.display()))?;
        let draft = parse_boundary_draft(name, &content)?;

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
        let openapi: Value = serde_yaml::from_str(&openapi_raw)
            .context("Failed to parse OpenAPI YAML")?;

        emit_drafts(
            name,
            &draft.operations_whitelist,
            &openapi,
            &data_api_dir,
            &proj_api_dir,
            verbose,
        )?;
    }

    Ok(())
}

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
    drafts.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(drafts)
}

/// Clean any pre-existing `.md` files under an `.api/` subfolder.
fn clean_api_subfolder(dir: &Path) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            fs::remove_file(&path)
                .with_context(|| format!("Failed to remove {}", path.display()))?;
        }
    }
    Ok(())
}

// ── Emission ─────────────────────────────────────────────────────────

fn emit_drafts(
    api_name: &str,
    whitelist: &[String],
    openapi: &Value,
    data_dir: &Path,
    proj_dir: &Path,
    verbose: bool,
) -> Result<()> {
    let schemas = extract_schemas(openapi);
    let operations = extract_operations(openapi);

    // Determine which operations to emit.
    let included: Vec<&Operation> = if whitelist.is_empty() {
        operations.iter().collect()
    } else {
        operations
            .iter()
            .filter(|op| whitelist.iter().any(|w| w == &op.operation_id))
            .collect()
    };
    if included.is_empty() {
        bail!(
            "{}: no operations matched the whitelist {:?}",
            api_name,
            whitelist,
        );
    }

    // Walk every included operation to collect referenced schemas (transitively).
    let mut referenced: BTreeMap<String, ()> = BTreeMap::new();
    for op in &included {
        for sch in op.referenced_schemas() {
            if !referenced.contains_key(&sch) {
                referenced.insert(sch.clone(), ());
                collect_transitive_refs(&sch, &schemas, &mut referenced);
            }
        }
    }

    // Emit data drafts.
    for type_name in referenced.keys() {
        let Some(schema) = schemas.get(type_name) else {
            continue;
        };
        let path = data_dir.join(format!("{}.md", type_name));
        let body = render_data_draft(api_name, type_name, schema);
        fs::write(&path, &body)
            .with_context(|| format!("Failed to write {}", path.display()))?;
        if verbose {
            println!("  emitted: {}", relative_to_workspace(&path));
        }
    }

    // Emit projection drafts.
    for op in &included {
        let snake = to_snake_case(&op.operation_id);
        let path = proj_dir.join(format!("{}.md", snake));
        let body = render_projection_draft(api_name, op, &schemas);
        fs::write(&path, &body)
            .with_context(|| format!("Failed to write {}", path.display()))?;
        if verbose {
            println!("  emitted: {}", relative_to_workspace(&path));
        }
    }

    if verbose {
        println!(
            "API `{}`: emitted {} data draft(s) and {} projection draft(s)",
            api_name,
            referenced.len(),
            included.len(),
        );
    }
    Ok(())
}

fn relative_to_workspace(path: &Path) -> String {
    // Best-effort relative path for verbose output.
    let cwd = std::env::current_dir().unwrap_or_default();
    path.strip_prefix(&cwd)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

// ── Data draft rendering ─────────────────────────────────────────────

fn render_data_draft(api_name: &str, type_name: &str, schema: &Value) -> String {
    let mut out = String::new();
    let openapi_ref = format!(
        "drafts/apis/{}.yaml#components/schemas/{}",
        api_name, type_name,
    );
    let hash = stable_hash(schema);
    writeln!(out, "<!-- reen:auto-generated from {} -->", openapi_ref).unwrap();
    writeln!(out, "<!-- DO NOT EDIT — regenerated by `reen apis ingest`. -->").unwrap();
    writeln!(out, "<!-- source-hash: {} -->", hash).unwrap();
    writeln!(out).unwrap();
    writeln!(out, "# Data: {}", type_name).unwrap();
    writeln!(out).unwrap();
    writeln!(out, "## Purpose").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "Wire shape of the {0} schema as exposed by the {1} API. \
         Domain rules attach in BA-authored contexts that consume this type — \
         this draft carries only the OpenAPI shape.",
        type_name, api_name,
    )
    .unwrap();
    writeln!(out).unwrap();

    writeln!(out, "## Attributes").unwrap();
    writeln!(out).unwrap();
    let mut sorted_props: BTreeMap<String, &Value> = BTreeMap::new();
    if let Some(props) = schema
        .as_mapping()
        .and_then(|m| m.get(Value::String("properties".to_string())))
        .and_then(Value::as_mapping)
    {
        for (key, val) in props {
            if let Some(name) = key.as_str() {
                sorted_props.insert(name.to_string(), val);
            }
        }
    }
    let enum_values = schema
        .as_mapping()
        .and_then(|m| m.get(Value::String("enum".to_string())))
        .and_then(Value::as_sequence);
    if let Some(values) = enum_values {
        let listed: Vec<&str> = values.iter().filter_map(Value::as_str).collect();
        writeln!(
            out,
            "- value: enumerated text — one of {}",
            listed
                .iter()
                .map(|s| format!("\"{}\"", s))
                .collect::<Vec<_>>()
                .join(", "),
        )
        .unwrap();
    } else if sorted_props.is_empty() {
        // oneOf or unmodelled — emit a placeholder so prepare doesn't bail.
        writeln!(out, "- value: an opaque value (schema has no declared properties)").unwrap();
    } else {
        for (name, val) in &sorted_props {
            let ba_name = camel_to_ba(name);
            writeln!(out, "- {}: {}", ba_name, attribute_description(val)).unwrap();
        }
    }
    writeln!(out).unwrap();

    writeln!(out, "## Invariants").unwrap();
    writeln!(out).unwrap();
    let required = schema
        .as_mapping()
        .and_then(|m| m.get(Value::String("required".to_string())))
        .and_then(Value::as_sequence);
    if let Some(req) = required {
        for name in req.iter().filter_map(Value::as_str) {
            let ba_name = camel_to_ba(name);
            writeln!(out, "- {} must be present", ba_name).unwrap();
        }
    }
    if let Some(values) = enum_values {
        let listed: Vec<&str> = values.iter().filter_map(Value::as_str).collect();
        if !listed.is_empty() {
            writeln!(
                out,
                "- value must be one of {}",
                listed
                    .iter()
                    .map(|s| format!("\"{}\"", s))
                    .collect::<Vec<_>>()
                    .join(", "),
            )
            .unwrap();
        }
    }
    writeln!(out).unwrap();
    out
}

fn attribute_description(val: &Value) -> String {
    let map = match val.as_mapping() {
        Some(m) => m,
        None => return "an opaque value".into(),
    };
    let ty = map
        .get(Value::String("type".to_string()))
        .and_then(Value::as_str)
        .unwrap_or("");
    let format = map
        .get(Value::String("format".to_string()))
        .and_then(Value::as_str);
    let reference = map
        .get(Value::String("$ref".to_string()))
        .and_then(Value::as_str)
        .map(ref_to_type_name);

    if let Some(name) = reference {
        return format!("a value of {}", name);
    }
    match ty {
        "string" => match format {
            Some("date") | Some("date-time") => "a date-and-time value".into(),
            Some(other) => format!("a text value (format `{}`)", other),
            None => "a text value".into(),
        },
        "integer" => "a whole number".into(),
        "number" => "a numeric value".into(),
        "boolean" => "a boolean value (true or false)".into(),
        "array" => {
            let item_ty = map
                .get(Value::String("items".to_string()))
                .map(attribute_description)
                .unwrap_or_else(|| "values".into());
            format!("a list of {}", item_ty.trim_start_matches("a ").trim_start_matches("an "))
        }
        "" => "an opaque value".into(),
        other => format!("a value of OpenAPI type `{}`", other),
    }
}

// ── Projection draft rendering ───────────────────────────────────────

fn render_projection_draft(api_name: &str, op: &Operation, schemas: &BTreeMap<String, Value>) -> String {
    let mut out = String::new();
    let openapi_ref = format!(
        "drafts/apis/{}.yaml#operations/{}",
        api_name, op.operation_id,
    );
    let hash = stable_hash(&op.fragment);
    writeln!(out, "<!-- reen:auto-generated from {} -->", openapi_ref).unwrap();
    writeln!(out, "<!-- DO NOT EDIT — regenerated by `reen apis ingest`. -->").unwrap();
    writeln!(out, "<!-- source-hash: {} -->", hash).unwrap();
    writeln!(out).unwrap();
    writeln!(out, "# Projection: {}", op.operation_id).unwrap();
    writeln!(out).unwrap();
    writeln!(out, "## Purpose").unwrap();
    writeln!(out).unwrap();
    let summary = op.summary.as_deref().unwrap_or(&op.operation_id);
    writeln!(
        out,
        "{}. Wire-level projection over the {} API; domain rules attach in \
         BA-authored contexts that compose this projection.",
        summary, api_name,
    )
    .unwrap();
    writeln!(out).unwrap();

    // Structured Source block — the canonical Phase A shape that signature
    // inference parses to derive the projection's signature.
    writeln!(out, "## Source").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "- OpenAPI: {}#operations/{}", api_name, op.operation_id).unwrap();
    writeln!(out, "- Method: {} {}", op.method, op.path).unwrap();
    writeln!(out, "- Parameters:").unwrap();
    if op.parameters.is_empty() {
        writeln!(out, "  - (none)").unwrap();
    } else {
        for p in &op.parameters {
            let req = if p.required { ", required" } else { "" };
            writeln!(
                out,
                "  - {} ({}{}): {}",
                p.name, p.location, req, p.rust_type,
            )
            .unwrap();
        }
    }
    writeln!(out, "- Returns: {}", op.return_type(schemas)).unwrap();
    writeln!(out).unwrap();

    // Role Players: one role player carrying the response shape (when the
    // operation returns data).
    writeln!(out, "## Role Players").unwrap();
    writeln!(out).unwrap();
    if let Some(role) = op.response_role_player(schemas) {
        writeln!(
            out,
            "- {}: {} — {}",
            role.name, role.data_type, role.responsibility,
        )
        .unwrap();
    } else {
        writeln!(out, "- response: () — the operation returns no data").unwrap();
    }
    writeln!(out).unwrap();

    // Functionalities: one per operation, named `call` (deterministic).
    writeln!(out, "## Functionalities").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "### call").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "Invokes the {} operation against the {} API and returns the response shape \
         declared in `## Source`. The implementer wires the HTTP transport via the \
         project's ambient client; this projection's contract is the signature.",
        op.operation_id, api_name,
    )
    .unwrap();
    writeln!(out).unwrap();
    out
}

struct ResponseRole {
    name: String,
    data_type: String,
    responsibility: String,
}

// ── OpenAPI domain model ─────────────────────────────────────────────

struct Operation {
    operation_id: String,
    summary: Option<String>,
    method: String,
    path: String,
    parameters: Vec<OpParam>,
    /// The 200-response schema (a `$ref` target, or `None` for no body).
    response_schema: Option<String>,
    /// Whether the response body is an array wrapping `response_schema`.
    response_is_list: bool,
    /// Verbatim YAML fragment for hashing.
    fragment: Value,
}

struct OpParam {
    name: String,
    location: String,
    required: bool,
    rust_type: String,
}

impl Operation {
    fn referenced_schemas(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for r in find_all_refs(&self.fragment) {
            if !out.contains(&r) {
                out.push(r);
            }
        }
        out
    }

    fn return_type(&self, schemas: &BTreeMap<String, Value>) -> String {
        match (&self.response_schema, self.response_is_list) {
            (Some(schema), true) => {
                // If the schema itself wraps an `array` field, that's the list.
                // Otherwise the response_is_list flag captures the array nature.
                format!("Vec<{}>", primary_payload(schema, schemas).unwrap_or_else(|| schema.clone()))
            }
            (Some(schema), false) => primary_payload(schema, schemas).unwrap_or_else(|| schema.clone()),
            (None, _) => "()".to_string(),
        }
    }

    fn response_role_player(&self, schemas: &BTreeMap<String, Value>) -> Option<ResponseRole> {
        let schema = self.response_schema.as_ref()?;
        let payload = primary_payload(schema, schemas).unwrap_or_else(|| schema.clone());
        let (role_name, responsibility) = if self.response_is_list {
            (
                snake_plural(&payload),
                format!("the {} list returned by the API", payload),
            )
        } else {
            (
                to_snake_case(&payload),
                format!("the {} returned by the API", payload),
            )
        };
        Some(ResponseRole {
            name: role_name,
            data_type: payload,
            responsibility,
        })
    }
}

/// If `schema` is an envelope that wraps a single payload (e.g.
/// `{ response: { accounts: array<AccountInfo> } }` or
/// `{ response: $ref: AccountInfo }`), return the payload's type name.
/// Otherwise return None and callers fall back to the envelope name.
fn primary_payload(schema_name: &str, schemas: &BTreeMap<String, Value>) -> Option<String> {
    let schema = schemas.get(schema_name)?;
    let mapping = schema.as_mapping()?;
    let response_field = mapping.get(Value::String("response".to_string()))?;

    // Direct $ref under `response`.
    if let Some(rf) = response_field
        .as_mapping()
        .and_then(|m| m.get(Value::String("$ref".to_string())))
        .and_then(Value::as_str)
    {
        return Some(ref_to_type_name(rf));
    }
    // `response` is itself an object whose first property is the payload
    // (e.g. `accounts: array<AccountInfo>`).
    let inner = response_field.as_mapping()?;
    let props = inner
        .get(Value::String("properties".to_string()))?
        .as_mapping()?;
    for (_, val) in props {
        let m = val.as_mapping()?;
        if let Some(items) = m.get(Value::String("items".to_string())) {
            if let Some(rf) = items
                .as_mapping()
                .and_then(|im| im.get(Value::String("$ref".to_string())))
                .and_then(Value::as_str)
            {
                return Some(ref_to_type_name(rf));
            }
        }
        if let Some(rf) = m
            .get(Value::String("$ref".to_string()))
            .and_then(Value::as_str)
        {
            return Some(ref_to_type_name(rf));
        }
    }
    None
}

fn extract_schemas(openapi: &Value) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    if let Some(components) = openapi
        .as_mapping()
        .and_then(|m| m.get(Value::String("components".to_string())))
        .and_then(Value::as_mapping)
    {
        if let Some(schemas) = components
            .get(Value::String("schemas".to_string()))
            .and_then(Value::as_mapping)
        {
            for (k, v) in schemas {
                if let Some(name) = k.as_str() {
                    out.insert(name.to_string(), v.clone());
                }
            }
        }
    }
    out
}

fn extract_operations(openapi: &Value) -> Vec<Operation> {
    let mut out = Vec::new();
    let Some(paths) = openapi
        .as_mapping()
        .and_then(|m| m.get(Value::String("paths".to_string())))
        .and_then(Value::as_mapping)
    else {
        return out;
    };
    for (path_key, path_item) in paths {
        let path_str = path_key.as_str().unwrap_or_default().to_string();
        let Some(path_map) = path_item.as_mapping() else {
            continue;
        };
        for method in &["get", "post", "put", "delete", "patch"] {
            let Some(op_value) = path_map.get(Value::String((*method).to_string())) else {
                continue;
            };
            let Some(op_map) = op_value.as_mapping() else {
                continue;
            };
            let operation_id = op_map
                .get(Value::String("operationId".to_string()))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if operation_id.is_empty() {
                continue;
            }
            let summary = op_map
                .get(Value::String("summary".to_string()))
                .and_then(Value::as_str)
                .map(str::to_string);
            let parameters = extract_op_parameters(op_map);
            let (response_schema, response_is_list) = extract_op_response(op_map);
            out.push(Operation {
                operation_id,
                summary,
                method: method.to_uppercase(),
                path: path_str.clone(),
                parameters,
                response_schema,
                response_is_list,
                fragment: op_value.clone(),
            });
        }
    }
    out
}

fn extract_op_parameters(op_map: &serde_yaml::Mapping) -> Vec<OpParam> {
    let mut out = Vec::new();
    let Some(params) = op_map
        .get(Value::String("parameters".to_string()))
        .and_then(Value::as_sequence)
    else {
        return out;
    };
    for entry in params {
        let Some(m) = entry.as_mapping() else { continue };
        let name = m
            .get(Value::String("name".to_string()))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }
        let location = m
            .get(Value::String("in".to_string()))
            .and_then(Value::as_str)
            .unwrap_or("query")
            .to_string();
        let required = m
            .get(Value::String("required".to_string()))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let schema = m
            .get(Value::String("schema".to_string()))
            .cloned()
            .unwrap_or(Value::Null);
        let rust_type = openapi_to_rust(&schema, true);
        // Header parameters (X-IBM-Client-Id, etc.) are infrastructure
        // concerns; the projection signature exposes path + query params
        // only. Headers stay implicit (handled by the ambient HTTP client).
        if location == "header" {
            continue;
        }
        out.push(OpParam {
            name: snake_case_param(&name),
            location,
            required,
            rust_type,
        });
    }
    out
}

fn extract_op_response(op_map: &serde_yaml::Mapping) -> (Option<String>, bool) {
    let Some(responses) = op_map
        .get(Value::String("responses".to_string()))
        .and_then(Value::as_mapping)
    else {
        return (None, false);
    };
    // Prefer 200; fall back to the first 2xx.
    let mut chosen: Option<&Value> = responses
        .get(Value::String("200".to_string()));
    if chosen.is_none() {
        for (k, v) in responses {
            if let Some(code) = k.as_str() {
                if code.starts_with('2') {
                    chosen = Some(v);
                    break;
                }
            }
        }
    }
    let Some(resp) = chosen else { return (None, false) };
    let Some(content) = resp
        .as_mapping()
        .and_then(|m| m.get(Value::String("content".to_string())))
        .and_then(Value::as_mapping)
    else {
        return (None, false);
    };
    // Take the first content entry's schema.
    for (_, ct_val) in content {
        if let Some(schema) = ct_val
            .as_mapping()
            .and_then(|m| m.get(Value::String("schema".to_string())))
        {
            // Direct $ref.
            if let Some(rf) = schema
                .as_mapping()
                .and_then(|m| m.get(Value::String("$ref".to_string())))
                .and_then(Value::as_str)
            {
                return (Some(ref_to_type_name(rf)), false);
            }
            // Array of $ref.
            if schema
                .as_mapping()
                .and_then(|m| m.get(Value::String("type".to_string())))
                .and_then(Value::as_str)
                == Some("array")
            {
                if let Some(rf) = schema
                    .as_mapping()
                    .and_then(|m| m.get(Value::String("items".to_string())))
                    .and_then(|i| i.as_mapping())
                    .and_then(|im| im.get(Value::String("$ref".to_string())))
                    .and_then(Value::as_str)
                {
                    return (Some(ref_to_type_name(rf)), true);
                }
            }
        }
    }
    (None, false)
}

fn openapi_to_rust(schema: &Value, by_ref: bool) -> String {
    let Some(map) = schema.as_mapping() else {
        return "()".into();
    };
    if let Some(rf) = map
        .get(Value::String("$ref".to_string()))
        .and_then(Value::as_str)
    {
        let name = ref_to_type_name(rf);
        return if by_ref { format!("&{}", name) } else { name };
    }
    let ty = map
        .get(Value::String("type".to_string()))
        .and_then(Value::as_str)
        .unwrap_or("");
    match ty {
        "string" => if by_ref { "&str".into() } else { "String".into() },
        "integer" => "i64".into(),
        "number" => "f64".into(),
        "boolean" => "bool".into(),
        "array" => {
            let item = map
                .get(Value::String("items".to_string()))
                .map(|v| openapi_to_rust(v, false))
                .unwrap_or_else(|| "()".into());
            format!("Vec<{}>", item)
        }
        _ => "()".into(),
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

fn ref_to_type_name(ref_str: &str) -> String {
    ref_str.rsplit('/').next().unwrap_or(ref_str).to_string()
}

fn collect_transitive_refs(
    type_name: &str,
    schemas: &BTreeMap<String, Value>,
    collected: &mut BTreeMap<String, ()>,
) {
    let Some(schema) = schemas.get(type_name) else {
        return;
    };
    for r in find_all_refs(schema) {
        if !collected.contains_key(&r) {
            collected.insert(r.clone(), ());
            collect_transitive_refs(&r, schemas, collected);
        }
    }
}

fn find_all_refs(value: &Value) -> Vec<String> {
    let mut refs = Vec::new();
    match value {
        Value::Mapping(map) => {
            if let Some(rf) = map
                .get(Value::String("$ref".to_string()))
                .and_then(Value::as_str)
            {
                refs.push(ref_to_type_name(rf));
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

/// Convert a YAML `Value` to a stable byte string and hash it. The
/// canonical serialiser sorts mapping keys, so the hash is reproducible
/// across runs even if the source YAML's key order changes.
fn stable_hash(value: &Value) -> String {
    let yaml = serde_yaml::to_string(value).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(yaml.as_bytes());
    let bytes = hasher.finalize();
    // Hex-encode the first 12 bytes for compactness.
    let mut out = String::with_capacity(24);
    for b in &bytes[..12] {
        write!(out, "{:02x}", b).unwrap();
    }
    out
}

fn camel_to_ba(name: &str) -> String {
    // `holderName` → `holder name`; `_id` → `id`; `account_type` → `account type`.
    let trimmed = name.trim_start_matches('_');
    let mut out = String::with_capacity(name.len() + 4);
    let mut prev_lower = false;
    for ch in trimmed.chars() {
        if ch == '_' || ch == '-' {
            out.push(' ');
            prev_lower = false;
            continue;
        }
        if ch.is_ascii_uppercase() {
            if prev_lower {
                out.push(' ');
            }
            out.push(ch.to_ascii_lowercase());
            prev_lower = false;
        } else {
            out.push(ch);
            prev_lower = ch.is_ascii_lowercase();
        }
    }
    out
}

fn to_snake_case(name: &str) -> String {
    crate::codegen::to_snake_case(name)
}

fn snake_case_param(name: &str) -> String {
    // Path/query params: keep the OpenAPI name (often camelCase) but produce
    // a Rust-friendly snake_case form for the generated code.
    to_snake_case(name)
}

fn snake_plural(payload: &str) -> String {
    // `AccountSummary` → `account_summaries`; `Transaction` → `transactions`.
    let snake = to_snake_case(payload);
    if snake.ends_with('y') && snake.len() > 1 {
        let base = &snake[..snake.len() - 1];
        format!("{}ies", base)
    } else {
        format!("{}s", snake)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(yaml: &str) -> Value {
        serde_yaml::from_str(yaml).expect("parse yaml")
    }

    #[test]
    fn ref_to_type_name_extracts_last_segment() {
        assert_eq!(
            ref_to_type_name("#/components/schemas/AccountSummary"),
            "AccountSummary",
        );
    }

    #[test]
    fn camel_to_ba_normalises_field_names() {
        assert_eq!(camel_to_ba("holderName"), "holder name");
        assert_eq!(camel_to_ba("account_type"), "account type");
        assert_eq!(camel_to_ba("_id"), "id");
        assert_eq!(camel_to_ba("availableBalance"), "available balance");
    }

    #[test]
    fn stable_hash_is_deterministic() {
        let v: Value = parse("a: 1\nb: [2, 3]\n");
        let h1 = stable_hash(&v);
        let h2 = stable_hash(&v);
        assert_eq!(h1, h2);
        // Different content → different hash.
        let v2: Value = parse("a: 2\nb: [2, 3]\n");
        assert_ne!(h1, stable_hash(&v2));
    }

    #[test]
    fn render_data_draft_carries_header_and_attributes() {
        let schema: Value = parse(
            "type: object\nproperties:\n  iban:\n    type: string\n  balance:\n    type: integer\nrequired:\n  - iban\n",
        );
        let body = render_data_draft("nordea-openbanking", "AccountSummary", &schema);
        assert!(body.starts_with("<!-- reen:auto-generated from drafts/apis/nordea-openbanking.yaml#components/schemas/AccountSummary -->"));
        assert!(body.contains("DO NOT EDIT"));
        assert!(body.contains("source-hash: "));
        assert!(body.contains("# Data: AccountSummary"));
        assert!(body.contains("- iban: a text value"));
        assert!(body.contains("- balance: a whole number"));
        assert!(body.contains("- iban must be present"));
    }

    #[test]
    fn render_projection_draft_carries_source_block() {
        let yaml = "
operationId: GetAccounts
summary: List the customer's accounts
parameters:
  - name: customerId
    in: path
    required: true
    schema:
      type: string
responses:
  \"200\":
    description: ok
    content:
      application/json:
        schema:
          $ref: \"#/components/schemas/AccountListResponse\"
";
        let op_value: Value = serde_yaml::from_str(yaml).unwrap();
        let op = Operation {
            operation_id: "GetAccounts".into(),
            summary: Some("List the customer's accounts".into()),
            method: "GET".into(),
            path: "/accounts/{customerId}".into(),
            parameters: vec![OpParam {
                name: "customer_id".into(),
                location: "path".into(),
                required: true,
                rust_type: "&str".into(),
            }],
            response_schema: Some("AccountListResponse".into()),
            response_is_list: false,
            fragment: op_value,
        };
        let body = render_projection_draft("nordea-openbanking", &op, &BTreeMap::new());
        assert!(body.contains("# Projection: GetAccounts"));
        assert!(body.contains("## Source"));
        assert!(body.contains("- Method: GET /accounts/{customerId}"));
        assert!(body.contains("- customer_id (path, required): &str"));
        assert!(body.contains("- Returns: "));
        assert!(body.contains("### call"));
    }

    #[test]
    fn snake_plural_handles_y_ending() {
        assert_eq!(snake_plural("AccountSummary"), "account_summaries");
        assert_eq!(snake_plural("Transaction"), "transactions");
    }

    #[test]
    fn openapi_to_rust_handles_primitives_and_refs() {
        assert_eq!(openapi_to_rust(&parse("type: string"), false), "String");
        assert_eq!(openapi_to_rust(&parse("type: string"), true), "&str");
        assert_eq!(openapi_to_rust(&parse("type: integer"), false), "i64");
        assert_eq!(openapi_to_rust(&parse("type: array\nitems:\n  type: string"), false), "Vec<String>");
        assert_eq!(
            openapi_to_rust(&parse("$ref: \"#/components/schemas/AccountInfo\""), false),
            "AccountInfo",
        );
    }
}
