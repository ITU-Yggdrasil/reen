use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;

use super::contract_store::{
    AmbiguityEntry, DecisionSource, DependencyBinding, DependencyMethodBinding, InterfaceField,
    InterfaceMethod, InterfaceParameter, InterfaceType, NameBinding, ResolvedInterface,
    ResolvedType,
};

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct InterfaceResolutionOutput {
    pub(crate) resolved_interface: ResolvedInterface,
    #[serde(default)]
    pub(crate) type_decisions: Vec<ResolvedType>,
    #[serde(default)]
    pub(crate) name_bindings: Vec<NameBinding>,
    #[serde(default)]
    pub(crate) dependency_bindings: Vec<DependencyBinding>,
    #[serde(default)]
    pub(crate) ambiguity_report: Vec<AmbiguityEntry>,
    #[serde(default)]
    pub(crate) decision_sources: Vec<DecisionSource>,
}

pub(crate) fn parse_interface_resolution_output(output: &str) -> Result<InterfaceResolutionOutput> {
    let json_candidate = extract_json_candidate(output);
    let root_value: Value = serde_json::from_str(json_candidate)
        .context("interface resolution output was not valid JSON")?;
    let Some(parsed) = find_interface_resolution_output(&root_value, 0) else {
        bail!(
            "interface resolution output JSON did not match the current or accepted legacy resolver schema"
        );
    };

    if parsed
        .resolved_interface
        .primary_export_name
        .trim()
        .is_empty()
    {
        bail!("interface resolution output did not include a primary export name");
    }

    Ok(parsed)
}

fn find_interface_resolution_output(
    value: &Value,
    depth: usize,
) -> Option<InterfaceResolutionOutput> {
    if depth > 8 {
        return None;
    }

    if let Some(parsed) = parse_interface_resolution_value(value) {
        return Some(parsed);
    }

    match value {
        Value::String(text) => parse_nested_json_value(text)
            .and_then(|nested| find_interface_resolution_output(&nested, depth + 1)),
        Value::Array(items) => items
            .iter()
            .find_map(|item| find_interface_resolution_output(item, depth + 1)),
        Value::Object(map) => {
            const WRAPPER_KEYS: &[&str] = &[
                "interface_resolution",
                "resolved_interface_contract",
                "resolved_contract",
                "payload",
                "result",
                "response",
                "output",
                "data",
                "json",
                "content",
                "message",
                "text",
            ];

            for key in WRAPPER_KEYS {
                if let Some(child) = map.get(*key) {
                    if let Some(parsed) = find_interface_resolution_output(child, depth + 1) {
                        return Some(parsed);
                    }
                }
            }

            for (key, child) in map {
                if WRAPPER_KEYS.contains(&key.as_str()) {
                    continue;
                }
                if let Some(parsed) = find_interface_resolution_output(child, depth + 1) {
                    return Some(parsed);
                }
            }

            None
        }
        _ => None,
    }
}

fn parse_nested_json_value(text: &str) -> Option<Value> {
    let candidate = extract_json_candidate(text);
    serde_json::from_str(candidate).ok()
}

fn parse_interface_resolution_value(value: &Value) -> Option<InterfaceResolutionOutput> {
    serde_json::from_value(value.clone())
        .ok()
        .or_else(|| normalize_legacy_interface_resolution_output(value))
}

fn normalize_legacy_interface_resolution_output(
    value: &Value,
) -> Option<InterfaceResolutionOutput> {
    let object = value.as_object()?;
    let resolved_interface =
        normalize_legacy_resolved_interface(object.get("resolved_interface")?)?;

    Some(InterfaceResolutionOutput {
        resolved_interface,
        type_decisions: normalize_legacy_resolved_types(object.get("type_decisions")),
        name_bindings: normalize_legacy_name_bindings(object.get("name_bindings")),
        dependency_bindings: normalize_dependency_bindings(object.get("dependency_bindings")),
        ambiguity_report: normalize_legacy_ambiguities(object.get("ambiguity_report")),
        decision_sources: normalize_legacy_decision_sources(object.get("decision_sources")),
    })
}

fn normalize_legacy_resolved_interface(value: &Value) -> Option<ResolvedInterface> {
    if let Ok(parsed) = serde_json::from_value::<ResolvedInterface>(value.clone()) {
        return Some(parsed);
    }

    let object = value.as_object()?;
    Some(ResolvedInterface {
        version: read_string(object, &["version"])
            .unwrap_or_else(|| "reen.interface/v2".to_string()),
        interface_fingerprint: read_string(object, &["interface_fingerprint"]).unwrap_or_default(),
        primary_export_name: read_string(object, &["primary_export_name"])?,
        artifact_kind: read_string(object, &["artifact_kind"])
            .unwrap_or_else(|| "data_module".to_string()),
        exported_types: normalize_legacy_exported_types(object.get("exported_types")),
        exported_methods: normalize_legacy_methods(object.get("exported_methods")),
        role_method_exports: normalize_legacy_methods(object.get("role_method_exports")),
        name_bindings: normalize_legacy_name_bindings(object.get("name_bindings")),
    })
}

fn normalize_legacy_exported_types(value: Option<&Value>) -> Vec<InterfaceType> {
    if let Some(value) = value {
        if let Ok(parsed) = serde_json::from_value::<Vec<InterfaceType>>(value.clone()) {
            return parsed;
        }
    }

    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(normalize_legacy_exported_type)
        .collect()
}

fn normalize_legacy_exported_type(value: &Value) -> Option<InterfaceType> {
    if let Ok(parsed) = serde_json::from_value::<InterfaceType>(value.clone()) {
        return Some(parsed);
    }

    let object = value.as_object()?;
    let export_name = read_string(object, &["export_name", "type_name", "rust_name", "name"])?;
    let semantic_name = read_string(object, &["semantic_name"])
        .unwrap_or_else(|| semantic_name_for_identifier(&export_name));
    let rust_name = read_string(object, &["rust_name"]).unwrap_or_else(|| export_name.clone());

    Some(InterfaceType {
        semantic_name,
        rust_name,
        export_name,
        kind: read_string(object, &["kind"]).unwrap_or_else(|| "struct".to_string()),
        fields: normalize_legacy_fields(object.get("fields")),
    })
}

fn normalize_legacy_fields(value: Option<&Value>) -> Vec<InterfaceField> {
    if let Some(value) = value {
        if let Ok(parsed) = serde_json::from_value::<Vec<InterfaceField>>(value.clone()) {
            return parsed;
        }
    }

    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(normalize_legacy_field)
        .collect()
}

fn normalize_legacy_field(value: &Value) -> Option<InterfaceField> {
    if let Ok(parsed) = serde_json::from_value::<InterfaceField>(value.clone()) {
        return Some(parsed);
    }

    let object = value.as_object()?;
    let export_name = read_string(object, &["export_name", "field_name", "rust_name", "name"])?;
    let semantic_name = read_string(object, &["semantic_name"])
        .unwrap_or_else(|| semantic_name_for_identifier(&export_name));
    let rust_name = read_string(object, &["rust_name"]).unwrap_or_else(|| export_name.clone());

    Some(InterfaceField {
        semantic_name,
        rust_name,
        export_name,
        type_ref: read_string(object, &["type_ref", "field_type", "type"])?,
    })
}

fn normalize_legacy_methods(value: Option<&Value>) -> Vec<InterfaceMethod> {
    if let Some(value) = value {
        if let Ok(parsed) = serde_json::from_value::<Vec<InterfaceMethod>>(value.clone()) {
            return parsed;
        }
    }

    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(normalize_legacy_method)
        .collect()
}

fn normalize_legacy_method(value: &Value) -> Option<InterfaceMethod> {
    if let Ok(parsed) = serde_json::from_value::<InterfaceMethod>(value.clone()) {
        return Some(parsed);
    }

    let object = value.as_object()?;
    let export_name = read_string(object, &["export_name", "method_name", "rust_name", "name"])?;
    let semantic_name = read_string(object, &["semantic_name"])
        .unwrap_or_else(|| semantic_name_for_identifier(&export_name));
    let rust_name = read_string(object, &["rust_name"]).unwrap_or_else(|| export_name.clone());
    let receiver = object
        .get("receiver")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            object
                .get("is_constructor")
                .and_then(Value::as_bool)
                .and_then(|is_constructor| is_constructor.then(|| "associated".to_string()))
        })
        .unwrap_or_else(|| "associated".to_string());
    let parameters = normalize_legacy_parameters(
        object
            .get("parameters")
            .or_else(|| object.get("params"))
            .or_else(|| object.get("inputs")),
    );
    let return_type = canonicalize_legacy_type_expr(
        &read_string(object, &["return_type", "output"]).unwrap_or_else(|| "()".to_string()),
    );

    Some(InterfaceMethod {
        semantic_name,
        rust_name,
        export_name: export_name.clone(),
        receiver: receiver.clone(),
        parameters: parameters.clone(),
        return_type: return_type.clone(),
        failure_shape: inferred_failure_shape(&return_type).to_string(),
        signature: build_legacy_signature(&export_name, &receiver, &parameters, &return_type),
    })
}

fn normalize_legacy_parameters(value: Option<&Value>) -> Vec<InterfaceParameter> {
    if let Some(value) = value {
        if let Ok(parsed) = serde_json::from_value::<Vec<InterfaceParameter>>(value.clone()) {
            return parsed;
        }
    }

    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(normalize_legacy_parameter)
        .collect()
}

fn normalize_legacy_parameter(value: &Value) -> Option<InterfaceParameter> {
    if let Ok(parsed) = serde_json::from_value::<InterfaceParameter>(value.clone()) {
        return Some(parsed);
    }

    let object = value.as_object()?;
    let rust_name = read_string(object, &["rust_name", "param_name", "name"])?;
    Some(InterfaceParameter {
        semantic_name: read_string(object, &["semantic_name"])
            .unwrap_or_else(|| semantic_name_for_identifier(&rust_name)),
        rust_name,
        type_ref: read_string(object, &["type_ref", "param_type", "type"])?,
    })
}

fn normalize_legacy_resolved_types(value: Option<&Value>) -> Vec<ResolvedType> {
    if let Some(value) = value {
        if let Ok(parsed) = serde_json::from_value::<Vec<ResolvedType>>(value.clone()) {
            return parsed;
        }
    }

    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(normalize_legacy_resolved_type)
        .collect()
}

fn normalize_legacy_resolved_type(value: &Value) -> Option<ResolvedType> {
    if let Ok(parsed) = serde_json::from_value::<ResolvedType>(value.clone()) {
        return Some(parsed);
    }

    let object = value.as_object()?;
    Some(ResolvedType {
        semantic_type: read_string(
            object,
            &["semantic_type", "field", "member", "field_or_param", "name"],
        )?,
        rust_type: canonicalize_legacy_type_expr(&read_string(
            object,
            &["rust_type", "chosen_type", "resolved_type"],
        )?),
        source: read_string(object, &["source", "rationale"])
            .unwrap_or_else(|| "legacy_resolver".to_string()),
    })
}

fn normalize_legacy_name_bindings(value: Option<&Value>) -> Vec<NameBinding> {
    if let Some(value) = value {
        if let Ok(parsed) = serde_json::from_value::<Vec<NameBinding>>(value.clone()) {
            return parsed;
        }
    }

    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(normalize_legacy_name_binding)
        .collect()
}

fn normalize_legacy_name_binding(value: &Value) -> Option<NameBinding> {
    if let Ok(parsed) = serde_json::from_value::<NameBinding>(value.clone()) {
        return Some(parsed);
    }

    let object = value.as_object()?;
    let semantic_name = read_string(
        object,
        &["semantic_name", "source_name", "binding_name", "name"],
    )?;
    let rust_identifier = read_string(
        object,
        &[
            "rust_identifier",
            "bound_name",
            "export_type",
            "bound_type",
            "name",
        ],
    )
    .unwrap_or_else(|| semantic_name.clone());
    let export_name = read_string(
        object,
        &[
            "export_name",
            "bound_name",
            "export_type",
            "bound_type",
            "name",
        ],
    )
    .unwrap_or_else(|| rust_identifier.clone());

    Some(NameBinding {
        semantic_name,
        rust_identifier,
        export_name,
        reason: read_string(object, &["reason", "source"])
            .unwrap_or_else(|| "legacy_resolver".to_string()),
    })
}

fn normalize_dependency_bindings(value: Option<&Value>) -> Vec<DependencyBinding> {
    if let Some(value) = value {
        if let Ok(parsed) = serde_json::from_value::<Vec<DependencyBinding>>(value.clone()) {
            return parsed;
        }
    }

    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(normalize_legacy_dependency_binding)
        .collect()
}

fn normalize_legacy_dependency_binding(value: &Value) -> Option<DependencyBinding> {
    if let Ok(parsed) = serde_json::from_value::<DependencyBinding>(value.clone()) {
        return Some(parsed);
    }

    let object = value.as_object()?;
    let semantic_dependency = read_string(
        object,
        &[
            "semantic_dependency",
            "role",
            "dependency",
            "dependency_name",
        ],
    )?;
    let rust_dependency = read_string(object, &["rust_dependency", "resolved_type", "role"])
        .unwrap_or_else(|| semantic_dependency.clone());
    let interface_name = read_string(object, &["interface_name", "source_interface"])
        .unwrap_or_else(|| rust_dependency.clone());
    let spec_path = read_string(object, &["spec_path", "source_spec_path"]).unwrap_or_default();
    let role_name = semantic_dependency.clone();
    let method_bindings = object
        .get("method_bindings")
        .or_else(|| object.get("bound_methods"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| normalize_legacy_dependency_method_binding(entry, &role_name))
        .collect::<Vec<_>>();

    Some(DependencyBinding {
        semantic_dependency,
        rust_dependency,
        spec_path,
        interface_name,
        method_bindings,
    })
}

fn normalize_legacy_dependency_method_binding(
    value: &Value,
    role_name: &str,
) -> Option<DependencyMethodBinding> {
    if let Ok(mut parsed) = serde_json::from_value::<DependencyMethodBinding>(value.clone()) {
        if !parsed.role_method.contains('.') {
            parsed.role_method = format!("{}.{}", role_name, parsed.role_method);
        }
        return Some(parsed);
    }

    let object = value.as_object()?;
    let role_method = read_string(object, &["role_method", "method", "name"])?;
    let role_method = if role_method.contains('.') {
        role_method
    } else {
        format!("{}.{}", role_name, role_method)
    };
    let upstream_method = read_string(
        object,
        &["upstream_method", "bound_method", "source_method", "method"],
    )?;

    Some(DependencyMethodBinding {
        role_method,
        upstream_method,
    })
}

fn normalize_legacy_ambiguities(value: Option<&Value>) -> Vec<AmbiguityEntry> {
    if let Some(value) = value {
        if let Ok(parsed) = serde_json::from_value::<Vec<AmbiguityEntry>>(value.clone()) {
            return parsed;
        }
    }

    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| !should_drop_legacy_ambiguity(entry))
        .filter_map(normalize_legacy_ambiguity)
        .collect()
}

fn normalize_legacy_ambiguity(value: &Value) -> Option<AmbiguityEntry> {
    if let Ok(parsed) = serde_json::from_value::<AmbiguityEntry>(value.clone()) {
        return Some(parsed);
    }

    let object = value.as_object()?;
    Some(AmbiguityEntry {
        class: read_string(object, &["class"]).unwrap_or_else(|| "behavioral".to_string()),
        subject: read_string(object, &["subject", "site"])?,
        detail: read_string(object, &["detail", "issue", "description"])?,
    })
}

fn normalize_legacy_decision_sources(value: Option<&Value>) -> Vec<DecisionSource> {
    if let Some(value) = value {
        if let Ok(parsed) = serde_json::from_value::<Vec<DecisionSource>>(value.clone()) {
            return parsed;
        }
    }

    let mut out = Vec::new();
    for entry in value.and_then(Value::as_array).into_iter().flatten() {
        if let Some(parsed) = normalize_legacy_decision_source(entry) {
            out.extend(parsed);
        }
    }
    out
}

fn normalize_legacy_decision_source(value: &Value) -> Option<Vec<DecisionSource>> {
    if let Ok(parsed) = serde_json::from_value::<DecisionSource>(value.clone()) {
        return Some(vec![parsed]);
    }

    let object = value.as_object()?;
    let subject = read_string(object, &["subject", "decision"])?;

    if let Some(detail) = read_string(object, &["detail", "source"]) {
        return Some(vec![DecisionSource {
            subject,
            source_kind: read_string(object, &["source_kind"])
                .unwrap_or_else(|| "legacy_resolver".to_string()),
            detail,
        }]);
    }

    let mut out = Vec::new();
    for detail in object
        .get("sources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        out.push(DecisionSource {
            subject: subject.clone(),
            source_kind: read_string(object, &["source_kind"])
                .unwrap_or_else(|| "legacy_resolver".to_string()),
            detail: detail.to_string(),
        });
    }

    (!out.is_empty()).then_some(out)
}

fn read_string(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = object.get(*key) {
            if let Some(text) = value.as_str() {
                return Some(text.to_string());
            }
        }
    }
    None
}

fn semantic_name_for_identifier(identifier: &str) -> String {
    identifier.trim_start_matches("r#").to_string()
}

fn inferred_failure_shape(return_type: &str) -> &'static str {
    let trimmed = return_type.trim();
    if matches!(
        trimmed,
        value if value.starts_with("Result<")
            || value.starts_with("core::result::Result<")
            || value.starts_with("std::result::Result<")
            || value.starts_with("anyhow::Result<")
    ) {
        "result"
    } else if matches!(
        trimmed,
        value
            if value.starts_with("Option<")
                || value.starts_with("core::option::Option<")
                || value.starts_with("std::option::Option<")
    ) {
        "option"
    } else {
        "plain"
    }
}

fn canonicalize_legacy_type_expr(type_ref: &str) -> String {
    placeholder_result_ok_type(type_ref)
        .map(|ok_type| format!("anyhow::Result<{ok_type}>"))
        .unwrap_or_else(|| type_ref.trim().to_string())
}

fn placeholder_result_ok_type(type_ref: &str) -> Option<&str> {
    let trimmed = type_ref.trim();
    for prefix in ["Result<", "core::result::Result<", "std::result::Result<"] {
        let Some(inner) = trimmed
            .strip_prefix(prefix)
            .and_then(|value| value.strip_suffix('>'))
        else {
            continue;
        };
        let args = split_top_level_type_args(inner);
        if args.len() == 2 && args[1].trim() == "_" {
            return Some(args[0].trim());
        }
    }
    None
}

fn split_top_level_type_args(input: &str) -> Vec<&str> {
    let mut args = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (idx, ch) in input.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                args.push(input[start..idx].trim());
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    args.push(input[start..].trim());
    args
}

fn should_drop_legacy_ambiguity(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    let subject = read_string(object, &["subject", "site"])
        .unwrap_or_default()
        .to_ascii_lowercase();
    let detail = read_string(object, &["detail", "issue", "description"])
        .unwrap_or_default()
        .to_ascii_lowercase();

    subject.contains("result<")
        && (detail.contains("opaque '_'")
            || (detail.contains("error slot")
                && detail.contains("cannot be resolved")
                && detail.contains("manifest-backed")))
}

fn build_legacy_signature(
    method_name: &str,
    receiver: &str,
    parameters: &[InterfaceParameter],
    return_type: &str,
) -> String {
    let mut parts = Vec::new();
    if receiver != "associated" && !receiver.is_empty() {
        parts.push(receiver.to_string());
    }
    parts.extend(
        parameters
            .iter()
            .map(|parameter| format!("{}: {}", parameter.rust_name, parameter.type_ref)),
    );
    let params = parts.join(", ");
    if return_type.trim() == "()" {
        format!("pub fn {method_name}({params})")
    } else {
        format!("pub fn {method_name}({params}) -> {return_type}")
    }
}

fn strip_code_fence(output: &str) -> &str {
    let trimmed = output.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        return rest.strip_suffix("```").unwrap_or(rest).trim();
    }
    if let Some(rest) = trimmed.strip_prefix("```") {
        return rest.strip_suffix("```").unwrap_or(rest).trim();
    }
    trimmed
}

fn extract_json_candidate(output: &str) -> &str {
    let trimmed = strip_code_fence(output);
    if trimmed.starts_with('{') {
        return trimmed;
    }

    if let Some(candidate) = extract_fenced_json_block(output) {
        return candidate;
    }

    if let Some(candidate) = extract_balanced_json_object(output) {
        return candidate;
    }

    trimmed
}

fn extract_fenced_json_block(output: &str) -> Option<&str> {
    let mut remainder = output;
    while let Some(start) = remainder.find("```") {
        let after_ticks = &remainder[start + 3..];
        let newline_idx = after_ticks.find('\n')?;
        let block_body_start = start + 3 + newline_idx + 1;
        let language = after_ticks[..newline_idx].trim();
        let rest = &remainder[block_body_start..];
        let end = rest.find("```")?;
        let body = rest[..end].trim();
        if language.is_empty() || language.eq_ignore_ascii_case("json") {
            if body.starts_with('{') {
                return Some(body);
            }
            if let Some(candidate) = extract_balanced_json_object(body) {
                return Some(candidate);
            }
        }
        remainder = &rest[end + 3..];
    }
    None
}

fn extract_balanced_json_object(output: &str) -> Option<&str> {
    let bytes = output.as_bytes();
    let mut start_idx = None;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (idx, &byte) in bytes.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            match byte {
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }

        match byte {
            b'"' => in_string = true,
            b'{' => {
                if depth == 0 {
                    start_idx = Some(idx);
                }
                depth += 1;
            }
            b'}' => {
                if depth == 0 {
                    continue;
                }
                depth -= 1;
                if depth == 0 {
                    if let Some(start) = start_idx {
                        return Some(output[start..=idx].trim());
                    }
                }
            }
            _ => {}
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::parse_interface_resolution_output;

    #[test]
    fn parses_fenced_json_interface_resolution() {
        let parsed = parse_interface_resolution_output(
            r#"```json
{
  "resolved_interface": {
    "version": "reen.interface/v2",
    "interface_fingerprint": "",
    "primary_export_name": "Board",
    "artifact_kind": "data_module",
    "exported_types": [],
    "exported_methods": [],
    "role_method_exports": [],
    "name_bindings": []
  },
  "type_decisions": [],
  "name_bindings": [],
  "dependency_bindings": [],
  "ambiguity_report": [],
  "decision_sources": []
}
```"#,
        )
        .expect("parse");

        assert_eq!(parsed.resolved_interface.primary_export_name, "Board");
    }

    #[test]
    fn parses_wrapped_json_interface_resolution() {
        let wrapped = json!({
            "result": {
                "content": "```json\n{\n  \"resolved_interface\": {\n    \"version\": \"reen.interface/v2\",\n    \"interface_fingerprint\": \"\",\n    \"primary_export_name\": \"Board\",\n    \"artifact_kind\": \"data_module\",\n    \"exported_types\": [],\n    \"exported_methods\": [],\n    \"role_method_exports\": [],\n    \"name_bindings\": []\n  },\n  \"type_decisions\": [],\n  \"name_bindings\": [],\n  \"dependency_bindings\": [],\n  \"ambiguity_report\": [],\n  \"decision_sources\": []\n}\n```"
            }
        });

        let parsed = parse_interface_resolution_output(&wrapped.to_string()).expect("parse");
        assert_eq!(parsed.resolved_interface.primary_export_name, "Board");
    }

    #[test]
    fn parses_legacy_interface_resolution_schema() {
        let legacy = json!({
          "resolved_interface": {
            "version": "reen.interface/v2",
            "interface_fingerprint": "fp",
            "primary_export_name": "Board",
            "artifact_kind": "data_module",
            "exported_types": [
              {
                "type_name": "Board",
                "kind": "struct",
                "fields": [
                  { "field_name": "width", "field_type": "u32" }
                ]
              }
            ],
            "exported_methods": [
              {
                "method_name": "new",
                "receiver": null,
                "params": [
                  { "param_name": "width", "param_type": "u32" }
                ],
                "return_type": "anyhow::Result<Board>",
                "is_constructor": true
              }
            ],
            "role_method_exports": [],
            "name_bindings": [
              { "source_name": "Board", "bound_name": "Board" }
            ]
          },
          "type_decisions": [
            {
              "field": "width",
              "chosen_type": "u32",
              "rationale": "legacy rationale"
            }
          ],
          "name_bindings": [
            { "source_name": "Board", "bound_name": "Board" }
          ],
          "dependency_bindings": [],
          "ambiguity_report": [
            {
              "site": "new -> Result<Board, E>",
              "issue": "error type is not specified"
            }
          ],
          "decision_sources": [
            {
              "decision": "Constructor is fallible",
              "sources": ["legacy source"]
            }
          ]
        });

        let parsed = parse_interface_resolution_output(&legacy.to_string()).expect("parse");
        assert_eq!(parsed.resolved_interface.primary_export_name, "Board");
        assert_eq!(
            parsed.resolved_interface.exported_types[0].export_name,
            "Board"
        );
        assert_eq!(
            parsed.resolved_interface.exported_methods[0].receiver,
            "associated"
        );
        assert_eq!(
            parsed.resolved_interface.exported_methods[0].failure_shape,
            "result"
        );
        assert_eq!(parsed.type_decisions[0].semantic_type, "width");
        assert_eq!(
            parsed.ambiguity_report[0].subject,
            "new -> Result<Board, E>"
        );
        assert_eq!(
            parsed.decision_sources[0].subject,
            "Constructor is fallible"
        );
    }

    #[test]
    fn canonicalizes_legacy_placeholder_result_types_to_anyhow() {
        let legacy = json!({
          "resolved_interface": {
            "version": "reen.interface/v2",
            "interface_fingerprint": "fp",
            "primary_export_name": "Board",
            "artifact_kind": "data_module",
            "exported_types": [
              {
                "type_name": "Board",
                "kind": "struct",
                "fields": []
              }
            ],
            "exported_methods": [
              {
                "method_name": "new",
                "receiver": null,
                "params": [],
                "return_type": "std::result::Result<Board, _>",
                "is_constructor": true
              }
            ],
            "role_method_exports": [],
            "name_bindings": []
          },
          "type_decisions": [
            {
              "field": "new return type",
              "chosen_type": "std::result::Result<Board, _>",
              "rationale": "legacy rationale"
            }
          ],
          "name_bindings": [],
          "dependency_bindings": [],
          "ambiguity_report": [
            {
              "site": "new -> Result<Board, E>",
              "issue": "The error slot cannot be resolved to a manifest-backed type without guessing. It is left as an opaque '_' to be filled by the implementation."
            }
          ],
          "decision_sources": []
        });

        let parsed = parse_interface_resolution_output(&legacy.to_string()).expect("parse");
        assert_eq!(
            parsed.resolved_interface.exported_methods[0].return_type,
            "anyhow::Result<Board>"
        );
        assert_eq!(parsed.type_decisions[0].rust_type, "anyhow::Result<Board>");
        assert!(parsed.ambiguity_report.is_empty());
    }

    #[test]
    fn normalizes_legacy_dependency_bindings_with_bound_methods() {
        let legacy = json!({
          "resolved_interface": {
            "version": "reen.interface/v2",
            "interface_fingerprint": "fp",
            "primary_export_name": "GameLoopContext",
            "artifact_kind": "context_module",
            "exported_types": [],
            "exported_methods": [],
            "role_method_exports": [],
            "name_bindings": []
          },
          "type_decisions": [],
          "name_bindings": [],
          "dependency_bindings": [
            {
              "role": "command",
              "resolved_type": "CommandInputContext",
              "source_interface": "CommandInputContext",
              "bound_methods": [
                {
                  "role_method": "capture",
                  "upstream_method": "capture",
                  "upstream_signature": "pub fn capture()"
                },
                {
                  "role_method": "next_action",
                  "upstream_method": "next_action",
                  "upstream_signature": "pub fn next_action() -> Option<UserAction>"
                }
              ]
            }
          ],
          "ambiguity_report": [],
          "decision_sources": []
        });

        let parsed = parse_interface_resolution_output(&legacy.to_string()).expect("parse");
        assert_eq!(parsed.dependency_bindings.len(), 1);
        let binding = &parsed.dependency_bindings[0];
        assert_eq!(binding.semantic_dependency, "command");
        assert_eq!(binding.rust_dependency, "CommandInputContext");
        assert_eq!(binding.interface_name, "CommandInputContext");
        assert_eq!(binding.spec_path, "");
        assert_eq!(binding.method_bindings.len(), 2);
        assert_eq!(binding.method_bindings[0].role_method, "command.capture");
        assert_eq!(binding.method_bindings[0].upstream_method, "capture");
        assert_eq!(
            binding.method_bindings[1].role_method,
            "command.next_action"
        );
        assert_eq!(binding.method_bindings[1].upstream_method, "next_action");
    }
}
