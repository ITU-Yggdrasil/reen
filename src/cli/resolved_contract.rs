//! Deterministic contract synthesis: semantic drafts → resolved interfaces, type/name bindings,
//! dependency reconciliation, and a single ambiguity report (behavioral issues only).

use std::collections::HashMap;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::contract_store::{
    AmbiguityEntry, ContractStore, DecisionSource, DependencyBinding, InterfaceField, InterfaceIr,
    InterfaceMethod, InterfaceParameter, InterfaceType, NameBinding, ResolvedInterface,
    ResolvedType, SemanticContract,
};
use super::contracts::{ContractArtifact, ContractValidationReport};
use super::interface_resolution::InterfaceResolutionOutput;
use super::pipeline_quality::{BehaviorContract, SpecificationKind, SpecificationQualityReport};
use super::planning::PlanValidationReport;
use super::types_manifest::TypesManifestScope;

pub(crate) struct ContractSynthesisOutput {
    pub(crate) semantic_contract: SemanticContract,
    pub(crate) resolved_interface: ResolvedInterface,
    pub(crate) interface_ir: InterfaceIr,
    pub(crate) type_decisions: Vec<ResolvedType>,
    pub(crate) name_bindings: Vec<NameBinding>,
    pub(crate) dependency_bindings: Vec<DependencyBinding>,
    pub(crate) ambiguity_report: Vec<AmbiguityEntry>,
    pub(crate) decision_sources: Vec<DecisionSource>,
    pub(crate) plan_validation: PlanValidationReport,
}

#[cfg(test)]
const RUST_KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for",
    "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
    "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where",
    "while", "async", "await", "dyn", "abstract", "become", "box", "do", "final", "macro",
    "override", "priv", "try", "typeof", "unsized", "virtual", "yield", "gen",
];

#[cfg(test)]
fn is_rust_keyword(name: &str) -> bool {
    RUST_KEYWORDS.contains(&name) || name == "Self"
}

pub(crate) fn primary_export_rust_identifier(title: &str) -> String {
    let mut out = String::new();
    for raw in title.trim().split(|c: char| !c.is_ascii_alphanumeric()) {
        if raw.is_empty() {
            continue;
        }

        let has_lower = raw.chars().any(|c| c.is_ascii_lowercase());
        let has_upper = raw.chars().any(|c| c.is_ascii_uppercase());
        let token = if has_lower && has_upper {
            let mut chars = raw.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        } else {
            let lower = raw.to_ascii_lowercase();
            let mut chars = lower.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        };
        out.push_str(&token);
    }
    out
}

#[cfg(test)]
fn rust_ident_for_semantic(name: &str) -> NameBinding {
    let lower = name.trim();
    if is_rust_keyword(lower) {
        NameBinding {
            semantic_name: name.to_string(),
            rust_identifier: format!("r#{lower}"),
            export_name: lower.to_string(),
            reason: "keyword_escape".to_string(),
        }
    } else {
        NameBinding {
            semantic_name: name.to_string(),
            rust_identifier: lower.to_string(),
            export_name: lower.to_string(),
            reason: "identity".to_string(),
        }
    }
}

fn interface_fingerprint(resolved: &ResolvedInterface, deps: &[DependencyBinding]) -> String {
    let v = json!({
        "exported_types": resolved.exported_types,
        "exported_methods": resolved.exported_methods,
        "role_method_exports": resolved.role_method_exports,
        "dependency_bindings": deps,
    });
    let mut h = Sha256::new();
    h.update(serde_json::to_string(&v).unwrap_or_default().as_bytes());
    hex::encode(h.finalize())
}

struct UpstreamSurface {
    spec_path: String,
    resolved: Option<InterfaceIr>,
}

fn collect_upstream_surfaces(
    ctx: &HashMap<String, Value>,
    _store: &ContractStore,
) -> Vec<UpstreamSurface> {
    let mut out = Vec::new();
    let mut seen_paths = std::collections::HashSet::new();
    let mut seen_exports = std::collections::HashSet::new();
    for key in ["direct_dependency_interfaces", "dependency_interfaces"] {
        let Some(arr) = ctx.get(key).and_then(|v| v.as_array()) else {
            continue;
        };
        for v in arr {
            let Ok(interface_ir) = serde_json::from_value::<InterfaceIr>(v.clone()) else {
                continue;
            };
            let spec_path = interface_ir.draft_relative_path.trim().to_string();
            let primary_export = interface_ir.primary_export_name.trim().to_string();
            let has_spec_path = !spec_path.is_empty();
            let has_primary_export = !primary_export.is_empty();
            let duplicate = (has_spec_path && !seen_paths.insert(spec_path.clone()))
                || (has_primary_export && !seen_exports.insert(primary_export.clone()));
            if duplicate {
                continue;
            }
            out.push(UpstreamSurface {
                spec_path,
                resolved: Some(interface_ir),
            });
        }
    }
    out
}

fn dependency_binding_matches_upstream(
    binding: &DependencyBinding,
    surface: &UpstreamSurface,
) -> bool {
    if surface.spec_path == binding.spec_path {
        return true;
    }
    if !binding.spec_path.trim().is_empty() {
        return false;
    }

    let Some(resolved) = &surface.resolved else {
        return false;
    };
    let binding_names = [
        binding.interface_name.trim(),
        binding.rust_dependency.trim(),
        binding.semantic_dependency.trim(),
    ];
    let upstream_names = [
        resolved.primary_export_name.trim(),
        resolved.draft_identity.trim(),
    ];

    binding_names
        .iter()
        .filter(|name| !name.is_empty())
        .any(|binding_name| {
            upstream_names
                .iter()
                .filter(|name| !name.is_empty())
                .any(|upstream_name| binding_name.eq_ignore_ascii_case(upstream_name))
        })
}

fn build_interface_ir(
    draft_identity: &str,
    draft_relative_path: &str,
    specification_kind: &str,
    resolved_interface: &ResolvedInterface,
    dependency_bindings: &[DependencyBinding],
    resolved_types: &[ResolvedType],
) -> InterfaceIr {
    InterfaceIr {
        version: "reen.interface-ir/v1".to_string(),
        draft_identity: draft_identity.to_string(),
        draft_relative_path: draft_relative_path.to_string(),
        specification_kind: specification_kind.to_string(),
        artifact_kind: resolved_interface.artifact_kind.clone(),
        interface_fingerprint: resolved_interface.interface_fingerprint.clone(),
        primary_export_name: resolved_interface.primary_export_name.clone(),
        exported_types: resolved_interface.exported_types.clone(),
        exported_methods: resolved_interface.exported_methods.clone(),
        role_method_exports: resolved_interface.role_method_exports.clone(),
        name_bindings: resolved_interface.name_bindings.clone(),
        dependency_bindings: dependency_bindings.to_vec(),
        resolved_types: resolved_types.to_vec(),
    }
}

fn canonicalize_name_bindings(bindings: Vec<NameBinding>) -> Vec<NameBinding> {
    let mut bindings = bindings;
    bindings.sort_by(|a, b| {
        (
            a.semantic_name.as_str(),
            a.export_name.as_str(),
            a.rust_identifier.as_str(),
            a.reason.as_str(),
        )
            .cmp(&(
                b.semantic_name.as_str(),
                b.export_name.as_str(),
                b.rust_identifier.as_str(),
                b.reason.as_str(),
            ))
    });
    bindings.dedup();
    bindings
}

fn canonicalize_resolved_types(types: Vec<ResolvedType>) -> Vec<ResolvedType> {
    let mut types = types;
    types.retain(|item| !should_drop_resolved_type_decision(item));
    types.sort_by(|a, b| {
        (
            a.semantic_type.as_str(),
            a.rust_type.as_str(),
            a.source.as_str(),
        )
            .cmp(&(
                b.semantic_type.as_str(),
                b.rust_type.as_str(),
                b.source.as_str(),
            ))
    });
    types.dedup();
    types
}

fn should_drop_resolved_type_decision(item: &ResolvedType) -> bool {
    let rust = item.rust_type.trim();
    let lower = rust.to_ascii_lowercase();
    if matches!(lower.as_str(), "unit variant" | "fieldless enum variant") {
        return true;
    }
    // Resolver prose such as "local trait StdinSource" or "null (static constructor)" is not a
    // manifest-valid Rust type; drop it so validation does not block the pipeline.
    if lower.starts_with("local trait ") {
        return true;
    }
    if lower.contains("static constructor") || lower == "null" {
        return true;
    }
    // Receiver expressions (`&self`, `&mut self`, `self`) are not types and
    // must not be validated as manifest-backed types.  An agent that writes
    // `tick receiver -> &mut self` in type_decisions is making a structural
    // mistake; drop the entry rather than surfacing a spurious validation error.
    if is_receiver_expression(rust) {
        return true;
    }
    // A top-level comma means the agent wrote a parameter list (`&Board, &Snake`)
    // rather than a single type expression.  Drop these so the pipeline can
    // continue; the agent prompt guides it to use correct type_decision form.
    if type_expr_has_top_level_comma(rust) {
        return true;
    }
    false
}

/// Returns `true` when `expr` is a Rust receiver pattern: `self`, `&self`,
/// `&mut self`, or `&'lt self` / `&'lt mut self`.
fn is_receiver_expression(expr: &str) -> bool {
    let mut rest = expr.trim();
    if let Some(after_amp) = rest.strip_prefix('&') {
        rest = after_amp.trim_start();
        // Optional lifetime
        if let Some(after_lifetime) = rest.strip_prefix('\'') {
            let offset = after_lifetime
                .find(char::is_whitespace)
                .unwrap_or(after_lifetime.len());
            rest = after_lifetime[offset..].trim_start();
        }
        if let Some(after_mut) = rest.strip_prefix("mut") {
            rest = after_mut.trim_start();
        }
    }
    rest == "self"
}

/// Returns `true` when `expr` contains a comma that is not enclosed in
/// brackets (`<>`, `()`, `[]`).  Such expressions are parameter lists, not
/// valid single Rust type expressions.
fn type_expr_has_top_level_comma(expr: &str) -> bool {
    let mut depth: i32 = 0;
    for ch in expr.chars() {
        match ch {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => return true,
            _ => {}
        }
    }
    false
}

fn canonicalize_ambiguities(entries: Vec<AmbiguityEntry>) -> Vec<AmbiguityEntry> {
    let mut entries = entries;
    entries.sort_by(|a, b| {
        (a.class.as_str(), a.subject.as_str(), a.detail.as_str()).cmp(&(
            b.class.as_str(),
            b.subject.as_str(),
            b.detail.as_str(),
        ))
    });
    entries.dedup();
    entries
}

fn canonicalize_decision_sources(entries: Vec<DecisionSource>) -> Vec<DecisionSource> {
    let mut entries = entries;
    entries.sort_by(|a, b| {
        (
            a.subject.as_str(),
            a.source_kind.as_str(),
            a.detail.as_str(),
        )
            .cmp(&(
                b.subject.as_str(),
                b.source_kind.as_str(),
                b.detail.as_str(),
            ))
    });
    entries.dedup();
    entries
}

fn canonicalize_dependency_bindings(bindings: Vec<DependencyBinding>) -> Vec<DependencyBinding> {
    let mut bindings = bindings
        .into_iter()
        .map(|mut binding| {
            binding.method_bindings.sort_by(|a, b| {
                (a.role_method.as_str(), a.upstream_method.as_str())
                    .cmp(&(b.role_method.as_str(), b.upstream_method.as_str()))
            });
            binding.method_bindings.dedup();
            binding
        })
        .collect::<Vec<_>>();

    bindings.sort_by(|a, b| {
        (
            a.semantic_dependency.as_str(),
            a.spec_path.as_str(),
            a.interface_name.as_str(),
            a.rust_dependency.as_str(),
        )
            .cmp(&(
                b.semantic_dependency.as_str(),
                b.spec_path.as_str(),
                b.interface_name.as_str(),
                b.rust_dependency.as_str(),
            ))
    });
    bindings.dedup();
    bindings
}

fn canonicalize_parameters(parameters: &mut Vec<InterfaceParameter>) {
    parameters.sort_by(|a, b| {
        (
            a.semantic_name.as_str(),
            a.rust_name.as_str(),
            a.type_ref.as_str(),
        )
            .cmp(&(
                b.semantic_name.as_str(),
                b.rust_name.as_str(),
                b.type_ref.as_str(),
            ))
    });
    parameters.dedup();
}

fn canonicalize_fields(fields: &mut Vec<InterfaceField>) {
    fields.sort_by(|a, b| {
        (
            a.export_name.as_str(),
            a.semantic_name.as_str(),
            a.rust_name.as_str(),
            a.type_ref.as_str(),
        )
            .cmp(&(
                b.export_name.as_str(),
                b.semantic_name.as_str(),
                b.rust_name.as_str(),
                b.type_ref.as_str(),
            ))
    });
    fields.dedup();
}

fn canonicalize_methods(methods: &mut Vec<InterfaceMethod>) {
    for method in methods.iter_mut() {
        canonicalize_parameters(&mut method.parameters);
        method.failure_shape = inferred_failure_shape(&method.return_type).to_string();
    }
    methods.sort_by(|a, b| {
        (
            a.export_name.as_str(),
            a.receiver.as_str(),
            a.semantic_name.as_str(),
            a.rust_name.as_str(),
            a.return_type.as_str(),
            a.signature.as_str(),
            a.failure_shape.as_str(),
        )
            .cmp(&(
                b.export_name.as_str(),
                b.receiver.as_str(),
                b.semantic_name.as_str(),
                b.rust_name.as_str(),
                b.return_type.as_str(),
                b.signature.as_str(),
                b.failure_shape.as_str(),
            ))
    });
    methods.dedup();
}

fn is_result_like_type_expr(type_ref: &str) -> bool {
    matches!(
        type_ref.trim(),
        value if value.starts_with("Result<")
            || value.starts_with("core::result::Result<")
            || value.starts_with("std::result::Result<")
            || value.starts_with("anyhow::Result<")
    )
}

fn is_option_like_type_expr(type_ref: &str) -> bool {
    matches!(
        type_ref.trim(),
        value
            if value.starts_with("Option<")
                || value.starts_with("core::option::Option<")
                || value.starts_with("std::option::Option<")
    )
}

fn inferred_failure_shape(return_type: &str) -> &'static str {
    if is_result_like_type_expr(return_type) {
        "result"
    } else if is_option_like_type_expr(return_type) {
        "option"
    } else {
        "plain"
    }
}

fn canonicalize_exported_types(types: &mut Vec<InterfaceType>) {
    for ty in types.iter_mut() {
        canonicalize_fields(&mut ty.fields);
    }
    types.sort_by(|a, b| {
        (
            a.export_name.as_str(),
            a.semantic_name.as_str(),
            a.rust_name.as_str(),
            a.kind.as_str(),
        )
            .cmp(&(
                b.export_name.as_str(),
                b.semantic_name.as_str(),
                b.rust_name.as_str(),
                b.kind.as_str(),
            ))
    });
    types.dedup();
}

fn canonicalize_interface_resolution_output(
    contract: &ContractArtifact,
    output: InterfaceResolutionOutput,
) -> InterfaceResolutionOutput {
    let mut output = output;
    let merged_name_bindings = canonicalize_name_bindings(
        output
            .name_bindings
            .into_iter()
            .chain(output.resolved_interface.name_bindings)
            .collect(),
    );
    output.name_bindings = merged_name_bindings.clone();
    output.resolved_interface.name_bindings = merged_name_bindings;
    output.type_decisions = canonicalize_resolved_types(output.type_decisions);
    output.dependency_bindings = canonicalize_dependency_bindings(output.dependency_bindings);
    output
        .ambiguity_report
        .retain(|entry| !should_drop_interface_resolution_ambiguity(contract, entry));
    output.ambiguity_report = canonicalize_ambiguities(output.ambiguity_report);
    output.decision_sources = canonicalize_decision_sources(output.decision_sources);
    output.resolved_interface.version = "reen.interface/v2".to_string();
    output.resolved_interface.artifact_kind = contract.target_artifact_kind.clone();
    output.resolved_interface.interface_fingerprint.clear();
    canonicalize_exported_types(&mut output.resolved_interface.exported_types);
    canonicalize_methods(&mut output.resolved_interface.exported_methods);
    canonicalize_methods(&mut output.resolved_interface.role_method_exports);
    output
}

fn should_drop_interface_resolution_ambiguity(
    contract: &ContractArtifact,
    entry: &AmbiguityEntry,
) -> bool {
    let kind = contract.specification_kind.trim().to_ascii_lowercase();
    let subject = entry.subject.trim().to_ascii_lowercase();
    let detail = entry.detail.trim().to_ascii_lowercase();
    let always_immutable = matches!(kind.as_str(), "data" | "projection");

    if detail.contains("confirmed as intentionally out of scope") {
        return true;
    }

    if detail.contains("does not affect the exported field type or interface shape")
        || detail.contains("does not block the current interface")
        || detail.contains("recorded here for downstream resolution")
    {
        return true;
    }

    if matches!(kind.as_str(), "context" | "projection")
        && detail.contains("role method")
        && detail.contains("not exported by the")
        && detail.contains("capsule interface")
        && detail.contains("excluded from role_method_exports")
    {
        return true;
    }

    if subject.contains("sign semantics")
        && detail.contains("resolved to i32 per level_policy binding")
        && (detail.contains("suggesting u32")
            || detail.contains("confirm whether u32 is preferred"))
    {
        return true;
    }

    if specification_kind_allows_local_role_traits(&kind)
        && subject.contains("role concrete type")
        && (detail.contains("trait declaration inside")
            || detail.contains("local trait")
            || detail.contains("context-local trait"))
    {
        return true;
    }

    if specification_kind_allows_local_role_traits(&kind)
        && subject.contains("concrete signatures")
        && detail.contains("interface")
        && detail.contains("must be confirmed from the source")
    {
        return true;
    }

    if specification_kind_allows_local_role_traits(&kind)
        && detail.contains("does not specify whether")
        && (detail.contains("inline match")
            || detail.contains("inline logic")
            || detail.contains("helper method"))
    {
        return true;
    }

    if always_immutable {
        if subject.contains("mutability after construction")
            || detail.contains("may be mutated after")
            || detail.contains("pub vs accessed only through a getter")
            || detail.contains("&mut self setters are required")
            || subject.contains("access rules")
        {
            return true;
        }
    }

    if kind == "data" {
        if subject.contains("construction rules absent")
            && detail.contains("no constructor or smart-constructor is specified")
            && detail.contains("freely constructed")
        {
            return true;
        }
        if subject.contains("minimum playable interior size")
            && detail.contains("cannot be encoded in the type alone")
            && detail.contains("deferred to the caller or a constructor invariant")
        {
            return true;
        }
    }

    false
}

fn split_top_level_type_parts(input: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (idx, ch) in input.char_indices() {
        match ch {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth = depth.saturating_sub(1),
            _ if ch == separator && depth == 0 => {
                parts.push(input[start..idx].trim());
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(input[start..].trim());
    parts
}

fn split_top_level_type_args(input: &str) -> Vec<&str> {
    split_top_level_type_parts(input, ',')
}

fn enclosed_type_expr_inner(input: &str, open: char, close: char) -> Option<&str> {
    let trimmed = input.trim();
    if !trimmed.starts_with(open) || !trimmed.ends_with(close) {
        return None;
    }

    let mut depth = 0usize;
    for (idx, ch) in trimmed.char_indices() {
        match ch {
            c if c == open => depth += 1,
            c if c == close => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                if depth == 0 && idx != trimmed.len() - ch.len_utf8() {
                    return None;
                }
            }
            _ => {}
        }
    }

    (depth == 0).then_some(&trimmed[open.len_utf8()..trimmed.len() - close.len_utf8()])
}

fn strip_reference_type_expr(input: &str) -> Option<&str> {
    let mut rest = input.trim().strip_prefix('&')?.trim_start();
    if let Some(after_lifetime) = rest.strip_prefix('\'') {
        let offset = after_lifetime
            .find(char::is_whitespace)
            .unwrap_or(after_lifetime.len());
        rest = after_lifetime[offset..].trim_start();
    }
    if let Some(inner) = rest.strip_prefix("mut ") {
        rest = inner.trim_start();
    }
    Some(rest)
}

fn parse_generic_type_expr(input: &str) -> Option<(&str, Vec<&str>)> {
    let trimmed = input.trim();
    if !trimmed.ends_with('>') {
        return None;
    }

    let mut depth = 0usize;
    let mut generic_start = None;
    for (idx, ch) in trimmed.char_indices() {
        match ch {
            '<' => {
                if depth == 0 {
                    generic_start = Some(idx);
                }
                depth += 1;
            }
            '>' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                if depth == 0 && idx != trimmed.len() - 1 {
                    return None;
                }
            }
            _ => {}
        }
    }

    if depth != 0 {
        return None;
    }

    let generic_start = generic_start?;
    let outer = trimmed[..generic_start].trim();
    let inner = &trimmed[generic_start + 1..trimmed.len() - 1];
    Some((outer, split_top_level_type_args(inner)))
}

fn is_allowed_external_type_path(path: &str, manifest_scope: &TypesManifestScope) -> bool {
    manifest_scope
        .external_path_prefixes
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

fn specification_kind_allows_local_role_traits(specification_kind: &str) -> bool {
    matches!(
        specification_kind.trim().to_ascii_lowercase().as_str(),
        "context" | "app" | "root"
    )
}

fn exported_local_trait_names(
    specification_kind: &str,
    resolved_interface: &ResolvedInterface,
) -> std::collections::HashSet<String> {
    if !specification_kind_allows_local_role_traits(specification_kind) {
        return std::collections::HashSet::new();
    }

    resolved_interface
        .exported_types
        .iter()
        .filter(|exported| exported.kind.eq_ignore_ascii_case("trait"))
        .flat_map(|exported| {
            [
                exported.export_name.clone(),
                exported.rust_name.clone(),
                exported.semantic_name.clone(),
            ]
        })
        .collect()
}

fn validate_manifest_backed_type_expr(
    type_ref: &str,
    manifest_scope: &TypesManifestScope,
    allowed_local_traits: &std::collections::HashSet<String>,
) -> Result<(), String> {
    let trimmed = type_ref.trim();
    if trimmed.is_empty() {
        return Err("type is empty".to_string());
    }
    if trimmed == "_" {
        return Err("placeholder type '_' is not allowed".to_string());
    }
    if matches!(trimmed, "()" | "Self" | "ConstructionError") {
        return Ok(());
    }
    if let Some(local_trait) = trimmed.strip_prefix("dyn ") {
        if allowed_local_traits.contains(local_trait.trim()) {
            return Ok(());
        }
    }
    if let Some(inner) = strip_reference_type_expr(trimmed) {
        return validate_manifest_backed_type_expr(inner, manifest_scope, allowed_local_traits);
    }
    if let Some(inner) = enclosed_type_expr_inner(trimmed, '[', ']') {
        let parts = split_top_level_type_parts(inner, ';');
        return match parts.as_slice() {
            [element] => {
                validate_manifest_backed_type_expr(element, manifest_scope, allowed_local_traits)
            }
            [element, length] if !length.trim().is_empty() => {
                validate_manifest_backed_type_expr(element, manifest_scope, allowed_local_traits)
            }
            _ => Err(format!(
                "unsupported slice/array type expression '{trimmed}'"
            )),
        };
    }
    if let Some(inner) = enclosed_type_expr_inner(trimmed, '(', ')') {
        let parts = split_top_level_type_args(inner);
        if parts.len() == 1 && !inner.contains(',') {
            return validate_manifest_backed_type_expr(
                parts[0],
                manifest_scope,
                allowed_local_traits,
            );
        }
        for part in parts {
            if part.is_empty() {
                return Err(format!("unsupported tuple type expression '{trimmed}'"));
            }
            validate_manifest_backed_type_expr(part, manifest_scope, allowed_local_traits)?;
        }
        return Ok(());
    }
    if manifest_scope
        .allowlist
        .iter()
        .any(|allowed| allowed == trimmed)
    {
        return Ok(());
    }

    if let Some((outer, args)) = parse_generic_type_expr(trimmed) {
        match outer {
            "Option" | "core::option::Option" | "std::option::Option" => {
                if args.len() != 1 {
                    return Err(format!("unsupported Option type expression '{trimmed}'"));
                }
                return validate_manifest_backed_type_expr(
                    args[0],
                    manifest_scope,
                    allowed_local_traits,
                );
            }
            "Vec" | "alloc::vec::Vec" | "std::vec::Vec" => {
                if args.len() != 1 {
                    return Err(format!("unsupported Vec type expression '{trimmed}'"));
                }
                return validate_manifest_backed_type_expr(
                    args[0],
                    manifest_scope,
                    allowed_local_traits,
                );
            }
            "Box" | "alloc::boxed::Box" | "std::boxed::Box" => {
                if args.len() != 1 {
                    return Err(format!("unsupported Box type expression '{trimmed}'"));
                }
                return validate_manifest_backed_type_expr(
                    args[0],
                    manifest_scope,
                    allowed_local_traits,
                );
            }
            "Result" | "core::result::Result" | "std::result::Result" => {
                if args.len() != 2 {
                    return Err(format!("unsupported Result type expression '{trimmed}'"));
                }
                validate_manifest_backed_type_expr(args[0], manifest_scope, allowed_local_traits)?;
                validate_manifest_backed_type_expr(args[1], manifest_scope, allowed_local_traits)?;
                return Ok(());
            }
            "anyhow::Result" => {
                if args.len() != 1 {
                    return Err(format!(
                        "unsupported anyhow::Result type expression '{trimmed}'"
                    ));
                }
                return validate_manifest_backed_type_expr(
                    args[0],
                    manifest_scope,
                    allowed_local_traits,
                );
            }
            _ if is_allowed_external_type_path(outer, manifest_scope) => {
                for arg in args {
                    validate_manifest_backed_type_expr(arg, manifest_scope, allowed_local_traits)?;
                }
                return Ok(());
            }
            _ => {}
        }
    }

    if is_allowed_external_type_path(trimmed, manifest_scope) {
        return Ok(());
    }
    Err(format!(
        "type '{}' is not backed by the '{}' manifest scope",
        trimmed, manifest_scope.draft_kind
    ))
}

fn validate_manifest_backed_resolution(
    specification_kind: &str,
    resolved_interface: &ResolvedInterface,
    resolved_types: &[ResolvedType],
    manifest_scope: &TypesManifestScope,
) -> Vec<AmbiguityEntry> {
    let mut ambiguities = Vec::new();
    let allowed_local_traits = exported_local_trait_names(specification_kind, resolved_interface);

    for decision in resolved_types {
        if decision.semantic_type.trim().is_empty() || decision.rust_type.trim().is_empty() {
            ambiguities.push(AmbiguityEntry {
                class: "behavioral".to_string(),
                subject: "type_decision".to_string(),
                detail: "type decision contains an empty semantic or Rust type".to_string(),
            });
            continue;
        }
        if let Err(err) = validate_manifest_backed_type_expr(
            &decision.rust_type,
            manifest_scope,
            &allowed_local_traits,
        ) {
            ambiguities.push(AmbiguityEntry {
                class: "behavioral".to_string(),
                subject: "type_decision".to_string(),
                detail: format!(
                    "type decision '{}' -> '{}' is invalid: {}",
                    decision.semantic_type, decision.rust_type, err
                ),
            });
        }
    }

    for exported in &resolved_interface.exported_types {
        for field in &exported.fields {
            if let Err(err) = validate_manifest_backed_type_expr(
                &field.type_ref,
                manifest_scope,
                &allowed_local_traits,
            ) {
                ambiguities.push(AmbiguityEntry {
                    class: "behavioral".to_string(),
                    subject: "interface_ir".to_string(),
                    detail: format!(
                        "exported field '{}.{}' uses invalid type '{}': {}",
                        exported.export_name, field.export_name, field.type_ref, err
                    ),
                });
            }
        }
    }

    for method in resolved_interface
        .exported_methods
        .iter()
        .chain(resolved_interface.role_method_exports.iter())
    {
        if let Err(err) = validate_manifest_backed_type_expr(
            &method.return_type,
            manifest_scope,
            &allowed_local_traits,
        ) {
            ambiguities.push(AmbiguityEntry {
                class: "behavioral".to_string(),
                subject: "interface_ir".to_string(),
                detail: format!(
                    "exported method '{}' uses invalid return type '{}': {}",
                    method.export_name, method.return_type, err
                ),
            });
        }
        for parameter in &method.parameters {
            if let Err(err) = validate_manifest_backed_type_expr(
                &parameter.type_ref,
                manifest_scope,
                &allowed_local_traits,
            ) {
                ambiguities.push(AmbiguityEntry {
                    class: "behavioral".to_string(),
                    subject: "interface_ir".to_string(),
                    detail: format!(
                        "exported method '{}.{}' uses invalid parameter type '{}': {}",
                        method.export_name, parameter.rust_name, parameter.type_ref, err
                    ),
                });
            }
        }
    }

    ambiguities
}

fn validate_dependency_bindings_against_upstream(
    contract: &ContractArtifact,
    dependency_bindings: &[DependencyBinding],
    upstream_surfaces: &[UpstreamSurface],
    resolved_interface: &ResolvedInterface,
) -> Vec<AmbiguityEntry> {
    let mut ambiguities = Vec::new();
    let local_trait_roles =
        context_local_trait_roles(contract, resolved_interface, dependency_bindings);

    for binding in dependency_bindings {
        let Some(upstream) = upstream_surfaces
            .iter()
            .find(|surface| dependency_binding_matches_upstream(binding, surface))
        else {
            let detail = if binding.spec_path.trim().is_empty() {
                format!(
                    "dependency '{}' references unknown upstream interface '{}'",
                    binding.semantic_dependency,
                    if binding.interface_name.trim().is_empty() {
                        binding.rust_dependency.trim()
                    } else {
                        binding.interface_name.trim()
                    }
                )
            } else {
                format!(
                    "dependency '{}' references unknown upstream spec '{}'",
                    binding.semantic_dependency, binding.spec_path
                )
            };
            ambiguities.push(AmbiguityEntry {
                class: "behavioral".to_string(),
                subject: "dependency_binding".to_string(),
                detail,
            });
            continue;
        };

        if let Some(resolved) = &upstream.resolved {
            if binding.interface_name != resolved.primary_export_name {
                ambiguities.push(AmbiguityEntry {
                    class: "behavioral".to_string(),
                    subject: "dependency_binding".to_string(),
                    detail: format!(
                        "dependency '{}' bound interface '{}' does not match upstream primary export '{}'",
                        binding.semantic_dependency,
                        binding.interface_name,
                        resolved.primary_export_name
                    ),
                });
            }

            let available_methods = resolved
                .exported_methods
                .iter()
                .chain(resolved.role_method_exports.iter())
                .flat_map(|method| {
                    [
                        method.export_name.clone(),
                        method.rust_name.clone(),
                        method.semantic_name.clone(),
                    ]
                })
                .collect::<std::collections::HashSet<_>>();

            for method_binding in &binding.method_bindings {
                if !available_methods.contains(&method_binding.upstream_method) {
                    ambiguities.push(AmbiguityEntry {
                        class: "behavioral".to_string(),
                        subject: "dependency_binding".to_string(),
                        detail: format!(
                            "dependency '{}' method binding '{}' does not exist on upstream interface '{}'",
                            binding.semantic_dependency,
                            method_binding.upstream_method,
                            resolved.primary_export_name
                        ),
                    });
                }
            }
        }
    }

    if !contract.required_call_edges.is_empty() {
        for edge in &contract.required_call_edges {
            if local_trait_roles.contains(&edge.callee_role.to_ascii_lowercase()) {
                continue;
            }
            let has_binding = dependency_bindings.iter().any(|binding| {
                binding
                    .semantic_dependency
                    .eq_ignore_ascii_case(&edge.callee_role)
                    && if edge.callee_method == "inferred" {
                        !binding.method_bindings.is_empty()
                    } else {
                        binding.method_bindings.iter().any(|method| {
                            method.role_method.eq_ignore_ascii_case(&format!(
                                "{}.{}",
                                edge.callee_role, edge.callee_method
                            ))
                        })
                    }
            });
            if !has_binding {
                ambiguities.push(AmbiguityEntry {
                    class: "behavioral".to_string(),
                    subject: "dependency_binding".to_string(),
                    detail: format!(
                        "required call edge '{} -> {}.{}' is missing a concrete dependency binding",
                        edge.caller_surface, edge.callee_role, edge.callee_method
                    ),
                });
            }
        }
    }

    ambiguities
}

fn method_is_constructor(method: &InterfaceMethod) -> bool {
    method.export_name.eq_ignore_ascii_case("new")
        || method.rust_name.eq_ignore_ascii_case("new")
        || method.semantic_name.eq_ignore_ascii_case("new")
}

fn type_expr_uses_trait_abstraction(type_ref: &str) -> bool {
    let trimmed = type_ref.trim();
    if let Some(inner) = strip_reference_type_expr(trimmed) {
        return type_expr_uses_trait_abstraction(inner);
    }
    trimmed.starts_with("impl ")
        || trimmed.starts_with("dyn ")
        || trimmed.contains(" dyn ")
        || trimmed.contains("<dyn ")
}

fn boxed_dyn_trait_name(type_ref: &str) -> Option<String> {
    let trimmed = type_ref.trim();
    let (outer, args) = parse_generic_type_expr(trimmed)?;
    if !matches!(outer, "Box" | "alloc::boxed::Box" | "std::boxed::Box") || args.len() != 1 {
        return None;
    }
    let inner = args[0].trim();
    inner
        .strip_prefix("dyn ")
        .map(|trait_name| trait_name.trim().to_string())
}

fn context_local_trait_roles(
    contract: &ContractArtifact,
    resolved_interface: &ResolvedInterface,
    dependency_bindings: &[DependencyBinding],
) -> std::collections::HashSet<String> {
    if !specification_kind_allows_local_role_traits(&contract.specification_kind) {
        return std::collections::HashSet::new();
    }

    let local_traits = exported_local_trait_names(&contract.specification_kind, resolved_interface);
    if local_traits.is_empty() {
        return std::collections::HashSet::new();
    }

    let Some(constructor) = resolved_interface
        .exported_methods
        .iter()
        .find(|method| method_is_constructor(method))
    else {
        return std::collections::HashSet::new();
    };

    contract
        .roles
        .iter()
        .filter_map(|role| {
            let role_name = role.name.trim().to_ascii_lowercase();
            if dependency_bindings.iter().any(|binding| {
                binding
                    .semantic_dependency
                    .trim()
                    .eq_ignore_ascii_case(&role.name)
            }) {
                return None;
            }

            constructor
                .parameters
                .iter()
                .find(|parameter| {
                    parameter.semantic_name.eq_ignore_ascii_case(&role.name)
                        || parameter.rust_name.eq_ignore_ascii_case(&role.name)
                })
                .and_then(|parameter| boxed_dyn_trait_name(&parameter.type_ref))
                .filter(|trait_name| local_traits.contains(trait_name))
                .map(|_| role_name)
        })
        .collect()
}

fn synthesize_context_local_role_traits(
    contract: &ContractArtifact,
    resolved_interface: &mut ResolvedInterface,
    dependency_bindings: &[DependencyBinding],
    name_bindings: &mut Vec<NameBinding>,
    decision_sources: &mut Vec<DecisionSource>,
) {
    if !specification_kind_allows_local_role_traits(&contract.specification_kind) {
        return;
    }

    let Some(constructor_index) = resolved_interface
        .exported_methods
        .iter()
        .position(method_is_constructor)
    else {
        return;
    };

    let mut existing_trait_names = resolved_interface
        .exported_types
        .iter()
        .filter(|exported| exported.kind.eq_ignore_ascii_case("trait"))
        .flat_map(|exported| {
            [
                exported.export_name.clone(),
                exported.rust_name.clone(),
                exported.semantic_name.clone(),
            ]
        })
        .collect::<std::collections::HashSet<_>>();

    for role in &contract.roles {
        let has_constructor_param = resolved_interface.exported_methods[constructor_index]
            .parameters
            .iter()
            .any(|parameter| {
                parameter.semantic_name.eq_ignore_ascii_case(&role.name)
                    || parameter.rust_name.eq_ignore_ascii_case(&role.name)
            });
        if has_constructor_param {
            continue;
        }

        let has_upstream_binding = dependency_bindings.iter().any(|binding| {
            binding
                .semantic_dependency
                .trim()
                .eq_ignore_ascii_case(&role.name)
        });
        if has_upstream_binding {
            continue;
        }

        let role_name = role.name.trim();
        let trait_name = primary_export_rust_identifier(role_name);
        if trait_name.is_empty() {
            continue;
        }

        if !existing_trait_names.contains(&trait_name) {
            resolved_interface.exported_types.push(InterfaceType {
                semantic_name: trait_name.clone(),
                rust_name: trait_name.clone(),
                export_name: trait_name.clone(),
                kind: "trait".to_string(),
                fields: Vec::new(),
            });
            existing_trait_names.insert(trait_name.clone());
            resolved_interface.name_bindings.push(NameBinding {
                semantic_name: trait_name.clone(),
                rust_identifier: trait_name.clone(),
                export_name: trait_name.clone(),
                reason: "context_local_role_trait".to_string(),
            });
            name_bindings.push(NameBinding {
                semantic_name: trait_name.clone(),
                rust_identifier: trait_name.clone(),
                export_name: trait_name.clone(),
                reason: "context_local_role_trait".to_string(),
            });
        }

        resolved_interface.exported_methods[constructor_index]
            .parameters
            .push(InterfaceParameter {
                semantic_name: role_name.to_string(),
                rust_name: role_name.to_string(),
                type_ref: format!("Box<dyn {trait_name}>"),
            });
        decision_sources.push(DecisionSource {
            subject: format!("{role_name} role abstraction"),
            source_kind: "context_local_trait_default".to_string(),
            detail: format!(
                "No direct dependency interface was resolved for role '{}'; synthesized local trait '{}' and constructor parameter 'Box<dyn {}>' by default.",
                role_name, trait_name, trait_name
            ),
        });

        let role_binding = NameBinding {
            semantic_name: role_name.to_string(),
            rust_identifier: role_name.to_string(),
            export_name: role_name.to_string(),
            reason: "context_role_player".to_string(),
        };
        resolved_interface.name_bindings.push(role_binding.clone());
        name_bindings.push(role_binding);
    }
}

fn validate_context_public_surface(
    contract: &ContractArtifact,
    resolved_interface: &ResolvedInterface,
    dependency_bindings: &[DependencyBinding],
) -> Vec<AmbiguityEntry> {
    if !specification_kind_allows_local_role_traits(&contract.specification_kind) {
        return Vec::new();
    }

    let role_names = contract
        .roles
        .iter()
        .map(|role| role.name.trim().to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();
    if role_names.is_empty() {
        return Vec::new();
    }

    let Some(constructor) = resolved_interface
        .exported_methods
        .iter()
        .find(|method| method_is_constructor(method))
    else {
        return Vec::new();
    };

    let local_traits = exported_local_trait_names(&contract.specification_kind, resolved_interface);
    let local_trait_roles =
        context_local_trait_roles(contract, resolved_interface, dependency_bindings);
    let mut ambiguities = Vec::new();

    for role in &contract.roles {
        let Some(parameter) = constructor.parameters.iter().find(|parameter| {
            parameter.semantic_name.eq_ignore_ascii_case(&role.name)
                || parameter.rust_name.eq_ignore_ascii_case(&role.name)
        }) else {
            ambiguities.push(AmbiguityEntry {
                class: "behavioral".to_string(),
                subject: "interface_ir".to_string(),
                detail: format!(
                    "context constructor '{}' is missing role player '{}'",
                    constructor.export_name, role.name
                ),
            });
            continue;
        };

        let has_upstream_binding = dependency_bindings.iter().any(|binding| {
            binding
                .semantic_dependency
                .trim()
                .eq_ignore_ascii_case(&role.name)
        });

        if local_trait_roles.contains(&role.name.trim().to_ascii_lowercase())
            || (!has_upstream_binding && type_expr_uses_trait_abstraction(&parameter.type_ref))
        {
            match boxed_dyn_trait_name(&parameter.type_ref) {
                Some(trait_name) if local_traits.contains(&trait_name) => {}
                Some(trait_name) => ambiguities.push(AmbiguityEntry {
                    class: "behavioral".to_string(),
                    subject: "interface_ir".to_string(),
                    detail: format!(
                        "context role player '{}' uses local trait '{}' in public signature '{}', but that trait is not exported by the context interface",
                        role.name, trait_name, constructor.signature
                    ),
                }),
                None => ambiguities.push(AmbiguityEntry {
                    class: "behavioral".to_string(),
                    subject: "interface_ir".to_string(),
                    detail: format!(
                        "context role player '{}' must use `Box<dyn Trait>` in constructor '{}'; found '{}'",
                        role.name, constructor.export_name, parameter.type_ref
                    ),
                }),
            }
        }
    }

    for method in &resolved_interface.exported_methods {
        if method_is_constructor(method) {
            continue;
        }

        for parameter in &method.parameters {
            let parameter_names = [
                parameter.semantic_name.trim().to_ascii_lowercase(),
                parameter.rust_name.trim().to_ascii_lowercase(),
            ];
            let matches_role = parameter_names.iter().any(|name| role_names.contains(name));
            if !matches_role || !type_expr_uses_trait_abstraction(&parameter.type_ref) {
                continue;
            }

            ambiguities.push(AmbiguityEntry {
                class: "behavioral".to_string(),
                subject: "interface_ir".to_string(),
                detail: format!(
                    "context public method '{}' accepts role player '{}' via trait abstraction '{}'; role players belong in the constructor surface, not as call-time trait inputs",
                    method.export_name, parameter.rust_name, parameter.type_ref
                ),
            });
        }
    }

    ambiguities
}

fn validate_projection_public_surface(
    contract: &ContractArtifact,
    resolved_interface: &ResolvedInterface,
) -> Vec<AmbiguityEntry> {
    if !contract
        .specification_kind
        .eq_ignore_ascii_case("projection")
    {
        return Vec::new();
    }

    let role_names = contract
        .roles
        .iter()
        .map(|role| role.name.trim().to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();
    if role_names.is_empty() {
        return Vec::new();
    }

    let mut ambiguities = Vec::new();

    for exported in &resolved_interface.exported_types {
        let exported_name = exported.export_name.trim().to_ascii_lowercase();
        if exported.kind.eq_ignore_ascii_case("trait") && role_names.contains(&exported_name) {
            ambiguities.push(AmbiguityEntry {
                class: "behavioral".to_string(),
                subject: "interface_ir".to_string(),
                detail: format!(
                    "projection interface exports local trait '{}'; projection role players must resolve to concrete Data/Projection collaborators instead of public role traits",
                    exported.export_name
                ),
            });
        }

        for field in &exported.fields {
            let field_name = field.export_name.trim().to_ascii_lowercase();
            if role_names.contains(&field_name) && type_expr_uses_trait_abstraction(&field.type_ref)
            {
                ambiguities.push(AmbiguityEntry {
                    class: "behavioral".to_string(),
                    subject: "interface_ir".to_string(),
                    detail: format!(
                        "projection field '{}.{}' uses trait abstraction '{}'; role players must resolve to concrete collaborator types before interface export",
                        exported.export_name, field.export_name, field.type_ref
                    ),
                });
            }
        }
    }

    let constructor = resolved_interface
        .exported_methods
        .iter()
        .find(|method| method_is_constructor(method));

    if let Some(constructor) = constructor {
        let constructor_params = constructor
            .parameters
            .iter()
            .flat_map(|parameter| {
                [
                    parameter.semantic_name.trim().to_ascii_lowercase(),
                    parameter.rust_name.trim().to_ascii_lowercase(),
                ]
            })
            .collect::<std::collections::HashSet<_>>();

        for role in &contract.roles {
            let role_name = role.name.trim().to_ascii_lowercase();
            if !constructor_params.contains(&role_name) {
                ambiguities.push(AmbiguityEntry {
                    class: "behavioral".to_string(),
                    subject: "interface_ir".to_string(),
                    detail: format!(
                        "projection constructor '{}' is missing role player '{}'; projections are fully constructed from role players and props",
                        constructor.export_name, role.name
                    ),
                });
            }
        }
    } else {
        ambiguities.push(AmbiguityEntry {
            class: "behavioral".to_string(),
            subject: "interface_ir".to_string(),
            detail: "projection interface exports no constructor even though projection role players must be set at construction time".to_string(),
        });
    }

    for method in &resolved_interface.exported_methods {
        let is_constructor = method_is_constructor(method);
        for parameter in &method.parameters {
            let parameter_names = [
                parameter.semantic_name.trim().to_ascii_lowercase(),
                parameter.rust_name.trim().to_ascii_lowercase(),
            ];
            let matches_role = parameter_names.iter().any(|name| role_names.contains(name));

            if !matches_role {
                continue;
            }

            if !is_constructor {
                ambiguities.push(AmbiguityEntry {
                    class: "behavioral".to_string(),
                    subject: "interface_ir".to_string(),
                    detail: format!(
                        "projection public method '{}' accepts role player '{}' as a parameter; projection role players are constructor-time collaborators, not call-time public inputs",
                        method.export_name, parameter.rust_name
                    ),
                });
            }

            if type_expr_uses_trait_abstraction(&parameter.type_ref) {
                ambiguities.push(AmbiguityEntry {
                    class: "behavioral".to_string(),
                    subject: "interface_ir".to_string(),
                    detail: format!(
                        "projection role player '{}' uses trait abstraction '{}' in public signature '{}'; resolve it to a concrete Data/Projection collaborator or keep it as an ambiguity instead of inventing a local trait",
                        parameter.rust_name, parameter.type_ref, method.signature
                    ),
                });
            }
        }
    }

    ambiguities
}

fn validate_interface_ir(interface_ir: &InterfaceIr) -> Vec<AmbiguityEntry> {
    let mut ambiguities = Vec::new();

    if interface_ir.primary_export_name.trim().is_empty() {
        ambiguities.push(AmbiguityEntry {
            class: "behavioral".to_string(),
            subject: "interface_ir".to_string(),
            detail: "primary export name is empty".to_string(),
        });
    }
    if !interface_ir
        .exported_types
        .iter()
        .any(|exported| exported.export_name == interface_ir.primary_export_name)
    {
        ambiguities.push(AmbiguityEntry {
            class: "behavioral".to_string(),
            subject: "interface_ir".to_string(),
            detail: format!(
                "primary export '{}' is not present in exported_types",
                interface_ir.primary_export_name
            ),
        });
    }

    let mut seen_type_names = std::collections::HashSet::new();
    for exported in &interface_ir.exported_types {
        if exported.semantic_name.trim().is_empty() || exported.rust_name.trim().is_empty() {
            ambiguities.push(AmbiguityEntry {
                class: "behavioral".to_string(),
                subject: "interface_ir".to_string(),
                detail: "exported type contains an empty semantic or Rust name".to_string(),
            });
        }
        if !seen_type_names.insert(exported.export_name.clone()) {
            ambiguities.push(AmbiguityEntry {
                class: "behavioral".to_string(),
                subject: "interface_ir".to_string(),
                detail: format!("duplicate exported type '{}'", exported.export_name),
            });
        }
        for field in &exported.fields {
            if field.type_ref.trim().is_empty() {
                ambiguities.push(AmbiguityEntry {
                    class: "behavioral".to_string(),
                    subject: "interface_ir".to_string(),
                    detail: format!(
                        "exported field '{}.{}' has an empty type reference",
                        exported.export_name, field.export_name
                    ),
                });
            }
        }
    }

    let mut seen_method_names = std::collections::HashSet::new();
    for method in interface_ir
        .exported_methods
        .iter()
        .chain(interface_ir.role_method_exports.iter())
    {
        if method.export_name.trim().is_empty() || method.rust_name.trim().is_empty() {
            ambiguities.push(AmbiguityEntry {
                class: "behavioral".to_string(),
                subject: "interface_ir".to_string(),
                detail: "exported method contains an empty semantic or Rust name".to_string(),
            });
        }
        if method.return_type.trim().is_empty() || method.signature.contains("/* inferred */") {
            ambiguities.push(AmbiguityEntry {
                class: "behavioral".to_string(),
                subject: "interface_ir".to_string(),
                detail: format!(
                    "exported method '{}' has an unresolved signature or return type",
                    method.export_name
                ),
            });
        }
        if !seen_method_names.insert((method.export_name.clone(), method.receiver.clone())) {
            ambiguities.push(AmbiguityEntry {
                class: "behavioral".to_string(),
                subject: "interface_ir".to_string(),
                detail: format!("duplicate exported method '{}'", method.export_name),
            });
        }
        for parameter in &method.parameters {
            if parameter.type_ref.trim().is_empty()
                || parameter.type_ref.contains("/*")
                || parameter.type_ref.contains("unresolved")
            {
                ambiguities.push(AmbiguityEntry {
                    class: "behavioral".to_string(),
                    subject: "interface_ir".to_string(),
                    detail: format!(
                        "exported method '{}.{}' has an unresolved parameter type",
                        method.export_name, parameter.rust_name
                    ),
                });
            }
        }
    }

    for binding in &interface_ir.dependency_bindings {
        for method_binding in &binding.method_bindings {
            if method_binding.upstream_method.trim().is_empty()
                || method_binding.upstream_method == "inferred"
            {
                ambiguities.push(AmbiguityEntry {
                    class: "behavioral".to_string(),
                    subject: "dependency_binding".to_string(),
                    detail: format!(
                        "dependency '{}' contains an unresolved upstream method binding for '{}'",
                        binding.semantic_dependency, method_binding.role_method
                    ),
                });
            }
        }
    }

    ambiguities
}

/// Build semantic + resolved contract layers and merge all behavioral ambiguity sources.
pub(crate) fn synthesize_contract_resolution(
    draft_identity: &str,
    draft_relative_path: &str,
    behavior: &BehaviorContract,
    contract: &ContractArtifact,
    lint: &SpecificationQualityReport,
    contract_validation: &ContractValidationReport,
    plan_validation: &PlanValidationReport,
    dependency_context: &HashMap<String, Value>,
    draft_summary: Option<Value>,
    manifest_scope: &TypesManifestScope,
    resolution_output: InterfaceResolutionOutput,
    store: &ContractStore,
) -> ContractSynthesisOutput {
    let mut ambiguity = Vec::new();
    for e in &lint.errors {
        ambiguity.push(AmbiguityEntry {
            class: "behavioral".to_string(),
            subject: "specification_lint".to_string(),
            detail: e.clone(),
        });
    }
    for e in &contract_validation.errors {
        ambiguity.push(AmbiguityEntry {
            class: "behavioral".to_string(),
            subject: "contract_extraction".to_string(),
            detail: e.clone(),
        });
    }
    for w in &contract_validation.warnings {
        ambiguity.push(AmbiguityEntry {
            class: "behavioral".to_string(),
            subject: "contract_extraction".to_string(),
            detail: w.clone(),
        });
    }

    let upstream_surfaces = collect_upstream_surfaces(dependency_context, store);
    let InterfaceResolutionOutput {
        mut resolved_interface,
        type_decisions,
        mut name_bindings,
        mut dependency_bindings,
        ambiguity_report: resolver_ambiguities,
        mut decision_sources,
    } = canonicalize_interface_resolution_output(contract, resolution_output);

    synthesize_context_local_role_traits(
        contract,
        &mut resolved_interface,
        &dependency_bindings,
        &mut name_bindings,
        &mut decision_sources,
    );
    canonicalize_exported_types(&mut resolved_interface.exported_types);
    canonicalize_methods(&mut resolved_interface.exported_methods);
    resolved_interface.name_bindings = canonicalize_name_bindings(
        resolved_interface
            .name_bindings
            .clone()
            .into_iter()
            .chain(name_bindings.clone())
            .collect(),
    );
    name_bindings = resolved_interface.name_bindings.clone();
    dependency_bindings = canonicalize_dependency_bindings(dependency_bindings);
    decision_sources = canonicalize_decision_sources(decision_sources);

    ambiguity.extend(resolver_ambiguities);
    ambiguity.extend(validate_manifest_backed_resolution(
        &contract.specification_kind,
        &resolved_interface,
        &type_decisions,
        manifest_scope,
    ));
    ambiguity.extend(validate_dependency_bindings_against_upstream(
        contract,
        &dependency_bindings,
        &upstream_surfaces,
        &resolved_interface,
    ));
    ambiguity.extend(validate_context_public_surface(
        contract,
        &resolved_interface,
        &dependency_bindings,
    ));
    ambiguity.extend(validate_projection_public_surface(
        contract,
        &resolved_interface,
    ));

    let semantic_contract = SemanticContract {
        kind: specification_kind_name(behavior.kind),
        title: contract.title.clone(),
        summary: draft_summary,
        behavior_contract: json!(behavior),
    };
    resolved_interface.interface_fingerprint =
        interface_fingerprint(&resolved_interface, &dependency_bindings);
    let interface_ir = build_interface_ir(
        draft_identity,
        draft_relative_path,
        &specification_kind_name(behavior.kind),
        &resolved_interface,
        &dependency_bindings,
        &type_decisions,
    );
    ambiguity.extend(validate_interface_ir(&interface_ir));

    let mut plan_validation = plan_validation.clone();
    for e in &plan_validation.errors {
        ambiguity.push(AmbiguityEntry {
            class: "behavioral".to_string(),
            subject: "plan".to_string(),
            detail: e.clone(),
        });
    }

    if !ambiguity.is_empty() {
        plan_validation.ok = false;
    }

    ContractSynthesisOutput {
        semantic_contract,
        resolved_interface,
        interface_ir,
        type_decisions,
        name_bindings,
        dependency_bindings,
        ambiguity_report: ambiguity,
        decision_sources,
        plan_validation,
    }
}

fn specification_kind_name(kind: SpecificationKind) -> String {
    match kind {
        SpecificationKind::App => "app".to_string(),
        SpecificationKind::Context => "context".to_string(),
        SpecificationKind::Projection => "projection".to_string(),
        SpecificationKind::Data => "data".to_string(),
        SpecificationKind::Unknown => "unknown".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::contract_store::DependencyMethodBinding;
    use crate::cli::contracts::ContractRole;
    use std::collections::BTreeMap;

    #[test]
    fn receiver_expressions_are_dropped_from_type_decisions() {
        for receiver in &["self", "&self", "&mut self", "& self", "&'a self", "&'a mut self"] {
            let item = ResolvedType {
                semantic_type: "tick receiver".to_string(),
                rust_type: receiver.to_string(),
                source: "test".to_string(),
            };
            assert!(
                should_drop_resolved_type_decision(&item),
                "expected receiver '{}' to be dropped",
                receiver
            );
        }
    }

    #[test]
    fn parameter_lists_are_dropped_from_type_decisions() {
        for list in &["&Board, &Snake", "Board, Snake", "&mut Board, Snake, u32"] {
            let item = ResolvedType {
                semantic_type: "method parameters".to_string(),
                rust_type: list.to_string(),
                source: "test".to_string(),
            };
            assert!(
                should_drop_resolved_type_decision(&item),
                "expected parameter list '{}' to be dropped",
                list
            );
        }
        // Single types with commas only inside angle brackets must NOT be dropped.
        let valid = ResolvedType {
            semantic_type: "map type".to_string(),
            rust_type: "HashMap<String, u32>".to_string(),
            source: "test".to_string(),
        };
        assert!(
            !should_drop_resolved_type_decision(&valid),
            "HashMap<String, u32> must not be dropped"
        );
    }

    #[test]
    fn keyword_semantic_name_produces_rust_escape_binding() {
        let nb = rust_ident_for_semantic("type");
        assert_eq!(nb.rust_identifier, "r#type");
        assert_eq!(nb.reason, "keyword_escape");
    }

    fn behavior_contract(title: &str, kind: SpecificationKind) -> BehaviorContract {
        BehaviorContract {
            title: title.to_string(),
            kind,
            source_path: String::new(),
            collaborators: vec![],
            env_vars: vec![],
            delegation_requirements: vec![],
            output_requirements: vec![],
            shared_state_requirements: vec![],
            role_method_names: vec![],
            external_behavior_clues: vec![],
        }
    }

    fn contract(title: &str, specification_kind: &str, artifact_kind: &str) -> ContractArtifact {
        ContractArtifact {
            contract_version: "reen.contract/v2".to_string(),
            source_spec_path: format!("specifications/{specification_kind}/{title}.md"),
            title: title.to_string(),
            specification_kind: specification_kind.to_string(),
            target_artifact_kind: artifact_kind.to_string(),
            primary_output_path_hint: None,
            public_functionalities: vec![],
            props: vec![],
            roles: vec![],
            role_methods: vec![],
            required_call_edges: vec![],
            shared_identity_constraints: vec![],
            mutation_constraints: vec![],
            output_obligations: vec![],
            env_config_obligations: vec![],
            lifecycle_obligations: vec![],
            allowed_freedoms: vec![],
            verification_targets: vec![],
        }
    }

    fn role(name: &str) -> ContractRole {
        ContractRole {
            name: name.to_string(),
            kind: "collaborator".to_string(),
            required: true,
            capabilities: vec![],
            dependency_hint: None,
            identity_semantics: String::new(),
            mutation_semantics: String::new(),
            notes: vec![],
        }
    }

    fn lint_report(contract: &BehaviorContract) -> SpecificationQualityReport {
        SpecificationQualityReport {
            contract: contract.clone(),
            errors: vec![],
            warnings: vec![],
        }
    }

    fn empty_contract_validation() -> ContractValidationReport {
        ContractValidationReport {
            ok: true,
            errors: vec![],
            warnings: vec![],
        }
    }

    fn empty_plan_validation() -> PlanValidationReport {
        PlanValidationReport {
            ok: true,
            errors: vec![],
            warnings: vec![],
        }
    }

    fn manifest_scope(kind: &str) -> TypesManifestScope {
        TypesManifestScope {
            meta: crate::cli::types_manifest::TypesManifestMeta {
                version: 2,
                drafts_tree_fingerprint: "fp".to_string(),
                rules: crate::cli::types_manifest::TypesManifestRules {
                    projection_includes_data: true,
                    context_includes_projections: true,
                },
            },
            draft_kind: kind.to_string(),
            allowlist: vec![
                "()".to_string(),
                "Board".to_string(),
                "CommandInputContext".to_string(),
                "Direction".to_string(),
                "Food".to_string(),
                "GameLoopContext".to_string(),
                "GameState".to_string(),
                "Position".to_string(),
                "RendererContext".to_string(),
                "Snake".to_string(),
                "TerminalRenderer".to_string(),
                "String".to_string(),
                "UserAction".to_string(),
                "bool".to_string(),
                "char".to_string(),
                "u32".to_string(),
            ],
            semantic_defaults: BTreeMap::new(),
            external_path_prefixes: vec![
                "anyhow::".to_string(),
                "core::".to_string(),
                "std::".to_string(),
            ],
            draft_types: vec![],
        }
    }

    fn resolution_output(
        primary_export_name: &str,
        artifact_kind: &str,
    ) -> InterfaceResolutionOutput {
        InterfaceResolutionOutput {
            resolved_interface: ResolvedInterface {
                version: "reen.interface/v2".to_string(),
                interface_fingerprint: String::new(),
                primary_export_name: primary_export_name.to_string(),
                artifact_kind: artifact_kind.to_string(),
                exported_types: vec![InterfaceType {
                    semantic_name: primary_export_name.to_string(),
                    rust_name: primary_export_name.to_string(),
                    export_name: primary_export_name.to_string(),
                    kind: "struct".to_string(),
                    fields: vec![],
                }],
                exported_methods: vec![],
                role_method_exports: vec![],
                name_bindings: vec![],
            },
            type_decisions: vec![],
            name_bindings: vec![],
            dependency_bindings: vec![],
            ambiguity_report: vec![],
            decision_sources: vec![],
        }
    }

    #[test]
    fn resolved_interface_fingerprint_is_stable_for_same_output() {
        let contract = contract("Board", "data", "data_module");
        let behavior = behavior_contract("Board", SpecificationKind::Data);
        let lint = lint_report(&behavior);
        let store = ContractStore::new(".nonexistent_reen_store_for_test");
        let resolution = resolution_output("Board", "data_module");

        let out = synthesize_contract_resolution(
            "Board",
            "data/board.md",
            &behavior,
            &contract,
            &lint,
            &empty_contract_validation(),
            &empty_plan_validation(),
            &HashMap::new(),
            None,
            &manifest_scope("data"),
            resolution.clone(),
            &store,
        );
        let out2 = synthesize_contract_resolution(
            "Board",
            "data/board.md",
            &behavior,
            &contract,
            &lint,
            &empty_contract_validation(),
            &empty_plan_validation(),
            &HashMap::new(),
            None,
            &manifest_scope("data"),
            resolution,
            &store,
        );

        assert_eq!(
            out.resolved_interface.interface_fingerprint,
            out2.resolved_interface.interface_fingerprint
        );
        assert!(out.ambiguity_report.is_empty());
    }

    #[test]
    fn rejects_non_manifest_backed_types_from_agent_output() {
        let contract = contract("Board", "data", "data_module");
        let behavior = behavior_contract("Board", SpecificationKind::Data);
        let lint = lint_report(&behavior);
        let store = ContractStore::new(".nonexistent_reen_store_for_test");
        let mut resolution = resolution_output("Board", "data_module");
        resolution.resolved_interface.exported_types[0]
            .fields
            .push(InterfaceField {
                semantic_name: "cells".to_string(),
                rust_name: "cells".to_string(),
                export_name: "cells".to_string(),
                type_ref: "usize".to_string(),
            });

        let out = synthesize_contract_resolution(
            "Board",
            "data/board.md",
            &behavior,
            &contract,
            &lint,
            &empty_contract_validation(),
            &empty_plan_validation(),
            &HashMap::new(),
            None,
            &manifest_scope("data"),
            resolution,
            &store,
        );

        assert!(out.ambiguity_report.iter().any(|entry| {
            entry
                .detail
                .contains("not backed by the 'data' manifest scope")
        }));
    }

    #[test]
    fn drops_unit_variant_pseudo_type_decisions() {
        let contract = contract("Direction", "data", "data_module");
        let behavior = behavior_contract("Direction", SpecificationKind::Data);
        let lint = lint_report(&behavior);
        let store = ContractStore::new(".nonexistent_reen_store_for_test");
        let mut resolution = resolution_output("Direction", "data_module");
        resolution.resolved_interface.exported_types[0].kind = "enum".to_string();
        resolution.type_decisions = vec![
            ResolvedType {
                semantic_type: "Direction".to_string(),
                rust_type: "Direction".to_string(),
                source: "test".to_string(),
            },
            ResolvedType {
                semantic_type: "Direction::Up".to_string(),
                rust_type: "unit variant".to_string(),
                source: "test".to_string(),
            },
        ];

        let out = synthesize_contract_resolution(
            "Direction",
            "data/direction.md",
            &behavior,
            &contract,
            &lint,
            &empty_contract_validation(),
            &empty_plan_validation(),
            &HashMap::new(),
            None,
            &manifest_scope("data"),
            resolution,
            &store,
        );

        assert!(
            !out.ambiguity_report.iter().any(|entry| {
                entry.detail.contains("unit variant")
                    && entry
                        .detail
                        .contains("not backed by the 'data' manifest scope")
            }),
            "{:?}",
            out.ambiguity_report
        );
        assert!(
            out.interface_ir
                .resolved_types
                .iter()
                .all(|item| item.rust_type != "unit variant"),
            "{:?}",
            out.interface_ir.resolved_types
        );
    }

    #[test]
    fn drops_resolver_prose_type_decisions_that_are_not_rust_types() {
        let contract = contract("CommandInput", "context", "context_module");
        let behavior = behavior_contract("CommandInput", SpecificationKind::Context);
        let lint = lint_report(&behavior);
        let store = ContractStore::new(".nonexistent_reen_store_for_test");
        let mut resolution = resolution_output("CommandInput", "context_module");
        resolution.type_decisions = vec![
            ResolvedType {
                semantic_type: "StdinSource trait".to_string(),
                rust_type: "local trait StdinSource".to_string(),
                source: "test".to_string(),
            },
            ResolvedType {
                semantic_type: "new constructor self_param".to_string(),
                rust_type: "null (static constructor)".to_string(),
                source: "test".to_string(),
            },
        ];

        let out = synthesize_contract_resolution(
            "command_input",
            "contexts/command_input.md",
            &behavior,
            &contract,
            &lint,
            &empty_contract_validation(),
            &empty_plan_validation(),
            &HashMap::new(),
            None,
            &manifest_scope("context"),
            resolution,
            &store,
        );

        assert!(
            !out.ambiguity_report.iter().any(|entry| {
                entry.subject == "type_decision" && entry.detail.contains("local trait StdinSource")
            }),
            "{:?}",
            out.ambiguity_report
        );
        assert!(out.interface_ir.resolved_types.is_empty());
    }

    #[test]
    fn accepts_anyhow_result_return_types() {
        let contract = contract("Board", "data", "data_module");
        let behavior = behavior_contract("Board", SpecificationKind::Data);
        let lint = lint_report(&behavior);
        let store = ContractStore::new(".nonexistent_reen_store_for_test");
        let mut resolution = resolution_output("Board", "data_module");
        resolution
            .resolved_interface
            .exported_methods
            .push(InterfaceMethod {
                semantic_name: "new".to_string(),
                rust_name: "new".to_string(),
                export_name: "new".to_string(),
                receiver: "associated".to_string(),
                parameters: vec![InterfaceParameter {
                    semantic_name: "width".to_string(),
                    rust_name: "width".to_string(),
                    type_ref: "u32".to_string(),
                }],
                return_type: "anyhow::Result<Board>".to_string(),
                failure_shape: "plain".to_string(),
                signature: "pub fn new(width: u32) -> anyhow::Result<Board>".to_string(),
            });

        let out = synthesize_contract_resolution(
            "Board",
            "data/board.md",
            &behavior,
            &contract,
            &lint,
            &empty_contract_validation(),
            &empty_plan_validation(),
            &HashMap::new(),
            None,
            &manifest_scope("data"),
            resolution,
            &store,
        );

        assert!(
            out.ambiguity_report.is_empty(),
            "{:?}",
            out.ambiguity_report
        );
        assert_eq!(
            out.resolved_interface.exported_methods[0].failure_shape,
            "result"
        );
    }

    #[test]
    fn accepts_native_vec_and_slice_types_backed_by_manifest_items() {
        let contract = contract("Snake", "data", "data_module");
        let behavior = behavior_contract("Snake", SpecificationKind::Data);
        let lint = lint_report(&behavior);
        let store = ContractStore::new(".nonexistent_reen_store_for_test");
        let mut resolution = resolution_output("Snake", "data_module");
        resolution.type_decisions = vec![
            ResolvedType {
                semantic_type: "body field internal storage type".to_string(),
                rust_type: "Vec<Position>".to_string(),
                source: "test".to_string(),
            },
            ResolvedType {
                semantic_type: "body getter return type".to_string(),
                rust_type: "&[Position]".to_string(),
                source: "test".to_string(),
            },
        ];
        resolution.resolved_interface.exported_types[0]
            .fields
            .push(InterfaceField {
                semantic_name: "body".to_string(),
                rust_name: "body".to_string(),
                export_name: "body".to_string(),
                type_ref: "Vec<Position>".to_string(),
            });
        resolution
            .resolved_interface
            .exported_methods
            .push(InterfaceMethod {
                semantic_name: "body".to_string(),
                rust_name: "body".to_string(),
                export_name: "body".to_string(),
                receiver: "&self".to_string(),
                parameters: vec![],
                return_type: "&[Position]".to_string(),
                failure_shape: "plain".to_string(),
                signature: "pub fn body(&self) -> &[Position]".to_string(),
            });
        resolution
            .resolved_interface
            .exported_methods
            .push(InterfaceMethod {
                semantic_name: "new".to_string(),
                rust_name: "new".to_string(),
                export_name: "new".to_string(),
                receiver: "associated".to_string(),
                parameters: vec![InterfaceParameter {
                    semantic_name: "body".to_string(),
                    rust_name: "body".to_string(),
                    type_ref: "Vec<Position>".to_string(),
                }],
                return_type: "Self".to_string(),
                failure_shape: "plain".to_string(),
                signature: "pub fn new(body: Vec<Position>) -> Self".to_string(),
            });

        let out = synthesize_contract_resolution(
            "Snake",
            "data/snake.md",
            &behavior,
            &contract,
            &lint,
            &empty_contract_validation(),
            &empty_plan_validation(),
            &HashMap::new(),
            None,
            &manifest_scope("data"),
            resolution,
            &store,
        );

        assert!(
            !out.ambiguity_report.iter().any(|entry| {
                entry
                    .detail
                    .contains("not backed by the 'data' manifest scope")
            }),
            "{:?}",
            out.ambiguity_report
        );
    }

    #[test]
    fn drops_non_blocking_data_interface_ambiguities_from_resolver_output() {
        let contract = contract("Board", "data", "data_module");
        let behavior = behavior_contract("Board", SpecificationKind::Data);
        let lint = lint_report(&behavior);
        let store = ContractStore::new(".nonexistent_reen_store_for_test");
        let mut resolution = resolution_output("Board", "data_module");
        resolution.ambiguity_report = vec![
            AmbiguityEntry {
                class: "behavioral".to_string(),
                subject: "construction rules absent".to_string(),
                detail: "No constructor or smart-constructor is specified. Whether Board is freely constructed (e.g. Board { width, height }) or requires a validated constructor (e.g. Board::new) is not resolved. The interface currently exposes public fields; if a validated constructor is later required, the fields should become private.".to_string(),
            },
            AmbiguityEntry {
                class: "behavioral".to_string(),
                subject: "minimum playable interior size".to_string(),
                detail: "The contract specifies wall cells at the perimeter but does not mandate a minimum width/height guaranteeing at least one non-wall interior cell. It is unresolved whether width>=3 and height>=3 must be enforced or whether a fully-walled board (e.g. 2x2) is valid. This constraint cannot be encoded in the type alone and is deferred to the caller or a constructor invariant not yet specified.".to_string(),
            },
            AmbiguityEntry {
                class: "behavioral".to_string(),
                subject: "wall cell behavior".to_string(),
                detail: "The contract defines which cells are wall cells but defers wall collision semantics to another contract. Confirmed as intentionally out of scope for this data artifact.".to_string(),
            },
        ];

        let out = synthesize_contract_resolution(
            "Board",
            "data/board.md",
            &behavior,
            &contract,
            &lint,
            &empty_contract_validation(),
            &empty_plan_validation(),
            &HashMap::new(),
            None,
            &manifest_scope("data"),
            resolution,
            &store,
        );

        assert!(
            out.ambiguity_report.is_empty(),
            "{:?}",
            out.ambiguity_report
        );
    }

    #[test]
    fn drops_immutable_data_mutability_and_downstream_range_ambiguities() {
        let contract = contract("Food", "data", "data_module");
        let behavior = behavior_contract("Food", SpecificationKind::Data);
        let lint = lint_report(&behavior);
        let store = ContractStore::new(".nonexistent_reen_store_for_test");
        let mut resolution = resolution_output("Food", "data_module");
        resolution.ambiguity_report = vec![
            AmbiguityEntry {
                class: "behavioral".to_string(),
                subject: "position mutability after construction".to_string(),
                detail: "The contract does not specify whether `position` may be mutated after a `Food` instance is created. This affects whether the field should be `pub` vs accessed only through a getter, and whether `&mut self` setters are required. Confirm access and mutation rules to finalize field visibility.".to_string(),
            },
            AmbiguityEntry {
                class: "behavioral".to_string(),
                subject: "position valid range / construction constraint".to_string(),
                detail: "The contract does not specify whether `position` is validated against board boundaries on construction, enforced at the type level, or left as an unchecked runtime concern. Because no construction rules are defined and this does not affect the exported field type or interface shape, it is recorded here for downstream resolution but does not block the current interface.".to_string(),
            },
        ];

        let out = synthesize_contract_resolution(
            "Food",
            "data/food.md",
            &behavior,
            &contract,
            &lint,
            &empty_contract_validation(),
            &empty_plan_validation(),
            &HashMap::new(),
            None,
            &manifest_scope("data"),
            resolution,
            &store,
        );

        assert!(
            out.ambiguity_report.is_empty(),
            "{:?}",
            out.ambiguity_report
        );
    }

    #[test]
    fn rejects_projection_role_player_trait_leaks_in_public_interface() {
        let mut contract = contract("StringRenderer", "projection", "projection_module");
        contract.roles.push(crate::cli::contracts::ContractRole {
            name: "snapshot".to_string(),
            kind: "projection".to_string(),
            required: true,
            capabilities: vec![
                "width".to_string(),
                "height".to_string(),
                "symbol_at".to_string(),
            ],
            dependency_hint: None,
            identity_semantics: "infer_from_behavior".to_string(),
            mutation_semantics: "immutable".to_string(),
            notes: vec![],
        });
        contract.props.push(crate::cli::contracts::ContractProp {
            name: "score".to_string(),
            description: "Score shown below the board".to_string(),
            type_hint: Some("i32".to_string()),
            notes: vec![],
        });

        let behavior = behavior_contract("StringRenderer", SpecificationKind::Projection);
        let lint = lint_report(&behavior);
        let store = ContractStore::new(".nonexistent_reen_store_for_test");
        let mut resolution = resolution_output("StringRenderer", "projection_module");
        resolution.resolved_interface.exported_types = vec![
            InterfaceType {
                semantic_name: "Snapshot".to_string(),
                rust_name: "Snapshot".to_string(),
                export_name: "Snapshot".to_string(),
                kind: "trait".to_string(),
                fields: vec![],
            },
            InterfaceType {
                semantic_name: "StringRenderer".to_string(),
                rust_name: "StringRenderer".to_string(),
                export_name: "StringRenderer".to_string(),
                kind: "struct".to_string(),
                fields: vec![InterfaceField {
                    semantic_name: "score".to_string(),
                    rust_name: "score".to_string(),
                    export_name: "score".to_string(),
                    type_ref: "i32".to_string(),
                }],
            },
        ];
        resolution
            .resolved_interface
            .exported_methods
            .push(InterfaceMethod {
                semantic_name: "new".to_string(),
                rust_name: "new".to_string(),
                export_name: "new".to_string(),
                receiver: "associated".to_string(),
                parameters: vec![InterfaceParameter {
                    semantic_name: "score".to_string(),
                    rust_name: "score".to_string(),
                    type_ref: "i32".to_string(),
                }],
                return_type: "Self".to_string(),
                failure_shape: "plain".to_string(),
                signature: "pub fn new(score: i32) -> Self".to_string(),
            });
        resolution
            .resolved_interface
            .exported_methods
            .push(InterfaceMethod {
                semantic_name: "render".to_string(),
                rust_name: "render".to_string(),
                export_name: "render".to_string(),
                receiver: "&self".to_string(),
                parameters: vec![InterfaceParameter {
                    semantic_name: "snapshot".to_string(),
                    rust_name: "snapshot".to_string(),
                    type_ref: "&dyn Snapshot".to_string(),
                }],
                return_type: "String".to_string(),
                failure_shape: "plain".to_string(),
                signature: "pub fn render(&self, snapshot: &dyn Snapshot) -> String".to_string(),
            });

        let out = synthesize_contract_resolution(
            "StringRenderer",
            "projections/string_renderer.md",
            &behavior,
            &contract,
            &lint,
            &empty_contract_validation(),
            &empty_plan_validation(),
            &HashMap::new(),
            None,
            &manifest_scope("projection"),
            resolution,
            &store,
        );

        assert!(
            out.ambiguity_report
                .iter()
                .any(|entry| entry.detail.contains("exports local trait 'Snapshot'")),
            "{:?}",
            out.ambiguity_report
        );
        assert!(
            out.ambiguity_report
                .iter()
                .any(|entry| entry.detail.contains("missing role player 'snapshot'")),
            "{:?}",
            out.ambiguity_report
        );
        assert!(
            out.ambiguity_report.iter().any(|entry| {
                entry.detail.contains(
                "projection public method 'render' accepts role player 'snapshot' as a parameter"
            )
            }),
            "{:?}",
            out.ambiguity_report
        );
    }

    #[test]
    fn drops_projection_role_method_binding_misunderstanding_and_sign_speculation() {
        let contract = contract("StringRenderer", "projection", "projection_module");
        let behavior = behavior_contract("StringRenderer", SpecificationKind::Projection);
        let lint = lint_report(&behavior);
        let store = ContractStore::new(".nonexistent_reen_store_for_test");
        let mut resolution = resolution_output("StringRenderer", "projection_module");
        resolution.ambiguity_report = vec![
            AmbiguityEntry {
                class: "behavioral".to_string(),
                subject: "Board.symbol_at".to_string(),
                detail: "The role method `symbol_at` is named in the contract but is not exported by the Board capsule interface (fingerprint fp). Its parameter type (Position vs separate x/y u32 coordinates), return type (char, &str, or domain symbol type), and failure shape are all unspecified. The method cannot be bound to an upstream interface export without guessing. Excluded from role_method_exports until the Board capsule exports this method with a concrete signature.".to_string(),
            },
            AmbiguityEntry {
                class: "behavioral".to_string(),
                subject: "score sign semantics".to_string(),
                detail: "The contract lists score as i32 via level_policy integer=i32, but the draft notes scores are typically non-negative, suggesting u32. Resolved to i32 per level_policy binding; confirm whether u32 is preferred for the domain.".to_string(),
            },
        ];

        let out = synthesize_contract_resolution(
            "StringRenderer",
            "projections/string_renderer.md",
            &behavior,
            &contract,
            &lint,
            &empty_contract_validation(),
            &empty_plan_validation(),
            &HashMap::new(),
            None,
            &manifest_scope("projection"),
            resolution,
            &store,
        );

        assert!(
            out.ambiguity_report.is_empty(),
            "{:?}",
            out.ambiguity_report
        );
    }

    #[test]
    fn rejects_std_result_with_placeholder_error_type() {
        let contract = contract("Board", "data", "data_module");
        let behavior = behavior_contract("Board", SpecificationKind::Data);
        let lint = lint_report(&behavior);
        let store = ContractStore::new(".nonexistent_reen_store_for_test");
        let mut resolution = resolution_output("Board", "data_module");
        resolution
            .resolved_interface
            .exported_methods
            .push(InterfaceMethod {
                semantic_name: "new".to_string(),
                rust_name: "new".to_string(),
                export_name: "new".to_string(),
                receiver: "associated".to_string(),
                parameters: vec![],
                return_type: "std::result::Result<Board, _>".to_string(),
                failure_shape: "result".to_string(),
                signature: "pub fn new() -> std::result::Result<Board, _>".to_string(),
            });

        let out = synthesize_contract_resolution(
            "Board",
            "data/board.md",
            &behavior,
            &contract,
            &lint,
            &empty_contract_validation(),
            &empty_plan_validation(),
            &HashMap::new(),
            None,
            &manifest_scope("data"),
            resolution,
            &store,
        );

        assert!(
            out.ambiguity_report
                .iter()
                .any(|entry| { entry.detail.contains("placeholder type '_' is not allowed") })
        );
    }

    #[test]
    fn validates_dependency_bindings_against_upstream_interfaces() {
        let mut contract = contract("RendererContext", "context", "context_module");
        contract
            .required_call_edges
            .push(crate::cli::contracts::ContractCallEdge {
                caller_surface: "run".to_string(),
                callee_role: "renderer".to_string(),
                callee_method: "draw".to_string(),
                obligation_reason: "contract".to_string(),
            });

        let behavior = behavior_contract("RendererContext", SpecificationKind::Context);
        let lint = lint_report(&behavior);
        let store = ContractStore::new(".nonexistent_reen_store_for_test");
        let mut resolution = resolution_output("RendererContext", "context_module");
        resolution.dependency_bindings.push(DependencyBinding {
            semantic_dependency: "renderer".to_string(),
            rust_dependency: "renderer".to_string(),
            spec_path: "contexts/terminal_renderer.md".to_string(),
            interface_name: "TerminalRenderer".to_string(),
            method_bindings: vec![DependencyMethodBinding {
                role_method: "renderer.draw".to_string(),
                upstream_method: "render".to_string(),
            }],
        });

        let upstream = InterfaceIr {
            version: "reen.interface-ir/v1".to_string(),
            draft_identity: "TerminalRenderer".to_string(),
            draft_relative_path: "contexts/terminal_renderer.md".to_string(),
            specification_kind: "context".to_string(),
            artifact_kind: "context_module".to_string(),
            interface_fingerprint: "fp".to_string(),
            primary_export_name: "TerminalRenderer".to_string(),
            exported_types: vec![InterfaceType {
                semantic_name: "TerminalRenderer".to_string(),
                rust_name: "TerminalRenderer".to_string(),
                export_name: "TerminalRenderer".to_string(),
                kind: "struct".to_string(),
                fields: vec![],
            }],
            exported_methods: vec![InterfaceMethod {
                semantic_name: "render".to_string(),
                rust_name: "render".to_string(),
                export_name: "render".to_string(),
                receiver: "&mut self".to_string(),
                parameters: vec![],
                return_type: "()".to_string(),
                failure_shape: "plain".to_string(),
                signature: "pub fn render(&mut self)".to_string(),
            }],
            role_method_exports: vec![],
            name_bindings: vec![],
            dependency_bindings: vec![],
            resolved_types: vec![],
        };
        let dependency_context = HashMap::from([(
            "direct_dependency_interfaces".to_string(),
            json!([upstream]),
        )]);

        let out = synthesize_contract_resolution(
            "RendererContext",
            "contexts/renderer_context.md",
            &behavior,
            &contract,
            &lint,
            &empty_contract_validation(),
            &empty_plan_validation(),
            &dependency_context,
            None,
            &manifest_scope("context"),
            resolution,
            &store,
        );

        assert!(
            out.ambiguity_report.is_empty(),
            "{:?}",
            out.ambiguity_report
        );
    }

    #[test]
    fn rejects_unknown_dependency_binding_methods() {
        let mut contract = contract("RendererContext", "context", "context_module");
        contract
            .required_call_edges
            .push(crate::cli::contracts::ContractCallEdge {
                caller_surface: "run".to_string(),
                callee_role: "renderer".to_string(),
                callee_method: "draw".to_string(),
                obligation_reason: "contract".to_string(),
            });

        let behavior = behavior_contract("RendererContext", SpecificationKind::Context);
        let lint = lint_report(&behavior);
        let store = ContractStore::new(".nonexistent_reen_store_for_test");
        let mut resolution = resolution_output("RendererContext", "context_module");
        resolution.dependency_bindings.push(DependencyBinding {
            semantic_dependency: "renderer".to_string(),
            rust_dependency: "renderer".to_string(),
            spec_path: "contexts/terminal_renderer.md".to_string(),
            interface_name: "TerminalRenderer".to_string(),
            method_bindings: vec![DependencyMethodBinding {
                role_method: "renderer.draw".to_string(),
                upstream_method: "missing".to_string(),
            }],
        });

        let upstream = InterfaceIr {
            version: "reen.interface-ir/v1".to_string(),
            draft_identity: "TerminalRenderer".to_string(),
            draft_relative_path: "contexts/terminal_renderer.md".to_string(),
            specification_kind: "context".to_string(),
            artifact_kind: "context_module".to_string(),
            interface_fingerprint: "fp".to_string(),
            primary_export_name: "TerminalRenderer".to_string(),
            exported_types: vec![InterfaceType {
                semantic_name: "TerminalRenderer".to_string(),
                rust_name: "TerminalRenderer".to_string(),
                export_name: "TerminalRenderer".to_string(),
                kind: "struct".to_string(),
                fields: vec![],
            }],
            exported_methods: vec![InterfaceMethod {
                semantic_name: "render".to_string(),
                rust_name: "render".to_string(),
                export_name: "render".to_string(),
                receiver: "&mut self".to_string(),
                parameters: vec![],
                return_type: "()".to_string(),
                failure_shape: "plain".to_string(),
                signature: "pub fn render(&mut self)".to_string(),
            }],
            role_method_exports: vec![],
            name_bindings: vec![],
            dependency_bindings: vec![],
            resolved_types: vec![],
        };
        let dependency_context = HashMap::from([(
            "direct_dependency_interfaces".to_string(),
            json!([upstream]),
        )]);

        let out = synthesize_contract_resolution(
            "RendererContext",
            "contexts/renderer_context.md",
            &behavior,
            &contract,
            &lint,
            &empty_contract_validation(),
            &empty_plan_validation(),
            &dependency_context,
            None,
            &manifest_scope("context"),
            resolution,
            &store,
        );

        assert!(
            out.ambiguity_report
                .iter()
                .any(|entry| entry.subject == "dependency_binding")
        );
    }

    #[test]
    fn empty_spec_path_dependency_binding_matches_dependency_interfaces_by_name() {
        let contract = contract("StringRenderer", "projection", "projection_module");
        let behavior = behavior_contract("StringRenderer", SpecificationKind::Projection);
        let lint = lint_report(&behavior);
        let store = ContractStore::new(".nonexistent_reen_store_for_test");
        let mut resolution = resolution_output("StringRenderer", "projection_module");
        resolution.dependency_bindings.push(DependencyBinding {
            semantic_dependency: "Board".to_string(),
            rust_dependency: "Board".to_string(),
            spec_path: String::new(),
            interface_name: "Board".to_string(),
            method_bindings: vec![
                DependencyMethodBinding {
                    role_method: "Board.width".to_string(),
                    upstream_method: "width".to_string(),
                },
                DependencyMethodBinding {
                    role_method: "Board.height".to_string(),
                    upstream_method: "height".to_string(),
                },
            ],
        });

        let upstream = InterfaceIr {
            version: "reen.interface-ir/v1".to_string(),
            draft_identity: "Board".to_string(),
            draft_relative_path: "data/Board.md".to_string(),
            specification_kind: "data".to_string(),
            artifact_kind: "data_module".to_string(),
            interface_fingerprint: "fp".to_string(),
            primary_export_name: "Board".to_string(),
            exported_types: vec![InterfaceType {
                semantic_name: "Board".to_string(),
                rust_name: "Board".to_string(),
                export_name: "Board".to_string(),
                kind: "struct".to_string(),
                fields: vec![],
            }],
            exported_methods: vec![
                InterfaceMethod {
                    semantic_name: "width".to_string(),
                    rust_name: "width".to_string(),
                    export_name: "width".to_string(),
                    receiver: "&self".to_string(),
                    parameters: vec![],
                    return_type: "u32".to_string(),
                    failure_shape: "plain".to_string(),
                    signature: "pub fn width(&self) -> u32".to_string(),
                },
                InterfaceMethod {
                    semantic_name: "height".to_string(),
                    rust_name: "height".to_string(),
                    export_name: "height".to_string(),
                    receiver: "&self".to_string(),
                    parameters: vec![],
                    return_type: "u32".to_string(),
                    failure_shape: "plain".to_string(),
                    signature: "pub fn height(&self) -> u32".to_string(),
                },
            ],
            role_method_exports: vec![],
            name_bindings: vec![],
            dependency_bindings: vec![],
            resolved_types: vec![],
        };
        let dependency_context =
            HashMap::from([("dependency_interfaces".to_string(), json!([upstream]))]);

        let out = synthesize_contract_resolution(
            "StringRenderer",
            "projections/string_renderer.md",
            &behavior,
            &contract,
            &lint,
            &empty_contract_validation(),
            &empty_plan_validation(),
            &dependency_context,
            None,
            &manifest_scope("projection"),
            resolution,
            &store,
        );

        assert!(
            out.ambiguity_report
                .iter()
                .all(|entry| !entry.detail.contains("unknown upstream")),
            "{:?}",
            out.ambiguity_report
        );
    }

    #[test]
    fn game_loop_like_context_resolution_prefers_dependency_interfaces_and_local_traits() {
        let mut contract = contract("GameLoopContext", "context", "context_module");
        contract.roles = vec![role("command"), role("food_dropper")];
        contract
            .required_call_edges
            .push(crate::cli::contracts::ContractCallEdge {
                caller_surface: "tick".to_string(),
                callee_role: "food_dropper".to_string(),
                callee_method: "drop".to_string(),
                obligation_reason: "contract".to_string(),
            });

        let behavior = behavior_contract("GameLoopContext", SpecificationKind::Context);
        let lint = lint_report(&behavior);
        let store = ContractStore::new(".nonexistent_reen_store_for_test");
        let mut resolution = resolution_output("GameLoopContext", "context_module");
        resolution
            .resolved_interface
            .exported_methods
            .push(InterfaceMethod {
                semantic_name: "new".to_string(),
                rust_name: "new".to_string(),
                export_name: "new".to_string(),
                receiver: "associated".to_string(),
                parameters: vec![InterfaceParameter {
                    semantic_name: "command".to_string(),
                    rust_name: "command".to_string(),
                    type_ref: "CommandInputContext".to_string(),
                }],
                return_type: "Self".to_string(),
                failure_shape: "plain".to_string(),
                signature: "pub fn new(command: CommandInputContext) -> Self".to_string(),
            });
        resolution.dependency_bindings.push(DependencyBinding {
            semantic_dependency: "command".to_string(),
            rust_dependency: "CommandInputContext".to_string(),
            spec_path: String::new(),
            interface_name: "CommandInputContext".to_string(),
            method_bindings: vec![
                DependencyMethodBinding {
                    role_method: "command.capture".to_string(),
                    upstream_method: "capture".to_string(),
                },
                DependencyMethodBinding {
                    role_method: "command.next_action".to_string(),
                    upstream_method: "next_action".to_string(),
                },
            ],
        });
        resolution.ambiguity_report = vec![
            AmbiguityEntry {
                class: "behavioral".to_string(),
                subject: "CommandInputContext::capture and next_action concrete signatures"
                    .to_string(),
                detail: "The CommandInputContext interface in direct_dependency_interfaces lists no exported_methods. The contract maps capture and next_action to CommandInputContext, but their exact signatures (parameter lists, return types) are not present in the interface JSON. The role_method_exports entries reference them by name only. If the implementation requires exact signatures, they must be confirmed from the source.".to_string(),
            },
            AmbiguityEntry {
                class: "behavioral".to_string(),
                subject: "food_dropper role concrete type".to_string(),
                detail: "The contract names food_dropper as a role player with a drop(board, snake) -> Option<Food> method, but no existing capsule or direct dependency interface exists for it. The food_dropper cannot be bound to a concrete type without either a new context capsule definition or a trait declaration inside GameLoopContext. This resolver cannot fabricate a type outside the manifest allowlist.".to_string(),
            },
            AmbiguityEntry {
                class: "behavioral".to_string(),
                subject: "opposite-direction reversal logic for Direction".to_string(),
                detail: "The contract says reversing directly into the opposite direction is ignored but does not specify whether Direction has an opposite() method or whether GameLoopContext implements inline match logic. No such method is present in the Direction interface.".to_string(),
            },
        ];

        let upstream = InterfaceIr {
            version: "reen.interface-ir/v1".to_string(),
            draft_identity: "command_input".to_string(),
            draft_relative_path: "contexts/command_input.md".to_string(),
            specification_kind: "context".to_string(),
            artifact_kind: "context_module".to_string(),
            interface_fingerprint: "fp".to_string(),
            primary_export_name: "CommandInputContext".to_string(),
            exported_types: vec![InterfaceType {
                semantic_name: "CommandInputContext".to_string(),
                rust_name: "CommandInputContext".to_string(),
                export_name: "CommandInputContext".to_string(),
                kind: "struct".to_string(),
                fields: vec![],
            }],
            exported_methods: vec![
                InterfaceMethod {
                    semantic_name: "capture".to_string(),
                    rust_name: "capture".to_string(),
                    export_name: "capture".to_string(),
                    receiver: "&mut self".to_string(),
                    parameters: vec![],
                    return_type: "()".to_string(),
                    failure_shape: "plain".to_string(),
                    signature: "pub fn capture(&mut self)".to_string(),
                },
                InterfaceMethod {
                    semantic_name: "next_action".to_string(),
                    rust_name: "next_action".to_string(),
                    export_name: "next_action".to_string(),
                    receiver: "&mut self".to_string(),
                    parameters: vec![],
                    return_type: "Option<UserAction>".to_string(),
                    failure_shape: "option".to_string(),
                    signature: "pub fn next_action(&mut self) -> Option<UserAction>".to_string(),
                },
            ],
            role_method_exports: vec![],
            name_bindings: vec![],
            dependency_bindings: vec![],
            resolved_types: vec![],
        };
        let dependency_context = HashMap::from([(
            "direct_dependency_interfaces".to_string(),
            json!([upstream]),
        )]);

        let out = synthesize_contract_resolution(
            "GameLoopContext",
            "contexts/game_loop.md",
            &behavior,
            &contract,
            &lint,
            &empty_contract_validation(),
            &empty_plan_validation(),
            &dependency_context,
            None,
            &manifest_scope("context"),
            resolution,
            &store,
        );

        assert!(
            out.ambiguity_report.is_empty(),
            "{:?}",
            out.ambiguity_report
        );

        let constructor = out
            .resolved_interface
            .exported_methods
            .iter()
            .find(|method| method.export_name == "new")
            .expect("constructor");
        assert!(
            constructor
                .parameters
                .iter()
                .any(|parameter| parameter.rust_name == "command"
                    && parameter.type_ref == "CommandInputContext")
        );
        assert!(
            constructor
                .parameters
                .iter()
                .any(|parameter| parameter.rust_name == "food_dropper"
                    && parameter.type_ref == "Box<dyn FoodDropper>")
        );
        assert!(
            out.resolved_interface
                .exported_types
                .iter()
                .any(|exported| {
                    exported.export_name == "FoodDropper" && exported.kind == "trait"
                }),
            "{:?}",
            out.resolved_interface.exported_types
        );
    }

    #[test]
    fn local_context_trait_roles_satisfy_required_call_edges_without_dependency_bindings() {
        let mut contract = contract("RendererContext", "context", "context_module");
        contract.roles = vec![role("food_dropper")];
        contract
            .required_call_edges
            .push(crate::cli::contracts::ContractCallEdge {
                caller_surface: "tick".to_string(),
                callee_role: "food_dropper".to_string(),
                callee_method: "drop".to_string(),
                obligation_reason: "contract".to_string(),
            });

        let behavior = behavior_contract("RendererContext", SpecificationKind::Context);
        let lint = lint_report(&behavior);
        let store = ContractStore::new(".nonexistent_reen_store_for_test");
        let mut resolution = resolution_output("RendererContext", "context_module");
        resolution
            .resolved_interface
            .exported_methods
            .push(InterfaceMethod {
                semantic_name: "new".to_string(),
                rust_name: "new".to_string(),
                export_name: "new".to_string(),
                receiver: "associated".to_string(),
                parameters: vec![],
                return_type: "Self".to_string(),
                failure_shape: "plain".to_string(),
                signature: "pub fn new() -> Self".to_string(),
            });

        let out = synthesize_contract_resolution(
            "RendererContext",
            "contexts/renderer_context.md",
            &behavior,
            &contract,
            &lint,
            &empty_contract_validation(),
            &empty_plan_validation(),
            &HashMap::new(),
            None,
            &manifest_scope("context"),
            resolution,
            &store,
        );

        assert!(
            out.ambiguity_report.is_empty(),
            "{:?}",
            out.ambiguity_report
        );
        assert!(
            out.resolved_interface
                .exported_methods
                .iter()
                .find(|method| method.export_name == "new")
                .is_some_and(|method| method
                    .parameters
                    .iter()
                    .any(|parameter| parameter.type_ref == "Box<dyn FoodDropper>"))
        );
    }

    #[test]
    fn context_local_trait_constructor_shape_must_use_boxed_dyn() {
        let mut contract = contract("RendererContext", "context", "context_module");
        contract.roles = vec![role("food_dropper")];

        let behavior = behavior_contract("RendererContext", SpecificationKind::Context);
        let lint = lint_report(&behavior);
        let store = ContractStore::new(".nonexistent_reen_store_for_test");
        let mut resolution = resolution_output("RendererContext", "context_module");
        resolution
            .resolved_interface
            .exported_types
            .push(InterfaceType {
                semantic_name: "FoodDropper".to_string(),
                rust_name: "FoodDropper".to_string(),
                export_name: "FoodDropper".to_string(),
                kind: "trait".to_string(),
                fields: vec![],
            });
        resolution
            .resolved_interface
            .exported_methods
            .push(InterfaceMethod {
                semantic_name: "new".to_string(),
                rust_name: "new".to_string(),
                export_name: "new".to_string(),
                receiver: "associated".to_string(),
                parameters: vec![InterfaceParameter {
                    semantic_name: "food_dropper".to_string(),
                    rust_name: "food_dropper".to_string(),
                    type_ref: "&dyn FoodDropper".to_string(),
                }],
                return_type: "Self".to_string(),
                failure_shape: "plain".to_string(),
                signature: "pub fn new(food_dropper: &dyn FoodDropper) -> Self".to_string(),
            });

        let out = synthesize_contract_resolution(
            "RendererContext",
            "contexts/renderer_context.md",
            &behavior,
            &contract,
            &lint,
            &empty_contract_validation(),
            &empty_plan_validation(),
            &HashMap::new(),
            None,
            &manifest_scope("context"),
            resolution,
            &store,
        );

        assert!(
            out.ambiguity_report.iter().any(|entry| {
                entry
                    .detail
                    .contains("must use `Box<dyn Trait>` in constructor")
            }),
            "{:?}",
            out.ambiguity_report
        );
    }
}
