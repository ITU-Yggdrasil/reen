//! Scaffold code generation for API operation clients.
//!
//! Generates thin Rust client stubs from `reen.operation/v1` specs.
//! Public method set: `{ call }` for single/list/none, `{ subscribe, next }` for stream_of.

use crate::codegen::to_snake_case;
use crate::prepared::ApiOperationSpec;
use crate::workspace::{GENERATED_MANIFEST, Workspace};
use anyhow::{Context, Result};
use std::fmt::Write as FmtWrite;
use std::fs;
use std::path::PathBuf;

/// Scaffold operation client stubs from operation specs under `drafts/.apis/operations/`.
pub fn scaffold_api_operations(workspace: &Workspace, verbose: bool, dry_run: bool) -> Result<()> {
    let ops_root = workspace.drafts_dir.join(".apis/operations");
    if !ops_root.is_dir() {
        if verbose {
            println!("No API operations to scaffold (drafts/.apis/operations/ not found)");
        }
        return Ok(());
    }

    let mut generated_files: Vec<String> = load_existing_manifest(workspace)?;
    let mut wrote = 0usize;

    // Iterate over api directories
    let mut api_dirs: Vec<_> = fs::read_dir(&ops_root)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();
    api_dirs.sort_by_key(|e| e.file_name());

    for api_entry in api_dirs {
        let api_name = api_entry.file_name().to_string_lossy().to_string();
        let api_dir = api_entry.path();

        let mut op_files: Vec<_> = fs::read_dir(&api_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    == Some("yml")
            })
            .collect();
        op_files.sort_by_key(|e| e.file_name());

        for op_entry in op_files {
            let raw = fs::read_to_string(op_entry.path())
                .with_context(|| format!("Failed to read {}", op_entry.path().display()))?;
            let spec: ApiOperationSpec = serde_yaml::from_str(&raw)
                .with_context(|| format!("Failed to parse {}", op_entry.path().display()))?;

            let code = generate_operation_client(&spec);
            let output_path = operation_output_path(workspace, &api_name, &spec.name);

            if dry_run {
                if verbose {
                    println!("[dry-run] would write {}", output_path.display());
                }
            } else {
                if let Some(parent) = output_path.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("Failed to create {}", parent.display()))?;
                }
                fs::write(&output_path, &code)
                    .with_context(|| format!("Failed to write {}", output_path.display()))?;

                let relative = output_path
                    .strip_prefix(&workspace.root)
                    .unwrap_or(&output_path)
                    .to_string_lossy()
                    .to_string();
                if !generated_files.contains(&relative) {
                    generated_files.push(relative);
                }
                wrote += 1;
                if verbose {
                    println!("  scaffolded: {}", output_path.display());
                }
            }
        }
    }

    if !dry_run && !generated_files.is_empty() {
        let manifest_path = workspace.root.join(GENERATED_MANIFEST);
        if let Some(parent) = manifest_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&generated_files)?;
        fs::write(&manifest_path, json)?;
    }

    if verbose {
        println!("Scaffolded {} operation client(s)", wrote);
    }
    Ok(())
}

fn load_existing_manifest(workspace: &Workspace) -> Result<Vec<String>> {
    let path = workspace.root.join(GENERATED_MANIFEST);
    if path.is_file() {
        let raw = fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&raw).unwrap_or_default())
    } else {
        Ok(Vec::new())
    }
}

fn operation_output_path(workspace: &Workspace, api_name: &str, op_name: &str) -> PathBuf {
    workspace
        .root
        .join("src/apis")
        .join(api_name)
        .join(format!("{}.rs", to_snake_case(op_name)))
}

/// Generate a thin Rust client stub for an operation.
pub fn generate_operation_client(spec: &ApiOperationSpec) -> String {
    let struct_name = &spec.name;
    let mut out = String::new();

    // Struct with fields mirroring the operation spec
    writeln!(out, "pub struct {} {{", struct_name).unwrap();

    // Transport fields
    writeln!(
        out,
        "    pub transport_kind: &'static str, // {:?}",
        spec.transport.kind
    )
    .unwrap();
    if let Some(url) = &spec.transport.url {
        writeln!(out, "    pub url: &'static str, // {}", url).unwrap();
    }
    if let Some(method) = &spec.transport.method {
        writeln!(out, "    pub method: &'static str, // {}", method).unwrap();
    }
    if let Some(path) = &spec.transport.path {
        writeln!(out, "    pub path: &'static str, // {}", path).unwrap();
    }

    // Auth fields
    if spec.auth.kind != "none" {
        if let Some(field) = &spec.auth.field {
            writeln!(
                out,
                "    pub auth_{}: (), // {} auth",
                to_snake_case(field),
                spec.auth.kind
            )
            .unwrap();
        }
    }

    // Input type placeholder
    if spec.input.kind != "none" {
        if let Some(dt) = &spec.input.data_type {
            writeln!(out, "    pub input: (), // {}", dt).unwrap();
        }
    }

    // Output type placeholder
    if spec.output.kind != "none" {
        if let Some(dt) = &spec.output.data_type {
            writeln!(out, "    pub output: (), // {}", dt).unwrap();
        }
    }

    // Error types
    for (i, err) in spec.errors.iter().enumerate() {
        writeln!(out, "    pub error_{}: (), // {}", i, err.data_type).unwrap();
    }

    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    // Impl block
    writeln!(out, "impl {} {{", struct_name).unwrap();

    let is_stream = spec.output.kind == "stream_of";

    if is_stream {
        // Stream operations: subscribe + next
        writeln!(out, "    /// Initiate the streaming connection.").unwrap();
        writeln!(out, "    pub fn subscribe(&self) {{").unwrap();
        writeln!(out, "        todo!()").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out).unwrap();
        writeln!(
            out,
            "    /// Receive the next item from the stream."
        )
        .unwrap();
        writeln!(out, "    pub fn next(&self) {{").unwrap();
        writeln!(out, "        todo!()").unwrap();
        writeln!(out, "    }}").unwrap();
    } else {
        // Single/list/none: call
        writeln!(out, "    /// Execute the API operation.").unwrap();
        writeln!(out, "    pub fn call(&self) {{").unwrap();
        writeln!(out, "        todo!()").unwrap();
        writeln!(out, "    }}").unwrap();
    }

    writeln!(out).unwrap();

    // Add notes as comments
    if !spec.notes.is_empty() {
        writeln!(out, "    // Notes from the boundary draft:").unwrap();
        for note in &spec.notes {
            writeln!(out, "    // - {}", note).unwrap();
        }
    }

    writeln!(out, "}}").unwrap();
    out
}

/// Get the expected public methods for an operation based on its output kind.
pub fn expected_operation_methods(spec: &ApiOperationSpec) -> Vec<String> {
    if spec.output.kind == "stream_of" {
        vec!["subscribe".to_string(), "next".to_string()]
    } else {
        vec!["call".to_string()]
    }
}

/// Get the expected struct fields for an operation.
pub fn expected_operation_fields(spec: &ApiOperationSpec) -> Vec<String> {
    let mut fields = Vec::new();

    fields.push("transport_kind".to_string());
    if spec.transport.url.is_some() {
        fields.push("url".to_string());
    }
    if spec.transport.method.is_some() {
        fields.push("method".to_string());
    }
    if spec.transport.path.is_some() {
        fields.push("path".to_string());
    }

    if spec.auth.kind != "none" {
        if let Some(field) = &spec.auth.field {
            fields.push(format!("auth_{}", to_snake_case(field)));
        }
    }

    if spec.input.kind != "none" && spec.input.data_type.is_some() {
        fields.push("input".to_string());
    }

    if spec.output.kind != "none" && spec.output.data_type.is_some() {
        fields.push("output".to_string());
    }

    for (i, _) in spec.errors.iter().enumerate() {
        fields.push(format!("error_{}", i));
    }

    fields
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prepared::*;

    fn test_stream_spec() -> ApiOperationSpec {
        ApiOperationSpec {
            schema: OPERATION_SCHEMA_VERSION.to_string(),
            api: "aisstream".to_string(),
            name: "Subscribe".to_string(),
            transport: TransportSpec {
                kind: "websocket".to_string(),
                url: Some("wss://stream.aisstream.io/v0/stream".to_string()),
                method: None,
                path: None,
            },
            auth: AuthSpec {
                kind: "api_key_body".to_string(),
                field: Some("APIKey".to_string()),
            },
            input: IoSpec {
                kind: "data".to_string(),
                data_type: Some("SubscriptionMessage".to_string()),
            },
            output: IoSpec {
                kind: "stream_of".to_string(),
                data_type: Some("AisStreamMessage".to_string()),
            },
            errors: Vec::new(),
            notes: vec!["WebSocket transport".to_string()],
        }
    }

    fn test_rest_spec() -> ApiOperationSpec {
        ApiOperationSpec {
            schema: OPERATION_SCHEMA_VERSION.to_string(),
            api: "example".to_string(),
            name: "GetWidget".to_string(),
            transport: TransportSpec {
                kind: "rest".to_string(),
                url: None,
                method: Some("GET".to_string()),
                path: Some("/widgets/{id}".to_string()),
            },
            auth: AuthSpec {
                kind: "none".to_string(),
                field: None,
            },
            input: IoSpec {
                kind: "none".to_string(),
                data_type: None,
            },
            output: IoSpec {
                kind: "single".to_string(),
                data_type: Some("Widget".to_string()),
            },
            errors: Vec::new(),
            notes: Vec::new(),
        }
    }

    #[test]
    fn stream_operation_generates_subscribe_and_next() {
        let code = generate_operation_client(&test_stream_spec());
        assert!(code.contains("pub struct Subscribe"));
        assert!(code.contains("pub fn subscribe("));
        assert!(code.contains("pub fn next("));
        assert!(!code.contains("pub fn call("));
        assert!(code.contains("transport_kind"));
        assert!(code.contains("auth_apikey"));
    }

    #[test]
    fn rest_operation_generates_call() {
        let code = generate_operation_client(&test_rest_spec());
        assert!(code.contains("pub struct GetWidget"));
        assert!(code.contains("pub fn call("));
        assert!(!code.contains("pub fn subscribe("));
        assert!(code.contains("method"));
        assert!(code.contains("path"));
    }

    #[test]
    fn expected_methods_for_stream() {
        let methods = expected_operation_methods(&test_stream_spec());
        assert_eq!(methods, vec!["subscribe", "next"]);
    }

    #[test]
    fn expected_methods_for_rest() {
        let methods = expected_operation_methods(&test_rest_spec());
        assert_eq!(methods, vec!["call"]);
    }

    #[test]
    fn expected_fields_for_stream() {
        let fields = expected_operation_fields(&test_stream_spec());
        assert!(fields.contains(&"transport_kind".to_string()));
        assert!(fields.contains(&"url".to_string()));
        assert!(fields.contains(&"auth_apikey".to_string()));
        assert!(fields.contains(&"input".to_string()));
        assert!(fields.contains(&"output".to_string()));
    }
}
