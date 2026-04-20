use crate::draft_parser::{
    ContextDraft, DataDraft, DraftDocument, ProjectionDraft, parse_draft_file,
};
use crate::workspace::Workspace;
use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DraftReviewCategory {
    Completeness,
    Concreteness,
    Ambiguity,
}

impl DraftReviewCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completeness => "completeness",
            Self::Concreteness => "concreteness",
            Self::Ambiguity => "ambiguity",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftReviewFinding {
    pub draft_path: String,
    pub source_line: Option<usize>,
    pub category: DraftReviewCategory,
    pub message: String,
    pub severity: Option<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct DraftReview {
    pub reviewed_paths: Vec<String>,
    pub findings: Vec<DraftReviewFinding>,
}

const VAGUE_PHRASES: &[&str] = &[
    "appropriately",
    "if needed",
    "when possible",
    "as required",
    "properly",
    "correctly",
    "suitable",
    "relevant",
    "the right thing",
    "the right way",
    "the right approach",
    "current situation",
    "etc.",
];

pub fn review_raw_drafts(workspace: &Workspace, paths: &[PathBuf]) -> Result<DraftReview> {
    let mut report = DraftReview {
        reviewed_paths: paths
            .iter()
            .map(|p| {
                p.strip_prefix(&workspace.root)
                    .unwrap_or(p)
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect(),
        findings: Vec::new(),
    };

    for path in paths {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let relative = path
            .strip_prefix(&workspace.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        match parse_draft_file(path, &workspace.drafts_dir, &content) {
            Ok(document) => {
                review_document(&relative, &content, &document, &mut report.findings);
            }
            Err(error) => {
                report.findings.push(DraftReviewFinding {
                    draft_path: relative,
                    source_line: None,
                    category: DraftReviewCategory::Completeness,
                    message: error.to_string(),
                    severity: None,
                });
            }
        }
    }

    sort_and_dedup_findings(&mut report.findings);
    Ok(report)
}

pub fn sort_and_dedup_findings(findings: &mut Vec<DraftReviewFinding>) {
    findings.sort_by(|a, b| {
        (
            a.draft_path.as_str(),
            a.source_line.unwrap_or(usize::MAX),
            a.category,
            a.message.as_str(),
        )
            .cmp(&(
                b.draft_path.as_str(),
                b.source_line.unwrap_or(usize::MAX),
                b.category,
                b.message.as_str(),
            ))
    });
    findings.dedup_by(|a, b| {
        a.draft_path == b.draft_path
            && a.source_line == b.source_line
            && a.category == b.category
            && a.message == b.message
    });
}

fn review_document(
    path: &str,
    content: &str,
    doc: &DraftDocument,
    findings: &mut Vec<DraftReviewFinding>,
) {
    match doc {
        DraftDocument::Data(data) => review_data(path, content, data, findings),
        DraftDocument::Projection(proj) => review_projection(path, content, proj, findings),
        DraftDocument::Context(ctx) => review_context(path, content, ctx, findings),
    }
}

fn review_data(
    path: &str,
    content: &str,
    data: &DataDraft,
    findings: &mut Vec<DraftReviewFinding>,
) {
    if data.attributes.is_empty() {
        push(findings, path, heading_line(content, "## Attributes"),
            DraftReviewCategory::Completeness,
            "data draft must declare at least one attribute");
    }
    if data.scenarios.is_empty() {
        push(findings, path, heading_line(content, "## Scenarios"),
            DraftReviewCategory::Completeness,
            "data draft must contain at least one scenario");
    }
    scan_vagueness(findings, path, content, &data.purpose, "purpose");
    for op in &data.operations {
        scan_vagueness(findings, path, content, &op.description, &format!("operation `{}`", op.name));
    }
}

fn review_projection(
    path: &str,
    content: &str,
    proj: &ProjectionDraft,
    findings: &mut Vec<DraftReviewFinding>,
) {
    if proj.source.is_empty() {
        push(findings, path, heading_line(content, "## Source"),
            DraftReviewCategory::Completeness,
            "projection draft must declare a source");
    }
    if proj.queries.is_empty() {
        push(findings, path, heading_line(content, "## Queries"),
            DraftReviewCategory::Completeness,
            "projection draft must declare at least one query");
    }
    if proj.scenarios.is_empty() {
        push(findings, path, heading_line(content, "## Scenarios"),
            DraftReviewCategory::Completeness,
            "projection draft must contain at least one scenario");
    }
    scan_vagueness(findings, path, content, &proj.purpose, "purpose");
}

fn review_context(
    path: &str,
    content: &str,
    ctx: &ContextDraft,
    findings: &mut Vec<DraftReviewFinding>,
) {
    if ctx.role_players.is_empty() {
        push(findings, path, heading_line(content, "## Role Players"),
            DraftReviewCategory::Completeness,
            "context draft must declare at least one role player");
    }
    if ctx.functionalities.is_empty() {
        push(findings, path, heading_line(content, "## Functionalities"),
            DraftReviewCategory::Completeness,
            "context draft must declare at least one functionality");
    }
    if ctx.scenarios.is_empty() {
        push(findings, path, heading_line(content, "## Scenarios"),
            DraftReviewCategory::Completeness,
            "context draft must contain at least one scenario");
    }
    // Every role player should have at least one role method group
    for rp in &ctx.role_players {
        if !ctx.role_methods.iter().any(|g| g.role == rp.name) {
            push(findings, path, find_line(content, &rp.name),
                DraftReviewCategory::Completeness,
                &format!("role player `{}` has no matching Role Methods subsection", rp.name));
        }
    }
    scan_vagueness(findings, path, content, &ctx.purpose, "purpose");
    for func in &ctx.functionalities {
        scan_vagueness(findings, path, content, &func.description, &format!("functionality `{}`", func.name));
    }
}

fn scan_vagueness(
    findings: &mut Vec<DraftReviewFinding>,
    path: &str,
    content: &str,
    text: &str,
    context: &str,
) {
    let lower = text.to_ascii_lowercase();
    for phrase in VAGUE_PHRASES {
        if lower.contains(phrase) {
            push(findings, path, find_line(content, phrase),
                DraftReviewCategory::Concreteness,
                &format!("{context} uses vague phrase `{phrase}` without an explicit criterion"));
        }
    }
}

fn push(
    findings: &mut Vec<DraftReviewFinding>,
    path: &str,
    line: Option<usize>,
    category: DraftReviewCategory,
    message: &str,
) {
    findings.push(DraftReviewFinding {
        draft_path: path.to_string(),
        source_line: line,
        category,
        message: message.to_string(),
        severity: None,
    });
}

fn heading_line(content: &str, heading: &str) -> Option<usize> {
    find_line(content, heading)
}

fn find_line(content: &str, needle: &str) -> Option<usize> {
    if needle.trim().is_empty() {
        return None;
    }
    let needle = needle.trim().to_ascii_lowercase();
    content.lines().enumerate().find_map(|(idx, line)| {
        if line.to_ascii_lowercase().contains(&needle) {
            Some(idx + 1)
        } else {
            None
        }
    })
}
