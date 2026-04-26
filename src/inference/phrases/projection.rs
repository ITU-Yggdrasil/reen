//! Projection-collection-return phrases.
//!
//! Used by [`super::classify_projection_return`] to recognise scenarios
//! whose `Then:` describes a collection of results (`returns the three
//! transactions …`, `all five entries are returned`, …). When any phrase
//! matches AND the projection has at least one role-player element type,
//! the inferred return is `Vec<<element>>`.

/// English idioms that mark a `Then:` clause as returning a collection
/// rather than a single value. The classifier checks these BEFORE
/// falling through to [`super::classify_return_shape`], so a phrase
/// here short-circuits the unit / boolean / String paths.
///
/// ```
/// use reen::inference::phrases::projection::matches_collection;
///
/// // Accepted shapes — drawn from lupa TransactionList and similar projections.
/// assert!(matches_collection("returns the three transactions with amounts 100, 200, 250"));
/// assert!(matches_collection("all the matching entries are returned"));
/// assert!(matches_collection("all five transactions are returned"));
/// assert!(matches_collection("only transactions in the bound are returned"));
///
/// // Rejected: scalar / unit / boolean returns.
/// assert!(!matches_collection("the answer is affirmative"));
/// assert!(!matches_collection("the result is \"100.50 DKK\""));
/// assert!(!matches_collection("the chat state becomes \"awaiting-input\""));
/// ```
pub fn matches_collection(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    COLLECTION_RETURN_PHRASES.iter().any(|p| lower.contains(p))
}

/// Collection-return phrase list — the registry behind
/// [`matches_collection`].
///
/// ```
/// use reen::inference::phrases::projection::COLLECTION_RETURN_PHRASES;
/// // Accept: every entry matches the canonical lupa phrasing.
/// for phrase in COLLECTION_RETURN_PHRASES {
///     let probe = format!("test text containing {} somewhere", phrase);
///     assert!(probe.to_ascii_lowercase().contains(*phrase));
/// }
/// // Reject: scalar-return phrasing.
/// assert!(!COLLECTION_RETURN_PHRASES.iter().any(|p| "the answer is affirmative".contains(p)));
/// ```
pub const COLLECTION_RETURN_PHRASES: &[&str] = &[
    "returns the ",
    "are returned",
    "only transactions",
    "all five",
    "all the",
];
