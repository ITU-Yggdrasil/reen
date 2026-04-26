//! Result-phrase classifiers and small string helpers shared by the
//! signature inference pass. Every classifier here is an explicit
//! substring allow-list; see `super::mod` for the design rationale.

/// Classify the return type a projection query produces. Recognises the
/// "returns the …" / "are returned" / "all the …" wording that lupa-style
/// projections use; falls back to [`classify_return_shape`] for anything
/// else.
///
/// `element_type` is the projection's first role-player data type — the
/// canonical list element. Empty element types skip the `Vec<…>` wrap.
pub fn classify_projection_return(then: &str, element_type: &str) -> String {
    let lower = then.to_ascii_lowercase();
    if lower.contains("returns the ")
        || lower.contains("are returned")
        || lower.contains("only transactions")
        || lower.contains("all five")
        || lower.contains("all the")
    {
        if !element_type.is_empty() {
            return format!("Vec<{}>", element_type);
        }
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
    let lower = then.to_ascii_lowercase();

    // Boolean: "the answer is affirmative|negative|yes|no"
    if lower.contains("the answer is affirmative")
        || lower.contains("the answer is negative")
        || lower.contains("the answer is yes")
        || lower.contains("the answer is no")
    {
        return "bool".into();
    }

    // Quoted text: `the result is "100.50 DKK"` → String
    if let Some(idx) = lower.find("the result is") {
        let tail = &then[idx + "the result is".len()..];
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

    // Rejection / acceptance: leave as unit (callers can upgrade to Result).
    if lower.contains("is rejected")
        || lower.contains("is accepted")
        || lower.contains("becomes ")
    {
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
    let lower = then.to_ascii_lowercase();
    lower.contains("is rejected")
        || lower.contains("returns an error")
        || lower.contains("with a currency mismatch")
        || lower.contains("transition is rejected")
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
