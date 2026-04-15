use crate::agent_runner::{AgentRequest, AgentRunner, SystemBlock};
use crate::prepared::{Ambiguity, PreparedArtifact};
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct FixPayload {
    draft_content: String,
    available_exports: Vec<String>,
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

You are given a DCI-English draft (markdown) and a list of unresolved ambiguities from a deterministic prepare pass. Each ambiguity has a `path` identifying where in the prepared artifact the gap is, a `message` describing what is missing, and `evidence` snippets from the draft.

Your job: resolve each ambiguity by reading the draft prose and the catalog of available exports. Use your understanding of:
- Field types implied by their meaning text and notes
- Role types implied by their purpose and expected behavior mapped to known exports
- Method signatures implied by prose descriptions of what the method does
- Return types implied by descriptions like "returns X" or "the result is Y"

Rules:
- For type ambiguities (path ends in `.type`): provide a valid Rust type as the value.
- For signature ambiguities (path ends in `.signature`): provide a full Rust-style signature like `method_name(&self, param: Type) -> ReturnType`. Include the receiver (&self, &mut self, or none).
- For return type ambiguities (path ends in `.returns`): provide just the Rust return type.
- Do NOT fix `.body` ambiguities — skip them entirely.
- Only use types from the available_exports list, Rust standard library types (String, Vec, Option, HashMap, bool, u8, u16, u32, u64, usize, i32, i64, f32, f64, char, ()), or combinations thereof.
- When a role's purpose maps to a known export, use that export as the type.
- When no export matches, infer the simplest reasonable type from the prose.
- Prefer `&self` receiver unless the method clearly mutates state (use `&mut self`).

Return a JSON object with a single key `fixes` containing an array of objects, each with:
- `path`: the ambiguity path (must match exactly)
- `value`: the inferred Rust type, signature, or return type
- `explanation`: a brief reason for the choice

Do not include fixes for ambiguities you cannot resolve. Do not include body fixes.
Return ONLY the JSON object, no surrounding text or markdown fences."#;

pub fn fix_ambiguities(
    draft_content: &str,
    available_exports: &[String],
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

    let payload = build_prompt_payload(draft_content, available_exports, &fixable);
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
        temperature: 0.2,
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
    available_exports: &[String],
    fixable: &[&Ambiguity],
) -> FixPayload {
    FixPayload {
        draft_content: draft_content.to_string(),
        available_exports: available_exports.to_vec(),
        ambiguities: fixable
            .iter()
            .map(|a| AmbiguityPayload {
                path: a.path.clone(),
                message: a.message.clone(),
                evidence: Vec::new(),
            })
            .collect(),
    }
}

pub fn parse_fix_response(json: &str) -> Result<FixResponse> {
    serde_json::from_str(json).context("Failed to parse fix response JSON from LLM")
}

use anyhow::Context;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_fix_response() {
        let json = r#"{
            "fixes": [
                {
                    "path": "roles[0].type",
                    "value": "Snake",
                    "explanation": "The snake role maps to the Snake export"
                },
                {
                    "path": "functionalities[1].signature",
                    "value": "capture(&mut self) -> ()",
                    "explanation": "capture mutates internal buffer"
                }
            ]
        }"#;
        let response = parse_fix_response(json).unwrap();
        assert_eq!(response.fixes.len(), 2);
        assert_eq!(response.fixes[0].path, "roles[0].type");
        assert_eq!(response.fixes[0].value, "Snake");
        assert_eq!(response.fixes[1].path, "functionalities[1].signature");
        assert_eq!(response.fixes[1].value, "capture(&mut self) -> ()");
    }

    #[test]
    fn parse_empty_fixes() {
        let json = r#"{"fixes": []}"#;
        let response = parse_fix_response(json).unwrap();
        assert!(response.fixes.is_empty());
    }

    #[test]
    fn build_prompt_includes_all_ambiguities() {
        let ambiguities = vec![
            Ambiguity {
                path: "roles[0].type".to_string(),
                severity: "blocking".to_string(),
                message: "no type for snake".to_string(),
                source_line: None,
            },
            Ambiguity {
                path: "functionalities[0].signature".to_string(),
                severity: "blocking".to_string(),
                message: "missing signature".to_string(),
                source_line: None,
            },
        ];
        let refs: Vec<&Ambiguity> = ambiguities.iter().collect();
        let payload = build_prompt_payload("# Test", &["Snake".to_string()], &refs);
        assert_eq!(payload.ambiguities.len(), 2);
        assert_eq!(payload.available_exports, vec!["Snake"]);
        assert_eq!(payload.draft_content, "# Test");
    }
}
