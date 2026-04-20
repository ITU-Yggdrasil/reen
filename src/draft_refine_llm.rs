//! LLM-backed behavioral ambiguity review for raw drafts.
//!
//! The deterministic `review_raw_drafts` in [`crate::draft_refine`] is purely lexical: it finds
//! missing sections, vague phrases from a fixed list, and symbolic ambiguity. It cannot notice
//! semantic *under-specifications* like "`Position` never says which way `y` grows" or
//! "`Direction::Up` never states an `x`/`y` delta" — decisions an implementer must guess at.
//!
//! This module runs an additional pass that sends all raw drafts to an LLM with instructions to
//! *ask questions* about missing decisions. Each question carries a `severity: 0..=100` score,
//! which `reen refine` uses to filter noise via the `--min-severity` dial. Responses are cached
//! by content hash so repeat `refine` invocations are free when no drafts changed.
//!
//! The module is deliberately separate from `draft_refine.rs` so the deterministic linter
//! remains network-free and easy to unit-test.

use crate::agent_runner::{AgentRequest, AgentRunner, SystemBlock, extract_json};
use crate::draft_refine::{DraftReviewCategory, DraftReviewFinding};
use crate::workspace::Workspace;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

/// Prompt iteration counter. Bump this whenever the system prompt changes so previously cached
/// responses are invalidated automatically.
pub const SYSTEM_PROMPT_VERSION: &str = "v2";

/// Default minimum severity for behavioral-ambiguity findings. Below this, questions are
/// considered low-signal and suppressed.
pub const DEFAULT_MIN_SEVERITY: u8 = 40;

const SYSTEM_PROMPT: &str = r#"You are a DCI draft reviewer for the Reen build system.

These drafts are written by BUSINESS ANALYSTS, not programmers. They describe WHAT the system
must do, not HOW it will be implemented. Do NOT ask implementation questions. Ask only about
*behavioral* decisions the draft leaves genuinely open.

You are given a set of raw English draft specifications (markdown) describing:
- Data types (fields, invariants, getters)
- Context types (role players, props, role methods, functionalities)
- Projections (read-only views)
- An application draft wiring everything together

========================================================================================
DCI CONVENTIONS YOU MUST RESPECT — questions that violate these are FALSE POSITIVES
========================================================================================

1. **Role players are fields on the context / projection struct, NOT method parameters.**
   A "Role Players" section lists named slots (e.g. `board`, `stdin_source`) that the
   context owns. References to them in a flow — e.g. `board.symbol_at(x, y)` or
   `board.width()` — mean `self.board.symbol_at(...)`. They are IMPLICIT collaborators and
   MUST NOT be listed in the functionality signature's parameter list.

   FALSE POSITIVE (do not ask):
     "Why is `board` not a parameter to `render` when the flow references `board.symbol_at`?"
     "How does the renderer access `board` if it's not a parameter?"
     "Should `board` be added to the `render` signature?"
   These have an obvious answer — `board` is a role player field bound at construction.

2. **Props are also fields on the context, NOT method parameters.**
   A "Props" section lists scalar/config values captured at construction (e.g. `score`,
   `tick_ms`). Flows reference them by bare name; this means `self.<prop>`. Do not ask why
   a prop isn't a parameter.

   EXCEPTION: if the SAME name appears both in Props and in a functionality's signature,
   that IS a real ambiguity worth asking about — which one wins?

3. **Role methods live on the context, not on the collaborator type.**
   A section like `### board` followed by `- **symbol_at** Signature: ...` defines a role
   method. A call like `board.symbol_at(p)` in a flow invokes the corresponding context
   wrapper. Do not ask "where does `symbol_at` come from?" — it comes from the role method
   table in the same draft.

4. **Data-type getters return the field they're named after.**
   If `Message` has a field `text` and a functionality `text(&self) -> String`, the flow
   is trivially "return a clone of text". Do not ask what `text()` returns.

5. **Construction flows that just store parameters are not ambiguous.**
   A `new(a, b)` flow that says "Store `a`. Store `b`." is fully specified for a BA audience.
   Do not ask about memory ownership, clone-vs-move, `Arc`, lifetimes, thread-safety, or
   other Rust-only concerns.

6. **`&self` / `&mut self` is not the BA's job.**
   Do not ask about receiver kinds, interior mutability, `&mut` vs `&`, etc.

========================================================================================
WHAT TO ACTUALLY ASK ABOUT
========================================================================================

A downstream build agent will translate these drafts into Rust. Your job is to read the drafts
as *specifications* and surface **missing decisions** that a reasonable implementer would
otherwise have to guess at — especially where two plausible readings lead to observably
different user-visible behavior.

Typical classes of under-specification worth surfacing:
- Coordinate conventions (does y=0 mean the top or the bottom? does x grow left-to-right?).
- Unit conventions (ms vs seconds, bytes vs characters, zero-based vs one-based indices).
- Iteration and ordering (does "first" mean insertion order, smallest, leftmost, topmost?).
- Semantics of "empty", "none", "zero", "default", "reset" where the draft leaves it implicit.
- Error and edge-case behavior (what happens on empty input, out-of-bounds, overflow, missing
  role player, duplicate key, concurrent call).
- Mutability semantics visible to the user (is a returned collection a snapshot or a live
  view — i.e. do later changes to the source show up in the returned value?).
- String/byte boundaries where a user would notice (grapheme clusters vs bytes in a count).
- Timing/scheduling (how often does a tick fire? synchronous vs asynchronous?).
- Invariants at construction that can fail and surface to the user (e.g. "can `new` return
  an error if the board is empty?").

Rules:
1. **Ask questions, do NOT propose answers.** Your output is a list of questions only.
2. Only ask questions where the draft *really* leaves the implementer a choice that a user
   would notice. If the answer follows obviously from DCI conventions or from elsewhere in
   the same draft, STAY SILENT.
3. Do not ask about Rust idioms (`Result` vs `Option`, `&str` vs `String`, naming conventions,
   ownership, lifetimes, `Arc`/`Rc`/`Box`, `clone()` vs move). Ask about *observable* behavior.
4. Score each question's `severity` on a 0..=100 scale:
   - 100: two plausible readings lead to clearly different, user-visible behavior (e.g. up/down
     keys inverted, a fatal crash vs silent no-op, data loss vs data preservation).
   - 70-90: two plausible readings differ in observable behavior but the wrong guess is likely
     to be caught quickly.
   - 40-60: the draft is ambiguous but the most likely reading is obvious in context.
   - 0-39: cosmetic or editorial, not worth blocking a build on.
5. Each question references a specific draft by its relative path (e.g. `drafts/data/Position.md`)
   and, when possible, an approximate `line` number counted from 1.
6. Suggest what *shape* of answer would resolve the question (e.g. "Add a sentence stating that
   y=0 is the top row and y grows downward."). Never emit the answer itself.
7. Prefer cross-draft observations. The most dangerous under-specifications sit at the boundary
   between two drafts (e.g. a `Direction` enum and a `tick` flow that increments `y`).
8. When in doubt, DO NOT ASK. A false-positive question blocks the build and wastes the
   author's time. A missed real ambiguity is recoverable — the build or the user will catch
   it later. Bias strongly toward silence.

Output format — a JSON object with a single top-level key `questions`, each element:

{
  "draft_path": "<relative path, e.g. drafts/data/Position.md>",
  "line": <1-based line number or null>,
  "severity": <integer 0..=100>,
  "question": "<the question, one sentence>",
  "why_this_matters": "<one sentence on observable consequence>",
  "suggested_answer_shape": "<one sentence on what kind of sentence to add>"
}

Return strictly valid JSON. No markdown, no commentary before or after. If the drafts contain
no meaningful under-specifications, return `{"questions": []}`.
"#;

/// Runtime knobs for the LLM-backed behavioral review.
#[derive(Debug, Clone)]
pub struct BehavioralReviewOptions {
    /// Minimum `severity` (0..=100) a question must have to be surfaced as a finding.
    pub min_severity: u8,
    /// When `true`, a missing API key or a network failure is a hard error. When `false`, the
    /// phase fails open with a warning and produces no findings.
    pub require_llm: bool,
    /// Directory where cached LLM responses are stored. Must already include the per-workspace
    /// prefix; a typical value is `{workspace}/.reen/cache/refine-llm`.
    pub cache_dir: PathBuf,
    pub verbose: bool,
}

impl Default for BehavioralReviewOptions {
    fn default() -> Self {
        Self {
            min_severity: DEFAULT_MIN_SEVERITY,
            require_llm: false,
            cache_dir: PathBuf::from(".reen/cache/refine-llm"),
            verbose: false,
        }
    }
}

/// A single behavioral-ambiguity question as returned by the LLM. Public so callers can
/// inspect raw model output if they want; the default path converts these to
/// [`DraftReviewFinding`]s.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BehavioralQuestion {
    pub draft_path: String,
    #[serde(default)]
    pub line: Option<usize>,
    pub severity: u8,
    pub question: String,
    #[serde(default)]
    pub why_this_matters: String,
    #[serde(default)]
    pub suggested_answer_shape: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct BehavioralResponse {
    #[serde(default)]
    questions: Vec<BehavioralQuestion>,
}

/// Describes one draft sent to the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DraftEntry {
    relative_path: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct BehavioralPayload<'a> {
    drafts: &'a [DraftEntry],
}

/// Cache file layout — carries enough metadata to detect stale cache files from earlier
/// prompt iterations.
#[derive(Debug, Serialize, Deserialize)]
struct CacheRecord {
    model: String,
    prompt_version: String,
    content_hash: String,
    questions: Vec<BehavioralQuestion>,
}

/// Top-level entry point used by `refine_workspace`.
pub fn review_raw_drafts_behavioral(
    workspace: &Workspace,
    paths: &[PathBuf],
    options: &BehavioralReviewOptions,
) -> Result<Vec<DraftReviewFinding>> {
    let drafts = read_drafts(workspace, paths)?;
    if drafts.is_empty() {
        return Ok(Vec::new());
    }

    let runner = match AgentRunner::from_env() {
        Ok(runner) => runner,
        Err(error) => {
            if options.require_llm {
                return Err(error.context(
                    "refine --require-llm-review was requested but the LLM runner could not be \
                     initialised",
                ));
            }
            if options.verbose {
                eprintln!(
                    "refine: skipping behavioral ambiguity review ({}).",
                    short_error_summary(&error)
                );
            }
            return Ok(Vec::new());
        }
    };

    let content_hash = compute_content_hash(&drafts);
    let cache_key = cache_key_for(&content_hash, runner.model(), SYSTEM_PROMPT_VERSION);
    let cache_path = options.cache_dir.join(format!("{cache_key}.json"));

    if let Some(record) = try_load_cache(&cache_path, &content_hash, runner.model()) {
        if options.verbose {
            eprintln!(
                "refine: using cached behavioral review from {}",
                cache_path.display()
            );
        }
        return Ok(questions_to_findings(
            &record.questions,
            options.min_severity,
        ));
    }

    let payload = serde_json::to_string(&BehavioralPayload { drafts: &drafts })
        .context("serialize behavioral-review payload")?;
    let request = AgentRequest {
        system: vec![SystemBlock::cached(SYSTEM_PROMPT)],
        user_content: &payload,
        temperature: 0.0,
        max_tokens: 8_000,
    };

    let raw = match runner.run_json(&request) {
        Ok(raw) => raw,
        Err(error) => {
            if options.require_llm {
                return Err(error.context(
                    "behavioral-ambiguity review failed and --require-llm-review is set",
                ));
            }
            if options.verbose {
                eprintln!(
                    "refine: skipping behavioral ambiguity review ({}).",
                    short_error_summary(&error)
                );
            }
            return Ok(Vec::new());
        }
    };

    let questions = match parse_review_response(&raw) {
        Ok(questions) => questions,
        Err(error) => {
            if options.require_llm {
                return Err(error.context("parse behavioral-review response"));
            }
            if options.verbose {
                eprintln!(
                    "refine: skipping behavioral ambiguity review (unparseable response: {error})."
                );
            }
            return Ok(Vec::new());
        }
    };

    if let Err(error) = write_cache(
        &cache_path,
        &CacheRecord {
            model: runner.model().to_string(),
            prompt_version: SYSTEM_PROMPT_VERSION.to_string(),
            content_hash: content_hash.clone(),
            questions: questions.clone(),
        },
    ) && options.verbose
    {
        eprintln!(
            "refine: failed to write behavioral review cache {}: {error}",
            cache_path.display()
        );
    }

    Ok(questions_to_findings(&questions, options.min_severity))
}

fn read_drafts(workspace: &Workspace, paths: &[PathBuf]) -> Result<Vec<DraftEntry>> {
    let mut drafts = Vec::with_capacity(paths.len());
    for path in paths {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let relative = path
            .strip_prefix(&workspace.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        drafts.push(DraftEntry {
            relative_path: relative,
            content,
        });
    }
    drafts.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(drafts)
}

/// Canonicalise the draft corpus into a single byte string and hash it. The hash is used as
/// one component of the cache key — two runs with identical drafts produce identical hashes
/// regardless of file-system traversal order. Sorts defensively so callers don't have to
/// remember to do it themselves.
fn compute_content_hash(drafts: &[DraftEntry]) -> String {
    let mut sorted: Vec<&DraftEntry> = drafts.iter().collect();
    sorted.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    let mut hasher = Sha256::new();
    for entry in sorted {
        hasher.update(entry.relative_path.as_bytes());
        hasher.update(b"\0");
        hasher.update(entry.content.as_bytes());
        hasher.update(b"\0");
    }
    hex_digest(&hasher.finalize())
}

/// Cache key = sha256(content_hash || prompt_version || model). The content hash is already a
/// digest; hashing again mixes in the prompt/model so a prompt tweak or model swap invalidates
/// the cache without us having to remember to bump version constants by hand.
fn cache_key_for(content_hash: &str, model: &str, prompt_version: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content_hash.as_bytes());
    hasher.update(b"|");
    hasher.update(prompt_version.as_bytes());
    hasher.update(b"|");
    hasher.update(model.as_bytes());
    hex_digest(&hasher.finalize())
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn try_load_cache(path: &Path, content_hash: &str, model: &str) -> Option<CacheRecord> {
    let raw = fs::read_to_string(path).ok()?;
    let record: CacheRecord = serde_json::from_str(&raw).ok()?;
    if record.content_hash == content_hash
        && record.prompt_version == SYSTEM_PROMPT_VERSION
        && record.model == model
    {
        Some(record)
    } else {
        None
    }
}

fn write_cache(path: &Path, record: &CacheRecord) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let body = serde_json::to_string_pretty(record)
        .context("serialize behavioral-review cache record")?;
    fs::write(path, body).with_context(|| format!("Failed to write {}", path.display()))
}

/// Parse the LLM's JSON response, tolerating both direct objects and markdown-fenced output.
pub fn parse_review_response(raw: &str) -> Result<Vec<BehavioralQuestion>> {
    let json = extract_json(raw);
    let response: BehavioralResponse = serde_json::from_str(&json)
        .with_context(|| format!("parse behavioral-review response: {}", truncate(&json, 300)))?;
    Ok(response.questions)
}

/// Drop questions below `min_severity` and convert the survivors into `DraftReviewFinding`s.
pub fn questions_to_findings(
    questions: &[BehavioralQuestion],
    min_severity: u8,
) -> Vec<DraftReviewFinding> {
    let mut findings = Vec::new();
    for question in questions {
        if question.severity < min_severity {
            continue;
        }
        findings.push(question_to_finding(question));
    }
    crate::draft_refine::sort_and_dedup_findings(&mut findings);
    findings
}

fn question_to_finding(question: &BehavioralQuestion) -> DraftReviewFinding {
    let mut message = question.question.trim().to_string();
    if !question.why_this_matters.trim().is_empty() {
        message.push(' ');
        message.push_str("— why this matters: ");
        message.push_str(question.why_this_matters.trim());
    }
    if !question.suggested_answer_shape.trim().is_empty() {
        message.push(' ');
        message.push_str("(suggested answer shape: ");
        message.push_str(question.suggested_answer_shape.trim());
        message.push(')');
    }
    DraftReviewFinding {
        draft_path: question.draft_path.clone(),
        source_line: question.line,
        category: DraftReviewCategory::BehavioralAmbiguity,
        message,
        severity: Some(question.severity.min(100)),
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut truncated: String = value.chars().take(max_chars).collect();
    truncated.push('…');
    truncated
}

fn short_error_summary(error: &anyhow::Error) -> String {
    let text = error.to_string();
    truncate(text.lines().next().unwrap_or(""), 160)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_drafts() -> Vec<DraftEntry> {
        vec![
            DraftEntry {
                relative_path: "drafts/data/Position.md".to_string(),
                content: "# Position\n\n- x, y fields\n".to_string(),
            },
            DraftEntry {
                relative_path: "drafts/data/Direction.md".to_string(),
                content: "# Direction\n\nUp | Down | Left | Right\n".to_string(),
            },
        ]
    }

    #[test]
    fn content_hash_is_insensitive_to_input_order() {
        // `compute_content_hash` sorts internally so the hash is stable regardless of the
        // order the caller happens to hand in. Otherwise a filesystem whose directory
        // iteration is unstable would miss the cache.
        let ordered = sample_drafts();
        let mut reversed = ordered.clone();
        reversed.reverse();

        let a = compute_content_hash(&ordered);
        let b = compute_content_hash(&reversed);
        assert_eq!(a, b, "same drafts should always hash to the same key");
    }

    #[test]
    fn content_hash_changes_when_any_draft_changes() {
        let base = sample_drafts();
        let mut tweaked = base.clone();
        tweaked[0].content.push_str("edit\n");
        assert_ne!(compute_content_hash(&base), compute_content_hash(&tweaked));
    }

    #[test]
    fn cache_key_depends_on_prompt_version_and_model() {
        let hash = "abcdef".to_string();
        let a = cache_key_for(&hash, "model-a", "v1");
        let b = cache_key_for(&hash, "model-b", "v1");
        let c = cache_key_for(&hash, "model-a", "v2");
        assert_ne!(a, b, "model must be part of the key");
        assert_ne!(a, c, "prompt_version must be part of the key");
    }

    #[test]
    fn parse_review_response_accepts_plain_json() {
        let raw = r#"{
            "questions": [
                {
                    "draft_path": "drafts/data/Position.md",
                    "line": 12,
                    "severity": 85,
                    "question": "Does y=0 represent the top or the bottom row?",
                    "why_this_matters": "Direction::Up sign depends on this.",
                    "suggested_answer_shape": "Add a sentence stating y=0 is the top."
                }
            ]
        }"#;
        let questions = parse_review_response(raw).expect("parse");
        assert_eq!(questions.len(), 1);
        assert_eq!(questions[0].severity, 85);
        assert_eq!(questions[0].line, Some(12));
    }

    #[test]
    fn parse_review_response_strips_markdown_fences() {
        let raw = "Here is the review:\n```json\n{\"questions\": []}\n```\n";
        let questions = parse_review_response(raw).expect("parse");
        assert!(questions.is_empty());
    }

    #[test]
    fn parse_review_response_tolerates_missing_optional_fields() {
        let raw = r#"{
            "questions": [
                {
                    "draft_path": "drafts/data/Position.md",
                    "severity": 80,
                    "question": "Does y=0 represent the top?"
                }
            ]
        }"#;
        let questions = parse_review_response(raw).expect("parse");
        assert_eq!(questions.len(), 1);
        assert!(questions[0].why_this_matters.is_empty());
        assert!(questions[0].suggested_answer_shape.is_empty());
        assert_eq!(questions[0].line, None);
    }

    #[test]
    fn severity_filter_drops_low_severity_questions() {
        let questions = vec![
            BehavioralQuestion {
                draft_path: "a.md".into(),
                line: Some(1),
                severity: 80,
                question: "high".into(),
                why_this_matters: String::new(),
                suggested_answer_shape: String::new(),
            },
            BehavioralQuestion {
                draft_path: "a.md".into(),
                line: Some(2),
                severity: 20,
                question: "low".into(),
                why_this_matters: String::new(),
                suggested_answer_shape: String::new(),
            },
        ];
        let findings = questions_to_findings(&questions, 40);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.starts_with("high"));
        assert_eq!(findings[0].severity, Some(80));
        assert_eq!(findings[0].category, DraftReviewCategory::BehavioralAmbiguity);
    }

    #[test]
    fn findings_include_why_and_suggestion_in_message() {
        let question = BehavioralQuestion {
            draft_path: "a.md".into(),
            line: None,
            severity: 90,
            question: "Does y=0 represent the top?".into(),
            why_this_matters: "Up/down signs depend on it.".into(),
            suggested_answer_shape: "State that y=0 is the top row.".into(),
        };
        let findings = questions_to_findings(&[question], 40);
        assert_eq!(findings.len(), 1);
        let message = &findings[0].message;
        assert!(message.contains("why this matters"));
        assert!(message.contains("suggested answer shape"));
        assert!(message.contains("y=0 is the top row"));
    }

    #[test]
    fn cache_roundtrip_preserves_questions() {
        let tmp = std::env::temp_dir().join(format!(
            "reen_refine_llm_cache_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = tmp.join("entry.json");
        let record = CacheRecord {
            model: "model-x".into(),
            prompt_version: SYSTEM_PROMPT_VERSION.into(),
            content_hash: "abc".into(),
            questions: vec![BehavioralQuestion {
                draft_path: "a.md".into(),
                line: Some(5),
                severity: 60,
                question: "Q?".into(),
                why_this_matters: "W".into(),
                suggested_answer_shape: "S".into(),
            }],
        };
        write_cache(&path, &record).expect("write cache");

        let loaded = try_load_cache(&path, "abc", "model-x").expect("cache hit");
        assert_eq!(loaded.questions, record.questions);

        assert!(try_load_cache(&path, "other", "model-x").is_none());
        assert!(try_load_cache(&path, "abc", "other-model").is_none());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn severity_clamped_to_100_in_finding() {
        let question = BehavioralQuestion {
            draft_path: "a.md".into(),
            line: None,
            severity: 130,
            question: "bad score".into(),
            why_this_matters: String::new(),
            suggested_answer_shape: String::new(),
        };
        let findings = questions_to_findings(&[question], 40);
        assert_eq!(findings[0].severity, Some(100));
    }
}
