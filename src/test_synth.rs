//! LLM-backed test synthesis.
//!
//! Invoked by `prepare` to generate additional test cases from prose invariants,
//! extensions, and guarantees. Uses the Anthropic Messages API directly (reqwest).
//!
//! Each call is a single-shot, structured prompt. The output is a list of
//! `SynthesisedTest` entries tagged with `source: "invariant"`.

use crate::prepared::{ArtifactKind, PreparedArtifact, SynthesisedTest};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const DEFAULT_MODEL: &str = "claude-haiku-4-5-20251001";
const MAX_TOKENS: u32 = 1024;

// ── Public API ────────────────────────────────────────────────────

/// Synthesise additional tests from prose in a prepared artifact.
///
/// Inspects invariants (data), functionality descriptions (context), and
/// query descriptions (projection) to generate Given/When/Then test cases
/// that go beyond what the BA wrote in the Scenarios section.
///
/// Returns an empty vec if:
/// - No API key is available (`ANTHROPIC_API_KEY` not set)
/// - The artifact has no prose worth synthesising from
/// - The LLM call fails (failure is non-fatal; logged to stderr)
pub fn synthesise_tests(artifact: &PreparedArtifact) -> Vec<SynthesisedTest> {
    let prose = collect_synthesisable_prose(artifact);
    if prose.is_empty() {
        return Vec::new();
    }

    let api_key = match std::env::var("ANTHROPIC_API_KEY") {
        Ok(key) if !key.is_empty() => key,
        _ => return Vec::new(),
    };

    match call_api(&api_key, artifact, &prose) {
        Ok(tests) => tests,
        Err(err) => {
            eprintln!("warn[test_synth]: LLM test synthesis failed: {err}");
            Vec::new()
        }
    }
}

// ── Prose collection ──────────────────────────────────────────────

#[derive(Debug)]
struct SynthProse {
    label: String,
    text: String,
}

fn collect_synthesisable_prose(artifact: &PreparedArtifact) -> Vec<SynthProse> {
    let mut prose = Vec::new();

    // Invariants (data artifacts)
    for inv in &artifact.invariants {
        prose.push(SynthProse {
            label: "invariant".to_string(),
            text: inv.clone(),
        });
    }

    // Functionality descriptions (context and projection artifacts)
    for func in &artifact.functionalities {
        if !func.description.trim().is_empty() {
            prose.push(SynthProse {
                label: format!("functionality `{}`", func.name),
                text: func.description.clone(),
            });
        }
    }

    // Operation descriptions (data artifacts)
    for op in &artifact.operations {
        if !op.description.trim().is_empty() {
            prose.push(SynthProse {
                label: format!("operation `{}`", op.name),
                text: op.description.clone(),
            });
        }
    }

    prose
}

// ── API call ──────────────────────────────────────────────────────

#[derive(Serialize)]
struct ApiRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<ApiMessage>,
    system: String,
}

#[derive(Serialize)]
struct ApiMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ApiResponse {
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
struct ContentBlock {
    text: Option<String>,
}

#[derive(Deserialize)]
struct SynthResult {
    tests: Vec<SynthTestEntry>,
}

#[derive(Deserialize)]
struct SynthTestEntry {
    given: String,
    when: String,
    then: String,
    citation: String,
}

fn call_api(
    api_key: &str,
    artifact: &PreparedArtifact,
    prose: &[SynthProse],
) -> Result<Vec<SynthesisedTest>> {
    let kind_str = match artifact.kind {
        ArtifactKind::Data => "data type",
        ArtifactKind::Projection => "projection",
        ArtifactKind::Context => "context",
        ArtifactKind::Operation => "operation",
        ArtifactKind::Component => "component",
        ArtifactKind::Brand => "brand",
        ArtifactKind::RoutingTable => "routing table",
        ArtifactKind::App => "application",
    };

    let existing_tests: Vec<String> = artifact
        .synthesised_tests
        .iter()
        .map(|t| format!("Given: {}\nWhen: {}\nThen: {}", t.given, t.when, t.then))
        .collect();

    let prose_block: String = prose
        .iter()
        .map(|p| format!("- {}: {}", p.label, p.text))
        .collect::<Vec<_>>()
        .join("\n");

    let existing_block = if existing_tests.is_empty() {
        "None yet.".to_string()
    } else {
        existing_tests.join("\n---\n")
    };

    let system = format!(
        "You are a DCI-aware test synthesis engine. You generate Given/When/Then test cases \
         for a {} called `{}`. Your tests exercise role-method responsibilities and \
         functionality enactments, not arbitrary behaviour.\n\n\
         Output ONLY a JSON object with a single key \"tests\" containing an array. \
         Each element has keys: \"given\", \"when\", \"then\", \"citation\".\n\
         The citation should reference which prose element motivated the test.\n\
         Generate 1-3 tests per prose element. Do not duplicate existing tests.\n\
         If you cannot generate meaningful tests, return {{\"tests\": []}}.",
        kind_str, artifact.export.name
    );

    let user_prompt = format!(
        "Artifact: {} ({})\n\n\
         Prose to synthesise tests from:\n{}\n\n\
         Existing tests (do not duplicate):\n{}\n\n\
         Generate additional Given/When/Then tests as JSON.",
        artifact.export.name, kind_str, prose_block, existing_block
    );

    let request = ApiRequest {
        model: DEFAULT_MODEL.to_string(),
        max_tokens: MAX_TOKENS,
        messages: vec![ApiMessage {
            role: "user".to_string(),
            content: user_prompt,
        }],
        system,
    };

    let client = reqwest::blocking::Client::new();
    let response = client
        .post(ANTHROPIC_API_URL)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&request)
        .send()
        .context("Failed to send test synthesis request")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        bail!("Anthropic API returned {status}: {body}");
    }

    let api_response: ApiResponse = response
        .json()
        .context("Failed to parse test synthesis response")?;

    let text = api_response
        .content
        .iter()
        .filter_map(|b| b.text.as_deref())
        .collect::<Vec<_>>()
        .join("");

    parse_synth_response(&text)
}

fn parse_synth_response(text: &str) -> Result<Vec<SynthesisedTest>> {
    // Extract JSON from the response — it may be wrapped in markdown code fences
    let json_str = extract_json_block(text);

    let result: SynthResult =
        serde_json::from_str(json_str).context("Failed to parse synthesised test JSON")?;

    Ok(result
        .tests
        .into_iter()
        .map(|entry| SynthesisedTest {
            source: "invariant".to_string(),
            given: entry.given,
            when: entry.when,
            then: entry.then,
            citation: Some(entry.citation),
        })
        .collect())
}

fn extract_json_block(text: &str) -> &str {
    // Try to find JSON inside ```json ... ``` fences
    if let Some(start) = text.find("```json") {
        let after_fence = &text[start + 7..];
        if let Some(end) = after_fence.find("```") {
            return after_fence[..end].trim();
        }
    }
    if let Some(start) = text.find("```") {
        let after_fence = &text[start + 3..];
        if let Some(end) = after_fence.find("```") {
            return after_fence[..end].trim();
        }
    }
    // Otherwise assume the whole thing is JSON
    text.trim()
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prepared::{ArtifactKind, PreparedArtifact};

    #[test]
    fn parse_synth_response_parses_valid_json() {
        let json = r#"{"tests": [
            {"given": "an amount of -5", "when": "Amount is constructed", "then": "construction fails or panics", "citation": "invariant: an Amount is never negative"}
        ]}"#;
        let tests = parse_synth_response(json).unwrap();
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].source, "invariant");
        assert_eq!(tests[0].given, "an amount of -5");
    }

    #[test]
    fn parse_synth_response_handles_code_fences() {
        let text = "Here are the tests:\n```json\n{\"tests\": [{\"given\": \"x\", \"when\": \"y\", \"then\": \"z\", \"citation\": \"c\"}]}\n```\n";
        let tests = parse_synth_response(text).unwrap();
        assert_eq!(tests.len(), 1);
    }

    #[test]
    fn parse_synth_response_handles_empty() {
        let json = r#"{"tests": []}"#;
        let tests = parse_synth_response(json).unwrap();
        assert!(tests.is_empty());
    }

    #[test]
    fn collect_prose_from_data_artifact() {
        let mut artifact =
            PreparedArtifact::new(ArtifactKind::Data, "data/Amount.md".into(), "Amount".into(), "Amount".into());
        artifact.invariants = vec!["an Amount is never negative".to_string()];
        artifact.operations.push(crate::prepared::OperationSpec {
            name: "add".to_string(),
            description: "Adds two amounts together.".to_string(),
            kind: None,
        });
        let prose = collect_synthesisable_prose(&artifact);
        assert_eq!(prose.len(), 2);
        assert_eq!(prose[0].label, "invariant");
        assert_eq!(prose[1].label, "operation `add`");
    }

    #[test]
    fn synthesise_tests_returns_empty_without_api_key() {
        // Ensure ANTHROPIC_API_KEY is not set for this test
        let original = std::env::var("ANTHROPIC_API_KEY").ok();
        // SAFETY: test runs single-threaded for this env var
        unsafe { std::env::remove_var("ANTHROPIC_API_KEY") };

        let artifact =
            PreparedArtifact::new(ArtifactKind::Data, "data/X.md".into(), "X".into(), "X".into());
        let tests = synthesise_tests(&artifact);
        assert!(tests.is_empty());

        // Restore
        if let Some(val) = original {
            // SAFETY: restoring previous value
            unsafe { std::env::set_var("ANTHROPIC_API_KEY", val) };
        }
    }
}
