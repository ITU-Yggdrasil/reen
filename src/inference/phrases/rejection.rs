//! Rejection-then phrases.
//!
//! Used by signature inference to exclude rejection scenarios from the
//! happy-path aggregate. Without this, a rejection scenario's `Then:`
//! (which has no useful return value) would force the derived signature
//! to `()` and the success-path's actual return type would be lost.
//!
//! See [`super::is_rejection_then`] for the consumer.

/// English idioms that mark a `Then:` clause as describing an error or
/// rejection path. Signature inference filters these out before
/// aggregating; the resulting signature reflects only the success path.
///
/// ```
/// use reen::inference::phrases::rejection::matches;
///
/// // Accepted shapes — every lupa rejection phrasing.
/// assert!(matches("the operation is rejected with a currency mismatch error"));
/// assert!(matches("the transition is rejected because \"idle\" to \"confirming\" is not valid"));
/// assert!(matches("the call returns an error explaining the constraint"));
/// assert!(matches("the operation fails with a currency mismatch detail"));
///
/// // Rejected: success-path scenarios.
/// assert!(!matches("the result is a Money of 7500 in DKK"));
/// assert!(!matches("the answer is affirmative"));
/// assert!(!matches("returns the three transactions"));
/// ```
pub fn matches(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    PHRASES.iter().any(|p| lower.contains(p))
}

/// Phrase registry behind [`matches`].
///
/// ```
/// use reen::inference::phrases::rejection::PHRASES;
/// // Accept: every registered idiom matches when present.
/// for phrase in PHRASES {
///     assert!(phrase.contains(phrase));
/// }
/// // Reject: success-path scenarios contain none of these phrases.
/// for then in [
///     "the answer is affirmative",
///     "the result is a Money of 100 in DKK",
///     "the chat state becomes \"idle\"",
/// ] {
///     assert!(!PHRASES.iter().any(|p| then.contains(p)),
///         "success-path scenario {then:?} accidentally matches a rejection phrase");
/// }
/// ```
pub const PHRASES: &[&str] = &[
    "is rejected",
    "returns an error",
    "with a currency mismatch",
    "transition is rejected",
];
