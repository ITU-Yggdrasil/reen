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
    for mut test in raw_tests {
        // Bug 7.3 — verbatim citations. Replace the LLM's paraphrase with
        // the canonical text from the artefact when we can match it; reject
        // the test entirely when the citation refers to nothing declared
        // (the LLM cites a fabricated invariant or functionality).
        if let Some(raw_citation) = test.citation.as_deref() {
            match canonicalise_citation(artifact, raw_citation) {
                Some(canonical) => test.citation = Some(canonical),
                None => {
                    outcome.findings.push(Ambiguity {
                        message: format!(
                            "Synthesised test rejected for `{}`: citation `{}` does not match any declared invariant, functionality, or operation",
                            artifact.export.name, raw_citation,
                        ),
                        severity: "low".to_string(),
                        suggested_resolution: Some(
                            "add an explicit scenario for this invariant if you want it tested".into(),
                        ),
                        source_line: None,
                    });
                    continue;
                }
            }
        }

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

/// Match the LLM's free-form citation against the artefact's declared
/// invariants / functionalities / operations and return the canonical text
/// when a match is found. Returns `None` when no canonical entry can be
/// matched — the caller drops the test.
///
/// Matching is permissive but anchored on canonical text. We accept a
/// match when the citation contains a salient run of words from any
/// canonical entry — that catches both `"invariant: BIC must contain only
/// letters in either case"` (verbatim) and the LLM-typo'd
/// `"…letters in their case, or digits"` (canonical wins, restoring
/// `either`).
pub fn canonicalise_citation(
    artifact: &PreparedArtifact,
    raw_citation: &str,
) -> Option<String> {
    let lower = raw_citation.to_ascii_lowercase();

    // 1. Invariants. Match if the citation overlaps the invariant by a
    //    significant run of words.
    if let Some(invariant) = best_invariant_match(&lower, &artifact.invariants) {
        return Some(format!("invariant: {}", invariant));
    }

    // 2. Functionalities. Match by name (`functionality \`<name>\``) or by
    //    description overlap.
    for func in &artifact.functionalities {
        let name_lower = func.name.to_ascii_lowercase();
        if lower.contains(&name_lower) {
            return Some(format!(
                "functionality `{}`: {}",
                func.name,
                func.description.lines().next().unwrap_or(&func.description),
            ));
        }
    }

    // 3. Operations. Same shape as functionalities.
    for op in &artifact.operations {
        let name_lower = op.name.to_ascii_lowercase();
        if lower.contains(&name_lower) {
            return Some(format!(
                "operation `{}`: {}",
                op.name,
                op.description.lines().next().unwrap_or(&op.description),
            ));
        }
    }

    None
}

/// Find the invariant whose text best overlaps the citation. We require at
/// least 5 consecutive words from the canonical text to appear in the
/// citation — that's enough to pin the citation to one invariant while
/// tolerating the LLM's paraphrase around the edges.
fn best_invariant_match<'a>(
    lower_citation: &str,
    invariants: &'a [String],
) -> Option<&'a String> {
    const MIN_RUN: usize = 5;
    for inv in invariants {
        let inv_lower = inv.to_ascii_lowercase();
        let words: Vec<&str> = inv_lower.split_whitespace().collect();
        if words.len() < MIN_RUN {
            // Short invariant: require a verbatim full-text match.
            if lower_citation.contains(&inv_lower) {
                return Some(inv);
            }
            continue;
        }
        // Slide a 5-word window across the invariant; if any window is in
        // the citation, the citation is referencing this invariant.
        for window in words.windows(MIN_RUN) {
            let probe = window.join(" ");
            if lower_citation.contains(&probe) {
                return Some(inv);
            }
        }
    }
    None
}

// ── Validation (bug 7.1: input/output simulation) ────────────────────
//
// For projection-query tests, parse the Given input set, the When bounds /
// search, and the Then expected output. Simulate the query against the
// inputs and compare to the claim. The patterns are an explicit allow-list
// — when the test's prose doesn't match any known shape, we simply pass
// the test through unchanged. We only reject when simulation contradicts.

/// Validate one synthesised test against simulation and the
/// declared-functionality allow-list. Returns `Ok(())` when the test
/// passes both checks. Returns `Err(reason)` when:
///
/// * the test's `When:` invokes zero declared functionalities (the LLM
///   invented a name), or invokes more than one (the LLM combined
///   independent functionalities into a fictitious composite), or
/// * the simulation of a recognised projection-query shape contradicts
///   the test's claim.
pub fn validate_synthesised_test(
    artifact: &PreparedArtifact,
    test: &SynthesisedTest,
) -> Result<(), String> {
    // Functionality-name allow-list (bug 7.2). The When clause must invoke
    // exactly one declared functionality / operation / query — zero means
    // the LLM invented a name, more than one means it combined independents
    // into a fictitious composite. We exempt tests whose When has no
    // snake_case identifiers at all (free prose like "the operation is
    // performed"), because there's nothing for the allow-list to check.
    let invoked = invoked_function_names(&test.when);
    let allow_list = declared_function_names(artifact);
    let known: Vec<&String> = invoked
        .iter()
        .filter(|name| allow_list.contains(*name))
        .collect();
    let unknown: Vec<&String> = invoked
        .iter()
        .filter(|name| !allow_list.contains(*name))
        .collect();
    if !invoked.is_empty() {
        if !unknown.is_empty() {
            return Err(format!(
                "the test's When clause invokes `{}`, which is not declared on `{}` (declared names: [{}])",
                unknown[0],
                artifact.export.name,
                allow_list.iter().cloned().collect::<Vec<_>>().join(", "),
            ));
        }
        if known.len() > 1 {
            return Err(format!(
                "the test's When clause combines multiple declared functionalities ({}), but `{}` declares each one independently — there is no composite",
                known.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "),
                artifact.export.name,
            ));
        }
    }

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

/// Collect the snake_case identifiers (two or more underscore-joined
/// segments, e.g. `by_amount_range`, `is_expired`) that appear in `when`.
/// Single-word verbs like "performed" are not function-name candidates and
/// are skipped — the allow-list check only fires on identifier-shaped
/// tokens that look like the canonical method names produced by codegen.
fn invoked_function_names(when: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in when.split(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
        if raw.is_empty() {
            continue;
        }
        // Lowercase identifier with at least one underscore separating
        // word segments — this matches every canonical method name our
        // codegen emits (`by_amount_range`, `is_expired`,
        // `transition_chat_state`).
        if !raw.contains('_') {
            continue;
        }
        if raw.chars().any(|c| c.is_ascii_uppercase()) {
            continue;
        }
        if !raw
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            continue;
        }
        // Discard duplicate occurrences in the same When.
        let s = raw.to_string();
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

/// Collect declared names from the artefact: data operations, context
/// functionalities, projection queries (functionalities). Each is
/// snake-cased to match the codegen output.
fn declared_function_names(artifact: &PreparedArtifact) -> std::collections::BTreeSet<String> {
    let mut set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for op in &artifact.operations {
        set.insert(crate::codegen::to_snake_case(&op.name));
    }
    for func in &artifact.functionalities {
        set.insert(crate::codegen::to_snake_case(&func.name));
    }
    set
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
        let mut a = PreparedArtifact::new(
            ArtifactKind::Projection,
            format!("projections/{}.md", name),
            name.into(),
            name.into(),
        );
        // Populate the canonical projection-query allow-list so simulation
        // tests aren't intercepted by the bug 7.2 allow-list check first.
        for fn_name in ["filtered", "by_counterparty", "by_amount_range"] {
            a.functionalities.push(crate::prepared::FunctionalitySpec {
                name: fn_name.into(),
                description: "test".into(),
                role_method_refs: vec![],
                ..Default::default()
            });
        }
        a
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
    fn simulation_skipped_for_non_projection_kinds() {
        // Simulation (range / substring) only fires for Projection
        // artefacts. The allow-list check (bug 7.2) still applies — see
        // `validation_drops_test_referencing_undeclared_functionality_on_data`
        // — but here we use prose-only When so the allow-list has nothing
        // to gate on.
        let artifact = PreparedArtifact::new(
            ArtifactKind::Data, "data/X.md".into(), "X".into(), "X".into(),
        );
        let test = invariant_test(
            "an instance of X",
            "the operation is performed",
            "the post-condition holds",
        );
        assert!(validate_synthesised_test(&artifact, &test).is_ok());
    }

    #[test]
    fn validation_drops_test_referencing_undeclared_functionality_on_data() {
        // Allow-list check applies to all artefact kinds — a Data artefact
        // with no declared `add` operation rejects a test that invokes it.
        let artifact = PreparedArtifact::new(
            ArtifactKind::Data, "data/Money.md".into(), "Money".into(), "Money".into(),
        );
        let test = invariant_test(
            "two amounts in DKK",
            "deep_arithmetic_combine is performed",
            "the result is correct",
        );
        let result = validate_synthesised_test(&artifact, &test);
        assert!(result.is_err());
    }

    // ── Bug 7.2: functionality-name allow-list ───────────────────

    #[test]
    fn validation_drops_test_referencing_undeclared_functionality() {
        // Lupa's TransactionList declares `filtered`, `by_counterparty`,
        // and `by_amount_range`. A test invoking `by_total_value` is
        // outside the allow-list — reject.
        let artifact = projection_artifact("TransactionList");
        let test = invariant_test(
            "transactions with amounts 100, 200, 300",
            "by_total_value is queried with threshold 100",
            "returns the three transactions",
        );
        let result = validate_synthesised_test(&artifact, &test);
        assert!(result.is_err(), "expected rejection, got Ok");
        assert!(
            result.as_ref().unwrap_err().contains("by_total_value"),
            "rejection should name the unknown functionality, got: {:?}",
            result,
        );
    }

    #[test]
    fn validation_drops_test_combining_multiple_declared_functionalities() {
        // The "combined filter" case from the FIX_PLAN: each name is
        // declared, but the projection has no composite — invoking three
        // in one When fabricates behaviour.
        let artifact = projection_artifact("TransactionList");
        let test = invariant_test(
            "transactions with amounts 100, 200, 300",
            "filtered AND by_counterparty AND by_amount_range are queried together",
            "returns the matching subset",
        );
        let result = validate_synthesised_test(&artifact, &test);
        assert!(result.is_err(), "expected rejection, got Ok");
        assert!(
            result.as_ref().unwrap_err().contains("combines multiple"),
            "rejection should mention the combination, got: {:?}",
            result,
        );
    }

    #[test]
    fn validation_keeps_test_referencing_declared_functionality() {
        let artifact = projection_artifact("TransactionList");
        let test = invariant_test(
            "transactions with amounts 100, 200, 300, 500",
            "by_amount_range is queried with minimum 100 and maximum 300",
            "returns the three transactions with amounts 100, 200, 300",
        );
        let result = validate_synthesised_test(&artifact, &test);
        assert!(result.is_ok(), "expected pass-through, got: {:?}", result);
    }

    #[test]
    fn validation_keeps_test_with_no_function_identifier() {
        // The When clause may be free prose ("the operation is performed")
        // rather than a name-shaped invocation. We can't allow-list-check
        // such tests, so they pass through.
        let artifact = projection_artifact("TransactionList");
        let test = invariant_test(
            "an empty transaction list",
            "the projection is queried",
            "the result is empty",
        );
        let result = validate_synthesised_test(&artifact, &test);
        assert!(result.is_ok(), "expected pass-through, got: {:?}", result);
    }

    #[test]
    fn invoked_function_names_picks_snake_case_only() {
        // Single-word verbs ("queried", "performed") are skipped — they
        // don't match the canonical method-name shape.
        let names = invoked_function_names(
            "by_amount_range is queried with minimum 100 and maximum 250",
        );
        assert_eq!(names, vec!["by_amount_range".to_string()]);

        // CamelCase identifiers are skipped (we never emit those as method
        // names).
        let names = invoked_function_names("ByAmountRange is queried");
        assert!(names.is_empty(), "got {:?}", names);
    }

    // ── Bug 7.3: verbatim citations ──────────────────────────────

    #[test]
    fn citation_is_copied_verbatim_from_prepared_yaml() {
        // The headline lupa typo: BIC's invariant says "letters in either
        // case", LLM paraphrases as "letters in their case". The
        // canonical form must replace the LLM string.
        let mut artifact = PreparedArtifact::new(
            ArtifactKind::Data, "data/BIC.md".into(), "BIC".into(), "BIC".into(),
        );
        artifact.invariants = vec![
            "BIC must contain only letters in either case, or digits".to_string(),
        ];
        let raw = "invariant: BIC must contain only letters in their case, or digits";
        let canonical = canonicalise_citation(&artifact, raw).expect("matched");
        assert_eq!(
            canonical,
            "invariant: BIC must contain only letters in either case, or digits",
        );
    }

    #[test]
    fn citation_matches_functionality_by_name() {
        let mut artifact = projection_artifact("TransactionList");
        artifact.functionalities[2].description =
            "Returns transactions whose amount falls within the given minimum and maximum, inclusive.".to_string();
        let canonical = canonicalise_citation(
            &artifact,
            "from the description of by_amount_range",
        )
        .expect("matched");
        assert!(canonical.starts_with("functionality `by_amount_range`"));
        assert!(canonical.contains("Returns transactions whose amount"));
    }

    #[test]
    fn citation_matches_operation_by_name() {
        let mut artifact = PreparedArtifact::new(
            ArtifactKind::Data, "data/Money.md".into(), "Money".into(), "Money".into(),
        );
        artifact.operations.push(crate::prepared::OperationSpec {
            name: "add".into(),
            description: "Sum two amounts in matching currency.".into(),
            kind: None,
            ..Default::default()
        });
        let canonical = canonicalise_citation(&artifact, "operation add semantics").expect("matched");
        assert_eq!(canonical, "operation `add`: Sum two amounts in matching currency.");
    }

    #[test]
    fn citation_returns_none_when_nothing_matches() {
        let artifact = PreparedArtifact::new(
            ArtifactKind::Data, "data/X.md".into(), "X".into(), "X".into(),
        );
        // Nothing declared — no invariants, no functionalities, no operations.
        // The citation cannot match anything.
        let result = canonicalise_citation(&artifact, "invariant: a non-existent rule");
        assert!(result.is_none());
    }

    #[test]
    fn validation_emits_finding_when_test_dropped() {
        // synthesise_tests is the orchestrator. We can't drive it past the
        // API-key gate without an LLM, so instead we mimic its body: a
        // mismatched citation + a contradicting simulation should both
        // produce findings on the artefact. Verified at the unit level
        // here; the end-to-end flow is exercised via the
        // `validate_synthesised_test` + `canonicalise_citation` calls in
        // `synthesise_tests`.
        let artifact = projection_artifact("TransactionList");
        // Citation references nothing.
        let test = SynthesisedTest {
            source: "invariant".into(),
            given: "transactions with amounts 1, 2, 3".into(),
            when: "by_amount_range is queried with minimum 1 and maximum 2".into(),
            then: "returns the two transactions with amounts 1, 2".into(),
            citation: Some("invariant: a fabricated rule that doesn't exist".into()),
        };
        let canonical = canonicalise_citation(&artifact, test.citation.as_deref().unwrap());
        assert!(canonical.is_none(), "citation must not match — finding emitted");
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
