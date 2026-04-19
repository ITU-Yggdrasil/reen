use crate::agent_runner::{AgentRequest, AgentRunner, SystemBlock};
use crate::prepared::{Ambiguity, PreparedArtifact};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct FixPayload {
    draft_content: String,
    available_types: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    dependency_notes: String,
    ambiguities: Vec<AmbiguityPayload>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AmbiguityPayload {
    path: String,
    message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    current_candidates: Vec<String>,
    evidence: Vec<EvidencePayload>,
}

#[derive(Debug, Serialize, Deserialize)]
struct EvidencePayload {
    section: String,
    text: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FixResponse {
    pub fixes: Vec<Fix>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Fix {
    pub path: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<String>,
    pub explanation: String,
}

const SYSTEM_PROMPT: &str = r#"You are a Rust type inference assistant for the Reen build system.

You are given:
- a DCI-English draft (markdown)
- a list of allowed Rust type hints
- unresolved ambiguities from a deterministic prepare pass

Each ambiguity has:
- `path`: the exact location to update in the prepared artifact
- `message`: what is missing
- `evidence`: focused snippets extracted from the draft and prepared artifact

Your job is to infer the best Rust type/signature/return type you can from the evidence.

Rules:
- Return a JSON object with a single key `fixes` containing an array of objects.
- Each fix object must contain:
  - `path`: copied exactly from the ambiguity
  - `value`: the Rust type, signature, or return type
  - optionally `candidates`: an ordered array of plausible alternatives
  - `explanation`: one short sentence
- Do NOT include fixes for `.body` ambiguities.
- For `.type`: return a valid Rust type.
- For `.signature`: return a full Rust-style signature like `tick(&mut self) -> PlayerState`.
- For `.returns`: return only the Rust return type.
- For `.type` and `.returns`, you may make a best-effort guess even when the evidence is
  incomplete. When uncertainty remains, include `candidates` ordered best-first. Put the chosen
  `value` first in that array, then list fallback candidates.
- `available_types` may contain exact type names and allowed module prefixes ending in `::`.
  If a prefix like `std::` or `crossterm::` is present, you may use a concrete path that starts
  with that prefix.
- Role players and app collaborators must resolve to nominal concrete collaborator types, not
  function pointers, closures, containers, or scalars.
- For role-method `.signature` fixes, return the collaborator-facing signature only
  (for example `next_action(&self) -> Option<UserAction>`), not the context wrapper with `<role>_`.
- Do not guess a passive value/container type for a behavioural collaborator or role unless the
  evidence explicitly says the collaborator itself is stored as that value. For example:
  - bad: `Vec<char>` for a stdin reader role
  - bad: `Food` for a dropper/chooser role
  - good: `std::io::Stdin`, `std::io::Stdout`, or a named exported type
- Prefer existing exported types when the name matches exactly (for example `TerminalRenderer`
  should stay `TerminalRenderer`, not `StringRenderer`).
- Use existing `current_candidates` when they are relevant, but you may refine or extend them.
- Keep candidate lists short and relevant; do not dump every available type.

Return ONLY the JSON object, with no markdown fences or extra text."#;

pub fn fix_ambiguities(
    draft_content: &str,
    available_types: &[String],
    prepared: &mut PreparedArtifact,
    verbose: bool,
) -> Result<usize> {
    fix_ambiguities_with_dependency_notes(draft_content, available_types, "", prepared, verbose)
}

/// Same as [`fix_ambiguities`] but allows the caller to embed a workspace-specific dependency
/// notes block (crate versions + curated API migration cues) into the prompt.
pub fn fix_ambiguities_with_dependency_notes(
    draft_content: &str,
    available_types: &[String],
    dependency_notes: &str,
    prepared: &mut PreparedArtifact,
    verbose: bool,
) -> Result<usize> {
    let blockers: Vec<&Ambiguity> = prepared.blocking_ambiguities().collect();
    let fixable: Vec<&Ambiguity> = blockers
        .into_iter()
        .filter(|a| !a.path.ends_with(".body"))
        .collect();

    if fixable.is_empty() {
        return Ok(0);
    }

    let runner = AgentRunner::from_env()?;
    let payload = build_prompt_payload(
        draft_content,
        available_types,
        dependency_notes,
        &fixable,
        prepared,
    );
    let user_content = serde_json::to_string_pretty(&payload)?;

    if verbose {
        eprintln!(
            "fix-agent: sending {} ambiguities for {} (model: {})",
            fixable.len(),
            prepared.source.path,
            runner.model()
        );
    }

    let json = runner.run_json(&AgentRequest {
        system: vec![SystemBlock::new(SYSTEM_PROMPT)],
        user_content: &user_content,
        temperature: 0.1,
        max_tokens: 4096,
    })?;
    let fix_response = parse_fix_response(&json)?;

    if verbose {
        eprintln!(
            "fix-agent: received {} fixes for {}",
            fix_response.fixes.len(),
            prepared.source.path
        );
    }

    let mut applied = 0usize;
    for fix in &fix_response.fixes {
        if fix.path.ends_with(".body") {
            continue;
        }
        if !looks_valid_fix(fix, prepared, available_types) {
            if verbose {
                eprintln!("  skipped {}: suspicious value `{}`", fix.path, fix.value);
            }
            continue;
        }
        let candidates = sanitize_fix_candidates(fix, prepared, available_types);
        if prepared.apply_fix_at_path_with_candidates(&fix.path, &fix.value, Some(&candidates)) {
            applied += 1;
            if verbose {
                if candidates.is_empty() {
                    eprintln!("  fixed {}: {} ({})", fix.path, fix.value, fix.explanation);
                } else {
                    eprintln!(
                        "  fixed {}: {} [{} alt] ({})",
                        fix.path,
                        fix.value,
                        candidates.len().saturating_sub(1),
                        fix.explanation
                    );
                }
            }
        } else if verbose {
            eprintln!("  skipped {}: path not found in artifact", fix.path);
        }
    }

    Ok(applied)
}

fn build_prompt_payload(
    draft_content: &str,
    available_types: &[String],
    dependency_notes: &str,
    fixable: &[&Ambiguity],
    prepared: &PreparedArtifact,
) -> FixPayload {
    FixPayload {
        draft_content: draft_content.to_string(),
        available_types: available_types.to_vec(),
        dependency_notes: dependency_notes.to_string(),
        ambiguities: fixable
            .iter()
            .map(|a| AmbiguityPayload {
                path: a.path.clone(),
                message: a.message.clone(),
                current_candidates: current_candidates_for_path(prepared, &a.path),
                evidence: describe_ambiguity(a, prepared),
            })
            .collect(),
    }
}

fn describe_ambiguity(ambiguity: &Ambiguity, prepared: &PreparedArtifact) -> Vec<EvidencePayload> {
    let path = &ambiguity.path;
    let mut evidence = Vec::new();

    if let Some(idx) = parse_single_index(path, "fields", "type") {
        if let Some(field) = prepared.fields.get(idx) {
            push_evidence(&mut evidence, "field meaning", &field.meaning);
            if !field.notes.is_empty() {
                push_evidence(&mut evidence, "field notes", &field.notes.join("; "));
            }
        }
        return evidence;
    }

    if let Some(idx) = parse_single_index(path, "props", "type") {
        if let Some(prop) = prepared.props.get(idx) {
            push_evidence(&mut evidence, "prop meaning", &prop.meaning);
            if !prop.notes.is_empty() {
                push_evidence(&mut evidence, "prop notes", &prop.notes.join("; "));
            }
        }
        return evidence;
    }

    if let Some(idx) = parse_single_index(path, "collaborators", "type") {
        if let Some(collaborator) = prepared.collaborators.get(idx) {
            push_evidence(
                &mut evidence,
                "collaborator responsibility",
                &collaborator.responsibility,
            );
            for item in &collaborator.type_status.evidence {
                push_evidence(&mut evidence, &item.section, &item.text);
            }
        }
        return evidence;
    }

    if let Some(idx) = parse_single_index(path, "roles", "type") {
        if let Some(role) = prepared.roles.get(idx) {
            push_evidence(&mut evidence, "role purpose", &role.purpose);
            push_evidence(
                &mut evidence,
                "role expected behaviour",
                &role.expected_behavior,
            );
            for method in &role.methods {
                if let Some(reason) = &method.signature.reason {
                    push_evidence(
                        &mut evidence,
                        &format!("role method {}", method.name),
                        reason,
                    );
                }
                if let Some(rust) = method.return_status.rust.as_deref() {
                    push_evidence(
                        &mut evidence,
                        &format!("role method {} return", method.name),
                        rust,
                    );
                }
            }
        }
        return evidence;
    }

    if let Some((ridx, midx)) = parse_double_index(path, "roles", "methods", "signature") {
        if let Some(role) = prepared.roles.get(ridx)
            && let Some(method) = role.methods.get(midx)
        {
            push_evidence(&mut evidence, "role purpose", &role.purpose);
            push_evidence(
                &mut evidence,
                "role expected behaviour",
                &role.expected_behavior,
            );
            if let Some(reason) = &method.signature.reason {
                push_evidence(&mut evidence, "signature reason", reason);
            }
            if let Some(ret) = method.return_status.rust.as_deref() {
                push_evidence(&mut evidence, "resolved return", ret);
            }
        }
        return evidence;
    }

    if let Some((ridx, midx)) = parse_double_index(path, "roles", "methods", "returns") {
        if let Some(role) = prepared.roles.get(ridx)
            && let Some(method) = role.methods.get(midx)
        {
            push_evidence(&mut evidence, "role purpose", &role.purpose);
            push_evidence(
                &mut evidence,
                "role expected behaviour",
                &role.expected_behavior,
            );
            if let Some(reason) = &method.return_status.reason {
                push_evidence(&mut evidence, "return reason", reason);
            }
        }
        return evidence;
    }

    if let Some(idx) = parse_single_index(path, "functionalities", "signature") {
        if let Some(func) = prepared.functionalities.get(idx) {
            push_evidence(&mut evidence, "functionality flow", &flow_summary(func));
            if let Some(ret) = func.return_status.rust.as_deref() {
                push_evidence(&mut evidence, "resolved return", ret);
            }
        }
        return evidence;
    }

    if let Some(idx) = parse_single_index(path, "functionalities", "returns") {
        if let Some(func) = prepared.functionalities.get(idx) {
            push_evidence(&mut evidence, "functionality flow", &flow_summary(func));
        }
        return evidence;
    }

    if let Some((fidx, midx)) = parse_double_index(path, "functionalities", "parameters", "type") {
        if let Some(func) = prepared.functionalities.get(fidx)
            && let Some(param) = func.parameters.get(midx)
        {
            push_evidence(&mut evidence, "parameter name", &param.name);
            push_evidence(&mut evidence, "functionality flow", &flow_summary(func));
        }
        return evidence;
    }

    evidence
}

fn push_evidence(out: &mut Vec<EvidencePayload>, section: &str, text: &str) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    out.push(EvidencePayload {
        section: section.to_string(),
        text: trimmed.to_string(),
    });
}

fn flow_summary(func: &crate::prepared::MethodSpec) -> String {
    let mut parts = Vec::new();
    if !func.flow.is_empty() {
        parts.push(format!("Flow: {}", func.flow.join(" ")));
    }
    if !func.extensions.is_empty() {
        parts.push(format!("Extensions: {}", func.extensions.join(" ")));
    }
    if !func.guarantee.is_empty() {
        parts.push(format!("Guarantee: {}", func.guarantee.join(" ")));
    }
    parts.join(" ")
}

pub fn parse_fix_response(json: &str) -> Result<FixResponse> {
    serde_json::from_str(json).context("Failed to parse fix response JSON from LLM")
}

fn looks_valid_fix(fix: &Fix, prepared: &PreparedArtifact, available_types: &[String]) -> bool {
    looks_valid_fix_value(&fix.path, &fix.value, prepared, available_types)
}

fn looks_valid_fix_value(
    path: &str,
    value: &str,
    prepared: &PreparedArtifact,
    available_types: &[String],
) -> bool {
    let value = value.trim();
    if value.is_empty() || value.len() > 256 || value.contains('\n') {
        return false;
    }

    if path.ends_with(".signature") {
        if is_role_method_signature_slot(path, prepared) {
            if role_signature_mentions_wrapper_param(value, path, prepared) {
                return false;
            }
            if role_signature_has_unplumbable_param(value, path, prepared) {
                return false;
            }
        }
        return value.contains('(') && value.contains(')');
    }

    if path.ends_with(".returns") {
        return !looks_like_prose(value);
    }

    if path.ends_with(".type") {
        if looks_like_prose(value) {
            return false;
        }
        if is_behavioral_type_slot(path, prepared) {
            if looks_like_non_nominal_role_type(value) {
                return false;
            }
            if !is_allowed_nominal_behavioral_type(value, available_types) {
                return false;
            }
        }
    }

    true
}

fn sanitize_fix_candidates(
    fix: &Fix,
    prepared: &PreparedArtifact,
    available_types: &[String],
) -> Vec<String> {
    let mut normalized = Vec::new();
    push_unique_candidate(
        &mut normalized,
        &fix.path,
        &fix.value,
        prepared,
        available_types,
    );
    for candidate in &fix.candidates {
        push_unique_candidate(
            &mut normalized,
            &fix.path,
            candidate,
            prepared,
            available_types,
        );
    }
    if normalized.len() == 1 {
        for candidate in current_candidates_for_path(prepared, &fix.path) {
            push_unique_candidate(
                &mut normalized,
                &fix.path,
                &candidate,
                prepared,
                available_types,
            );
        }
    }
    normalized
}

fn push_unique_candidate(
    normalized: &mut Vec<String>,
    path: &str,
    value: &str,
    prepared: &PreparedArtifact,
    available_types: &[String],
) {
    let trimmed = value.trim();
    if trimmed.is_empty() || !looks_valid_fix_value(path, trimmed, prepared, available_types) {
        return;
    }
    if !normalized.iter().any(|existing| existing == trimmed) {
        normalized.push(trimmed.to_string());
    }
}

fn is_allowed_nominal_behavioral_type(value: &str, available_types: &[String]) -> bool {
    let trimmed = value.trim();
    available_types.iter().any(|allowed| {
        if allowed.ends_with("::") {
            trimmed.starts_with(allowed)
                && trimmed
                    .rsplit("::")
                    .next()
                    .and_then(|segment| segment.chars().next())
                    .is_some_and(|ch| ch.is_ascii_uppercase())
        } else {
            trimmed == allowed
        }
    })
}

fn current_candidates_for_path(prepared: &PreparedArtifact, path: &str) -> Vec<String> {
    current_value_status(prepared, path)
        .map(|status| status.candidates.clone())
        .unwrap_or_default()
}

fn current_value_status<'a>(
    prepared: &'a PreparedArtifact,
    path: &str,
) -> Option<&'a crate::prepared::ValueStatus> {
    if let Some(idx) = parse_single_index(path, "fields", "type") {
        return prepared.fields.get(idx).map(|field| &field.type_status);
    }
    if let Some(idx) = parse_single_index(path, "props", "type") {
        return prepared.props.get(idx).map(|prop| &prop.type_status);
    }
    if let Some(idx) = parse_single_index(path, "collaborators", "type") {
        return prepared
            .collaborators
            .get(idx)
            .map(|item| &item.type_status);
    }
    if let Some(idx) = parse_single_index(path, "roles", "type") {
        return prepared.roles.get(idx).map(|role| &role.type_status);
    }
    if let Some((role_idx, method_idx)) = parse_double_index(path, "roles", "methods", "signature")
    {
        return prepared
            .roles
            .get(role_idx)
            .and_then(|role| role.methods.get(method_idx))
            .map(|method| &method.signature);
    }
    if let Some((role_idx, method_idx)) = parse_double_index(path, "roles", "methods", "returns") {
        return prepared
            .roles
            .get(role_idx)
            .and_then(|role| role.methods.get(method_idx))
            .map(|method| &method.return_status);
    }
    if let Some((func_idx, param_idx)) =
        parse_double_index(path, "functionalities", "parameters", "type")
    {
        return prepared
            .functionalities
            .get(func_idx)
            .and_then(|method| method.parameters.get(param_idx))
            .map(|param| &param.type_status);
    }
    if let Some(idx) = parse_single_index(path, "functionalities", "signature") {
        return prepared
            .functionalities
            .get(idx)
            .map(|method| &method.signature);
    }
    if let Some(idx) = parse_single_index(path, "functionalities", "returns") {
        return prepared
            .functionalities
            .get(idx)
            .map(|method| &method.return_status);
    }
    None
}

fn is_behavioral_type_slot(path: &str, prepared: &PreparedArtifact) -> bool {
    if path.starts_with("collaborators[") && path.ends_with(".type") {
        return true;
    }
    if let Some(idx) = parse_single_index(path, "roles", "type") {
        return prepared
            .roles
            .get(idx)
            .is_some_and(|role| !role.methods.is_empty());
    }
    false
}

fn looks_like_container_or_scalar(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with("Vec<")
        || trimmed.starts_with("Option<")
        || trimmed.starts_with("HashMap<")
        || trimmed.starts_with("std::collections::HashMap<")
        || trimmed.starts_with("fn(")
        || matches!(
            trimmed,
            "String"
                | "str"
                | "bool"
                | "char"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "usize"
                | "i8"
                | "i16"
                | "i32"
                | "i64"
                | "isize"
                | "f32"
                | "f64"
                | "()"
        )
}

fn looks_like_non_nominal_role_type(value: &str) -> bool {
    looks_like_container_or_scalar(value)
}

fn looks_like_prose(s: &str) -> bool {
    let mut depth = 0i32;
    let mut spaces = 0usize;
    for ch in s.chars() {
        match ch {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            ' ' if depth == 0 => spaces += 1,
            _ => {}
        }
    }
    spaces > 2
}

fn parse_single_index(path: &str, collection: &str, field: &str) -> Option<usize> {
    let prefix = format!("{collection}[");
    let suffix = format!("].{field}");
    let rest = path.strip_prefix(&prefix)?;
    let rest = rest.strip_suffix(&suffix)?;
    rest.parse().ok()
}

fn parse_double_index(path: &str, outer: &str, inner: &str, field: &str) -> Option<(usize, usize)> {
    let prefix = format!("{outer}[");
    let rest = path.strip_prefix(&prefix)?;
    let close = rest.find(']')?;
    let outer_idx: usize = rest[..close].parse().ok()?;
    let mid = format!("].{inner}[");
    let rest = rest[close..].strip_prefix(&mid)?;
    let close2 = rest.find(']')?;
    let inner_idx: usize = rest[..close2].parse().ok()?;
    let expected_tail = format!("].{field}");
    rest[close2..].strip_prefix(&expected_tail)?;
    Some((outer_idx, inner_idx))
}

fn is_role_method_signature_slot(path: &str, prepared: &PreparedArtifact) -> bool {
    let Some((role_idx, _method_idx)) = parse_double_index(path, "roles", "methods", "signature")
    else {
        return false;
    };
    prepared.roles.get(role_idx).is_some()
}

fn role_signature_mentions_wrapper_param(
    value: &str,
    path: &str,
    prepared: &PreparedArtifact,
) -> bool {
    let Some((role_idx, _method_idx)) = parse_double_index(path, "roles", "methods", "signature")
    else {
        return false;
    };
    let Some(role) = prepared.roles.get(role_idx) else {
        return false;
    };
    value.contains(&format!("{}_", role.name))
}

/// Reject role-method signatures that introduce parameters whose names cannot be supplied by
/// the enclosing context.
///
/// A role method is called from a functionality, so every non-self parameter must eventually be
/// plumbable from the functionality side. We accept a parameter name if:
///
/// - It matches a context prop or role.
/// - It matches a parameter of any functionality declared on the same context.
/// - It is a placeholder-like name such as `_`, `arg0`, etc. (too generic to confidently reject).
///
/// A false positive would reject a legitimate flow value computed locally by the caller, so we
/// only reject when the name clearly looks like an identifier the LLM was hoping would "just
/// exist" somewhere on the context.
fn role_signature_has_unplumbable_param(
    value: &str,
    path: &str,
    prepared: &PreparedArtifact,
) -> bool {
    let Some((_role_idx, _method_idx)) = parse_double_index(path, "roles", "methods", "signature")
    else {
        return false;
    };
    let Some(open) = value.find('(') else {
        return false;
    };
    let Some(close) = value.rfind(')') else {
        return false;
    };
    if close <= open + 1 {
        return false;
    }
    let inner = &value[open + 1..close];

    let plumbable = plumbable_param_names(prepared);
    for part in split_top_level_commas(inner) {
        let part = part.trim();
        if part.is_empty() || matches!(part, "&self" | "&mut self" | "self") {
            continue;
        }
        let Some((name, _)) = part.split_once(':') else {
            continue;
        };
        let name = name
            .trim()
            .trim_start_matches('&')
            .trim()
            .trim_end_matches('_');
        let normalized = name.to_ascii_lowercase();
        if normalized.is_empty() || normalized == "self" {
            continue;
        }
        if normalized.len() == 1 || normalized.starts_with('_') || normalized.starts_with("arg") {
            continue;
        }
        if plumbable.iter().any(|candidate| candidate == &normalized) {
            continue;
        }
        return true;
    }
    false
}

fn plumbable_param_names(prepared: &PreparedArtifact) -> Vec<String> {
    let mut names = Vec::new();
    for prop in &prepared.props {
        names.push(prop.name.to_ascii_lowercase());
    }
    for role in &prepared.roles {
        names.push(role.name.to_ascii_lowercase());
    }
    for functionality in &prepared.functionalities {
        for param in &functionality.parameters {
            names.push(param.name.to_ascii_lowercase());
        }
    }
    for collab in &prepared.collaborators {
        names.push(collab.name.to_ascii_lowercase());
    }
    names.sort();
    names.dedup();
    names
}

fn split_top_level_commas(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for ch in value.chars() {
        match ch {
            '<' | '(' | '[' => {
                depth += 1;
                current.push(ch);
            }
            '>' | ')' | ']' => {
                depth -= 1;
                current.push(ch);
            }
            ',' if depth == 0 => {
                parts.push(std::mem::take(&mut current));
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        parts.push(current);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prepared::{MethodSpec, ParameterSpec, RoleSpec, ValueStatus};

    #[test]
    fn parse_valid_fix_response() {
        let json = r#"{
            "fixes": [
                {
                    "path": "roles[0].type",
                    "value": "Snake",
                    "candidates": ["Snake", "Board"],
                    "explanation": "The snake role maps to the Snake export"
                }
            ]
        }"#;
        let response = parse_fix_response(json).unwrap();
        assert_eq!(response.fixes.len(), 1);
        assert_eq!(response.fixes[0].path, "roles[0].type");
        assert_eq!(response.fixes[0].value, "Snake");
        assert_eq!(
            response.fixes[0].candidates,
            vec!["Snake".to_string(), "Board".to_string()]
        );
    }

    #[test]
    fn rejects_container_type_for_behavioral_role() {
        let mut prepared = PreparedArtifact::empty(
            "context",
            "x.md".to_string(),
            "X".to_string(),
            "X".to_string(),
            true,
        );
        prepared.roles.push(RoleSpec {
            name: "stdin_source".to_string(),
            purpose: "Reads stdin".to_string(),
            expected_behavior: "Provides keypresses".to_string(),
            type_status: ValueStatus::missing("missing".to_string(), Vec::new()),
            methods: vec![MethodSpec {
                name: "read_available".to_string(),
                signature: ValueStatus::missing("missing".to_string(), Vec::new()),
                receiver: None,
                parameters: Vec::<ParameterSpec>::new(),
                return_status: ValueStatus::missing("missing".to_string(), Vec::new()),
                flow: Vec::new(),
                extensions: Vec::new(),
                guarantee: Vec::new(),
                references: None,
                body: None,
            }],
        });

        let fix = Fix {
            path: "roles[0].type".to_string(),
            value: "Vec<char>".to_string(),
            candidates: Vec::new(),
            explanation: "bad".to_string(),
        };
        assert!(!looks_valid_fix(&fix, &prepared, &["std::".to_string()]));
    }

    #[test]
    fn sanitize_fix_candidates_preserves_existing_alternatives_when_agent_omits_them() {
        let mut prepared = PreparedArtifact::empty(
            "context",
            "x.md".to_string(),
            "X".to_string(),
            "X".to_string(),
            true,
        );
        prepared.roles.push(RoleSpec {
            name: "food_dropper".to_string(),
            purpose: "Chooses food".to_string(),
            expected_behavior: "Returns a random food position".to_string(),
            type_status: ValueStatus::ambiguous(
                vec![
                    "rand::rngs::ThreadRng".to_string(),
                    "rand::rngs::StdRng".to_string(),
                ],
                "ambiguous".to_string(),
                Vec::new(),
            ),
            methods: vec![MethodSpec {
                name: "drop".to_string(),
                signature: ValueStatus::missing("missing".to_string(), Vec::new()),
                receiver: None,
                parameters: Vec::<ParameterSpec>::new(),
                return_status: ValueStatus::missing("missing".to_string(), Vec::new()),
                flow: Vec::new(),
                extensions: Vec::new(),
                guarantee: Vec::new(),
                references: None,
                body: None,
            }],
        });

        let fix = Fix {
            path: "roles[0].type".to_string(),
            value: "rand::rngs::ThreadRng".to_string(),
            candidates: Vec::new(),
            explanation: "best guess".to_string(),
        };

        assert_eq!(
            sanitize_fix_candidates(&fix, &prepared, &["rand::".to_string()]),
            vec![
                "rand::rngs::ThreadRng".to_string(),
                "rand::rngs::StdRng".to_string()
            ]
        );
    }

    #[test]
    fn rejects_behavioral_role_type_outside_available_types() {
        let mut prepared = PreparedArtifact::empty(
            "context",
            "x.md".to_string(),
            "X".to_string(),
            "X".to_string(),
            true,
        );
        prepared.roles.push(RoleSpec {
            name: "food_dropper".to_string(),
            purpose: "Chooses food".to_string(),
            expected_behavior: "Returns a random food position".to_string(),
            type_status: ValueStatus::missing("missing".to_string(), Vec::new()),
            methods: vec![MethodSpec {
                name: "drop".to_string(),
                signature: ValueStatus::missing("missing".to_string(), Vec::new()),
                receiver: None,
                parameters: Vec::<ParameterSpec>::new(),
                return_status: ValueStatus::missing("missing".to_string(), Vec::new()),
                flow: Vec::new(),
                extensions: Vec::new(),
                guarantee: Vec::new(),
                references: None,
                body: None,
            }],
        });

        let fix = Fix {
            path: "roles[0].type".to_string(),
            value: "FoodDropper".to_string(),
            candidates: Vec::new(),
            explanation: "invented".to_string(),
        };

        assert!(!looks_valid_fix(&fix, &prepared, &["rand::".to_string()]));
    }

    #[test]
    fn build_prompt_includes_available_types() {
        let ambiguities = vec![Ambiguity {
            path: "roles[0].type".to_string(),
            severity: "blocking".to_string(),
            message: "no type for snake".to_string(),
            source_line: None,
        }];
        let refs: Vec<&Ambiguity> = ambiguities.iter().collect();
        let prepared = PreparedArtifact::empty(
            "context",
            "x.md".to_string(),
            "X".to_string(),
            "X".to_string(),
            true,
        );
        let payload = build_prompt_payload(
            "# Test",
            &["Snake".to_string(), "std::".to_string()],
            "",
            &refs,
            &prepared,
        );
        assert_eq!(payload.available_types, vec!["Snake", "std::"]);
        assert_eq!(payload.draft_content, "# Test");
    }
}
