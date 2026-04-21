//! Auditor output parsing.
//!
//! The reen-auditor agent produces structured markdown findings.
//! This module parses that output into typed [`AuditFinding`] values
//! for programmatic consumption and test assertions.

/// A single discrepancy reported by the auditor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditFinding {
    pub kind: AuditFindingKind,
    pub severity: AuditSeverity,
    pub spec_citation: String,
    pub code_citation: String,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditFindingKind {
    RoleMethodResponsibilityMissing,
    FunctionalityDivergentFlow,
    InvariantNotEnforced,
    CodeNotInSpec,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AuditSeverity {
    Low,
    Medium,
    High,
}

/// The result of parsing auditor output.
#[derive(Debug, Clone)]
pub struct AuditReport {
    pub findings: Vec<AuditFinding>,
}

impl AuditReport {
    pub fn has_finding_of_kind(&self, kind: AuditFindingKind) -> bool {
        self.findings.iter().any(|f| f.kind == kind)
    }

    pub fn high_severity_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == AuditSeverity::High)
            .count()
    }
}

/// Parse the structured markdown output from the reen-auditor agent.
///
/// The expected format is a sequence of `### Finding N` blocks, each containing
/// key-value lines prefixed with `- **key**:`.
pub fn parse_audit_output(markdown: &str) -> AuditReport {
    if markdown.contains("No discrepancies found") {
        return AuditReport {
            findings: Vec::new(),
        };
    }

    let mut findings = Vec::new();
    let mut current: Option<FindingBuilder> = None;

    for line in markdown.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("### Finding") {
            if let Some(builder) = current.take() {
                if let Some(finding) = builder.build() {
                    findings.push(finding);
                }
            }
            current = Some(FindingBuilder::default());
            continue;
        }

        if let Some(ref mut builder) = current {
            if let Some(rest) = trimmed.strip_prefix("- **kind**:") {
                builder.kind = Some(rest.trim().to_string());
            } else if let Some(rest) = trimmed.strip_prefix("- **severity**:") {
                builder.severity = Some(rest.trim().to_string());
            } else if let Some(rest) = trimmed.strip_prefix("- **spec_citation**:") {
                builder.spec_citation = Some(rest.trim().to_string());
            } else if let Some(rest) = trimmed.strip_prefix("- **code_citation**:") {
                builder.code_citation = Some(rest.trim().to_string());
            } else if let Some(rest) = trimmed.strip_prefix("- **description**:") {
                builder.description = Some(rest.trim().to_string());
            }
        }
    }

    if let Some(builder) = current.take() {
        if let Some(finding) = builder.build() {
            findings.push(finding);
        }
    }

    AuditReport { findings }
}

#[derive(Default)]
struct FindingBuilder {
    kind: Option<String>,
    severity: Option<String>,
    spec_citation: Option<String>,
    code_citation: Option<String>,
    description: Option<String>,
}

impl FindingBuilder {
    fn build(self) -> Option<AuditFinding> {
        Some(AuditFinding {
            kind: parse_kind(self.kind.as_deref()?)?,
            severity: parse_severity(self.severity.as_deref()?)?,
            spec_citation: self.spec_citation?,
            code_citation: self.code_citation?,
            description: self.description?,
        })
    }
}

fn parse_kind(s: &str) -> Option<AuditFindingKind> {
    match s {
        "role_method_responsibility_missing" => Some(AuditFindingKind::RoleMethodResponsibilityMissing),
        "functionality_divergent_flow" => Some(AuditFindingKind::FunctionalityDivergentFlow),
        "invariant_not_enforced" => Some(AuditFindingKind::InvariantNotEnforced),
        "code_not_in_spec" => Some(AuditFindingKind::CodeNotInSpec),
        _ => None,
    }
}

fn parse_severity(s: &str) -> Option<AuditSeverity> {
    match s {
        "low" => Some(AuditSeverity::Low),
        "medium" => Some(AuditSeverity::Medium),
        "high" => Some(AuditSeverity::High),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_no_discrepancies() {
        let output = "No discrepancies found. The implementation matches the DCI specification.";
        let report = parse_audit_output(output);
        assert!(report.findings.is_empty());
    }

    #[test]
    fn parse_single_finding() {
        let output = r#"
### Finding 1

- **kind**: invariant_not_enforced
- **severity**: high
- **spec_citation**: Snake.md, Invariants / "All positions in the body are unique"
- **code_citation**: data/snake.rs:12
- **description**: The uniqueness check on body positions is missing from Snake::new
"#;
        let report = parse_audit_output(output);
        assert_eq!(report.findings.len(), 1);
        let f = &report.findings[0];
        assert_eq!(f.kind, AuditFindingKind::InvariantNotEnforced);
        assert_eq!(f.severity, AuditSeverity::High);
        assert!(f.spec_citation.contains("Snake.md"));
        assert!(f.code_citation.contains("snake.rs"));
        assert!(f.description.contains("uniqueness"));
    }

    #[test]
    fn parse_multiple_findings() {
        let output = r#"
### Finding 1

- **kind**: invariant_not_enforced
- **severity**: high
- **spec_citation**: Snake.md, Invariants
- **code_citation**: data/snake.rs:12
- **description**: Missing uniqueness check

### Finding 2

- **kind**: code_not_in_spec
- **severity**: low
- **spec_citation**: (none)
- **code_citation**: data/snake.rs:55
- **description**: Extra helper method display_body has no spec basis
"#;
        let report = parse_audit_output(output);
        assert_eq!(report.findings.len(), 2);
        assert_eq!(report.high_severity_count(), 1);
        assert!(report.has_finding_of_kind(AuditFindingKind::InvariantNotEnforced));
        assert!(report.has_finding_of_kind(AuditFindingKind::CodeNotInSpec));
    }

    #[test]
    fn parse_all_finding_kinds() {
        let output = r#"
### Finding 1
- **kind**: role_method_responsibility_missing
- **severity**: medium
- **spec_citation**: spec
- **code_citation**: code:1
- **description**: desc1

### Finding 2
- **kind**: functionality_divergent_flow
- **severity**: high
- **spec_citation**: spec
- **code_citation**: code:2
- **description**: desc2

### Finding 3
- **kind**: invariant_not_enforced
- **severity**: high
- **spec_citation**: spec
- **code_citation**: code:3
- **description**: desc3

### Finding 4
- **kind**: code_not_in_spec
- **severity**: low
- **spec_citation**: spec
- **code_citation**: code:4
- **description**: desc4
"#;
        let report = parse_audit_output(output);
        assert_eq!(report.findings.len(), 4);
        assert_eq!(report.findings[0].kind, AuditFindingKind::RoleMethodResponsibilityMissing);
        assert_eq!(report.findings[1].kind, AuditFindingKind::FunctionalityDivergentFlow);
        assert_eq!(report.findings[2].kind, AuditFindingKind::InvariantNotEnforced);
        assert_eq!(report.findings[3].kind, AuditFindingKind::CodeNotInSpec);
    }
}
