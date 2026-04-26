//! Projection-query parameter extraction.
//!
//! Parses the `When:` clause of a projection-query scenario and produces
//! a list of [`ParamSpec`] entries. The allow-list of recognised shapes:
//!
//!   * `<query> is queried with no <X>` → no parameters
//!   * `<query> is queried with <name1> <num1> and <name2> <num2>`
//!     → two `i64` parameters named `<name1>`, `<name2>`
//!   * `<query> is queried with "<text>"` → one `&str` parameter named
//!     `query`
//!   * `<query> is queried with <name> "<text>"` → one `&str` parameter
//!     named `<name>`
//!
//! Any other shape produces no parameters; the codegen path emits a
//! `(&self)` stub and the implementer fills in the right signature
//! later.

use crate::prepared::ParamSpec;

/// `queried with minimum 100 and maximum 250` → two i64 params.
/// `queried with "alice"` → one &str param named `query`.
/// `queried with no date range` → no params.
/// `queried with a range covering the last seven days` → no params (left to /implement).
pub fn parse_query_parameters(when: &str) -> Vec<ParamSpec> {
    let when_lower = when.to_ascii_lowercase();
    let mut params: Vec<ParamSpec> = Vec::new();

    // "queried with no <X>" → nothing
    if when_lower.contains("with no ") {
        return params;
    }

    let Some(rest) = when_lower.split_once(" with ").map(|(_, b)| b) else {
        return params;
    };

    let mut emitted_query = false;
    for chunk in rest.split(" and ") {
        let chunk = chunk.trim();
        if chunk.is_empty() {
            continue;
        }

        // Pattern A: a single quoted token like `"alice"` — treat as one
        // `query: &str` parameter regardless of how many such scenarios the
        // projection has (a query is a query, named or not).
        if is_single_quoted_token(chunk) {
            if !emitted_query {
                params.push(ParamSpec {
                    name: "query".into(),
                    rust_type: "&str".into(),
                });
                emitted_query = true;
            }
            continue;
        }

        // Pattern B: `<name> <value>` where value is numeric or quoted.
        let mut iter = chunk.split_whitespace();
        let (Some(name), Some(value)) = (iter.next(), iter.next()) else {
            continue;
        };
        if value.parse::<i64>().is_ok() {
            params.push(ParamSpec {
                name: name.trim_end_matches(',').to_string(),
                rust_type: "i64".into(),
            });
            continue;
        }
        // `<name> "value"` → string param keyed by name.
        if value.starts_with('"') && chunk.contains('"') {
            params.push(ParamSpec {
                name: name.trim_end_matches(',').to_string(),
                rust_type: "&str".into(),
            });
            continue;
        }
    }

    params
}

/// Recognise a chunk that consists of a single double-quoted token (allowing
/// trailing punctuation). Used by `parse_query_parameters` to distinguish
/// `"alice"` (one positional `query` arg) from `name "alice"` (named arg).
pub fn is_single_quoted_token(chunk: &str) -> bool {
    let trimmed = chunk.trim().trim_end_matches([',', '.']);
    trimmed.len() >= 2
        && trimmed.starts_with('"')
        && trimmed.ends_with('"')
        && trimmed[1..trimmed.len() - 1].find('"').is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_query_parameters_single_quoted_token_yields_query_str() {
        // Bug 2.5b regression: `queried with "alice"` must produce one
        // `query: &str` parameter, not zero.
        let params = parse_query_parameters("by_counterparty is queried with \"alice\"");
        assert_eq!(params.len(), 1, "expected one param, got {:?}", params);
        assert_eq!(params[0].name, "query");
        assert_eq!(params[0].rust_type, "&str");
    }

    #[test]
    fn parse_query_parameters_named_value_unchanged() {
        let params = parse_query_parameters(
            "by_amount_range is queried with minimum 100 and maximum 250",
        );
        assert_eq!(params.len(), 2, "expected two params, got {:?}", params);
        assert_eq!(params[0].name, "minimum");
        assert_eq!(params[0].rust_type, "i64");
        assert_eq!(params[1].name, "maximum");
        assert_eq!(params[1].rust_type, "i64");
    }

    #[test]
    fn parse_query_parameters_no_filter_unchanged() {
        let params = parse_query_parameters("filtered is queried with no date range");
        assert!(
            params.is_empty(),
            "no-filter scenarios produce no params, got {:?}", params,
        );
    }
}
