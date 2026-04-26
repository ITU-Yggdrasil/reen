//! Phrase registry for inference passes.
//!
//! Every English-idiom allow-list used by signature inference,
//! multiplicity inference, or query-parameter parsing lives in one of
//! the sub-modules below. The classifier functions stay here so the
//! existing `crate::inference::phrases::is_rejection_then` /
//! `classify_return_shape` / `count_phrase` paths keep working without
//! caller updates.
//!
//! ## Why an explicit allow-list, no regex
//!
//! Every classifier here is an explicit substring allow-list. New
//! shapes get added by extending the list, not by widening a pattern.
//! BA prose drifts in small ways across drafts; a regex that "almost"
//! matches is hard to audit when it produces a wrong inference. An
//! explicit list:
//!
//! - makes the supported shapes legible at review time;
//! - forces a deliberate decision when the vocabulary expands;
//! - lets each constant carry doc-tests that pin both accept and
//!   reject cases — the doc-test IS the corpus.
//!
//! When no shape matches, the classifier returns the safe default
//! (unit return, single multiplicity, empty parameter list) and
//! codegen falls back to its existing `(&self)` stub. The BA sees no
//! error; the implementer sees a `// TODO` in the scaffolded code.
//!
//! ## How to add a new phrase
//!
//! 1. Pick the right sub-module (return shape, projection, rejection,
//!    multiplicity, query).
//! 2. Append the phrase to the `pub const …_PHRASES` slice (or the
//!    relevant marker / delimiter constant).
//! 3. Add at least one accept and one reject doc-test exercising the
//!    new phrase.
//! 4. Run `cargo test --doc` and the compile gate.
//!
//! Out of scope here: numeric / shape-based validation in
//! `src/test_synth.rs` (those are simulators, not phrase lists) and
//! anything in `codegen` that reads structural markers (those moved
//! into named constants in 7d, but they are codegen contracts not
//! phrase classifications).

pub mod multiplicity;
pub mod projection;
pub mod query;
pub mod rejection;
pub mod return_shape;

/// Classify the return type a projection query produces. Recognises the
/// "returns the …" / "are returned" / "all the …" wording that lupa-style
/// projections use; falls back to [`classify_return_shape`] for anything
/// else.
///
/// `element_type` is the projection's first role-player data type — the
/// canonical list element. Empty element types skip the `Vec<…>` wrap.
pub fn classify_projection_return(then: &str, element_type: &str) -> String {
    if projection::matches_collection(then) && !element_type.is_empty() {
        return format!("Vec<{}>", element_type);
    }
    classify_return_shape(then, element_type)
}

/// Classify the return type from the `Then:` clause of a scenario. The
/// allow-list:
///
///  * `the answer is affirmative|negative|yes|no` → `bool`
///  * `the result is "<...>"` → `String`
///  * `the result is a <PascalCase>` → that type
///  * rejection / acceptance / state-change phrases → `()`
///  * absence → `()`
pub fn classify_return_shape(then: &str, host_type: &str) -> String {
    if return_shape::matches_boolean(then) {
        return "bool".into();
    }

    let lower = then.to_ascii_lowercase();

    // Quoted text: `the result is "100.50 DKK"` → String
    if let Some(idx) = lower.find(return_shape::RESULT_ANCHOR) {
        let tail = &then[idx + return_shape::RESULT_ANCHOR.len()..];
        let trimmed = tail.trim_start();
        if trimmed.starts_with('"') || trimmed.starts_with('\'') {
            return "String".into();
        }
        // `the result is a <Type> of/with...`
        if let Some(after_a) = trimmed.strip_prefix("a ").or_else(|| trimmed.strip_prefix("an ")) {
            // First word should be a PascalCase type.
            let first = after_a.split_whitespace().next().unwrap_or("");
            if !first.is_empty() && first.chars().next().unwrap().is_ascii_uppercase() {
                return first.trim_end_matches([',', '.', ';']).to_string();
            }
        }
    }

    // Rejection / acceptance / state-change: leave as unit (callers can upgrade to Result).
    if return_shape::matches_state_change(then) {
        return "()".into();
    }

    // Default: unit.
    let _ = host_type;
    "()".into()
}

/// Recognise scenarios whose `Then:` describes an error / rejection path.
/// Used by signature inference to exclude rejection scenarios from the
/// happy-path aggregate so the derived return type isn't degraded to `()`.
pub fn is_rejection_then(then: &str) -> bool {
    rejection::matches(then)
}

/// Count case-insensitive occurrences of `needle` in `haystack`. Returns 0
/// for an empty needle.
pub fn count_phrase(haystack: &str, needle: &str) -> usize {
    let h = haystack.to_ascii_lowercase();
    let n = needle.to_ascii_lowercase();
    if n.is_empty() {
        return 0;
    }
    h.matches(&n).count()
}

/// Find the first `"…"` or `'…'` quoted substring in `text` that appears
/// after `marker` (case-insensitive marker match). Used by signature
/// inference to detect quoted parameters in `When:` clauses (e.g.
/// `transition_chat_state to "awaiting-input"`).
pub fn first_quoted_after(text: &str, marker: &str) -> Option<String> {
    let idx = text.to_ascii_lowercase().find(marker)?;
    let tail = &text[idx + marker.len()..];
    let mut chars = tail.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' || c == '\'' {
            let quote = c;
            let mut acc = String::new();
            for d in chars.by_ref() {
                if d == quote {
                    return Some(acc);
                }
                acc.push(d);
            }
        }
    }
    None
}
