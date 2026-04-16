use crate::agent_runner::{AgentRequest, AgentRunner, SystemBlock};
use crate::prepared::{Ambiguity, PreparedArtifact};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct FixPayload {
    draft_content: String,
    available_types: Vec<String>,
    ambiguities: Vec<AmbiguityPayload>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AmbiguityPayload {
    path: String,
    message: String,
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

Your job is to infer only high-confidence Rust types/signatures/return types.

Rules:
- Return a JSON object with a single key `fixes` containing an array of objects.
- Each fix object must contain:
  - `path`: copied exactly from the ambiguity
  - `value`: the Rust type, signature, or return type
  - `explanation`: one short sentence
- Do NOT include fixes for `.body` ambiguities.
- For `.type`: return a valid Rust type.
- For `.signature`: return a full Rust-style signature like `tick(&mut self) -> PlayerState`.
- For `.returns`: return only the Rust return type.
- `available_types` may contain exact type names and allowed module prefixes ending in `::`.
  If a prefix like `std::` or `crossterm::` is present, you may use a concrete path that starts
  with that prefix.
- Function pointer types like `fn(&Board, &Snake) -> Option<Position>` are allowed when the draft
  clearly describes a behavioural collaborator and no named export fits better.
- Do not guess a passive value/container type for a behavioural collaborator or role unless the
  evidence explicitly says the collaborator itself is stored as that value. For example:
  - bad: `Vec<char>` for a stdin reader role
  - bad: `Food` for a dropper/chooser role
  - good: `std::io::Stdin`, `std::io::Stdout`, or a suitable function type
- Prefer existing exported types when the name matches exactly (for example `TerminalRenderer`
  should stay `TerminalRenderer`, not `StringRenderer`).
- Only include fixes you can justify from the draft and evidence. Skip low-confidence guesses.

Return ONLY the JSON object, with no markdown fences or extra text."#;

pub fn fix_ambiguities(
    draft_content: &str,
    available_types: &[String],
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
    let payload = build_prompt_payload(draft_content, available_types, &fixable, prepared);
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
        if !looks_valid_fix(fix, prepared) {
            if verbose {
                eprintln!("  skipped {}: suspicious value `{}`", fix.path, fix.value);
            }
            continue;
        }
        if prepared.apply_fix_at_path(&fix.path, &fix.value) {
            applied += 1;
            if verbose {
                eprintln!("  fixed {}: {} ({})", fix.path, fix.value, fix.explanation);
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
    fixable: &[&Ambiguity],
    prepared: &PreparedArtifact,
) -> FixPayload {
    FixPayload {
        draft_content: draft_content.to_string(),
        available_types: available_types.to_vec(),
        ambiguities: fixable
            .iter()
            .map(|a| AmbiguityPayload {
                path: a.path.clone(),
                message: a.message.clone(),
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

fn looks_valid_fix(fix: &Fix, prepared: &PreparedArtifact) -> bool {
    let value = fix.value.trim();
    if value.is_empty() || value.len() > 256 || value.contains('\n') {
        return false;
    }

    if fix.path.ends_with(".signature") {
        return value.contains('(') && value.contains(')');
    }

    if fix.path.ends_with(".returns") {
        return !looks_like_prose(value);
    }

    if fix.path.ends_with(".type") {
        if looks_like_prose(value) {
            return false;
        }
        if is_behavioral_type_slot(&fix.path, prepared) && looks_like_container_or_scalar(value) {
            return false;
        }
    }

    true
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
                    "explanation": "The snake role maps to the Snake export"
                }
            ]
        }"#;
        let response = parse_fix_response(json).unwrap();
        assert_eq!(response.fixes.len(), 1);
        assert_eq!(response.fixes[0].path, "roles[0].type");
        assert_eq!(response.fixes[0].value, "Snake");
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
            explanation: "bad".to_string(),
        };
        assert!(!looks_valid_fix(&fix, &prepared));
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
            &refs,
            &prepared,
        );
        assert_eq!(payload.available_types, vec!["Snake", "std::"]);
        assert_eq!(payload.draft_content, "# Test");
    }
}
