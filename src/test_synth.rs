//! LLM-backed test synthesis.
//!
//! Invoked by `prepare` to generate additional test cases from prose invariants,
//! extensions, and guarantees. Uses the Anthropic Messages API directly (reqwest).
//!
//! Each call is a single-shot, structured prompt. The output is a list of
//! `SynthesisedTest` entries tagged with `source: "invariant"`.

use crate::prepared::{Ambiguity, ArtifactKind, PreparedArtifact, SynthesisedTest};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const DEFAULT_MODEL: &str = "claude-haiku-4-5-20251001";
const MAX_TOKENS: u32 = 1024;

// ── Public API ────────────────────────────────────────────────────

/// The output of one synthesis pass: the kept tests plus any
/// informational findings raised when a candidate failed validation.
/// Findings are surfaced via the prepared YAML's `ambiguities` list so
/// the BA sees which invariants could not be auto-tested.
#[derive(Debug, Default)]
pub struct SynthesisOutcome {
    pub tests: Vec<SynthesisedTest>,
    pub findings: Vec<Ambiguity>,
}

/// Synthesise additional tests from prose in a prepared artifact.
///
/// Inspects invariants (data), functionality descriptions (context), and
/// query descriptions (projection) to generate Given/When/Then test cases
/// that go beyond what the BA wrote in the Scenarios section.
///
/// Returns an empty outcome if:
/// - No API key is available (`ANTHROPIC_API_KEY` not set)
/// - The artifact has no prose worth synthesising from
/// - The LLM call fails (failure is non-fatal; logged to stderr)
///
/// Each candidate test produced by the LLM passes through
/// [`validate_synthesised_test`]; tests that fail validation are dropped
/// and replaced with an informational finding in the outcome's `findings`.
pub fn synthesise_tests(artifact: &PreparedArtifact) -> SynthesisOutcome {
    let prose = collect_synthesisable_prose(artifact);
    if prose.is_empty() {
        return SynthesisOutcome::default();
    }

    let api_key = match std::env::var("ANTHROPIC_API_KEY") {
        Ok(key) if !key.is_empty() => key,
        _ => return SynthesisOutcome::default(),
    };

    let raw_tests = match call_api(&api_key, artifact, &prose) {
        Ok(tests) => tests,
        Err(err) => {
            eprintln!("warn[test_synth]: LLM test synthesis failed: {err}");
            return SynthesisOutcome::default();
        }
    };

    let mut outcome = SynthesisOutcome::default();
    for test in raw_tests {
        match validate_synthesised_test(artifact, &test) {
            Ok(()) => outcome.tests.push(test),
            Err(reason) => outcome.findings.push(Ambiguity {
                message: format!(
                    "Synthesised test rejected for `{}`: {}",
                    artifact.export.name, reason,
                ),
                severity: "low".to_string(),
                suggested_resolution: Some(
                    "add an explicit scenario for this invariant if you want it tested".into(),
                ),
                source_line: None,
            }),
        }
    }
    outcome
}

// ── Validation (bug 7.1: input/output simulation) ────────────────────
//
// For projection-query tests, parse the Given input set, the When bounds /
// search, and the Then expected output. Simulate the query against the
// inputs and compare to the claim. The patterns are an explicit allow-list
// — when the test's prose doesn't match any known shape, we simply pass
// the test through unchanged. We only reject when simulation contradicts.

/// Validate one synthesised test against simulation. Returns `Ok(())` when
/// the test either matches simulation or has a shape we don't know how to
/// simulate. Returns `Err(reason)` only when the simulation contradicts
/// the test's claim.
pub fn validate_synthesised_test(
    artifact: &PreparedArtifact,
    test: &SynthesisedTest,
) -> Result<(), String> {
    if artifact.kind != ArtifactKind::Projection {
        return Ok(());
    }

    if let Some(result) = simulate_amount_range_test(test) {
        return result;
    }
    if let Some(result) = simulate_counterparty_substring_test(test) {
        return result;
    }
    Ok(())
}

/// Recognise and simulate `Given: <noun> with amounts X, Y, Z / When: …
/// queried with minimum N and maximum M / Then: returns the <count> <noun>
/// with amounts …`. Returns `None` when the shape doesn't match.
fn simulate_amount_range_test(test: &SynthesisedTest) -> Option<Result<(), String>> {
    let inputs = parse_amounts_list(&test.given)?;
    let (min, max) = parse_amount_range_bounds(&test.when)?;

    let mut expected: Vec<i64> = inputs
        .iter()
        .copied()
        .filter(|n| *n >= min && *n <= max)
        .collect();
    expected.sort();

    // Validate the count claim if one is present, then validate the listed
    // values claim if one is present. Either may be absent.
    let then_lower = test.then.to_ascii_lowercase();
    if let Some(claimed_count) = parse_count_word(&then_lower) {
        if claimed_count != expected.len() {
            return Some(Err(format!(
                "the test claims {} matching transactions but simulation finds {} (inputs {:?}, range {}..={})",
                claimed_count, expected.len(), inputs, min, max,
            )));
        }
    }
    if let Some(claimed_values) = parse_amounts_list(&test.then) {
        let mut sorted = claimed_values.clone();
        sorted.sort();
        if sorted != expected {
            return Some(Err(format!(
                "the test claims values {:?} but simulation finds {:?} (inputs {:?}, range {}..={})",
                claimed_values, expected, inputs, min, max,
            )));
        }
    }
    Some(Ok(()))
}

/// Recognise and simulate `Given: <noun> with counterparties "A", "B", "C"
/// / When: … queried with "search" / Then: returns the <count> …`. Match is
/// case-insensitive substring (the projection's documented semantics).
fn simulate_counterparty_substring_test(
    test: &SynthesisedTest,
) -> Option<Result<(), String>> {
    let inputs = parse_quoted_strings_after(&test.given, "counterparties ")?;
    let search = parse_quoted_string_after(&test.when, " with ")?;
    let needle = search.to_ascii_lowercase();

    let mut expected: Vec<String> = inputs
        .iter()
        .filter(|s| s.to_ascii_lowercase().contains(&needle))
        .cloned()
        .collect();
    expected.sort();

    let then_lower = test.then.to_ascii_lowercase();
    if let Some(claimed_count) = parse_count_word(&then_lower) {
        if claimed_count != expected.len() {
            return Some(Err(format!(
                "the test claims {} matching counterparties but simulation finds {} (inputs {:?}, search \"{}\")",
                claimed_count, expected.len(), inputs, search,
            )));
        }
    }
    if let Some(claimed) = parse_quoted_strings_in(&test.then) {
        let mut sorted = claimed.clone();
        sorted.sort();
        if sorted != expected {
            return Some(Err(format!(
                "the test claims values {:?} but simulation finds {:?} (inputs {:?}, search \"{}\")",
                claimed, expected, inputs, search,
            )));
        }
    }
    Some(Ok(()))
}

// ── Parser helpers ────────────────────────────────────────────────────

/// Extract `[X, Y, Z]` from a fragment like `transactions with amounts 75,
/// 100, 250` or `Given: transactions with amounts 5, 10, 15 in DKK`.
fn parse_amounts_list(text: &str) -> Option<Vec<i64>> {
    let lower = text.to_ascii_lowercase();
    let idx = lower.find("amounts ")?;
    let after = &text[idx + "amounts ".len()..];
    // Take the run of digits/commas/whitespace until we hit a non-numeric
    // token (e.g. `in DKK` or `, and excludes …`). Numbers may be negative.
    let mut nums: Vec<i64> = Vec::new();
    let mut current = String::new();
    let mut found_any = false;
    for ch in after.chars() {
        if ch.is_ascii_digit() || ch == '-' {
            current.push(ch);
            found_any = true;
        } else if ch == ',' || ch == ' ' || ch == '\n' || ch == '\t' {
            if !current.is_empty() {
                if let Ok(n) = current.parse::<i64>() {
                    nums.push(n);
                }
                current.clear();
            }
        } else if ch == '.' && current.is_empty() {
            // Sentence terminator after the list.
            break;
        } else {
            // First non-numeric, non-separator: close out the current run
            // and stop.
            if !current.is_empty() {
                if let Ok(n) = current.parse::<i64>() {
                    nums.push(n);
                }
            }
            break;
        }
    }
    if !current.is_empty() {
        if let Ok(n) = current.parse::<i64>() {
            nums.push(n);
        }
    }
    if !found_any || nums.is_empty() {
        return None;
    }
    Some(nums)
}

/// Extract `(min, max)` from `with minimum N and maximum M` (case-insensitive).
fn parse_amount_range_bounds(when: &str) -> Option<(i64, i64)> {
    let lower = when.to_ascii_lowercase();
    let min_idx = lower.find("minimum ")?;
    let after_min = &when[min_idx + "minimum ".len()..];
    let min: i64 = after_min
        .split(|c: char| !c.is_ascii_digit() && c != '-')
        .find(|s| !s.is_empty())?
        .parse()
        .ok()?;

    let max_idx = lower.find("maximum ")?;
    let after_max = &when[max_idx + "maximum ".len()..];
    let max: i64 = after_max
        .split(|c: char| !c.is_ascii_digit() && c != '-')
        .find(|s| !s.is_empty())?
        .parse()
        .ok()?;

    Some((min, max))
}

/// Map English count words (`one`..`twelve`, `all <n>`, plain digits) to a
/// count, or `None` when no count is mentioned.
fn parse_count_word(then_lower: &str) -> Option<usize> {
    const WORDS: &[(&str, usize)] = &[
        ("zero", 0), ("one", 1), ("two", 2), ("three", 3), ("four", 4),
        ("five", 5), ("six", 6), ("seven", 7), ("eight", 8), ("nine", 9),
        ("ten", 10), ("eleven", 11), ("twelve", 12),
    ];
    // `returns the <word> <noun>` or `returns <word> <noun>` or `all <word>`.
    for (word, count) in WORDS {
        // Word-bounded match — guard against false positives like `none` ⊃ `one`.
        for needle in [
            format!("returns the {} ", word),
            format!("returns {} ", word),
            format!("the {} ", word),
            format!("all {} ", word),
        ] {
            if then_lower.contains(&needle) {
                return Some(*count);
            }
        }
    }
    None
}

fn parse_quoted_strings_after(text: &str, marker: &str) -> Option<Vec<String>> {
    let lower = text.to_ascii_lowercase();
    let idx = lower.find(&marker.to_ascii_lowercase())?;
    let after = &text[idx + marker.len()..];
    let strings = collect_quoted_strings(after);
    if strings.is_empty() { None } else { Some(strings) }
}

fn parse_quoted_string_after(text: &str, marker: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let idx = lower.find(&marker.to_ascii_lowercase())?;
    let after = &text[idx + marker.len()..];
    collect_quoted_strings(after).into_iter().next()
}

fn parse_quoted_strings_in(text: &str) -> Option<Vec<String>> {
    let strings = collect_quoted_strings(text);
    if strings.is_empty() { None } else { Some(strings) }
}

fn collect_quoted_strings(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '"' || c == '\'' {
            let quote = c;
            let mut acc = String::new();
            for d in chars.by_ref() {
                if d == quote {
                    break;
                }
                acc.push(d);
            }
            if !acc.is_empty() {
                out.push(acc);
            }
        }
    }
    out
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
            kind: None, ..Default::default()
        });
        let prose = collect_synthesisable_prose(&artifact);
        assert_eq!(prose.len(), 2);
        assert_eq!(prose[0].label, "invariant");
        assert_eq!(prose[1].label, "operation `add`");
    }

    // ── Bug 7.1: simulation tests ────────────────────────────────

    fn projection_artifact(name: &str) -> PreparedArtifact {
        PreparedArtifact::new(
            ArtifactKind::Projection,
            format!("projections/{}.md", name),
            name.into(),
            name.into(),
        )
    }

    fn invariant_test(given: &str, when: &str, then: &str) -> SynthesisedTest {
        SynthesisedTest {
            source: "invariant".to_string(),
            given: given.into(),
            when: when.into(),
            then: then.into(),
            citation: Some("test".into()),
        }
    }

    #[test]
    fn validation_drops_test_with_inconsistent_input_output_amount_range() {
        // The exact lupa shape from the FIX_PLAN: input is five values,
        // range 100..=250, but the test claims "three" matches when the
        // simulation finds two ({100, 250}).
        let artifact = projection_artifact("TransactionList");
        let test = invariant_test(
            "transactions with amounts 75, 100, 250, 251, 300",
            "by_amount_range is queried with minimum 100 and maximum 250",
            "returns the three transactions with amounts 100 and 250",
        );
        let result = validate_synthesised_test(&artifact, &test);
        assert!(result.is_err(), "expected rejection, got Ok");
        let reason = result.unwrap_err();
        assert!(
            reason.contains("3") && reason.contains("2"),
            "rejection should name the count mismatch, got: {}",
            reason,
        );
    }

    #[test]
    fn validation_drops_test_with_inconsistent_input_output_counterparty() {
        // Counterparties contain "John Smith", "Smith Enterprises", "john",
        // "JOHN SMITH". Search "smith" matches three (case-insensitive).
        // The test wrongly claims all four.
        let artifact = projection_artifact("TransactionList");
        let test = invariant_test(
            "counterparties \"John Smith\", \"Smith Enterprises\", \"john\", \"JOHN SMITH\"",
            "by_counterparty is queried with \"smith\"",
            "returns all four counterparties",
        );
        let result = validate_synthesised_test(&artifact, &test);
        assert!(result.is_err(), "expected rejection, got Ok");
    }

    #[test]
    fn validation_keeps_consistent_test_amount_range() {
        // A correct test passes through unchanged.
        let artifact = projection_artifact("TransactionList");
        let test = invariant_test(
            "transactions with amounts 50, 150, 200, 250, 300, 500",
            "by_amount_range is queried with minimum 100 and maximum 250",
            "returns the three transactions with amounts 150, 200, 250",
        );
        let result = validate_synthesised_test(&artifact, &test);
        assert!(result.is_ok(), "expected pass-through, got: {:?}", result);
    }

    #[test]
    fn validation_keeps_consistent_test_counterparty() {
        let artifact = projection_artifact("TransactionList");
        let test = invariant_test(
            "counterparties \"Alice\", \"Bob\", \"Alice Corp\"",
            "by_counterparty is queried with \"alice\"",
            "returns the two values \"Alice\", \"Alice Corp\"",
        );
        let result = validate_synthesised_test(&artifact, &test);
        assert!(result.is_ok(), "expected pass-through, got: {:?}", result);
    }

    #[test]
    fn validation_unrecognised_shape_passes_through() {
        // A test whose Given/When/Then doesn't match any known projection
        // shape is left alone — we only reject when simulation contradicts.
        let artifact = projection_artifact("TransactionList");
        let test = invariant_test(
            "an authenticated session in the idle state",
            "the cart is rendered",
            "the rendered cart has zero items visible",
        );
        let result = validate_synthesised_test(&artifact, &test);
        assert!(result.is_ok());
    }

    #[test]
    fn validation_skipped_for_non_projection_kinds() {
        // Validation only fires for Projection artefacts; data and context
        // synthesised tests are not simulated.
        let artifact = PreparedArtifact::new(
            ArtifactKind::Data, "data/X.md".into(), "X".into(), "X".into(),
        );
        let test = invariant_test(
            "transactions with amounts 1, 2, 3",
            "by_amount_range is queried with minimum 1 and maximum 2",
            "returns all five transactions",
        );
        // Even though the claim is wrong, we don't simulate non-projection
        // tests — leaves room for data-specific validation in a later group.
        assert!(validate_synthesised_test(&artifact, &test).is_ok());
    }

    #[test]
    fn parse_amounts_list_handles_in_currency_suffix() {
        // The amounts list may be followed by `in DKK` or similar; the
        // parser must terminate at the non-numeric token without consuming
        // it.
        let nums = parse_amounts_list("transactions with amounts 50, 150, 200 in DKK")
            .expect("parsed");
        assert_eq!(nums, vec![50, 150, 200]);
    }

    #[test]
    fn parse_amount_range_bounds_handles_typical_phrasing() {
        let (min, max) = parse_amount_range_bounds(
            "by_amount_range is queried with minimum 100 and maximum 250",
        )
        .expect("parsed");
        assert_eq!((min, max), (100, 250));
    }

    #[test]
    fn parse_count_word_recognises_returns_the_three() {
        assert_eq!(parse_count_word("returns the three transactions with …"), Some(3));
        assert_eq!(parse_count_word("returns four entries that match …"), Some(4));
        assert_eq!(parse_count_word("the result is empty"), None);
    }

    #[test]
    fn synthesise_tests_returns_empty_without_api_key() {
        // Ensure ANTHROPIC_API_KEY is not set for this test
        let original = std::env::var("ANTHROPIC_API_KEY").ok();
        // SAFETY: test runs single-threaded for this env var
        unsafe { std::env::remove_var("ANTHROPIC_API_KEY") };

        let artifact =
            PreparedArtifact::new(ArtifactKind::Data, "data/X.md".into(), "X".into(), "X".into());
        let outcome = synthesise_tests(&artifact);
        assert!(outcome.tests.is_empty());
        assert!(outcome.findings.is_empty());

        // Restore
        if let Some(val) = original {
            // SAFETY: restoring previous value
            unsafe { std::env::set_var("ANTHROPIC_API_KEY", val) };
        }
    }
}
