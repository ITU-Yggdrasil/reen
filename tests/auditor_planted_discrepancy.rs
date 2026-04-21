//! Integration tests for the auditor output parser using recorded fixtures.
//!
//! The auditor is an LLM-backed agent. In CI, we exercise the post-processing
//! path using canned responses rather than live API calls. Each fixture
//! represents a plausible auditor response for a known scenario.

use reen::audit::{parse_audit_output, AuditFindingKind, AuditSeverity};
use std::fs;

fn fixture(name: &str) -> String {
    let path = format!("tests/fixtures/audit/{}.md", name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {}: {}", path, e))
}

// ── Clean code produces zero findings ────────────────────────────────

#[test]
fn clean_snake_data_produces_no_findings() {
    let output = fixture("snake_data_snake_clean");
    let report = parse_audit_output(&output);
    assert!(
        report.findings.is_empty(),
        "known-green Snake data should produce zero findings, got: {:?}",
        report.findings
    );
    assert_eq!(report.high_severity_count(), 0);
}

// ── Planted discrepancy is caught ────────────────────────────────────

#[test]
fn planted_missing_uniqueness_check_is_caught() {
    // Scenario: the Snake::new constructor has its uniqueness check removed.
    // The auditor should flag this as invariant_not_enforced with high severity.
    let output = fixture("snake_data_snake_missing_uniqueness");
    let report = parse_audit_output(&output);

    assert!(
        !report.findings.is_empty(),
        "planted discrepancy should produce at least one finding"
    );

    // The specific invariant finding
    let invariant_finding = report
        .findings
        .iter()
        .find(|f| f.kind == AuditFindingKind::InvariantNotEnforced)
        .expect("should have an invariant_not_enforced finding");

    assert_eq!(
        invariant_finding.severity,
        AuditSeverity::High,
        "missing invariant enforcement should be high severity"
    );
    assert!(
        invariant_finding.spec_citation.contains("Snake.md"),
        "spec_citation should reference the Snake draft, got: {}",
        invariant_finding.spec_citation
    );
    assert!(
        invariant_finding.description.contains("unique")
            || invariant_finding.description.contains("overlap"),
        "description should mention the uniqueness invariant, got: {}",
        invariant_finding.description
    );
    assert!(
        invariant_finding.code_citation.contains("snake.rs"),
        "code_citation should reference the snake source file, got: {}",
        invariant_finding.code_citation
    );
}

// ── Report accessors work correctly ──────────────────────────────────

#[test]
fn report_high_severity_count_matches() {
    let output = fixture("snake_data_snake_missing_uniqueness");
    let report = parse_audit_output(&output);
    assert_eq!(report.high_severity_count(), 1);
}

#[test]
fn report_has_finding_of_kind_works() {
    let output = fixture("snake_data_snake_missing_uniqueness");
    let report = parse_audit_output(&output);
    assert!(report.has_finding_of_kind(AuditFindingKind::InvariantNotEnforced));
    assert!(!report.has_finding_of_kind(AuditFindingKind::FunctionalityDivergentFlow));
}
