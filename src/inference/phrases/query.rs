//! Query-parameter chunk-shape registry.
//!
//! `parse_query_parameters` tokenises a projection-query `When:` into
//! chunks separated by [`AND_DELIMITER`], anchored on [`WITH_MARKER`],
//! and short-circuits on [`NO_FILTER_PREFIX`]. The constants here pin
//! those structural anchors so a future change to the recognised
//! phrasing forces a deliberate edit (and trips the doc-tests).

/// The opening anchor of a query-parameter list. Everything after
/// the first occurrence of this substring is the chunked argument
/// list. Note the leading and trailing spaces — bare `with` matches
/// inside other words (`within`, `withdraw`).
///
/// ```
/// use reen::inference::phrases::query::WITH_MARKER;
///
/// // Accepted: standard "queried with …" form.
/// assert!("by_amount_range is queried with minimum 100".contains(WITH_MARKER));
/// assert!("by_counterparty is queried with \"alice\"".contains(WITH_MARKER));
///
/// // Rejected: When clauses without a parameter list.
/// assert!(!"by_counterparty is performed".contains(WITH_MARKER));
/// // Rejected: bare `with` inside another word doesn't match.
/// assert!(!"the user can withdraw funds".contains(WITH_MARKER));
/// ```
pub const WITH_MARKER: &str = " with ";

/// Short-circuit prefix for "no parameter" queries. When this appears
/// the parser returns an empty parameter list without further
/// tokenisation.
///
/// ```
/// use reen::inference::phrases::query::NO_FILTER_PREFIX;
///
/// // Accepted: lupa filtered's no-filter scenario.
/// assert!("filtered is queried with no date range".contains(NO_FILTER_PREFIX));
/// assert!("queried with no bounds".contains(NO_FILTER_PREFIX));
///
/// // Rejected: When clauses that DO carry parameters.
/// assert!(!"by_amount_range is queried with minimum 100".contains(NO_FILTER_PREFIX));
/// ```
pub const NO_FILTER_PREFIX: &str = "with no ";

/// Sub-chunk delimiter inside the parameter list. The parser splits
/// on this and treats each chunk as one named-value pair (or a single
/// quoted token for the positional `query: &str` shape).
///
/// ```
/// use reen::inference::phrases::query::AND_DELIMITER;
///
/// // Accepted: standard two-parameter shape.
/// let parts: Vec<&str> = "minimum 100 and maximum 250".split(AND_DELIMITER).collect();
/// assert_eq!(parts, vec!["minimum 100", "maximum 250"]);
///
/// // Rejected: the delimiter requires the literal " and " — bare
/// // `and` mid-word doesn't accidentally split.
/// let parts: Vec<&str> = "android version 7".split(AND_DELIMITER).collect();
/// assert_eq!(parts, vec!["android version 7"]);
/// ```
pub const AND_DELIMITER: &str = " and ";
