//! Audit-loop persistence: durable findings, AUDIT.md rendering, and tracker
//! stale-flips with finding text.
//!
//! The `/audit` slash command invokes the reen-auditor agent, captures its
//! findings as structured JSON, then calls `reen audit record` to:
//!
//!   1. Canonicalise findings into `.reen/audit.json`.
//!   2. Render `AUDIT.md` in the project root from the canonical JSON.
//!   3. For findings at or above a severity threshold (default `medium`),
//!      flip the matching tracker entry to `stale` with a `finding` field.
//!
//! Re-running the loop overwrites `audit.json` and `AUDIT.md` cleanly. Tracker
//! flips are idempotent — re-flipping an already-stale entry with byte-identical
//! finding text is a no-op.
//!
//! A finding that cannot be matched to a tracker entry is still persisted to
//! `audit.json` and rendered in `AUDIT.md`; only the tracker flip is skipped.

use crate::build_tracker::keys::ArtefactKey;
use crate::build_tracker::{ArtefactState, BuildTracker, StaleReason};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// Severity scale for both findings and the tracker-flip threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Low,
    Medium,
    High,
}

impl Severity {
    pub fn parse(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            other => Err(anyhow::anyhow!(
                "unrecognised severity `{}` — expected low, medium, or high",
                other
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

/// One audit finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// `context`, `data`, `projection`, `component`, `brand`, `routing_table`, etc.
    pub artefact_kind: String,
    /// The export name of the artefact (snake_case for contexts/projections,
    /// PascalCase for data and component).
    pub artefact_name: String,
    /// Repository-relative path to the Rust file the finding applies to.
    pub affected_path: String,
    /// Method name in the impl block. Empty when the finding applies at
    /// artefact level (e.g. struct field discrepancy).
    #[serde(default)]
    pub affected_method: String,
    /// Auditor's existing finding categorisation
    /// (`invariant_not_enforced`, `role_method_responsibility_missing`,
    /// `functionality_divergent_flow`, `code_not_in_spec`,
    /// `context_could_be_projection`, …).
    pub kind: String,
    pub severity: Severity,
    pub detail: String,
}

/// One open spec question. These do not flip tracker entries — they prompt the
/// BA to address them in the next `/refine` session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenSpecQuestion {
    pub topic: String,
    /// Repository-relative draft paths the question affects.
    #[serde(default)]
    pub drafts: Vec<String>,
    pub detail: String,
}

/// Top-level audit report — the schema serialized as `.reen/audit.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditReport {
    /// Schema version. Additive — older readers ignore unknown fields and
    /// version mismatches that only add fields.
    #[serde(default = "default_version")]
    pub version: u32,
    /// ISO-8601 timestamp of when the report was produced. Optional so the
    /// auditor agent does not need to know the time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub produced_at: Option<String>,
    #[serde(default)]
    pub findings: Vec<Finding>,
    #[serde(default)]
    pub open_spec_questions: Vec<OpenSpecQuestion>,
}

fn default_version() -> u32 {
    1
}

impl AuditReport {
    pub fn from_json(s: &str) -> Result<Self> {
        let mut report: Self = serde_json::from_str(s)
            .with_context(|| "failed to parse audit findings JSON")?;
        if report.version == 0 {
            report.version = default_version();
        }
        Ok(report)
    }
}

/// Outcome of `record` — what the slash command can show to the user.
#[derive(Debug, Clone)]
pub struct RecordOutcome {
    pub findings_count: usize,
    pub open_questions_count: usize,
    pub flipped_count: usize,
    pub idempotent_skipped_count: usize,
    pub unmatched_count: usize,
    pub audit_json_path: String,
    pub audit_md_path: String,
}

/// Apply an audit report durably: write audit.json, write AUDIT.md, flip
/// tracker entries above the severity threshold.
pub fn record(
    report: &AuditReport,
    workspace_root: &Path,
    state_dir: &Path,
    threshold: Severity,
) -> Result<RecordOutcome> {
    fs::create_dir_all(state_dir)
        .with_context(|| format!("failed to create {}", state_dir.display()))?;

    let audit_json_path = state_dir.join("audit.json");
    let audit_md_path = workspace_root.join("AUDIT.md");

    // Canonical JSON
    let canonical = serde_json::to_string_pretty(report)
        .context("failed to serialise audit findings as JSON")?;
    write_atomic(&audit_json_path, canonical.as_bytes())?;

    // Rendered AUDIT.md
    let rendered = render_markdown(report);
    write_atomic(&audit_md_path, rendered.as_bytes())?;

    // Tracker flips
    let tracker_path = state_dir.join("build.json");
    let mut flipped = 0usize;
    let mut idempotent_skipped = 0usize;
    let mut unmatched = 0usize;

    if tracker_path.is_file() {
        let mut tracker = BuildTracker::load(state_dir)
            .with_context(|| format!("failed to load tracker from {}", state_dir.display()))?;
        let mut changed = false;
        for finding in &report.findings {
            if finding.severity < threshold {
                continue;
            }
            let Some(key) = match_finding_to_key(finding, &tracker) else {
                unmatched += 1;
                continue;
            };
            let Some(entry) = tracker.artefacts.get_mut(&key) else {
                unmatched += 1;
                continue;
            };
            let new_text = render_finding_text(finding);
            if entry.state == ArtefactState::Stale && entry.audit_finding.as_deref() == Some(new_text.as_str()) {
                idempotent_skipped += 1;
                continue;
            }
            entry.state = ArtefactState::Stale;
            entry.reason = Some(StaleReason::AuditFinding);
            entry.audit_finding = Some(new_text);
            flipped += 1;
            changed = true;
        }
        if changed {
            tracker
                .save(state_dir)
                .with_context(|| format!("failed to save tracker to {}", state_dir.display()))?;
        }
    } else {
        // No tracker — every finding is unmatched.
        unmatched = report
            .findings
            .iter()
            .filter(|f| f.severity >= threshold)
            .count();
    }

    Ok(RecordOutcome {
        findings_count: report.findings.len(),
        open_questions_count: report.open_spec_questions.len(),
        flipped_count: flipped,
        idempotent_skipped_count: idempotent_skipped,
        unmatched_count: unmatched,
        audit_json_path: audit_json_path.display().to_string(),
        audit_md_path: audit_md_path.display().to_string(),
    })
}

/// Best-effort match from a finding to a tracker key.
///
/// Strategy: derive candidate keys from `(artefact_kind, artefact_name,
/// affected_method)` and look them up in the tracker. The tracker's path
/// strings are derivable from these tuples without disambiguation.
fn match_finding_to_key(finding: &Finding, tracker: &BuildTracker) -> Option<ArtefactKey> {
    let kind = finding.artefact_kind.to_ascii_lowercase();
    let method = finding.affected_method.trim();
    let name = finding.artefact_name.trim();
    if name.is_empty() {
        return None;
    }
    let candidates: Vec<ArtefactKey> = match kind.as_str() {
        "context" => {
            if method.is_empty() {
                Vec::new()
            } else {
                let mut out = Vec::new();
                out.push(ArtefactKey::functionality(name, method));
                // Try every role/method combination the tracker knows.
                for key in tracker.artefacts.keys() {
                    if let ArtefactKey::RoleMethod {
                        context, role, method: m,
                    } = key
                    {
                        if context == &crate::codegen::to_snake_case(name) && m == &crate::codegen::to_snake_case(method) {
                            out.push(ArtefactKey::role_method(name, role, method));
                        }
                    }
                }
                out
            }
        }
        "data" => {
            if method.is_empty() {
                vec![ArtefactKey::data_type(name)]
            } else if method == "new" {
                vec![ArtefactKey::data_constructor(name)]
            } else {
                vec![
                    ArtefactKey::data_operation(name, method),
                    ArtefactKey::data_getter(name, method),
                ]
            }
        }
        "projection" => {
            if method.is_empty() {
                Vec::new()
            } else {
                let mut out = Vec::new();
                out.push(ArtefactKey::projection_functionality(name, method));
                for key in tracker.artefacts.keys() {
                    if let ArtefactKey::ProjectionRoleMethod {
                        projection, role, method: m,
                    } = key
                    {
                        if projection == &crate::codegen::to_snake_case(name)
                            && m == &crate::codegen::to_snake_case(method)
                        {
                            out.push(ArtefactKey::projection_role_method(name, role, method));
                        }
                    }
                }
                out
            }
        }
        _ => Vec::new(),
    };
    candidates.into_iter().find(|k| tracker.artefacts.contains_key(k))
}

fn render_finding_text(finding: &Finding) -> String {
    format!(
        "[{}/{}] {}",
        finding.severity.as_str(),
        finding.kind,
        finding.detail.trim()
    )
}

/// Render the AUDIT.md document. Stable, deterministic format.
fn render_markdown(report: &AuditReport) -> String {
    let mut out = String::new();
    out.push_str("# Audit report\n\n");
    if let Some(ts) = &report.produced_at {
        out.push_str(&format!("Produced: {}\n\n", ts));
    }

    if report.findings.is_empty() {
        out.push_str("No discrepancies found. The implementation matches the DCI specification.\n");
    } else {
        out.push_str(&format!(
            "{} finding(s).\n\n",
            report.findings.len()
        ));

        // Group findings by severity, then by artefact.
        for level in [Severity::High, Severity::Medium, Severity::Low] {
            let group: Vec<&Finding> = report
                .findings
                .iter()
                .filter(|f| f.severity == level)
                .collect();
            if group.is_empty() {
                continue;
            }
            out.push_str(&format!("## {} severity ({})\n\n", level.as_str(), group.len()));
            let mut by_artefact: std::collections::BTreeMap<(String, String), Vec<&Finding>> =
                std::collections::BTreeMap::new();
            for f in group {
                by_artefact
                    .entry((f.artefact_kind.clone(), f.artefact_name.clone()))
                    .or_default()
                    .push(f);
            }
            for ((kind, name), findings) in by_artefact {
                out.push_str(&format!("### {} `{}`\n\n", kind, name));
                for f in findings {
                    let target = if f.affected_method.is_empty() {
                        f.affected_path.clone()
                    } else {
                        format!("{}::{}", f.affected_path, f.affected_method)
                    };
                    out.push_str(&format!(
                        "- **{}** ({}): {}  \n  {}\n",
                        f.kind, f.severity.as_str(), target, f.detail.trim()
                    ));
                }
                out.push('\n');
            }
        }
    }

    if !report.open_spec_questions.is_empty() {
        out.push_str("## Open spec questions\n\n");
        out.push_str("These findings are spec-side rather than implementation-side. They do not flip tracker entries; address them in the next `/refine` session.\n\n");
        for q in &report.open_spec_questions {
            out.push_str(&format!("- **{}**", q.topic));
            if !q.drafts.is_empty() {
                out.push_str(&format!(" — affects: {}", q.drafts.join(", ")));
            }
            out.push_str("  \n  ");
            out.push_str(q.detail.trim());
            out.push('\n');
        }
    }

    out
}

fn write_atomic(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create {}", parent.display()))?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, content)
        .with_context(|| format!("failed to write {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("failed to rename {} to {}", tmp.display(), path.display()))?;
    Ok(())
}

/// Used by the auditor agent to enumerate the tracker entries it can flip,
/// so its findings emit the right `affected_method` field.
pub fn tracker_entry_keys(state_dir: &Path) -> Result<BTreeSet<String>> {
    let tracker = BuildTracker::load(state_dir)?;
    Ok(tracker.artefacts.keys().map(|k| k.to_string()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_tracker::keys::ArtefactEntryKind;
    use crate::build_tracker::ArtefactEntry;

    fn report_with_one_finding() -> AuditReport {
        AuditReport {
            version: 1,
            produced_at: Some("2026-04-25T10:00:00Z".into()),
            findings: vec![Finding {
                artefact_kind: "context".into(),
                artefact_name: "audit_recording".into(),
                affected_path: "src/contexts/audit_recording.rs".into(),
                affected_method: "audit_entry_persist".into(),
                kind: "invariant_not_enforced".into(),
                severity: Severity::High,
                detail: "the body is empty; append-only invariant unenforced.".into(),
            }],
            open_spec_questions: vec![],
        }
    }

    fn tracker_with_role_method() -> BuildTracker {
        let mut tracker = BuildTracker::new();
        let key = ArtefactKey::role_method("audit_recording", "audit_entry", "persist");
        tracker.artefacts.insert(
            key,
            ArtefactEntry {
                kind: ArtefactEntryKind::RoleMethod,
                prepared_hash: "abc123".into(),
                upstream: vec![],
                state: ArtefactState::Fresh,
                reason: None,
                audit_finding: None,
            },
        );
        tracker
    }

    #[test]
    fn round_trip_json() {
        let report = report_with_one_finding();
        let json = serde_json::to_string(&report).unwrap();
        let parsed = AuditReport::from_json(&json).unwrap();
        assert_eq!(parsed, report);
    }

    #[test]
    fn render_markdown_groups_by_severity_and_artefact() {
        let report = report_with_one_finding();
        let md = render_markdown(&report);
        assert!(md.contains("# Audit report"));
        assert!(md.contains("## high severity (1)"));
        assert!(md.contains("### context `audit_recording`"));
        assert!(md.contains("invariant_not_enforced"));
        assert!(md.contains("audit_entry_persist"));
    }

    #[test]
    fn record_writes_files_and_flips_tracker() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path();
        let state_dir = workspace.join(".reen");
        fs::create_dir_all(&state_dir).unwrap();

        let tracker = tracker_with_role_method();
        tracker.save(&state_dir).unwrap();

        let report = AuditReport {
            version: 1,
            produced_at: None,
            findings: vec![Finding {
                artefact_kind: "context".into(),
                artefact_name: "audit_recording".into(),
                affected_path: "src/contexts/audit_recording.rs".into(),
                affected_method: "persist".into(), // matches RoleMethod with method="persist"
                kind: "invariant_not_enforced".into(),
                severity: Severity::High,
                detail: "missing append-only enforcement".into(),
            }],
            open_spec_questions: vec![],
        };

        let outcome = record(&report, workspace, &state_dir, Severity::Medium).unwrap();
        assert_eq!(outcome.findings_count, 1);
        assert_eq!(outcome.flipped_count, 1);
        assert_eq!(outcome.idempotent_skipped_count, 0);
        assert_eq!(outcome.unmatched_count, 0);

        // audit.json + AUDIT.md exist
        assert!(state_dir.join("audit.json").is_file());
        assert!(workspace.join("AUDIT.md").is_file());

        // tracker entry flipped
        let reloaded = BuildTracker::load(&state_dir).unwrap();
        let key = ArtefactKey::role_method("audit_recording", "audit_entry", "persist");
        let entry = reloaded.artefacts.get(&key).unwrap();
        assert_eq!(entry.state, ArtefactState::Stale);
        assert_eq!(entry.reason, Some(StaleReason::AuditFinding));
        assert!(entry.audit_finding.as_ref().unwrap().contains("missing append-only enforcement"));
    }

    #[test]
    fn record_is_idempotent_for_same_finding_text() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path();
        let state_dir = workspace.join(".reen");
        fs::create_dir_all(&state_dir).unwrap();

        let tracker = tracker_with_role_method();
        tracker.save(&state_dir).unwrap();

        let report = AuditReport {
            version: 1,
            produced_at: None,
            findings: vec![Finding {
                artefact_kind: "context".into(),
                artefact_name: "audit_recording".into(),
                affected_path: "src/contexts/audit_recording.rs".into(),
                affected_method: "persist".into(),
                kind: "invariant_not_enforced".into(),
                severity: Severity::High,
                detail: "the same finding text".into(),
            }],
            open_spec_questions: vec![],
        };

        let first = record(&report, workspace, &state_dir, Severity::Medium).unwrap();
        assert_eq!(first.flipped_count, 1);

        let second = record(&report, workspace, &state_dir, Severity::Medium).unwrap();
        // Same finding text on already-stale entry — idempotent skip.
        assert_eq!(second.flipped_count, 0);
        assert_eq!(second.idempotent_skipped_count, 1);
    }

    #[test]
    fn record_skips_below_threshold() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path();
        let state_dir = workspace.join(".reen");
        fs::create_dir_all(&state_dir).unwrap();

        let tracker = tracker_with_role_method();
        tracker.save(&state_dir).unwrap();

        let report = AuditReport {
            version: 1,
            produced_at: None,
            findings: vec![Finding {
                artefact_kind: "context".into(),
                artefact_name: "audit_recording".into(),
                affected_path: "src/contexts/audit_recording.rs".into(),
                affected_method: "persist".into(),
                kind: "code_not_in_spec".into(),
                severity: Severity::Low, // below medium threshold
                detail: "minor finding".into(),
            }],
            open_spec_questions: vec![],
        };

        let outcome = record(&report, workspace, &state_dir, Severity::Medium).unwrap();
        // Persisted to audit.json and AUDIT.md but not flipped.
        assert_eq!(outcome.flipped_count, 0);
        assert!(state_dir.join("audit.json").is_file());

        let reloaded = BuildTracker::load(&state_dir).unwrap();
        let key = ArtefactKey::role_method("audit_recording", "audit_entry", "persist");
        let entry = reloaded.artefacts.get(&key).unwrap();
        assert_eq!(entry.state, ArtefactState::Fresh);
    }

    #[test]
    fn unmatched_findings_persist_but_skip_tracker() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path();
        let state_dir = workspace.join(".reen");
        fs::create_dir_all(&state_dir).unwrap();

        let tracker = tracker_with_role_method();
        tracker.save(&state_dir).unwrap();

        let report = AuditReport {
            version: 1,
            produced_at: None,
            findings: vec![Finding {
                artefact_kind: "context".into(),
                artefact_name: "no_such_context".into(),
                affected_path: "src/contexts/missing.rs".into(),
                affected_method: "persist".into(),
                kind: "invariant_not_enforced".into(),
                severity: Severity::High,
                detail: "no matching tracker entry".into(),
            }],
            open_spec_questions: vec![],
        };

        let outcome = record(&report, workspace, &state_dir, Severity::Medium).unwrap();
        assert_eq!(outcome.flipped_count, 0);
        assert_eq!(outcome.unmatched_count, 1);
        // The finding still appears in AUDIT.md.
        let md = fs::read_to_string(workspace.join("AUDIT.md")).unwrap();
        assert!(md.contains("no matching tracker entry"));
    }
}
