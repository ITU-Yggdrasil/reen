//! Multiplicity inference phrase registry.
//!
//! Three categories live here:
//!
//! - [`IRREGULAR_PLURALS`] — irregular English plural→singular pairs.
//!   Used by both `name_plurality` (recognise plural names) and
//!   `depluralise` (singularise role-player names for substring search).
//! - [`COUNT_WORDS`] — count words that mark "multiple instances" in a
//!   scenario `Given:` (`five transactions`, `three accounts`).
//! - [`PROSE_PHRASES`] — explicit "this is a collection" idioms in
//!   description prose.
//!
//! See [`super::super::multiplicity`] for the algorithms that consume
//! these registries.

/// Irregular English plural → singular pairs. The plural side feeds
/// `name_plurality` (presence of the plural form is a strong list cue);
/// the singular side feeds `depluralise` for scenario substring search
/// (`a child`, `the person`).
///
/// ```
/// use reen::inference::phrases::multiplicity::IRREGULAR_PLURALS;
///
/// // Accepted: presence in the table catches both directions.
/// assert!(IRREGULAR_PLURALS.iter().any(|(p, _)| *p == "children"));
/// assert!(IRREGULAR_PLURALS.iter().any(|(_, s)| *s == "person"));
///
/// // Rejected: regular plurals (covered by the `s`/`es`/`ies` rule
/// // in `name_plurality`, not by this table).
/// assert!(!IRREGULAR_PLURALS.iter().any(|(p, _)| *p == "transactions"));
/// assert!(!IRREGULAR_PLURALS.iter().any(|(p, _)| *p == "accounts"));
/// ```
pub const IRREGULAR_PLURALS: &[(&str, &str)] = &[
    ("children", "child"),
    ("people", "person"),
    ("men", "man"),
    ("women", "woman"),
    ("feet", "foot"),
    ("teeth", "tooth"),
    ("mice", "mouse"),
    ("geese", "goose"),
];

/// Count words that signal "multiple instances of X" in a scenario
/// `Given:`. Matched as `<word> <role-name>` (e.g. `three accounts`).
///
/// ```
/// use reen::inference::phrases::multiplicity::COUNT_WORDS;
///
/// // Accepted: every count word produces a list cue.
/// assert!(COUNT_WORDS.contains(&"three"));
/// assert!(COUNT_WORDS.contains(&"many"));
/// assert!(COUNT_WORDS.contains(&"several"));
///
/// // Rejected: numeric digits are not in the list (the parser handles
/// // them via a separate regex-free path elsewhere) and "one" is not
/// // a multiplicity cue — singular is the default.
/// assert!(!COUNT_WORDS.contains(&"one"));
/// assert!(!COUNT_WORDS.contains(&"3"));
/// ```
pub const COUNT_WORDS: &[&str] = &[
    "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
    "eleven", "twelve", "many", "several",
];

/// Description-prose phrases that explicitly assert a role-player or
/// attribute is a collection. The cue is weaker than name plurality
/// or scenario shape but still informs the aggregate when both stronger
/// cues are silent.
///
/// ```
/// use reen::inference::phrases::multiplicity::matches_prose_collection;
///
/// // Accepted shapes — common BA "this is a list" idioms.
/// assert!(matches_prose_collection("a list of consent scope identifiers"));
/// assert!(matches_prose_collection("the list of recent turns"));
/// assert!(matches_prose_collection("the ordered transactions"));
/// assert!(matches_prose_collection("the collection of audit entries"));
/// assert!(matches_prose_collection("all the matching accounts"));
///
/// // Rejected: singular-leaning prose.
/// assert!(!matches_prose_collection("the active session"));
/// assert!(!matches_prose_collection("a Money value in the host currency"));
/// ```
pub fn matches_prose_collection(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    PROSE_PHRASES.iter().any(|p| lower.contains(p))
}

/// Phrase registry behind [`matches_prose_collection`].
///
/// ```
/// use reen::inference::phrases::multiplicity::PROSE_PHRASES;
/// // Accept: every registered phrase matches when present.
/// for phrase in PROSE_PHRASES {
///     assert!(format!("{} entries are returned", phrase).contains(phrase));
/// }
/// // Reject: singular phrasing carries none of these.
/// assert!(!PROSE_PHRASES.iter().any(|p| "the active session".contains(p)));
/// ```
pub const PROSE_PHRASES: &[&str] = &[
    "a list of",
    "the list of",
    "the ordered ",
    "the collection of",
    "all the ",
];
