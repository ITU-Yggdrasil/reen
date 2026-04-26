//! Phrase registry for return-type classification of `Then:` clauses.
//!
//! Used by [`super::classify_return_shape`] to map a scenario's `Then:`
//! prose to a Rust return-type fragment. Each constant lists the phrases
//! a category accepts; the classifier remains structural (it still does
//! the "the result is …" anchor + lookahead parsing) but its
//! substring-equality calls go through the helpers here.
//!
//! See [`super`] for the rationale behind explicit allow-lists vs regex.

/// English idioms that mark a `Then:` clause as expressing a boolean
/// answer. The classifier returns `bool` when any of these substrings
/// appear (case-insensitive).
///
/// ```
/// use reen::inference::phrases::return_shape::{matches_boolean, BOOLEAN_PHRASES};
///
/// // Accepted shapes — all four lupa Session.is_expired phrasings.
/// assert!(matches_boolean("the answer is affirmative"));
/// assert!(matches_boolean("the answer is negative"));
/// assert!(matches_boolean("the answer is yes"));
/// assert!(matches_boolean("the answer is no"));
/// // Case-insensitive.
/// assert!(matches_boolean("The Answer Is Affirmative"));
///
/// // Rejected: not an "is the answer …" shape.
/// assert!(!matches_boolean("the result is \"100.50 DKK\""));
/// assert!(!matches_boolean("the operation succeeds"));
///
/// // Constant is non-empty so callers can rely on it as the audit trail.
/// assert!(!BOOLEAN_PHRASES.is_empty());
/// ```
pub const BOOLEAN_PHRASES: &[&str] = &[
    "the answer is affirmative",
    "the answer is negative",
    "the answer is yes",
    "the answer is no",
];

/// True when `text` contains any [`BOOLEAN_PHRASES`] entry, case-insensitive.
pub fn matches_boolean(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    BOOLEAN_PHRASES.iter().any(|p| lower.contains(p))
}

/// Anchor for "the result is …" sentences. The classifier finds this
/// substring and then inspects the next character: a quote (`"` or `'`)
/// means a `String` return; `a <PascalCase>` means that data type.
///
/// ```
/// use reen::inference::phrases::return_shape::RESULT_ANCHOR;
///
/// // Accepted: every "the result is …" shape from lupa Money operations.
/// assert!("the result is \"100.50 DKK\"".to_ascii_lowercase().contains(RESULT_ANCHOR));
/// assert!("the result is a Money of 7500 in DKK".to_ascii_lowercase().contains(RESULT_ANCHOR));
///
/// // Rejected: scenarios that don't open with the result phrase.
/// assert!(!"the answer is affirmative".contains(RESULT_ANCHOR));
/// assert!(!"the chat state becomes \"awaiting-input\"".contains(RESULT_ANCHOR));
/// ```
pub const RESULT_ANCHOR: &str = "the result is";

/// English idioms that signify the operation observably mutated state or
/// completed-without-return. The classifier returns `()` when it sees one
/// of these (a future change can upgrade the return to `Result<(), E>`).
///
/// ```
/// use reen::inference::phrases::return_shape::matches_state_change;
///
/// // Accepted shapes — common "no useful return value" patterns.
/// assert!(matches_state_change("the operation is rejected"));
/// assert!(matches_state_change("the Money is accepted"));
/// assert!(matches_state_change("the chat state becomes \"awaiting-input\""));
///
/// // Rejected: scenarios that genuinely return data.
/// assert!(!matches_state_change("the answer is affirmative"));
/// assert!(!matches_state_change("returns the three transactions"));
/// ```
pub fn matches_state_change(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    STATE_CHANGE_PHRASES.iter().any(|p| lower.contains(p))
}

/// State-change / acceptance / rejection phrases that map to the unit
/// return. The `becomes ` entry has a trailing space so it doesn't match
/// inside other words.
///
/// ```
/// use reen::inference::phrases::return_shape::STATE_CHANGE_PHRASES;
/// // Accept: each entry survives a substring check.
/// for phrase in STATE_CHANGE_PHRASES {
///     assert!(format!("the operation {} successfully", phrase)
///         .contains(phrase));
/// }
/// // Reject: a non-state-change return phrasing.
/// assert!(!STATE_CHANGE_PHRASES.iter().any(|p| "returns the three transactions".contains(p)));
/// ```
pub const STATE_CHANGE_PHRASES: &[&str] = &[
    "is rejected",
    "is accepted",
    "becomes ",
];
