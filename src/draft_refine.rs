use crate::draft_parser::{
    AppDraft, CompositeDraft, DataDraft, DraftDocument, FunctionalityDraft, RolePlayer,
    normalize_type_notation, parse_draft_file,
};
use crate::workspace::Workspace;
use anyhow::{Context, Result};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DraftReviewCategory {
    Completeness,
    Concreteness,
    Ambiguity,
    /// Semantic under-specification surfaced by the LLM-backed behavioral review phase.
    ///
    /// These are *questions* the model wants answered in the draft — they sort last so that
    /// structural issues (missing sections, vague phrasing, symbolic ambiguity) are addressed
    /// first.
    BehavioralAmbiguity,
}

impl DraftReviewCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completeness => "completeness",
            Self::Concreteness => "concreteness",
            Self::Ambiguity => "ambiguity",
            Self::BehavioralAmbiguity => "behavioral ambiguity",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftReviewFinding {
    pub draft_path: String,
    pub source_line: Option<usize>,
    pub category: DraftReviewCategory,
    pub message: String,
    /// LLM-assigned severity score (0..=100) for `BehavioralAmbiguity` findings.
    ///
    /// `None` for deterministic findings produced by `review_raw_drafts`. Higher scores mean a
    /// larger risk that an implementer would guess wrong without a draft clarification.
    pub severity: Option<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct DraftReview {
    pub reviewed_paths: Vec<String>,
    pub findings: Vec<DraftReviewFinding>,
}

#[derive(Debug, Clone)]
struct ReviewedDraft {
    relative_path: String,
    content: String,
    document: DraftDocument,
}

#[derive(Debug, Clone)]
struct ParsedSignature {
    receiver: Option<String>,
    parameters: Vec<SignatureParameter>,
    return_type: String,
}

#[derive(Debug, Clone)]
struct SignatureParameter {
    name: String,
    rust: String,
}

#[derive(Debug, Clone)]
struct ExportMethod {
    export_name: String,
    signature: ParsedSignature,
}

#[derive(Debug, Clone, Default)]
struct ExportIndex {
    by_export: BTreeMap<String, BTreeMap<String, ExportMethod>>,
    normalized_exports: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Default)]
struct AppFlowEnv {
    variables: BTreeMap<String, String>,
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
    // "the right" is intentionally NOT a bare prefix here: phrases like "the right side" or
    // "the right half" are concrete spatial references, not vague hand-waving. Only the
    // explicitly vague completions are listed below.
    "the right thing",
    "the right way",
    "the right approach",
    "the right choice",
    "the right decision",
    "the right behavior",
    "the right behaviour",
    "current situation",
    "etc.",
];

const FILLER_WORDS: &[&str] = &["something", "stuff"];

pub fn review_raw_drafts(workspace: &Workspace, paths: &[PathBuf]) -> Result<DraftReview> {
    let mut report = DraftReview {
        reviewed_paths: paths
            .iter()
            .map(|path| {
                path.strip_prefix(&workspace.root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect(),
        findings: Vec::new(),
    };
    let mut reviewed = Vec::new();

    for path in paths {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let relative_path = path
            .strip_prefix(&workspace.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        match parse_draft_file(path, &workspace.drafts_dir, &content) {
            Ok(document) => reviewed.push(ReviewedDraft {
                relative_path,
                content,
                document,
            }),
            Err(error) => {
                report.findings.push(DraftReviewFinding {
                    draft_path: relative_path,
                    source_line: None,
                    category: DraftReviewCategory::Completeness,
                    message: error.to_string(),
                    severity: None,
                });
            }
        }
    }

    for draft in &reviewed {
        review_document_completeness(draft, &mut report.findings);
        review_document_concreteness(draft, &mut report.findings);
    }

    let index = collect_export_index(&reviewed, &mut report.findings);
    review_ambiguous_export_references(&reviewed, &index, &mut report.findings);
    review_role_method_contracts(&reviewed, &index, &mut report.findings);
    review_app_flow_contracts(&reviewed, &index, &mut report.findings);

    sort_and_dedup_findings(&mut report.findings);

    Ok(report)
}

/// Sort findings by draft path / line / category / message and drop exact duplicates.
///
/// Severity is deliberately ignored when deduplicating: the same behavioral question surfaced
/// with a slightly different severity on two runs should still collapse to one finding. We
/// keep the first severity seen per unique (path, line, category, message) tuple.
pub fn sort_and_dedup_findings(findings: &mut Vec<DraftReviewFinding>) {
    findings.sort_by(|left, right| {
        (
            left.draft_path.as_str(),
            left.source_line.unwrap_or(usize::MAX),
            left.category,
            left.message.as_str(),
        )
            .cmp(&(
                right.draft_path.as_str(),
                right.source_line.unwrap_or(usize::MAX),
                right.category,
                right.message.as_str(),
            ))
    });
    findings.dedup_by(|left, right| {
        left.draft_path == right.draft_path
            && left.source_line == right.source_line
            && left.category == right.category
            && left.message == right.message
    });
}

fn review_document_completeness(draft: &ReviewedDraft, findings: &mut Vec<DraftReviewFinding>) {
    match &draft.document {
        DraftDocument::Data(data) => review_data_completeness(draft, data, findings),
        DraftDocument::Projection(composite) | DraftDocument::Context(composite) => {
            review_composite_completeness(draft, composite, findings)
        }
        DraftDocument::App(app) => review_app_completeness(draft, app, findings),
    }
}

fn review_data_completeness(
    draft: &ReviewedDraft,
    data: &DataDraft,
    findings: &mut Vec<DraftReviewFinding>,
) {
    if data.description.trim().is_empty() {
        push_finding(
            findings,
            &draft.relative_path,
            heading_line(&draft.content, "## Description"),
            DraftReviewCategory::Completeness,
            "data draft description must contain real content".to_string(),
        );
    }

    for functionality in &data.functionalities {
        if functionality.behavior.trim().is_empty() {
            push_finding(
                findings,
                &draft.relative_path,
                markdown_line(&draft.content, &format!("`{}`", functionality.name)),
                DraftReviewCategory::Completeness,
                format!(
                    "data functionality `{}` must describe observable behavior",
                    functionality.name
                ),
            );
        }
    }
}

fn review_composite_completeness(
    draft: &ReviewedDraft,
    composite: &CompositeDraft,
    findings: &mut Vec<DraftReviewFinding>,
) {
    if composite.purpose.trim().is_empty() {
        push_finding(
            findings,
            &draft.relative_path,
            heading_line(&draft.content, "## Purpose"),
            DraftReviewCategory::Completeness,
            "purpose must contain real content".to_string(),
        );
    }

    for role in &composite.roles {
        if role.why_involved.trim().is_empty() {
            push_finding(
                findings,
                &draft.relative_path,
                table_cell_line(&draft.content, &role.name),
                DraftReviewCategory::Completeness,
                format!("role `{}` is missing `Why involved` content", role.name),
            );
        }
        if role.expected_behavior.trim().is_empty() {
            push_finding(
                findings,
                &draft.relative_path,
                table_cell_line(&draft.content, &role.name),
                DraftReviewCategory::Completeness,
                format!(
                    "role `{}` is missing `Expected behaviour` content",
                    role.name
                ),
            );
        }
    }

    for functionality in &composite.functionalities {
        review_functionality_completeness(draft, functionality, findings);
    }
}

fn review_functionality_completeness(
    draft: &ReviewedDraft,
    functionality: &FunctionalityDraft,
    findings: &mut Vec<DraftReviewFinding>,
) {
    let line = heading_line(&draft.content, &format!("### {}", functionality.name));

    if functionality.interactions.is_empty() {
        push_finding(
            findings,
            &draft.relative_path,
            line,
            DraftReviewCategory::Completeness,
            format!(
                "functionality `{}` must declare a `Started by | Uses | Result` row",
                functionality.name
            ),
        );
    }

    for interaction in &functionality.interactions {
        if interaction.started_by.trim().is_empty()
            || interaction.uses.trim().is_empty()
            || interaction.result.trim().is_empty()
        {
            push_finding(
                findings,
                &draft.relative_path,
                line,
                DraftReviewCategory::Completeness,
                format!(
                    "functionality `{}` must keep `Started by`, `Uses`, and `Result` concrete and non-empty",
                    functionality.name
                ),
            );
            break;
        }
    }

    if functionality.flow.iter().all(|step| step.trim().is_empty()) {
        push_finding(
            findings,
            &draft.relative_path,
            line,
            DraftReviewCategory::Completeness,
            format!(
                "functionality `{}` must contain at least one numbered flow step",
                functionality.name
            ),
        );
    }

    if functionality.examples.is_empty() {
        push_finding(
            findings,
            &draft.relative_path,
            line,
            DraftReviewCategory::Completeness,
            format!(
                "functionality `{}` must contain at least one `Given | When | Then` example row",
                functionality.name
            ),
        );
    }
    for example in &functionality.examples {
        if example.given.trim().is_empty()
            || example.when.trim().is_empty()
            || example.then.trim().is_empty()
        {
            push_finding(
                findings,
                &draft.relative_path,
                line,
                DraftReviewCategory::Completeness,
                format!(
                    "functionality `{}` example rows must keep `Given`, `When`, and `Then` non-empty",
                    functionality.name
                ),
            );
            break;
        }
    }

    for extension in &functionality.extensions {
        if !extension_has_concrete_outcome(extension) {
            push_finding(
                findings,
                &draft.relative_path,
                find_line_containing(&draft.content, extension),
                DraftReviewCategory::Completeness,
                format!(
                    "functionality `{}` extension `{}` must state a concrete outcome or continuation rule",
                    functionality.name, extension
                ),
            );
        }
    }
}

fn review_app_completeness(
    draft: &ReviewedDraft,
    app: &AppDraft,
    findings: &mut Vec<DraftReviewFinding>,
) {
    for section in ["Description", "Collaborators and Wiring", "Startup Sequence"] {
        let body = app
            .sections
            .get(section)
            .map(String::as_str)
            .unwrap_or_default();
        if body.trim().is_empty() {
            push_finding(
                findings,
                &draft.relative_path,
                heading_line(&draft.content, &format!("## {section}")),
                DraftReviewCategory::Completeness,
                format!("app draft section `## {section}` must contain real content"),
            );
        }
    }

    let primary_flow_sections = app_primary_flow_sections(app);
    if primary_flow_sections.is_empty() {
        push_finding(
            findings,
            &draft.relative_path,
            heading_line(&draft.content, "## Main Flow")
                .or_else(|| heading_line(&draft.content, "## Main Loop Behavior")),
            DraftReviewCategory::Completeness,
            "app draft must contain real content in either `## Main Flow` or `## Main Loop Behavior`"
                .to_string(),
        );
    }
}

fn review_document_concreteness(draft: &ReviewedDraft, findings: &mut Vec<DraftReviewFinding>) {
    match &draft.document {
        DraftDocument::Data(data) => {
            scan_text_for_vagueness(
                findings,
                &draft.relative_path,
                &draft.content,
                data.description.as_str(),
                heading_line(&draft.content, "## Description"),
                "description",
            );
            for functionality in &data.functionalities {
                scan_text_for_vagueness(
                    findings,
                    &draft.relative_path,
                    &draft.content,
                    functionality.behavior.as_str(),
                    markdown_line(&draft.content, &format!("`{}`", functionality.name)),
                    &format!("data functionality `{}` behavior", functionality.name),
                );
            }
        }
        DraftDocument::Projection(composite) | DraftDocument::Context(composite) => {
            scan_text_for_vagueness(
                findings,
                &draft.relative_path,
                &draft.content,
                composite.purpose.as_str(),
                heading_line(&draft.content, "## Purpose"),
                "purpose",
            );
            for role in &composite.roles {
                scan_text_for_vagueness(
                    findings,
                    &draft.relative_path,
                    &draft.content,
                    role.expected_behavior.as_str(),
                    table_cell_line(&draft.content, &role.name),
                    &format!("role `{}` expected behaviour", role.name),
                );
            }
            for functionality in &composite.functionalities {
                let line = heading_line(&draft.content, &format!("### {}", functionality.name));
                for interaction in &functionality.interactions {
                    scan_text_for_vagueness(
                        findings,
                        &draft.relative_path,
                        &draft.content,
                        interaction.result.as_str(),
                        line,
                        &format!("functionality `{}` result", functionality.name),
                    );
                }
                for step in &functionality.flow {
                    scan_text_for_vagueness(
                        findings,
                        &draft.relative_path,
                        &draft.content,
                        step.as_str(),
                        find_line_containing(&draft.content, step),
                        &format!("functionality `{}` flow", functionality.name),
                    );
                }
                for extension in &functionality.extensions {
                    scan_text_for_vagueness(
                        findings,
                        &draft.relative_path,
                        &draft.content,
                        extension.as_str(),
                        find_line_containing(&draft.content, extension),
                        &format!("functionality `{}` extension", functionality.name),
                    );
                }
                for guarantee in &functionality.guarantee {
                    scan_text_for_vagueness(
                        findings,
                        &draft.relative_path,
                        &draft.content,
                        guarantee.as_str(),
                        find_line_containing(&draft.content, guarantee),
                        &format!("functionality `{}` guarantee", functionality.name),
                    );
                }
            }
        }
        DraftDocument::App(app) => {
            for (section, body) in &app.sections {
                scan_text_for_vagueness(
                    findings,
                    &draft.relative_path,
                    &draft.content,
                    body.as_str(),
                    heading_line(&draft.content, &format!("## {section}")),
                    &format!("app section `{section}`"),
                );
            }
        }
    }
}

fn scan_text_for_vagueness(
    findings: &mut Vec<DraftReviewFinding>,
    draft_path: &str,
    content: &str,
    text: &str,
    fallback_line: Option<usize>,
    context: &str,
) {
    for phrase in VAGUE_PHRASES {
        if text_contains_vague_phrase(text, phrase) {
            push_finding(
                findings,
                draft_path,
                find_line_containing(content, phrase).or(fallback_line),
                DraftReviewCategory::Concreteness,
                format!("{context} uses vague phrase `{phrase}` without an explicit criterion"),
            );
        }
    }

    for filler in FILLER_WORDS {
        let pattern = format!(" {filler} ");
        let lower = format!(" {} ", text.to_ascii_lowercase());
        if lower.contains(&pattern) {
            push_finding(
                findings,
                draft_path,
                find_line_containing(content, filler).or(fallback_line),
                DraftReviewCategory::Concreteness,
                format!("{context} uses filler word `{filler}` instead of concrete behavior"),
            );
        }
    }
}

fn collect_export_index(
    reviewed: &[ReviewedDraft],
    findings: &mut Vec<DraftReviewFinding>,
) -> ExportIndex {
    let mut index = ExportIndex::default();
    for draft in reviewed {
        let export_name = draft_title(&draft.document).to_string();
        index
            .normalized_exports
            .entry(normalize_symbol(&export_name))
            .or_default()
            .push(export_name.clone());

        let methods = collect_public_methods(draft, findings);
        if !methods.is_empty() {
            index.by_export.insert(export_name, methods);
        }
    }
    for exports in index.normalized_exports.values_mut() {
        exports.sort();
        exports.dedup();
    }
    index
}

fn collect_public_methods(
    draft: &ReviewedDraft,
    findings: &mut Vec<DraftReviewFinding>,
) -> BTreeMap<String, ExportMethod> {
    let export_name = draft_title(&draft.document).to_string();
    let mut methods = BTreeMap::new();
    match &draft.document {
        DraftDocument::Data(data) => {
            for functionality in &data.functionalities {
                match parse_signature(&functionality.signature, &functionality.name, &export_name) {
                    Ok(signature) => {
                        methods.insert(
                            functionality.name.clone(),
                            ExportMethod {
                                export_name: export_name.clone(),
                                signature,
                            },
                        );
                    }
                    Err(error) => {
                        push_finding(
                            findings,
                            &draft.relative_path,
                            markdown_line(&draft.content, &format!("`{}`", functionality.name)),
                            DraftReviewCategory::Completeness,
                            format!(
                                "data functionality `{}` has an invalid explicit signature: {}",
                                functionality.name, error
                            ),
                        );
                    }
                }
            }
        }
        DraftDocument::Projection(composite) | DraftDocument::Context(composite) => {
            for functionality in &composite.functionalities {
                let Some(signature_text) = extract_signature_marker(&functionality.flow) else {
                    continue;
                };
                match parse_signature(&signature_text, &functionality.name, &export_name) {
                    Ok(signature) => {
                        methods.insert(
                            functionality.name.clone(),
                            ExportMethod {
                                export_name: export_name.clone(),
                                signature,
                            },
                        );
                    }
                    Err(error) => {
                        push_finding(
                            findings,
                            &draft.relative_path,
                            heading_line(&draft.content, &format!("### {}", functionality.name)),
                            DraftReviewCategory::Completeness,
                            format!(
                                "functionality `{}` has an invalid explicit signature: {}",
                                functionality.name, error
                            ),
                        );
                    }
                }
            }
        }
        DraftDocument::App(_) => {}
    }
    methods
}

fn review_ambiguous_export_references(
    reviewed: &[ReviewedDraft],
    index: &ExportIndex,
    findings: &mut Vec<DraftReviewFinding>,
) {
    let token_regex = Regex::new(r"`([^`]+)`").expect("backtick token regex");
    for draft in reviewed {
        for captures in token_regex.captures_iter(&draft.content) {
            let Some(value) = captures.get(1) else {
                continue;
            };
            let token = value.as_str().trim();
            if token.is_empty()
                || token.contains('(')
                || token.contains(')')
                || token.contains("::")
                || token.contains(' ')
            {
                continue;
            }
            let Some(exports) = index.normalized_exports.get(&normalize_symbol(token)) else {
                continue;
            };
            if exports.len() < 2 {
                continue;
            }
            push_finding(
                findings,
                &draft.relative_path,
                line_for_offset(&draft.content, value.start()),
                DraftReviewCategory::Ambiguity,
                format!(
                    "backticked reference `{token}` is ambiguous across selected exports: {}",
                    exports.join(", ")
                ),
            );
        }
    }
}

fn review_role_method_contracts(
    reviewed: &[ReviewedDraft],
    index: &ExportIndex,
    findings: &mut Vec<DraftReviewFinding>,
) {
    for draft in reviewed {
        let composite = match &draft.document {
            DraftDocument::Projection(composite) | DraftDocument::Context(composite) => composite,
            _ => continue,
        };

        for role_group in &composite.role_methods {
            let Some(role_player) = composite
                .roles
                .iter()
                .find(|role| normalize_symbol(&role.name) == normalize_symbol(&role_group.role))
            else {
                continue;
            };
            let targets = role_targets(role_player, index);
            if targets.len() > 1 {
                push_finding(
                    findings,
                    &draft.relative_path,
                    heading_line(&draft.content, &format!("### {}", role_group.role)),
                    DraftReviewCategory::Ambiguity,
                    format!(
                        "role `{}` names multiple possible collaborator exports: {}",
                        role_group.role,
                        targets.iter().cloned().collect::<Vec<_>>().join(", ")
                    ),
                );
                continue;
            }
            let Some(target_export) = targets.into_iter().next() else {
                continue;
            };
            let Some(target_methods) = index.by_export.get(&target_export) else {
                continue;
            };

            for method in &role_group.methods {
                let Some(signature_text) = extract_inline_signature(&method.detail) else {
                    continue;
                };
                let role_signature =
                    match parse_signature(&signature_text, &method.name, &target_export) {
                        Ok(signature) => signature,
                        Err(error) => {
                            push_finding(
                                findings,
                                &draft.relative_path,
                                markdown_line(&draft.content, &format!("**{}**", method.name)),
                                DraftReviewCategory::Completeness,
                                format!(
                                    "role method `{}` has an invalid explicit signature: {}",
                                    method.name, error
                                ),
                            );
                            continue;
                        }
                    };
                let Some(target_method) = target_methods.get(&method.name) else {
                    continue;
                };
                if !signatures_compatible(&target_method.signature, &role_signature, &target_export)
                {
                    push_finding(
                        findings,
                        &draft.relative_path,
                        markdown_line(&draft.content, &format!("**{}**", method.name)),
                        DraftReviewCategory::Ambiguity,
                        format!(
                            "role method `{}` expects `{}` on `{}`, but `{}` explicitly declares `{}`",
                            method.name,
                            render_signature(
                                &target_method.signature,
                                &method.name,
                                &target_export
                            ),
                            target_export,
                            role_group.role,
                            render_signature(&role_signature, &method.name, &target_export)
                        ),
                    );
                }
            }
        }
    }
}

fn review_app_flow_contracts(
    reviewed: &[ReviewedDraft],
    index: &ExportIndex,
    findings: &mut Vec<DraftReviewFinding>,
) {
    for draft in reviewed {
        let app = match &draft.document {
            DraftDocument::App(app) => app,
            _ => continue,
        };
        let mut env = AppFlowEnv::default();
        let mut section_names = vec!["Startup Sequence"];
        section_names.extend(app_primary_flow_sections(app));
        for section_name in section_names {
            let body = app
                .sections
                .get(section_name)
                .map(String::as_str)
                .unwrap_or_default();
            for line in body
                .lines()
                .map(str::trim)
                .filter(|line| line.starts_with("- "))
            {
                review_app_flow_line(
                    &draft.relative_path,
                    &draft.content,
                    line,
                    index,
                    &mut env,
                    findings,
                );
            }
        }
    }
}

fn review_app_flow_line(
    draft_path: &str,
    content: &str,
    line: &str,
    index: &ExportIndex,
    env: &mut AppFlowEnv,
    findings: &mut Vec<DraftReviewFinding>,
) {
    let Some((binding, expr)) = parse_let_call(line) else {
        if let Some(expr) = parse_call(line) {
            validate_call_contract(draft_path, content, line, &expr, index, env, findings);
        }
        return;
    };
    if let Some(return_type) =
        validate_call_contract(draft_path, content, line, &expr, index, env, findings)
    {
        env.variables.insert(binding, return_type);
    }
}

fn validate_call_contract(
    draft_path: &str,
    content: &str,
    original_line: &str,
    expr: &str,
    index: &ExportIndex,
    env: &AppFlowEnv,
    findings: &mut Vec<DraftReviewFinding>,
) -> Option<String> {
    let call = parse_call_expression(expr)?;
    let method = resolve_method_for_call(&call, index, env)?;
    if method.signature.parameters.len() != call.args.len() {
        push_finding(
            findings,
            draft_path,
            find_line_containing(content, original_line),
            DraftReviewCategory::Ambiguity,
            format!(
                "call `{expr}` passes {} argument(s), but `{}` explicitly expects {}",
                call.args.len(),
                render_signature(&method.signature, &call.method, &method.export_name),
                method.signature.parameters.len()
            ),
        );
        return Some(resolve_self_type(
            &method.signature.return_type,
            &method.export_name,
        ));
    }

    for (expected, actual) in method.signature.parameters.iter().zip(&call.args) {
        let Some(actual_type) = infer_app_arg_type(actual, env) else {
            continue;
        };
        if !types_compatible(&expected.rust, &actual_type, &method.export_name) {
            push_finding(
                findings,
                draft_path,
                find_line_containing(content, original_line),
                DraftReviewCategory::Ambiguity,
                format!(
                    "call `{expr}` passes `{}` as `{}`, but `{}` explicitly expects `{}`",
                    actual, actual_type, expected.name, expected.rust
                ),
            );
        }
    }

    Some(resolve_self_type(
        &method.signature.return_type,
        &method.export_name,
    ))
}

fn role_targets(role: &RolePlayer, index: &ExportIndex) -> BTreeSet<String> {
    let mut targets = BTreeSet::new();
    if let Some(explicit_type) = &role.explicit_type {
        if let Some(export) = resolve_export_name_from_type(explicit_type, index) {
            targets.insert(export);
        }
    }
    for token in backticked_tokens(&role.why_involved)
        .into_iter()
        .chain(backticked_tokens(&role.expected_behavior))
    {
        let normalized = normalize_symbol(&token);
        if let Some(exports) = index.normalized_exports.get(&normalized) {
            targets.extend(exports.iter().cloned());
        }
    }
    targets
}

fn parse_let_call(line: &str) -> Option<(String, String)> {
    let regex = Regex::new(r#"^- Let `([^`]+)` be `([^`]+)`\.?$"#).expect("let call regex");
    let captures = regex.captures(line)?;
    let binding = captures.get(1)?.as_str().trim().to_string();
    let expr = captures.get(2)?.as_str().trim().to_string();
    Some((binding, expr))
}

fn parse_call(line: &str) -> Option<String> {
    let regex = Regex::new(r#"^- Call `([^`]+)`\.?$"#).expect("call regex");
    let captures = regex.captures(line)?;
    Some(captures.get(1)?.as_str().trim().to_string())
}

#[derive(Debug, Clone)]
struct ParsedCallExpression {
    receiver: String,
    method: String,
    args: Vec<String>,
    receiver_is_type: bool,
}

fn parse_call_expression(expr: &str) -> Option<ParsedCallExpression> {
    let expr = expr.trim();
    let open = expr.find('(')?;
    let close = expr.rfind(')')?;
    let head = expr[..open].trim();
    let args = split_top_level(&expr[open + 1..close], ',');
    if let Some((receiver, method)) = head.rsplit_once("::") {
        return Some(ParsedCallExpression {
            receiver: receiver.trim().to_string(),
            method: method.trim().to_string(),
            args,
            receiver_is_type: true,
        });
    }
    if let Some((receiver, method)) = head.rsplit_once('.') {
        return Some(ParsedCallExpression {
            receiver: receiver.trim().to_string(),
            method: method.trim().to_string(),
            args,
            receiver_is_type: false,
        });
    }
    None
}

fn resolve_method_for_call<'a>(
    call: &ParsedCallExpression,
    index: &'a ExportIndex,
    env: &AppFlowEnv,
) -> Option<&'a ExportMethod> {
    let receiver_export = if call.receiver_is_type {
        resolve_export_name_from_type(&call.receiver, index)?
    } else {
        let receiver_type = env.variables.get(call.receiver.as_str())?;
        resolve_export_name_from_type(receiver_type, index)?
    };
    index.by_export.get(&receiver_export)?.get(&call.method)
}

fn infer_app_arg_type(arg: &str, env: &AppFlowEnv) -> Option<String> {
    let arg = arg.trim();
    if arg.is_empty() {
        return None;
    }
    if arg.starts_with('"') && arg.ends_with('"') {
        return Some("String".to_string());
    }
    if matches!(arg, "true" | "false") {
        return Some("bool".to_string());
    }
    if arg.chars().all(|ch| ch.is_ascii_digit()) {
        return Some("i64".to_string());
    }
    if let Some(value) = env.variables.get(arg) {
        return Some(value.clone());
    }
    None
}

fn app_primary_flow_sections(app: &AppDraft) -> Vec<&'static str> {
    ["Main Flow", "Main Loop Behavior"]
        .into_iter()
        .filter(|section| {
            app.sections
                .get(*section)
                .map(|body| !body.trim().is_empty())
                .unwrap_or(false)
        })
        .collect()
}

fn extract_signature_marker(flow: &[String]) -> Option<String> {
    let regex = Regex::new(r#"Signature:\s*`([^`]+)`"#).expect("flow signature regex");
    flow.iter()
        .find_map(|line| regex.captures(line).and_then(|captures| captures.get(1)))
        .map(|value| value.as_str().trim().to_string())
}

fn extract_inline_signature(detail: &str) -> Option<String> {
    let regex = Regex::new(r#"Signature:\s*`([^`]+)`"#).expect("inline signature regex");
    regex
        .captures(detail)
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().trim().to_string())
}

fn parse_signature(
    signature: &str,
    expected_name: &str,
    export_name: &str,
) -> Result<ParsedSignature> {
    let trimmed = strip_backticks(signature);
    let Some((left, right)) = trimmed.split_once("->") else {
        anyhow::bail!("signature `{trimmed}` is missing `->`");
    };
    let left = left.trim();
    let return_type = normalize_signature_type(right.trim(), export_name);
    let Some(open) = left.find('(') else {
        anyhow::bail!("signature `{trimmed}` is missing `(`");
    };
    let Some(close) = left.rfind(')') else {
        anyhow::bail!("signature `{trimmed}` is missing `)`");
    };
    let name = left[..open].trim();
    if normalize_symbol(name) != normalize_symbol(expected_name) {
        anyhow::bail!("signature `{trimmed}` describes `{name}` but expected `{expected_name}`");
    }
    let inner = left[open + 1..close].trim();
    let mut receiver = None;
    let mut parameters = Vec::new();
    for part in split_top_level(inner, ',') {
        if part.is_empty() {
            continue;
        }
        let part = part.trim();
        if matches!(part, "&self" | "&mut self" | "self") {
            receiver = Some(part.to_string());
            continue;
        }
        let Some((name, rust)) = part.split_once(':') else {
            anyhow::bail!("signature parameter `{part}` is missing `:`");
        };
        parameters.push(SignatureParameter {
            name: name.trim().to_string(),
            rust: normalize_signature_type(rust.trim(), export_name),
        });
    }
    Ok(ParsedSignature {
        receiver,
        parameters,
        return_type,
    })
}

fn render_signature(signature: &ParsedSignature, method_name: &str, export_name: &str) -> String {
    let mut parts = Vec::new();
    if let Some(receiver) = &signature.receiver {
        parts.push(receiver.clone());
    }
    parts.extend(
        signature
            .parameters
            .iter()
            .map(|parameter| format!("{}: {}", parameter.name, parameter.rust)),
    );
    format!(
        "{}({}) -> {}",
        method_name,
        parts.join(", "),
        resolve_self_type(&signature.return_type, export_name)
    )
}

fn signatures_compatible(
    expected: &ParsedSignature,
    actual: &ParsedSignature,
    export_name: &str,
) -> bool {
    if expected.receiver != actual.receiver {
        return false;
    }
    if expected.parameters.len() != actual.parameters.len() {
        return false;
    }
    if !types_compatible(&expected.return_type, &actual.return_type, export_name) {
        return false;
    }
    expected
        .parameters
        .iter()
        .zip(&actual.parameters)
        .all(|(left, right)| {
            normalize_symbol(&left.name) == normalize_symbol(&right.name)
                && types_compatible(&left.rust, &right.rust, export_name)
        })
}

fn types_compatible(expected: &str, actual: &str, export_name: &str) -> bool {
    canonical_type(expected, export_name) == canonical_type(actual, export_name)
        || strip_reference(&canonical_type(expected, export_name))
            == strip_reference(&canonical_type(actual, export_name))
}

fn canonical_type(raw: &str, export_name: &str) -> String {
    resolve_self_type(&normalize_signature_type(raw, export_name), export_name)
}

fn normalize_signature_type(raw: &str, export_name: &str) -> String {
    let raw = raw.trim();
    if let Some(rest) = raw.strip_prefix("&mut ") {
        return format!(
            "&mut {}",
            normalize_type_notation(&resolve_self_type(rest, export_name))
        );
    }
    if let Some(rest) = raw.strip_prefix('&') {
        return format!(
            "&{}",
            normalize_type_notation(&resolve_self_type(rest, export_name))
        );
    }
    normalize_type_notation(&resolve_self_type(raw, export_name))
}

fn resolve_self_type(raw: &str, export_name: &str) -> String {
    let trimmed = raw.trim();
    if trimmed == "Self" {
        return export_name.to_string();
    }
    if trimmed == "&Self" {
        return format!("&{export_name}");
    }
    if trimmed == "&mut Self" {
        return format!("&mut {export_name}");
    }
    trimmed.to_string()
}

fn resolve_export_name_from_type(raw_type: &str, index: &ExportIndex) -> Option<String> {
    let stripped = strip_reference(raw_type);
    let simple = stripped
        .rsplit_once("::")
        .map(|(_, tail)| tail)
        .unwrap_or(stripped)
        .trim();
    let normalized = normalize_symbol(simple);
    let matches = index.normalized_exports.get(&normalized)?;
    if matches.len() == 1 {
        Some(matches[0].clone())
    } else {
        None
    }
}

fn extension_has_concrete_outcome(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if lower.contains('→') || lower.contains("->") {
        return true;
    }
    [
        "return ",
        "return.",
        "exit",
        "continue",
        "skip",
        "retry",
        "go to step",
        "resume at step",
        "call ",
        "set ",
        "store ",
        "clear ",
        "append ",
        "insert ",
        "remove ",
        "overwrite",
        "overwritten",
        "display ",
        "print ",
        "render ",
        "prompt ",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn text_contains_vague_phrase(text: &str, phrase: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let phrase = phrase.to_ascii_lowercase();
    let mut start = 0usize;
    while let Some(offset) = lower[start..].find(&phrase) {
        let idx = start + offset;
        let sentence_tail = &lower[idx..sentence_end(&lower, idx)];
        if !has_explicit_criterion(sentence_tail) {
            return true;
        }
        start = idx + phrase.len();
    }
    false
}

fn has_explicit_criterion(sentence_tail: &str) -> bool {
    [
        " if ",
        " when ",
        " because ",
        " by ",
        " based on ",
        " according to ",
        " so that ",
    ]
    .iter()
    .any(|needle| sentence_tail.contains(needle))
}

fn sentence_end(text: &str, from: usize) -> usize {
    for (offset, ch) in text[from..].char_indices() {
        if matches!(ch, '.' | '!' | '?') {
            return from + offset;
        }
    }
    text.len()
}

fn backticked_tokens(text: &str) -> Vec<String> {
    let regex = Regex::new(r"`([^`]+)`").expect("token regex");
    regex
        .captures_iter(text)
        .filter_map(|captures| captures.get(1))
        .map(|value| value.as_str().trim().to_string())
        .collect()
}

fn push_finding(
    findings: &mut Vec<DraftReviewFinding>,
    draft_path: &str,
    source_line: Option<usize>,
    category: DraftReviewCategory,
    message: String,
) {
    findings.push(DraftReviewFinding {
        draft_path: draft_path.to_string(),
        source_line,
        category,
        message,
        severity: None,
    });
}

fn draft_title(document: &DraftDocument) -> &str {
    match document {
        DraftDocument::Data(data) => &data.info.title,
        DraftDocument::Projection(projection) | DraftDocument::Context(projection) => {
            &projection.info.title
        }
        DraftDocument::App(app) => &app.info.title,
    }
}

fn heading_line(content: &str, heading: &str) -> Option<usize> {
    find_line_containing(content, heading)
}

fn table_cell_line(content: &str, needle: &str) -> Option<usize> {
    find_line_containing(content, needle)
}

fn markdown_line(content: &str, needle: &str) -> Option<usize> {
    find_line_containing(content, needle)
}

fn find_line_containing(content: &str, needle: &str) -> Option<usize> {
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

fn line_for_offset(content: &str, offset: usize) -> Option<usize> {
    let mut consumed = 0usize;
    for (idx, line) in content.lines().enumerate() {
        consumed += line.len() + 1;
        if consumed > offset {
            return Some(idx + 1);
        }
    }
    if !content.is_empty() {
        Some(content.lines().count())
    } else {
        None
    }
}

fn split_top_level(value: &str, delimiter: char) -> Vec<String> {
    let mut items = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (idx, ch) in value.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => {
                    escaped = true;
                    continue;
                }
                '"' => {
                    in_string = false;
                    continue;
                }
                _ => continue,
            }
        }
        if ch == '"' {
            in_string = true;
            continue;
        }
        match ch {
            '(' | '{' | '[' | '<' => depth += 1,
            ')' | '}' | ']' | '>' => depth -= 1,
            _ => {}
        }
        if depth == 0 && ch == delimiter {
            items.push(value[start..idx].trim().to_string());
            start = idx + ch.len_utf8();
        }
    }
    let tail = value[start..].trim();
    if !tail.is_empty() {
        items.push(tail.to_string());
    }
    items
}

fn strip_reference(value: &str) -> &str {
    value
        .trim()
        .strip_prefix("&mut ")
        .or_else(|| value.trim().strip_prefix('&'))
        .unwrap_or(value.trim())
        .trim()
}

fn strip_backticks(value: &str) -> String {
    value.trim().trim_matches('`').to_string()
}

fn normalize_symbol(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{Selection, Workspace};
    use anyhow::Result;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_workspace(name: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        // A monotonic counter combined with the clock defends against parallel test runs
        // landing on the same nanosecond — `SystemTime::now` is not unique under
        // `cargo test` because tests can start within the same tick.
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("reen_draft_refine_{name}_{stamp}_{seq}"))
    }

    fn write_file(root: &Path, relative: &str, content: &str) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(path, content).expect("write");
    }

    fn review_files(files: &[(&str, &str)]) -> Result<DraftReview> {
        let root = temp_workspace("files");
        for (relative, content) in files {
            write_file(&root, relative, content);
        }
        let workspace = Workspace::discover(root.clone())?;
        let paths = workspace.raw_draft_paths(&Selection::default())?;
        review_raw_drafts(&workspace, &paths)
    }

    #[test]
    fn vague_phrase_without_criterion_is_blocking() -> Result<()> {
        let report = review_files(&[(
            "drafts/app.md",
            r#"# Hello App

## Description

Simple app.

## Collaborators and Wiring

| Collaborator | Responsibility |
|---|---|
| `GreeterContext` | Greets. |

## Startup Sequence

- Let `greeter` be `GreeterContext::new()`.

## Main Flow

- Keep the greeting interaction running in the appropriate way for the current situation.
"#,
        )])?;

        assert!(report.findings.iter().any(|finding| {
            finding.category == DraftReviewCategory::Concreteness
                && finding.message.contains("current situation")
        }));
        Ok(())
    }

    #[test]
    fn extension_without_concrete_outcome_is_blocking() -> Result<()> {
        let report = review_files(&[(
            "drafts/contexts/Greeter.md",
            r#"# GreeterContext

## Purpose

Greets.

## Role Players

| Role player | Why involved | Expected behaviour |
|---|---|---|
| message | Holds text | Uses `Message` and provides text |

## Role Methods

### message

- **text**
  Signature: `text(&self) -> String`

## Props

| Prop | Meaning | Notes |
|---|---|---|

## Functionalities

### render

| Started by | Uses | Result |
|---|---|---|
| app | message | a greeting string |

**Flow:**
1. Signature: `render(&self) -> String`
2. Return `message.text()`.

**Extensions:**
- 2a. If the message is blank, handle it.

| Given | When | Then |
|---|---|---|
| a message exists | render is called | text is returned |
"#,
        )])?;

        assert!(report.findings.iter().any(|finding| {
            finding.category == DraftReviewCategory::Completeness
                && finding.message.contains("must state a concrete outcome")
        }));
        Ok(())
    }

    #[test]
    fn empty_result_cell_is_blocking() -> Result<()> {
        let report = review_files(&[(
            "drafts/contexts/Greeter.md",
            r#"# GreeterContext

## Purpose

Greets.

## Role Players

| Role player | Why involved | Expected behaviour |
|---|---|---|
| message | Holds text | Uses `Message` and provides text |

## Role Methods

### message

- **text**
  Signature: `text(&self) -> String`

## Props

| Prop | Meaning | Notes |
|---|---|---|

## Functionalities

### render

| Started by | Uses | Result |
|---|---|---|
| app | message | |

**Flow:**
1. Signature: `render(&self) -> String`
2. Return `message.text()`.

| Given | When | Then |
|---|---|---|
| a message exists | render is called | text is returned |
"#,
        )])?;

        assert!(report.findings.iter().any(|finding| {
            finding.category == DraftReviewCategory::Completeness
                && finding.message.contains("Started by")
        }));
        Ok(())
    }

    #[test]
    fn ambiguous_backticked_export_name_is_blocking() -> Result<()> {
        let report = review_files(&[
            (
                "drafts/data/Board.md",
                r#"# Board

## Description

Primary board.

## Fields

| Field | Meaning | Notes |
|---|---|---|
| width | Width | Type is `u32`. |
"#,
            ),
            (
                "drafts/data/BoardBang.md",
                r#"# Board!

## Description

Secondary board.

## Fields

| Field | Meaning | Notes |
|---|---|---|
| width | Width | Type is `u32`. |
"#,
            ),
            (
                "drafts/app.md",
                r#"# Board App

## Description

App.

## Collaborators and Wiring

| Collaborator | Responsibility |
|---|---|
| `Board` | Used by the app. |

## Startup Sequence

- Let `board` be `Board::new()`.

## Main Flow

- Call `println!("{:?}", board)`.
"#,
            ),
        ])?;

        assert!(report.findings.iter().any(|finding| {
            finding.category == DraftReviewCategory::Ambiguity
                && finding
                    .message
                    .contains("ambiguous across selected exports")
        }));
        Ok(())
    }

    #[test]
    fn app_flow_type_mismatch_is_blocking() -> Result<()> {
        let report = review_files(&[
            (
                "drafts/app.md",
                r#"# Snake App

## Description

App.

## Collaborators and Wiring

| Collaborator | Responsibility |
|---|---|
| `GameLoop` | Produces board picture. |
| `Renderer` | Renders board picture. |

## Startup Sequence

- Let `game_loop` be `GameLoop::new()`.
- Let `renderer` be `Renderer::new()`.

## Main Flow

- Let `board_picture` be `game_loop.current_board()`.
- Let `rendered` be `renderer.render(board_picture)`.
"#,
            ),
            (
                "drafts/data/GameLoop.md",
                r#"# GameLoop

## Description

Loop.

## Fields

| Field | Meaning | Notes |
|---|---|---|
| tick | Tick | Type is `u32`. |

## Functionalities

| Method | Kind | Signature | Behavior |
|---|---|---|---|
| `new` | Constructor | `new() -> Self` | Creates a game loop. |
| `current_board` | Instance method | `current_board(&self) -> std::collections::HashMap<Position, char>` | Returns the board picture. |
"#,
            ),
            (
                "drafts/data/Renderer.md",
                r#"# Renderer

## Description

Renderer.

## Fields

| Field | Meaning | Notes |
|---|---|---|
| theme | Theme | Type is `String`. |

## Functionalities

| Method | Kind | Signature | Behavior |
|---|---|---|---|
| `new` | Constructor | `new() -> Self` | Creates a renderer. |
| `render` | Instance method | `render(&self, board: Board) -> String` | Returns rendered text. |
"#,
            ),
        ])?;

        assert!(report.findings.iter().any(|finding| {
            finding.category == DraftReviewCategory::Ambiguity
                && finding.message.contains("explicitly expects `Board`")
        }));
        Ok(())
    }

    #[test]
    fn app_main_loop_behavior_counts_as_primary_flow() -> Result<()> {
        let report = review_files(&[(
            "drafts/app.md",
            r#"# Snake App

## Application Kind
`cli_app`

## Description

App.

## Collaborators and Wiring

| Collaborator | Responsibility |
|---|---|
| `GameLoop` | Advances play. |

## Startup Sequence

- Let `game_loop` be `GameLoop::new()`.

## Main Loop Behavior

- Call `game_loop.tick()`.
"#,
        )])?;

        assert!(
            !report.findings.iter().any(|finding| {
                finding.category == DraftReviewCategory::Completeness
                    && finding.message.contains("Main Flow")
            }),
            "{:?}",
            report.findings
        );
        Ok(())
    }

    fn finding(
        path: &str,
        line: Option<usize>,
        category: DraftReviewCategory,
        message: &str,
        severity: Option<u8>,
    ) -> DraftReviewFinding {
        DraftReviewFinding {
            draft_path: path.to_string(),
            source_line: line,
            category,
            message: message.to_string(),
            severity,
        }
    }

    #[test]
    fn sort_and_dedup_orders_behavioral_ambiguity_after_other_categories() {
        // Within the same (path, line) tuple, deterministic categories should print before
        // the LLM-sourced behavioral-ambiguity category. This protects the "fix structural
        // gaps first, then answer questions" flow from being re-ordered by accident.
        let mut findings = vec![
            finding(
                "drafts/data/Position.md",
                Some(12),
                DraftReviewCategory::BehavioralAmbiguity,
                "Does y=0 represent the top?",
                Some(85),
            ),
            finding(
                "drafts/data/Position.md",
                Some(12),
                DraftReviewCategory::Ambiguity,
                "ambiguous reference",
                None,
            ),
            finding(
                "drafts/data/Position.md",
                Some(12),
                DraftReviewCategory::Completeness,
                "missing section",
                None,
            ),
        ];
        sort_and_dedup_findings(&mut findings);
        let categories: Vec<_> = findings.iter().map(|f| f.category).collect();
        assert_eq!(
            categories,
            vec![
                DraftReviewCategory::Completeness,
                DraftReviewCategory::Ambiguity,
                DraftReviewCategory::BehavioralAmbiguity,
            ]
        );
    }

    #[test]
    fn sort_and_dedup_collapses_identical_behavioral_findings_regardless_of_severity() {
        // Two runs can return the same question with slightly different severities. They must
        // still dedupe to a single finding so the report doesn't drown in near-duplicates.
        let mut findings = vec![
            finding(
                "drafts/data/Position.md",
                Some(12),
                DraftReviewCategory::BehavioralAmbiguity,
                "Does y=0 represent the top?",
                Some(85),
            ),
            finding(
                "drafts/data/Position.md",
                Some(12),
                DraftReviewCategory::BehavioralAmbiguity,
                "Does y=0 represent the top?",
                Some(90),
            ),
        ];
        sort_and_dedup_findings(&mut findings);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn the_right_side_is_not_flagged_as_vague() -> Result<()> {
        // Regression: "the right" as a spatial direction (opposite of left) must not be
        // treated as vague hand-waving. Only the explicitly vague completions like "the right
        // thing" / "the right way" are flagged.
        let report = review_files(&[(
            "drafts/app.md",
            r#"# Border App

## Description

App.

## Collaborators and Wiring

| Collaborator | Responsibility |
|---|---|
| `Frame` | Draws the frame. |

## Startup Sequence

- Let `frame` be `Frame::new()`.

## Main Flow

- Clear leftover characters from the right side of every game-over row before drawing.
- Place the score indicator on the right half of the status bar.
"#,
        )])?;
        assert!(
            !report.findings.iter().any(|finding| {
                finding.category == DraftReviewCategory::Concreteness
                    && finding.message.contains("the right")
            }),
            "`the right side`/`the right half` must not trigger the vague-phrase check: {:?}",
            report.findings
        );
        Ok(())
    }

    #[test]
    fn the_right_thing_is_still_flagged_as_vague() -> Result<()> {
        // Keep catching the *actually* vague completions. "the right thing" gives an
        // implementer no guidance on what concrete behavior to produce.
        let report = review_files(&[(
            "drafts/app.md",
            r#"# Vague App

## Description

App.

## Collaborators and Wiring

| Collaborator | Responsibility |
|---|---|
| `Handler` | Handles things. |

## Startup Sequence

- Let `handler` be `Handler::new()`.

## Main Flow

- Always do the right thing.
"#,
        )])?;
        assert!(
            report.findings.iter().any(|finding| {
                finding.category == DraftReviewCategory::Concreteness
                    && finding.message.contains("the right thing")
            }),
            "`the right thing` must still trigger the vague-phrase check: {:?}",
            report.findings
        );
        Ok(())
    }

    #[test]
    fn behavioral_ambiguity_category_prints_with_human_label() {
        assert_eq!(
            DraftReviewCategory::BehavioralAmbiguity.as_str(),
            "behavioral ambiguity"
        );
    }
}
