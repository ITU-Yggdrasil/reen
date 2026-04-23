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

// ── Tracker integration ──────────────────────────────────────────

/// The default minimum severity for an audit finding to flip a tracker entry
/// to stale. Findings below this threshold are reported but do not affect
/// the tracker.
pub const DEFAULT_STALE_THRESHOLD: AuditSeverity = AuditSeverity::High;

/// Apply audit findings to the build tracker, flipping entries at or above
/// `threshold` to `stale` with `reason: "audit_finding"` and the finding
/// text attached.
///
/// Returns the number of entries flipped.
pub fn apply_findings_to_tracker(
    report: &AuditReport,
    tracker: &mut crate::build_tracker::BuildTracker,
    artifact: &crate::prepared::PreparedArtifact,
    threshold: AuditSeverity,
) -> usize {
    let mut flipped = 0;

    for finding in &report.findings {
        if finding.severity < threshold {
            continue;
        }

        // Try to derive a canonical key from the finding
        let keys = keys_from_finding(finding, artifact);
        for key in keys {
            if tracker.artefacts.contains_key(&key) {
                tracker.mark_stale_audit(
                    &key,
                    format!("{}: {}", finding.kind.as_str(), finding.description),
                );
                flipped += 1;
            }
        }
    }

    flipped
}

/// Derive canonical artefact keys from an audit finding.
///
/// Uses the finding's code_citation (e.g. `data/snake.rs:12`) and kind
/// to determine which artefact(s) the finding applies to.
fn keys_from_finding(
    finding: &AuditFinding,
    artifact: &crate::prepared::PreparedArtifact,
) -> Vec<crate::build_tracker::keys::ArtefactKey> {
    use crate::build_tracker::keys::ArtefactKey;
    use crate::prepared::ArtifactKind;

    let export_name = &artifact.export.name;
    let mut keys = Vec::new();

    match finding.kind {
        AuditFindingKind::RoleMethodResponsibilityMissing => {
            // Try to extract the role method name from the description
            for group in &artifact.role_methods {
                for rm in &group.methods {
                    if finding.description.contains(&rm.canonical_name)
                        || finding.description.contains(&rm.name)
                    {
                        keys.push(ArtefactKey::role_method(export_name, &group.role, &rm.name));
                    }
                }
            }
        }
        AuditFindingKind::FunctionalityDivergentFlow => {
            for func in &artifact.functionalities {
                let func_snake = crate::codegen::to_snake_case(&func.name);
                if finding.description.contains(&func_snake)
                    || finding.description.contains(&func.name)
                {
                    keys.push(ArtefactKey::functionality(export_name, &func.name));
                }
            }
        }
        AuditFindingKind::InvariantNotEnforced => {
            // Invariant findings affect the data type and all its operations
            if artifact.kind == ArtifactKind::Data {
                for op in &artifact.operations {
                    if finding.description.contains(&op.name)
                        || finding.description.contains(&crate::codegen::to_snake_case(&op.name))
                    {
                        keys.push(ArtefactKey::data_operation(export_name, &op.name));
                    }
                }
            }
        }
        AuditFindingKind::CodeNotInSpec => {
            // CodeNotInSpec findings don't flip anything — they're informational
        }
    }

    keys
}

impl AuditFindingKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RoleMethodResponsibilityMissing => "role_method_responsibility_missing",
            Self::FunctionalityDivergentFlow => "functionality_divergent_flow",
            Self::InvariantNotEnforced => "invariant_not_enforced",
            Self::CodeNotInSpec => "code_not_in_spec",
        }
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

    // ── Tracker integration tests ───────────────────────────────

    #[test]
    fn apply_findings_flips_at_threshold() {
        use crate::build_tracker::*;
        use crate::build_tracker::keys::ArtefactKey;
        use crate::prepared::*;

        let mut tracker = BuildTracker::new();
        let key = ArtefactKey::role_method("Game", "snake", "move_forward");
        tracker.upsert_artefact(
            key.clone(),
            ArtefactEntry {
                kind: keys::ArtefactEntryKind::RoleMethod,
                prepared_hash: "hash".into(),
                upstream: vec![],
                state: ArtefactState::Fresh,
                reason: None,
                audit_finding: None,
            },
        );

        let mut artifact = PreparedArtifact::new(
            ArtifactKind::Context, "contexts/game.md".into(), "Game".into(), "Game".into(),
        );
        artifact.role_methods = vec![RoleMethodGroupSpec {
            role: "snake".into(),
            methods: vec![RoleMethodSpec {
                name: "move_forward".into(),
                canonical_name: "snake_move_forward".into(),
                signature_hint: None,
                citation: "moves the snake forward".into(),
            }],
        }];

        let report = AuditReport {
            findings: vec![AuditFinding {
                kind: AuditFindingKind::RoleMethodResponsibilityMissing,
                severity: AuditSeverity::High,
                spec_citation: "Game.md".into(),
                code_citation: "contexts/game.rs:10".into(),
                description: "snake_move_forward does not check bounds".into(),
            }],
        };

        let flipped = apply_findings_to_tracker(
            &report, &mut tracker, &artifact, AuditSeverity::High,
        );
        assert_eq!(flipped, 1);
        assert_eq!(tracker.artefacts[&key].state, ArtefactState::Stale);
        assert_eq!(tracker.artefacts[&key].reason, Some(StaleReason::AuditFinding));
        assert!(tracker.artefacts[&key].audit_finding.as_ref().unwrap().contains("does not check bounds"));
    }

    #[test]
    fn apply_findings_skips_below_threshold() {
        use crate::build_tracker::*;
        use crate::build_tracker::keys::ArtefactKey;
        use crate::prepared::*;

        let mut tracker = BuildTracker::new();
        let key = ArtefactKey::functionality("Game", "play");
        tracker.upsert_artefact(
            key.clone(),
            ArtefactEntry {
                kind: keys::ArtefactEntryKind::Functionality,
                prepared_hash: "hash".into(),
                upstream: vec![],
                state: ArtefactState::Fresh,
                reason: None,
                audit_finding: None,
            },
        );

        let mut artifact = PreparedArtifact::new(
            ArtifactKind::Context, "contexts/game.md".into(), "Game".into(), "Game".into(),
        );
        artifact.functionalities = vec![FunctionalitySpec {
            name: "play".into(),
            description: "Play the game.".into(),
            role_method_refs: vec![],
        }];

        // Medium severity finding with High threshold — should not flip
        let report = AuditReport {
            findings: vec![AuditFinding {
                kind: AuditFindingKind::FunctionalityDivergentFlow,
                severity: AuditSeverity::Medium,
                spec_citation: "Game.md".into(),
                code_citation: "contexts/game.rs:20".into(),
                description: "play has extra logging".into(),
            }],
        };

        let flipped = apply_findings_to_tracker(
            &report, &mut tracker, &artifact, AuditSeverity::High,
        );
        assert_eq!(flipped, 0);
        assert_eq!(tracker.artefacts[&key].state, ArtefactState::Fresh);
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
