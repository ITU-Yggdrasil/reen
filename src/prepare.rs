use crate::build_tracker::{BuildTracker, hash_file};
use crate::draft_parser::{
    AppDraft, CompositeDraft, DataDraft, DraftDocument, FunctionalityDraft, RoleMethodGroup,
    parse_draft_file,
};
use crate::draft_refine::{DraftReview, DraftReviewFinding, review_raw_drafts};
use crate::fix_agent;
use crate::prepared::{
    Ambiguity, Body, CollaboratorSpec, ConstructorPolicy, Evidence, ExportInfo, Expression,
    FieldSpec, GetterSpec, MethodReferences, MethodSpec, ParameterSpec, PreparedArtifact, PropSpec,
    RoleSpec, SourceInfo, Statement, StructFieldValue, ValueStatus, VariantSpec,
};
use crate::prepared_contracts::collect_contract_issues;
use crate::workspace::Workspace;
use anyhow::{Context, Result};
use regex::Regex;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct PrepareOptions {
    pub selection: crate::workspace::Selection,
    pub profile: Option<String>,
    pub fix: bool,
    pub verbose: bool,
    pub debug: bool,
    pub dry_run: bool,
}

#[derive(Debug, Clone)]
pub struct RefineOptions {
    pub selection: crate::workspace::Selection,
    pub verbose: bool,
    pub drafts_only: bool,
    pub prepared_only: bool,
    /// Minimum behavioral-ambiguity severity (0..=100) the LLM review must hit before a
    /// question is surfaced as a finding. Defaults to
    /// [`crate::draft_refine_llm::DEFAULT_MIN_SEVERITY`].
    pub min_behavioral_severity: u8,
    /// Skip the LLM-backed behavioral review entirely.
    pub skip_llm_review: bool,
    /// Treat LLM unavailability (missing API key, network failure, malformed response) as a
    /// hard error rather than silently skipping the behavioral phase.
    pub require_llm_review: bool,
}

impl Default for RefineOptions {
    fn default() -> Self {
        Self {
            selection: crate::workspace::Selection::default(),
            verbose: false,
            drafts_only: false,
            prepared_only: false,
            min_behavioral_severity: crate::draft_refine_llm::DEFAULT_MIN_SEVERITY,
            skip_llm_review: false,
            require_llm_review: false,
        }
    }
}

#[derive(Debug, Clone)]
struct DraftCatalog {
    exports: BTreeSet<String>,
    normalized: BTreeMap<String, Vec<String>>,
    role_hints: BTreeMap<String, Vec<String>>,
    /// Resolved Rust types for roles and props, loaded from previously-prepared YAML artifacts.
    ///
    /// Keyed by the sanitized role/prop *name* (not the owning artifact). Used as a first-class
    /// source when resolving a role or collaborator with the same name in another draft, so that
    /// a fixed type in `game_loop.md` propagates to `app.md` without requiring a second prepare
    /// pass.
    resolved_role_types: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default)]
struct BodyParseContext {
    /// Maps role field name → set of method names declared on that role.
    role_methods: BTreeMap<String, BTreeSet<String>>,
    /// Role field names declared on the context (keys of `role_methods`, kept separately for
    /// clarity and so role-only contexts with no role methods still work).
    role_names: BTreeSet<String>,
    /// Prop names declared on the context.
    prop_names: BTreeSet<String>,
}

#[derive(Debug, Default, Deserialize)]
struct TypesManifest {
    #[serde(default)]
    primitives: Vec<String>,
    #[serde(default)]
    external_path_prefixes: Vec<String>,
    #[serde(default)]
    allowlists: ManifestAllowlists,
}

#[derive(Debug, Default, Deserialize)]
struct ManifestAllowlists {
    #[serde(default)]
    data: Vec<String>,
    #[serde(default)]
    projection: Vec<String>,
    #[serde(default)]
    context: Vec<String>,
}

pub fn prepare_workspace(workspace: &Workspace, options: &PrepareOptions) -> Result<()> {
    let selected_paths = workspace.raw_draft_paths(&options.selection)?;
    let all_docs = load_supported_documents(workspace)?;
    let resolved_role_types = load_resolved_role_types(workspace);
    let catalog = build_catalog(&all_docs, resolved_role_types);
    let types_manifest = load_types_manifest(workspace)?;
    let mut tracker = BuildTracker::load(&workspace.root)?;
    let mut wrote = 0usize;
    let mut blocking_total = 0usize;

    if options.verbose {
        println!("Preparing {} draft(s)", selected_paths.len());
    }
    if options.verbose {
        if let Some(profile) = &options.profile {
            println!("Profile `{profile}` is reserved; v2 prepare runs deterministically.");
        }
    }

    for path in &selected_paths {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let draft = parse_draft_file(path, &workspace.drafts_dir, &content)?;
        let output_path = workspace.prepared_output_path(path)?;
        let input_hash = hash_file(path)?;
        let track_key = format!("prepare:{}", draft.info().relative_path());
        if tracker.is_current("prepare", &track_key, &input_hash) && output_path.exists() {
            if options.verbose {
                println!("skip {}", draft.info().relative_path());
            }
            continue;
        }
        let mut prepared = prepare_document(&draft, &catalog)?;
        prepared.propagate_resolved_types();
        prepared.refresh_ambiguity_index();
        enrich_ambiguity_lines(&mut prepared, &content);
        let has_blockers = prepared.blocking_ambiguities().next().is_some();

        if has_blockers && options.fix {
            let available_types =
                available_types_for_draft(types_manifest.as_ref(), draft.info().kind, &catalog);
            let dependency_notes =
                crate::build_agent::render_dependency_context(workspace).unwrap_or_default();
            let fixed_count = fix_agent::fix_ambiguities_with_dependency_notes(
                &content,
                &available_types,
                &dependency_notes,
                &mut prepared,
                options.verbose,
            )?;
            if fixed_count > 0 {
                prepared.propagate_resolved_types();
                prepared.refresh_ambiguity_index();
                let added = ensure_external_dependencies_from_prepared(workspace, &prepared)?;
                if options.verbose {
                    eprintln!(
                        "fix-agent: applied {} fix(es) for {}",
                        fixed_count,
                        draft.info().relative_path()
                    );
                    if added > 0 {
                        eprintln!(
                            "fix-agent: auto-registered {added} external crate(s) in drafts manifests"
                        );
                    }
                }
            }
        }

        let blockers: Vec<_> = prepared.blocking_ambiguities().collect();
        if !blockers.is_empty() {
            blocking_total += blockers.len();
            let draft_path = path.strip_prefix(&workspace.root).unwrap_or(path);
            for ambiguity in blockers {
                let location = match ambiguity.source_line {
                    Some(line) => format!("{}:{}", draft_path.display(), line),
                    None => format!("{}", draft_path.display()),
                };
                eprintln!("error[prepare]: {}", ambiguity.message);
                eprintln!("  --> {} ({})", location, ambiguity.path);
            }
        }
        let yaml = serde_yaml::to_string(&prepared)?;
        if options.dry_run {
            if options.verbose {
                println!("[dry-run] would write {}", output_path.display());
            }
        } else {
            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create {}", parent.display()))?;
            }
            fs::write(&output_path, yaml)
                .with_context(|| format!("Failed to write {}", output_path.display()))?;
            tracker.update("prepare", track_key, input_hash);
            wrote += 1;
        }

        if options.debug && !options.dry_run {
            write_prepare_debug_dump(workspace, &prepared)?;
        }
    }

    if !options.dry_run {
        tracker.save(&workspace.root)?;
    }

    if !options.dry_run && blocking_total == 0 {
        let patched = apply_cross_artifact_derive_inference(workspace)?;
        if patched > 0 && options.verbose {
            println!("Updated derives on {patched} prepared artifact(s)");
        }
        let synced = apply_cross_artifact_role_method_sync(workspace)?;
        if synced > 0 && options.verbose {
            println!("Synced role-method mutability on {synced} prepared artifact(s)");
        }
    }

    if options.verbose {
        println!("Prepared {} artifact(s)", wrote);
    }
    if blocking_total > 0 {
        anyhow::bail!(
            "prepare found {} blocking ambiguit{} across draft(s); see errors above",
            blocking_total,
            if blocking_total == 1 { "y" } else { "ies" }
        );
    }
    Ok(())
}

pub fn refine_workspace(workspace: &Workspace, options: &RefineOptions) -> Result<()> {
    if options.verbose {
        println!(
            "Refine options: min-severity={} skip-llm-review={} require-llm-review={}",
            options.min_behavioral_severity,
            options.skip_llm_review,
            options.require_llm_review,
        );
    }
    let mut draft_phase = DraftPhaseReport::Skipped {
        reason: "skipped by `--prepared-only`".to_string(),
    };
    let prepared_phase;

    if options.prepared_only {
        let prepared_paths = workspace.prepared_paths(&options.selection)?;
        if options.verbose {
            println!("Refining {} prepared artifact(s)", prepared_paths.len());
        }
        let summary = review_prepared_paths(workspace, &prepared_paths)?;
        print_prepared_findings(&summary);
        let has_failures = summary.blocking_ambiguities > 0 || summary.contract_mismatches > 0;
        prepared_phase = PreparedPhaseReport::Completed(summary);
        write_refine_report(workspace, &draft_phase, &prepared_phase)?;
        if has_failures {
            let PreparedPhaseReport::Completed(summary) = &prepared_phase else {
                unreachable!()
            };
            anyhow::bail!(
                "refine found {} blocking ambiguit{} and {} contract mismatch{} across prepared artifact(s); see errors above",
                summary.blocking_ambiguities,
                if summary.blocking_ambiguities == 1 {
                    "y"
                } else {
                    "ies"
                },
                summary.contract_mismatches,
                if summary.contract_mismatches == 1 {
                    ""
                } else {
                    "es"
                }
            );
        }
        if options.verbose {
            println!("Refine found no blocking ambiguities or contract mismatches");
        }
        return Ok(());
    }

    let raw_paths = workspace.raw_draft_paths(&options.selection)?;
    if options.verbose {
        println!("Reviewing {} raw draft(s)", raw_paths.len());
    }
    let mut draft_review = review_raw_drafts(workspace, &raw_paths)?;

    // Always run the behavioral phase alongside the deterministic one so the user sees every
    // ambiguity in a single pass. Short-circuiting when deterministic findings exist meant
    // semantic gaps stayed hidden behind a single "vague phrase" hit and forced multiple
    // fix/rerun rounds. The cache keeps repeat runs free when drafts haven't changed.
    if !options.skip_llm_review {
        let behavioral_options = crate::draft_refine_llm::BehavioralReviewOptions {
            min_severity: options.min_behavioral_severity,
            require_llm: options.require_llm_review,
            cache_dir: workspace.state_dir.join("cache").join("refine-llm"),
            verbose: options.verbose,
        };
        let behavioral_findings = crate::draft_refine_llm::review_raw_drafts_behavioral(
            workspace,
            &raw_paths,
            &behavioral_options,
        )?;
        if !behavioral_findings.is_empty() {
            draft_review.findings.extend(behavioral_findings);
            crate::draft_refine::sort_and_dedup_findings(&mut draft_review.findings);
        }
    }

    print_draft_findings(&draft_review);
    let draft_failures = draft_review.findings.len();
    draft_phase = DraftPhaseReport::Completed(draft_review);

    if draft_failures > 0 {
        prepared_phase = PreparedPhaseReport::Skipped {
            reason: "skipped because draft review found blocking issues".to_string(),
        };
        write_refine_report(workspace, &draft_phase, &prepared_phase)?;
        anyhow::bail!(
            "refine found {} blocking draft review finding{}; see errors above",
            draft_failures,
            if draft_failures == 1 { "" } else { "s" }
        );
    }

    if options.drafts_only {
        prepared_phase = PreparedPhaseReport::Skipped {
            reason: "skipped by `--drafts-only`".to_string(),
        };
        write_refine_report(workspace, &draft_phase, &prepared_phase)?;
        if options.verbose {
            println!("Draft review found no blocking issues");
        }
        return Ok(());
    }

    let prepared_paths = workspace.matching_prepared_paths_for_raw(&raw_paths)?;
    if prepared_paths.is_empty() {
        prepared_phase = PreparedPhaseReport::Skipped {
            reason: "skipped because no matching prepared artifacts exist".to_string(),
        };
        write_refine_report(workspace, &draft_phase, &prepared_phase)?;
        if options.verbose {
            println!("Draft review found no blocking issues; prepared review skipped");
        }
        return Ok(());
    }

    if options.verbose {
        println!(
            "Refining {} matching prepared artifact(s)",
            prepared_paths.len()
        );
    }
    let summary = review_prepared_paths(workspace, &prepared_paths)?;
    print_prepared_findings(&summary);
    let has_failures = summary.blocking_ambiguities > 0 || summary.contract_mismatches > 0;
    prepared_phase = PreparedPhaseReport::Completed(summary);
    write_refine_report(workspace, &draft_phase, &prepared_phase)?;

    if has_failures {
        let PreparedPhaseReport::Completed(summary) = &prepared_phase else {
            unreachable!()
        };
        anyhow::bail!(
            "refine found {} blocking ambiguit{} and {} contract mismatch{} across prepared artifact(s); see errors above",
            summary.blocking_ambiguities,
            if summary.blocking_ambiguities == 1 {
                "y"
            } else {
                "ies"
            },
            summary.contract_mismatches,
            if summary.contract_mismatches == 1 {
                ""
            } else {
                "es"
            }
        );
    }

    if options.verbose {
        println!("Refine found no blocking draft issues, ambiguities, or contract mismatches");
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct PreparedReviewFinding {
    location_path: String,
    location_line: Option<usize>,
    detail_path: String,
    message: String,
}

#[derive(Debug, Clone, Default)]
struct PreparedReviewSummary {
    reviewed_paths: Vec<String>,
    findings: Vec<PreparedReviewFinding>,
    blocking_ambiguities: usize,
    contract_mismatches: usize,
}

#[derive(Debug, Clone)]
enum DraftPhaseReport {
    Completed(DraftReview),
    Skipped { reason: String },
}

#[derive(Debug, Clone)]
enum PreparedPhaseReport {
    Completed(PreparedReviewSummary),
    Skipped { reason: String },
}

fn review_prepared_paths(
    workspace: &Workspace,
    prepared_paths: &[PathBuf],
) -> Result<PreparedReviewSummary> {
    let mut summary = PreparedReviewSummary {
        reviewed_paths: prepared_paths
            .iter()
            .map(|path| {
                path.strip_prefix(&workspace.root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect(),
        ..PreparedReviewSummary::default()
    };
    let mut artifacts = Vec::new();

    for path in prepared_paths {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let mut prepared: PreparedArtifact = serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse prepared artifact {}", path.display()))?;
        prepared.refresh_ambiguity_index();
        let source_path = workspace.root.join(&prepared.source.path);
        if source_path.is_file() {
            let source = fs::read_to_string(&source_path)
                .with_context(|| format!("Failed to read {}", source_path.display()))?;
            enrich_ambiguity_lines(&mut prepared, &source);
        }
        prepared.validate()?;

        let blockers = prepared.blocking_ambiguities().cloned().collect::<Vec<_>>();
        summary.blocking_ambiguities += blockers.len();
        summary
            .findings
            .extend(blockers.into_iter().map(|ambiguity| PreparedReviewFinding {
                location_path: prepared.source.path.clone(),
                location_line: ambiguity.source_line,
                detail_path: ambiguity.path,
                message: ambiguity.message,
            }));

        artifacts.push(prepared);
    }

    let contract_issues = collect_contract_issues(&artifacts);
    summary.contract_mismatches = contract_issues.len();
    for issue in contract_issues {
        let prepared = &artifacts[issue.artifact_index];
        summary.findings.push(PreparedReviewFinding {
            location_path: prepared.source.path.clone(),
            location_line: issue.ambiguity.source_line,
            detail_path: issue.ambiguity.path,
            message: issue.ambiguity.message,
        });
    }

    Ok(summary)
}

fn print_draft_findings(review: &DraftReview) {
    for finding in &review.findings {
        let location = match finding.source_line {
            Some(line) => format!("{}:{}", finding.draft_path, line),
            None => finding.draft_path.clone(),
        };
        match finding.severity {
            Some(severity) => eprintln!(
                "error[refine]: [severity {}] {}",
                severity, finding.message
            ),
            None => eprintln!("error[refine]: {}", finding.message),
        }
        eprintln!("  --> {} ({})", location, finding.category.as_str());
    }
}

fn print_prepared_findings(summary: &PreparedReviewSummary) {
    for finding in &summary.findings {
        let location = match finding.location_line {
            Some(line) => format!("{}:{}", finding.location_path, line),
            None => finding.location_path.clone(),
        };
        eprintln!("error[refine]: {}", finding.message);
        eprintln!("  --> {} ({})", location, finding.detail_path);
    }
}

fn write_refine_report(
    workspace: &Workspace,
    draft_phase: &DraftPhaseReport,
    prepared_phase: &PreparedPhaseReport,
) -> Result<()> {
    let path = workspace.refine_report_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let report = render_refine_report(draft_phase, prepared_phase);
    fs::write(&path, report).with_context(|| format!("Failed to write {}", path.display()))
}

fn render_refine_report(
    draft_phase: &DraftPhaseReport,
    prepared_phase: &PreparedPhaseReport,
) -> String {
    let mut out = String::new();
    out.push_str("# Reen Refine Report\n\n");
    out.push_str("## Summary\n\n");
    match draft_phase {
        DraftPhaseReport::Completed(review) => {
            out.push_str(&format!(
                "- Draft review: {} draft(s), {} blocking finding(s)\n",
                review.reviewed_paths.len(),
                review.findings.len()
            ));
        }
        DraftPhaseReport::Skipped { reason } => {
            out.push_str(&format!("- Draft review: skipped ({reason})\n"));
        }
    }
    match prepared_phase {
        PreparedPhaseReport::Completed(summary) => {
            out.push_str(&format!(
                "- Prepared review: {} artifact(s), {} blocking ambiguities, {} contract mismatches\n",
                summary.reviewed_paths.len(),
                summary.blocking_ambiguities,
                summary.contract_mismatches
            ));
        }
        PreparedPhaseReport::Skipped { reason } => {
            out.push_str(&format!("- Prepared review: skipped ({reason})\n"));
        }
    }
    out.push_str("\n## Draft Review\n\n");
    match draft_phase {
        DraftPhaseReport::Completed(review) => {
            if review.findings.is_empty() {
                out.push_str("No blocking draft-review findings.\n");
            } else {
                let mut grouped = BTreeMap::<&str, Vec<&DraftReviewFinding>>::new();
                for finding in &review.findings {
                    grouped
                        .entry(finding.draft_path.as_str())
                        .or_default()
                        .push(finding);
                }
                for path in &review.reviewed_paths {
                    out.push_str(&format!("\n### {path}\n\n"));
                    if let Some(items) = grouped.get(path.as_str()) {
                        for finding in items {
                            let severity_suffix = finding
                                .severity
                                .map(|value| format!(" [severity {value}]"))
                                .unwrap_or_default();
                            match finding.source_line {
                                Some(line) => out.push_str(&format!(
                                    "- Line {} [{}]{} {}\n",
                                    line,
                                    finding.category.as_str(),
                                    severity_suffix,
                                    finding.message
                                )),
                                None => out.push_str(&format!(
                                    "- [{}]{} {}\n",
                                    finding.category.as_str(),
                                    severity_suffix,
                                    finding.message
                                )),
                            }
                        }
                    } else {
                        out.push_str("- No blocking findings.\n");
                    }
                }
            }
        }
        DraftPhaseReport::Skipped { reason } => {
            out.push_str(&format!("Skipped: {reason}\n"));
        }
    }

    out.push_str("\n## Prepared Review\n\n");
    match prepared_phase {
        PreparedPhaseReport::Completed(summary) => {
            if summary.findings.is_empty() {
                out.push_str("No blocking prepared-review findings.\n");
            } else {
                let mut grouped = BTreeMap::<&str, Vec<&PreparedReviewFinding>>::new();
                for finding in &summary.findings {
                    grouped
                        .entry(finding.location_path.as_str())
                        .or_default()
                        .push(finding);
                }
                for path in &summary.reviewed_paths {
                    out.push_str(&format!("\n### {path}\n\n"));
                    if let Some(items) = grouped.get(path.as_str()) {
                        for finding in items {
                            match finding.location_line {
                                Some(line) => out.push_str(&format!(
                                    "- Line {} [{}] {}\n",
                                    line, finding.detail_path, finding.message
                                )),
                                None => out.push_str(&format!(
                                    "- [{}] {}\n",
                                    finding.detail_path, finding.message
                                )),
                            }
                        }
                    } else {
                        out.push_str("- No blocking findings.\n");
                    }
                }
            }
        }
        PreparedPhaseReport::Skipped { reason } => {
            out.push_str(&format!("Skipped: {reason}\n"));
        }
    }
    out
}

fn load_types_manifest(workspace: &Workspace) -> Result<Option<TypesManifest>> {
    let path = workspace.drafts_dir.join("types-manifest.yml");
    if !path.is_file() {
        return Ok(None);
    }
    let raw =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    let manifest = serde_yaml::from_str(&raw)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(Some(manifest))
}

fn available_types_for_draft(
    manifest: Option<&TypesManifest>,
    kind: crate::draft_parser::ArtifactKind,
    catalog: &DraftCatalog,
) -> Vec<String> {
    let mut types = BTreeSet::new();
    types.extend(catalog.exports.iter().cloned());

    let Some(manifest) = manifest else {
        return types.into_iter().collect();
    };

    types.extend(manifest.primitives.iter().cloned());
    types.extend(manifest.external_path_prefixes.iter().cloned());
    for prefix in &manifest.external_path_prefixes {
        types.extend(
            builtin_external_type_candidates(prefix)
                .iter()
                .map(|value| value.to_string()),
        );
    }

    match kind {
        crate::draft_parser::ArtifactKind::Data => {
            types.extend(manifest.allowlists.data.iter().cloned());
        }
        crate::draft_parser::ArtifactKind::Projection => {
            types.extend(manifest.allowlists.projection.iter().cloned());
        }
        crate::draft_parser::ArtifactKind::Context => {
            types.extend(manifest.allowlists.context.iter().cloned());
        }
        crate::draft_parser::ArtifactKind::App => {
            types.extend(manifest.allowlists.data.iter().cloned());
            types.extend(manifest.allowlists.projection.iter().cloned());
            types.extend(manifest.allowlists.context.iter().cloned());
        }
        crate::draft_parser::ArtifactKind::UnsupportedApi => {}
    }

    types.into_iter().collect()
}

fn builtin_external_type_candidates(prefix: &str) -> &'static [&'static str] {
    match prefix {
        "rand::" => &[
            "rand::rngs::ThreadRng",
            "rand::rngs::StdRng",
            "rand::rngs::SmallRng",
        ],
        _ => &[],
    }
}

fn enrich_ambiguity_lines(prepared: &mut PreparedArtifact, source: &str) {
    let lines: Vec<&str> = source.lines().collect();
    for ambiguity in &mut prepared.ambiguities {
        if ambiguity.source_line.is_some() {
            continue;
        }
        ambiguity.source_line = guess_ambiguity_line(&ambiguity.path, &ambiguity.message, &lines);
    }
}

fn guess_ambiguity_line(path: &str, message: &str, lines: &[&str]) -> Option<usize> {
    let needle = extract_ambiguity_subject(path, message)?;
    for (idx, line) in lines.iter().enumerate() {
        let lower = line.to_ascii_lowercase();
        if lower.contains(&needle.to_ascii_lowercase()) {
            return Some(idx + 1);
        }
    }
    None
}

fn extract_ambiguity_subject(path: &str, message: &str) -> Option<String> {
    let backtick_re = Regex::new(r"`([^`]+)`").expect("backtick regex");
    if let Some(cap) = backtick_re.captures(message) {
        return Some(cap.get(1).unwrap().as_str().to_string());
    }
    let last_segment = path.rsplit('.').next().unwrap_or(path);
    if last_segment != "type" && last_segment != "body" && last_segment != "signature" {
        return Some(last_segment.to_string());
    }
    None
}

pub fn clear_prepared_outputs(workspace: &Workspace, dry_run: bool) -> Result<()> {
    if dry_run {
        println!(
            "[dry-run] would remove {}",
            workspace.prepared_dir.display()
        );
    } else if workspace.prepared_dir.exists() {
        fs::remove_dir_all(&workspace.prepared_dir)
            .with_context(|| format!("Failed to remove {}", workspace.prepared_dir.display()))?;
    }
    let mut tracker = BuildTracker::load(&workspace.root)?;
    tracker.clear_stage("prepare");
    if !dry_run {
        tracker.save(&workspace.root)?;
    }
    Ok(())
}

fn write_prepare_debug_dump(workspace: &Workspace, prepared: &PreparedArtifact) -> Result<()> {
    let path = workspace
        .state_dir
        .join("debug")
        .join("prepare")
        .join(&prepared.source.path)
        .with_extension("yml");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let yaml = serde_yaml::to_string(prepared)?;
    fs::write(&path, yaml).with_context(|| format!("Failed to write {}", path.display()))
}

fn load_supported_documents(workspace: &Workspace) -> Result<Vec<DraftDocument>> {
    let mut paths = Vec::new();
    for kind_dir in ["data", "projections", "contexts"] {
        let dir = workspace.drafts_dir.join(kind_dir);
        if dir.is_dir() {
            collect_markdown_files(&dir, &mut paths)?;
        }
    }
    let app = workspace.drafts_dir.join("app.md");
    if app.is_file() {
        paths.push(app);
    }
    paths.sort();

    let mut docs = Vec::new();
    for path in paths {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        docs.push(parse_draft_file(&path, &workspace.drafts_dir, &content)?);
    }
    Ok(docs)
}

fn collect_markdown_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_markdown_files(&path, out)?;
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) == Some("md") {
            out.push(path);
        }
    }
    Ok(())
}

fn build_catalog(
    docs: &[DraftDocument],
    resolved_role_types: BTreeMap<String, String>,
) -> DraftCatalog {
    let mut exports = BTreeSet::new();
    let mut normalized = BTreeMap::<String, Vec<String>>::new();
    let mut role_hints = BTreeMap::<String, Vec<String>>::new();
    for doc in docs {
        let export = export_name(doc.info().title());
        exports.insert(export.clone());
        normalized
            .entry(normalize_symbol(&export))
            .or_default()
            .push(export.clone());
        normalized
            .entry(normalize_symbol(doc.info().title()))
            .or_default()
            .push(export.clone());
        normalized
            .entry(normalize_symbol(doc.info().stem()))
            .or_default()
            .push(export.clone());
        if let DraftDocument::Projection(draft) | DraftDocument::Context(draft) = doc {
            for role in &draft.roles {
                let entry = role_hints
                    .entry(sanitize_identifier(&role.name))
                    .or_default();
                if !role.why_involved.trim().is_empty() {
                    entry.push(role.why_involved.trim().to_string());
                }
                if !role.expected_behavior.trim().is_empty() {
                    entry.push(role.expected_behavior.trim().to_string());
                }
            }
        }
    }
    for values in normalized.values_mut() {
        values.sort();
        values.dedup();
    }
    for values in role_hints.values_mut() {
        values.sort();
        values.dedup();
    }
    DraftCatalog {
        exports,
        normalized,
        role_hints,
        resolved_role_types,
    }
}

/// Load previously-prepared YAML artifacts under `workspace/drafts/prepare/` and build a map of
/// resolved Rust types for each role and prop, keyed by sanitized name.
///
/// When the same role/prop name appears in multiple prepared artifacts with different resolved
/// types, the first one encountered wins; a subsequent same-name entry is skipped (because we
/// would not know which one to prefer without more context).
fn load_resolved_role_types(workspace: &Workspace) -> BTreeMap<String, String> {
    let mut out = BTreeMap::<String, String>::new();
    if !workspace.prepared_dir.is_dir() {
        return out;
    }
    let mut paths = Vec::new();
    if walk_yaml(&workspace.prepared_dir, &mut paths).is_err() {
        return out;
    }
    for path in paths {
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(artifact) = serde_yaml::from_str::<PreparedArtifact>(&raw) else {
            continue;
        };
        for role in &artifact.roles {
            if let Some(rust) = role.type_status.rust() {
                out.entry(role.name.clone())
                    .or_insert_with(|| rust.to_string());
            }
        }
        for prop in &artifact.props {
            if let Some(rust) = prop.type_status.rust() {
                out.entry(prop.name.clone())
                    .or_insert_with(|| rust.to_string());
            }
        }
        for collab in &artifact.collaborators {
            if let Some(rust) = collab.type_status.rust() {
                out.entry(collab.name.clone())
                    .or_insert_with(|| rust.to_string());
            }
        }
    }
    out
}

fn prepare_document(draft: &DraftDocument, catalog: &DraftCatalog) -> Result<PreparedArtifact> {
    match draft {
        DraftDocument::Data(data) => prepare_data(data, catalog),
        DraftDocument::Projection(projection) => prepare_composite(projection, catalog),
        DraftDocument::Context(context) => prepare_composite(context, catalog),
        DraftDocument::App(app) => prepare_app(app, catalog),
    }
}

fn prepare_data(draft: &DataDraft, catalog: &DraftCatalog) -> Result<PreparedArtifact> {
    let export = export_name(draft.info.title());
    // Data types are immutable value objects.
    let mut prepared = PreparedArtifact::empty(
        draft.info.kind.as_str(),
        draft.info.relative_path.clone(),
        draft.info.title.clone(),
        export,
        false,
    );
    prepared.source = SourceInfo {
        path: draft.info.relative_path.clone(),
        kind: draft.info.kind.as_str().to_string(),
        title: draft.info.title.clone(),
    };
    prepared.export = ExportInfo {
        name: export_name(draft.info.title()),
    };
    prepared.invariants.extend(draft.rules.clone());
    prepared.derives = infer_derives(&draft.notes);

    if !draft.fields.is_empty() {
        prepared.constructor = Some(ConstructorPolicy {
            kind: "all_fields".to_string(),
        });
        for field in &draft.fields {
            let type_status = resolve_data_field_type(
                field.name.as_str(),
                &[&field.meaning, &field.notes],
                catalog,
            );
            prepared.fields.push(FieldSpec {
                name: sanitize_identifier(&field.name),
                meaning: field.meaning.clone(),
                type_status: type_status.clone(),
                getter_accessible: field.getter_accessible,
                notes: non_empty_lines(&field.notes),
            });
            if field.getter_accessible {
                prepared.getters.push(GetterSpec {
                    name: sanitize_identifier(&field.name),
                    field: sanitize_identifier(&field.name),
                    mode: getter_mode(type_status.rust()).to_string(),
                });
            }
        }
    }
    for variant in &draft.variants {
        prepared.variants.push(VariantSpec {
            name: export_name(&variant.name),
            payload_types: variant
                .payload_types
                .iter()
                .map(|value| crate::draft_parser::normalize_type_notation(value))
                .collect(),
            meaning: variant.meaning.clone(),
            notes: non_empty_lines(&variant.notes),
        });
    }
    for functionality in &draft.functionalities {
        prepared
            .functionalities
            .push(prepare_data_functionality(functionality)?);
    }
    Ok(prepared)
}

fn prepare_composite(draft: &CompositeDraft, catalog: &DraftCatalog) -> Result<PreparedArtifact> {
    let export = export_name(draft.info.title());
    // Contexts are mutable (they orchestrate a use case and may update their own state).
    // Projections are immutable (pure read-only views).
    let is_context = draft.info.kind == crate::draft_parser::ArtifactKind::Context;
    let mut prepared = PreparedArtifact::empty(
        draft.info.kind.as_str(),
        draft.info.relative_path.clone(),
        draft.info.title.clone(),
        export.clone(),
        is_context,
    );
    prepared.invariants.extend(draft.notes.clone());
    let mut role_method_lookup = BTreeMap::<String, BTreeSet<String>>::new();

    for role in &draft.roles {
        let role_type = if let Some(explicit) = &role.explicit_type {
            resolve_nominal_role_type(
                &role.name,
                explicit,
                &[&role.why_involved, &role.expected_behavior],
                catalog,
            )
        } else {
            resolve_nominal_role_type_from_texts(
                &role.name,
                &[&role.why_involved, &role.expected_behavior],
                catalog,
            )
        };
        prepared.roles.push(RoleSpec {
            name: sanitize_identifier(&role.name),
            purpose: role.why_involved.clone(),
            expected_behavior: role.expected_behavior.clone(),
            type_status: role_type,
            methods: Vec::new(),
        });
    }
    for prop in &draft.props {
        let prop_type = resolve_named_type(&prop.name, &[&prop.meaning, &prop.notes], catalog);
        prepared.props.push(PropSpec {
            name: sanitize_identifier(&prop.name),
            meaning: prop.meaning.clone(),
            type_status: prop_type,
            notes: non_empty_lines(&prop.notes),
        });
    }

    for group in &draft.role_methods {
        let role_key = sanitize_identifier(&group.role);
        let Some(role_index) = prepared.roles.iter().position(|role| role.name == role_key) else {
            prepared.ambiguities.push(Ambiguity {
                path: format!("roles.{role_key}"),
                severity: "blocking".to_string(),
                message: format!(
                    "role method group `{}` does not map to a declared role",
                    group.role
                ),
                source_line: None,
            });
            continue;
        };
        let inferred_role_type = prepared.roles[role_index].type_status.clone();
        let methods = prepare_role_methods(group, &role_key, &inferred_role_type);
        for method in &methods {
            role_method_lookup
                .entry(role_key.clone())
                .or_default()
                .insert(method.name.clone());
        }
        prepared.roles[role_index].methods = methods;
    }

    let role_names: BTreeSet<String> = draft
        .roles
        .iter()
        .map(|role| sanitize_identifier(&role.name))
        .collect();
    let prop_names: BTreeSet<String> = draft
        .props
        .iter()
        .map(|prop| sanitize_identifier(&prop.name))
        .collect();
    let body_context = BodyParseContext {
        role_methods: role_method_lookup,
        role_names,
        prop_names,
    };
    for functionality in &draft.functionalities {
        prepared.functionalities.push(prepare_functionality(
            functionality,
            draft,
            &body_context,
            catalog,
        )?);
    }
    Ok(prepared)
}

fn prepare_role_methods(
    group: &RoleMethodGroup,
    role_key: &str,
    role_type: &ValueStatus,
) -> Vec<MethodSpec> {
    group
        .methods
        .iter()
        .map(|method| {
            let evidence = vec![Evidence {
                section: format!("Role Methods / {}", group.role),
                text: method.detail.clone(),
            }];
            let parsed_signature = extract_signature_marker(&[method.detail.clone()])
                .and_then(|signature| parse_signature(&signature, &method.name).ok());
            let role_player_param = ParameterSpec {
                name: format!("{role_key}_"),
                type_status: role_type
                    .rust()
                    .map(|ty| ValueStatus::resolved(format!("&{ty}"), "prepare.role_player"))
                    .unwrap_or_else(|| {
                        ValueStatus::missing(
                            format!(
                                "role method `{}` is waiting for role `{}` to resolve",
                                method.name, group.role
                            ),
                            evidence.clone(),
                        )
                    }),
            };

            let (signature, receiver, parameters, return_status) = if let Some(parsed) =
                parsed_signature.clone()
            {
                // Receiver is always `&self`; the context is never mutated by a role method call.
                let receiver = Some("&self".to_string());
                let mut parameters = vec![role_player_param.clone()];
                parameters.extend(parsed.parameters
                    .iter()
                    .map(|parameter| ParameterSpec {
                        name: parameter.0.clone(),
                        type_status: ValueStatus::resolved(
                            parameter.1.clone(),
                            "prepare.signature",
                        ),
                    })
                    .collect::<Vec<_>>());
                let return_status =
                    ValueStatus::resolved(parsed.return_type.clone(), "prepare.signature");
                let signature = if role_player_param.type_status.is_resolved() {
                    ValueStatus::resolved(
                        render_signature(
                            &sanitize_identifier(&method.name),
                            receiver.as_deref(),
                            &parameters,
                            parsed.return_type.as_str(),
                        ),
                        "prepare.signature",
                    )
                } else {
                    ValueStatus::missing(
                        format!(
                            "role method `{}` on role `{}` is waiting for the role type before its wrapper signature can be finalized",
                            method.name, group.role
                        ),
                        evidence.clone(),
                    )
                };
                (signature, receiver, parameters, return_status)
            } else {
                (
                    ValueStatus::missing(
                        format!(
                            "role method `{}` on role `{}` has no explicit signature",
                            method.name, group.role
                        ),
                        evidence.clone(),
                    ),
                    Some("&self".to_string()),
                    vec![role_player_param.clone()],
                    ValueStatus::missing(
                        format!("role method `{}` return type is unknown", method.name),
                        evidence.clone(),
                    ),
                )
            };

            let body = parsed_signature.map(|parsed| {
                delegated_role_method_body(role_key, &method.name, &parsed.parameters)
            });

            // Role methods in this implementation are instance methods on the context where
            // the first argument is the role player (named `<role>_` by convention).
            // If we have a delegated body there is no need for a behavioral description;
            // if the signature was not resolved we have nothing useful to store yet either,
            // so flow/extensions/guarantee stay empty for role methods.
            MethodSpec {
                name: sanitize_identifier(&method.name),
                signature,
                receiver,
                parameters,
                return_status,
                flow: Vec::new(),
                extensions: Vec::new(),
                guarantee: Vec::new(),
                references: None,
                body,
            }
        })
        .collect()
}

fn prepare_functionality(
    functionality: &FunctionalityDraft,
    draft: &CompositeDraft,
    body_context: &BodyParseContext,
    _catalog: &DraftCatalog,
) -> Result<MethodSpec> {
    // Auto-generate `new` as a trivial struct constructor only when the draft leaves the
    // flow empty. When the draft provides explicit flow for `new` (e.g. a context that must
    // perform scenario setup like picking an initial food position), fall through to the
    // normal per-functionality path so the build agent — not prepare — implements the body.
    // `new` stays an associated function (no `self` receiver) regardless.
    let is_new = functionality.name == "new";
    if is_new && functionality.flow.is_empty() {
        return Ok(auto_constructor_method(draft, _catalog));
    }

    let is_context = draft.info.kind == crate::draft_parser::ArtifactKind::Context;
    // Functionalities on contexts default to `&self` but are upgraded to `&mut self` when the
    // behavioral description implies mutation (verb-based detection + `self.X = …` patterns).
    // Projections stay `&self` by definition. `new` is always an associated function, so it
    // has no default receiver — we force `None` below.
    let mutates = !is_new
        && is_context
        && flow_implies_mutation(
            &functionality.flow,
            &functionality.extensions,
            &functionality.guarantee,
        );
    let default_receiver = if mutates { "&mut self" } else { "&self" };

    let signature_text = extract_signature_marker(&functionality.flow);
    let signature = if let Some(text) = signature_text.clone() {
        parse_signature(&text, &functionality.name).ok()
    } else {
        None
    };
    let evidence = vec![Evidence {
        section: format!("Functionalities / {}", functionality.name),
        text: functionality.flow.join(" "),
    }];
    let (signature_status, receiver, parameters, return_status) = if let Some(parsed) = &signature {
        (
            ValueStatus::resolved(parsed.original.clone(), "prepare.signature"),
            // Honour any receiver the BA put in the Signature marker; fall back to the type
            // default — except for `new`, which is always an associated function and must
            // keep `receiver = None` even when the Signature marker omits one.
            if is_new {
                parsed.receiver.clone()
            } else {
                parsed
                    .receiver
                    .clone()
                    .or_else(|| Some(default_receiver.to_string()))
            },
            parsed
                .parameters
                .iter()
                .map(|parameter| ParameterSpec {
                    name: parameter.0.clone(),
                    type_status: ValueStatus::resolved(parameter.1.clone(), "prepare.signature"),
                })
                .collect::<Vec<_>>(),
            ValueStatus::resolved(parsed.return_type.clone(), "prepare.signature"),
        )
    } else {
        (
            ValueStatus::missing(
                format!(
                    "functionality `{}` is missing a parseable signature",
                    functionality.name
                ),
                evidence.clone(),
            ),
            if is_new {
                None
            } else {
                Some(default_receiver.to_string())
            },
            Vec::new(),
            ValueStatus::missing(
                format!(
                    "functionality `{}` return type is unknown",
                    functionality.name
                ),
                evidence.clone(),
            ),
        )
    };

    // Dual-path: try to reduce to deterministic IR first; fall back to storing the
    // behavioral description verbatim so the AI implementation agent can use it.
    let body = parse_body_from_rules(&functionality.flow, body_context).ok();
    let (flow, extensions, guarantee, references) = if body.is_none() {
        let refs = extract_references(
            &functionality.flow,
            &functionality.extensions,
            &functionality.guarantee,
            body_context,
        );
        (
            functionality.flow.clone(),
            functionality.extensions.clone(),
            functionality.guarantee.clone(),
            Some(refs),
        )
    } else {
        (Vec::new(), Vec::new(), Vec::new(), None)
    };

    Ok(MethodSpec {
        name: sanitize_identifier(&functionality.name),
        signature: signature_status,
        receiver,
        parameters,
        return_status,
        flow,
        extensions,
        guarantee,
        references,
        body,
    })
}

fn prepare_data_functionality(
    functionality: &crate::draft_parser::DataFunctionalityDraft,
) -> Result<MethodSpec> {
    let parsed = parse_signature(&functionality.signature, &functionality.name)?;
    Ok(MethodSpec {
        name: sanitize_identifier(&functionality.name),
        signature: ValueStatus::resolved(
            render_signature(
                &sanitize_identifier(&functionality.name),
                parsed.receiver.as_deref(),
                &parsed
                    .parameters
                    .iter()
                    .map(|parameter| ParameterSpec {
                        name: parameter.0.clone(),
                        type_status: ValueStatus::resolved(
                            parameter.1.clone(),
                            "draft.functionality",
                        ),
                    })
                    .collect::<Vec<_>>(),
                &parsed.return_type,
            ),
            "draft.functionality",
        ),
        receiver: parsed.receiver,
        parameters: parsed
            .parameters
            .iter()
            .map(|parameter| ParameterSpec {
                name: parameter.0.clone(),
                type_status: ValueStatus::resolved(parameter.1.clone(), "draft.functionality"),
            })
            .collect(),
        return_status: ValueStatus::resolved(parsed.return_type, "draft.functionality"),
        flow: Vec::new(),
        extensions: Vec::new(),
        guarantee: Vec::new(),
        references: None,
        body: None,
    })
}

fn auto_constructor_method(draft: &CompositeDraft, catalog: &DraftCatalog) -> MethodSpec {
    let mut params = Vec::new();
    let mut fields = Vec::new();
    for role in &draft.roles {
        let ty = if let Some(explicit) = &role.explicit_type {
            resolve_nominal_role_type(
                &role.name,
                explicit,
                &[&role.why_involved, &role.expected_behavior],
                catalog,
            )
        } else {
            resolve_nominal_role_type_from_texts(
                &role.name,
                &[&role.why_involved, &role.expected_behavior],
                catalog,
            )
        };
        params.push(ParameterSpec {
            name: sanitize_identifier(&role.name),
            type_status: ty.clone(),
        });
        fields.push(StructFieldValue {
            name: sanitize_identifier(&role.name),
            expr: Expression::Var {
                name: sanitize_identifier(&role.name),
            },
        });
    }
    for prop in &draft.props {
        let ty = resolve_named_type(&prop.name, &[&prop.meaning, &prop.notes], catalog);
        params.push(ParameterSpec {
            name: sanitize_identifier(&prop.name),
            type_status: ty.clone(),
        });
        fields.push(StructFieldValue {
            name: sanitize_identifier(&prop.name),
            expr: Expression::Var {
                name: sanitize_identifier(&prop.name),
            },
        });
    }

    let signature_text = format!(
        "new({}) -> Self",
        params
            .iter()
            .map(|parameter| format!(
                "{}: {}",
                parameter.name,
                parameter
                    .type_status
                    .rust
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string())
            ))
            .collect::<Vec<_>>()
            .join(", ")
    );

    MethodSpec {
        name: "new".to_string(),
        signature: ValueStatus::resolved(signature_text, "prepare.auto"),
        receiver: None,
        parameters: params,
        return_status: ValueStatus::resolved("Self", "prepare.auto"),
        flow: Vec::new(),
        extensions: Vec::new(),
        guarantee: Vec::new(),
        references: None,
        body: Some(Body {
            steps: vec![Statement::Return {
                expr: Some(Expression::ConstructStruct {
                    type_name: "Self".to_string(),
                    fields,
                }),
            }],
        }),
    }
}

fn prepare_app(app: &AppDraft, catalog: &DraftCatalog) -> Result<PreparedArtifact> {
    let export = export_name(app.info.title());
    // App entry points drive mutable state (main loop, startup sequence).
    let mut prepared = PreparedArtifact::empty(
        app.info.kind.as_str(),
        app.info.relative_path.clone(),
        app.info.title.clone(),
        export,
        true,
    );

    for collaborator in &app.collaborators {
        prepared.collaborators.push(CollaboratorSpec {
            name: sanitize_identifier(&collaborator.name),
            responsibility: collaborator.responsibility.clone(),
            type_status: resolve_nominal_collaborator_type_from_texts(
                &collaborator.name,
                &[&collaborator.responsibility],
                catalog,
            ),
        });
    }

    let mut rules = Vec::new();
    if let Some(body) = app.sections.get("Startup Sequence") {
        rules.extend(parse_loose_steps(body));
    }
    if let Some(body) = app.sections.get("Main Flow") {
        rules.extend(parse_loose_steps(body));
    } else if let Some(body) = app.sections.get("Main Loop Behavior") {
        rules.extend(parse_loose_steps(body));
    }

    let body_result = parse_body_from_rules(&rules, &BodyParseContext::default());
    let body = body_result.ok();
    let flow = if body.is_some() { Vec::new() } else { rules };
    prepared.functionalities.push(MethodSpec {
        name: "main".to_string(),
        signature: ValueStatus::resolved("main() -> ()", "prepare.auto"),
        receiver: None,
        parameters: Vec::new(),
        return_status: ValueStatus::resolved("()", "prepare.auto"),
        flow,
        extensions: Vec::new(),
        guarantee: Vec::new(),
        references: None,
        body,
    });
    Ok(prepared)
}

/// Extract identifiers referenced in a method's behavioral description and classify them against
/// the DCI vocabulary known at prepare time (roles, props, role methods, types).
///
/// Scans backtick-quoted tokens in flow/extensions/guarantee. Each token is matched against:
///  - known role field names → `roles`
///  - known prop names → `props`
///  - known role methods (`<role>_<method>` where role is a known role field) → `role_methods`
///  - PascalCase identifiers → `types`
///
/// Identifiers that do not match any category are silently ignored (they are likely local
/// variable names or Rust expressions and are not meaningful at this level).
fn extract_references(
    flow: &[String],
    extensions: &[String],
    guarantee: &[String],
    body_context: &BodyParseContext,
) -> MethodReferences {
    let backtick_re = Regex::new(r"`([^`]+)`").expect("backtick regex");
    let mut refs = MethodReferences::default();

    let all_text = flow.iter().chain(extensions).chain(guarantee);
    for text in all_text {
        for cap in backtick_re.captures_iter(text) {
            let token = cap.get(1).unwrap().as_str().trim();
            // Strip method-call suffixes like `.method(...)` or `(args)` to isolate the
            // leading identifier (e.g. `stdin_source.read_keys()` → `stdin_source`).
            let identifier = token
                .split(|ch: char| ch == '.' || ch == '(')
                .next()
                .unwrap_or(token)
                .trim();
            let sanitized = sanitize_identifier(identifier);
            if sanitized.is_empty() {
                continue;
            }

            // 1. Exact role name (e.g. `board`, `stdin_source`).
            if body_context.role_names.contains(&sanitized) {
                if !refs.roles.contains(&sanitized) {
                    refs.roles.push(sanitized.clone());
                }
                continue;
            }

            // 2. Exact prop name.
            if body_context.prop_names.contains(&sanitized) {
                if !refs.props.contains(&sanitized) {
                    refs.props.push(sanitized.clone());
                }
                continue;
            }

            // 3. Role method: `<role>_<method>` where the role prefix is a known role name.
            let is_role_method = body_context.role_names.iter().any(|role| {
                sanitized.starts_with(&format!("{role}_")) && sanitized.len() > role.len() + 1
            });
            if is_role_method {
                if !refs.role_methods.contains(&sanitized) {
                    refs.role_methods.push(sanitized.clone());
                }
                continue;
            }

            // 4. PascalCase → treat as a type reference.
            if sanitized
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_uppercase())
            {
                if !refs.types.contains(&sanitized) {
                    refs.types.push(sanitized.clone());
                }
                continue;
            }

            // Everything else is assumed to be a local variable name or Rust keyword; skip.
        }
    }

    refs.roles.sort();
    refs.props.sort();
    refs.types.sort();
    refs.role_methods.sort();
    refs
}

fn parse_loose_steps(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            let trimmed = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
                .or_else(|| {
                    let digits = trimmed.chars().take_while(|ch| ch.is_ascii_digit()).count();
                    if digits > 0 && trimmed[digits..].starts_with(". ") {
                        Some(trimmed[digits + 2..].trim())
                    } else {
                        None
                    }
                })
                .unwrap_or(trimmed);
            Some(trimmed.to_string())
        })
        .collect()
}

fn parse_body_from_rules(rules: &[String], context: &BodyParseContext) -> Result<Body> {
    let mut steps = Vec::new();
    for rule in rules {
        if rule.trim().starts_with("Signature:") || rule.trim() == "Steps:" {
            continue;
        }
        steps.push(parse_step(rule, context)?);
    }
    if steps.is_empty() {
        anyhow::bail!("no executable steps found");
    }
    Ok(Body { steps })
}

fn parse_step(rule: &str, context: &BodyParseContext) -> Result<Statement> {
    if let Some(captures) = Regex::new(r#"^Let `([^`]+)` be `([^`]+)`\.?$"#)
        .expect("let regex")
        .captures(rule.trim())
    {
        return Ok(Statement::Let {
            name: sanitize_identifier(captures.get(1).unwrap().as_str()),
            expr: parse_expression(captures.get(2).unwrap().as_str(), context)?,
        });
    }
    if let Some(captures) = Regex::new(r#"^Set `([^`]+)` to `([^`]+)`\.?$"#)
        .expect("assign regex")
        .captures(rule.trim())
    {
        return Ok(Statement::AssignLocal {
            name: sanitize_identifier(captures.get(1).unwrap().as_str()),
            expr: parse_expression(captures.get(2).unwrap().as_str(), context)?,
        });
    }
    if let Some(captures) = Regex::new(r#"^Call `([^`]+)`\.?$"#)
        .expect("call regex")
        .captures(rule.trim())
    {
        return Ok(Statement::Call {
            expr: parse_expression(captures.get(1).unwrap().as_str(), context)?,
        });
    }
    if let Some(captures) = Regex::new(r#"^Return `([^`]+)`\.?$"#)
        .expect("return regex")
        .captures(rule.trim())
    {
        return Ok(Statement::Return {
            expr: Some(parse_expression(
                captures.get(1).unwrap().as_str(),
                context,
            )?),
        });
    }
    if let Some(captures) = Regex::new(r#"^Read current UTC millisecond time into `([^`]+)`\.?$"#)
        .expect("time regex")
        .captures(rule.trim())
    {
        return Ok(Statement::ReadUtcNowMs {
            name: sanitize_identifier(captures.get(1).unwrap().as_str()),
        });
    }
    if let Some(captures) = Regex::new(r#"^Sleep `([^`]+)` milliseconds\.?$"#)
        .expect("sleep regex")
        .captures(rule.trim())
    {
        return Ok(Statement::SleepMs {
            expr: parse_expression(captures.get(1).unwrap().as_str(), context)?,
        });
    }
    anyhow::bail!("Unsupported deterministic step: {rule}")
}

fn parse_expression(code: &str, context: &BodyParseContext) -> Result<Expression> {
    let trimmed = code.trim();
    if let Some((left, operator, right)) = find_top_level_binary_op(trimmed) {
        return Ok(Expression::BinaryOp {
            operator: operator.to_string(),
            left: Box::new(parse_expression(left, context)?),
            right: Box::new(parse_expression(right, context)?),
        });
    }
    if let Some(value) = trimmed.strip_prefix('!') {
        return Ok(Expression::UnaryOp {
            operator: "!".to_string(),
            expr: Box::new(parse_expression(value, context)?),
        });
    }
    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        return Ok(Expression::Literal {
            kind: "string".to_string(),
            value: trimmed[1..trimmed.len() - 1].to_string(),
        });
    }
    if trimmed == "true" || trimmed == "false" {
        return Ok(Expression::Literal {
            kind: "bool".to_string(),
            value: trimmed.to_string(),
        });
    }
    if trimmed.chars().all(|ch| ch.is_ascii_digit()) {
        return Ok(Expression::Literal {
            kind: "integer".to_string(),
            value: trimmed.to_string(),
        });
    }
    if let Some((type_name, fields)) = parse_struct_literal(trimmed, context)? {
        return Ok(Expression::ConstructStruct { type_name, fields });
    }
    if trimmed.contains("::")
        && !trimmed.contains('(')
        && trimmed.rsplit("::").next().is_some_and(|part| {
            part.chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_uppercase())
        })
    {
        let type_name = trimmed.split("::").next().unwrap_or_default().to_string();
        let variant = trimmed.rsplit("::").next().unwrap_or_default().to_string();
        return Ok(Expression::ConstructEnum { type_name, variant });
    }
    if let Some((callee, args)) = parse_call(trimmed)? {
        if let Some((left, method)) = callee.rsplit_once('.') {
            if context
                .role_methods
                .get(&sanitize_identifier(left))
                .is_some_and(|methods| methods.contains(&sanitize_identifier(method)))
            {
                return Ok(Expression::CallRoleMethod {
                    role: sanitize_identifier(left),
                    method: sanitize_identifier(method),
                    args: parse_call_args(&args, context)?,
                });
            }
            return Ok(Expression::CallInstanceMethod {
                receiver: Box::new(parse_expression(left, context)?),
                method: sanitize_identifier(method.trim_end_matches('!')),
                args: parse_call_args(&args, context)?,
            });
        }
        return Ok(Expression::CallLocalMethod {
            name: callee.to_string(),
            args: parse_call_args(&args, context)?,
        });
    }
    if let Some((base, field)) = trimmed.rsplit_once('.') {
        return Ok(Expression::Field {
            base: sanitize_identifier(base),
            name: sanitize_identifier(field),
        });
    }
    Ok(Expression::Var {
        name: sanitize_identifier(trimmed),
    })
}

fn parse_call(code: &str) -> Result<Option<(String, String)>> {
    let Some(open) = code.find('(') else {
        return Ok(None);
    };
    if !code.ends_with(')') {
        anyhow::bail!("Unsupported call expression `{code}`");
    }
    Ok(Some((
        code[..open].trim().to_string(),
        code[open + 1..code.len() - 1].trim().to_string(),
    )))
}

fn parse_call_args(args: &str, context: &BodyParseContext) -> Result<Vec<Expression>> {
    if args.trim().is_empty() {
        return Ok(Vec::new());
    }
    split_top_level(args, ',')
        .into_iter()
        .map(|item| parse_expression(item.trim(), context))
        .collect()
}

fn parse_struct_literal(
    code: &str,
    context: &BodyParseContext,
) -> Result<Option<(String, Vec<StructFieldValue>)>> {
    let Some(open) = code.find('{') else {
        return Ok(None);
    };
    if !code.ends_with('}') {
        return Ok(None);
    }
    let type_name = code[..open].trim();
    if type_name.is_empty() {
        return Ok(None);
    }
    let inner = code[open + 1..code.len() - 1].trim();
    let mut fields = Vec::new();
    for part in split_top_level(inner, ',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let Some((name, value)) = part.split_once(':') else {
            anyhow::bail!("Unsupported struct field assignment `{part}`");
        };
        fields.push(StructFieldValue {
            name: sanitize_identifier(name),
            expr: parse_expression(value.trim(), context)?,
        });
    }
    Ok(Some((type_name.to_string(), fields)))
}

fn find_top_level_binary_op(code: &str) -> Option<(&str, &'static str, &str)> {
    for operator in ["==", "!=", ">=", "<=", "+", "-", "*", "/", ">", "<"] {
        let mut depth = 0i32;
        let chars = code.char_indices().collect::<Vec<_>>();
        for (idx, ch) in chars {
            match ch {
                '(' | '{' | '[' | '<' => depth += 1,
                ')' | '}' | ']' | '>' => depth -= 1,
                _ => {}
            }
            if depth == 0 && code[idx..].starts_with(operator) {
                let left = code[..idx].trim();
                let right = code[idx + operator.len()..].trim();
                if !left.is_empty() && !right.is_empty() {
                    return Some((left, operator, right));
                }
            }
        }
    }
    None
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
    items.push(value[start..].trim().to_string());
    items
}

fn delegated_role_method_body(role: &str, method: &str, parameters: &[(String, String)]) -> Body {
    // The role player is received as the first parameter named `<role>_`.
    // Delegate to the role player's own method, passing the remaining domain parameters.
    let args = parameters
        .iter()
        .map(|parameter| Expression::Var {
            name: sanitize_identifier(&parameter.0),
        })
        .collect::<Vec<_>>();
    Body {
        steps: vec![Statement::Return {
            expr: Some(Expression::CallInstanceMethod {
                receiver: Box::new(Expression::Var {
                    name: format!("{role}_"),
                }),
                method: sanitize_identifier(method),
                args,
            }),
        }],
    }
}

#[derive(Debug, Clone)]
struct ParsedSignature {
    original: String,
    receiver: Option<String>,
    parameters: Vec<(String, String)>,
    return_type: String,
}

fn parse_signature(signature: &str, expected_name: &str) -> Result<ParsedSignature> {
    let trimmed = strip_backticks(signature);
    let Some((left, right)) = trimmed.split_once("->") else {
        anyhow::bail!("signature `{trimmed}` is missing `->`");
    };
    let left = left.trim();
    let return_type = right.trim().to_string();
    let Some(open) = left.find('(') else {
        anyhow::bail!("signature `{trimmed}` is missing `(`");
    };
    let Some(close) = left.rfind(')') else {
        anyhow::bail!("signature `{trimmed}` is missing `)`");
    };
    let name = left[..open].trim();
    if sanitize_identifier(name) != sanitize_identifier(expected_name) {
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
        let Some((name, ty)) = part.split_once(':') else {
            anyhow::bail!("signature parameter `{part}` is missing `:`");
        };
        parameters.push((sanitize_identifier(name), ty.trim().to_string()));
    }
    Ok(ParsedSignature {
        original: trimmed,
        receiver,
        parameters,
        return_type,
    })
}

fn extract_signature_marker(lines: &[String]) -> Option<String> {
    let regex = Regex::new(r#"Signature:\s*`([^`]+)`"#).expect("signature regex");
    let mut matches = lines
        .iter()
        .filter_map(|line| regex.captures(line).and_then(|caps| caps.get(1)))
        .map(|value| value.as_str().trim().to_string())
        .collect::<Vec<_>>();
    matches.dedup();
    matches.into_iter().next()
}

/// Heuristic mutation detector for a functionality's behavioral description.
///
/// Returns `true` when the flow/extensions/guarantee text implies the functionality mutates
/// the owning context. Used to infer `&mut self` receivers before scaffold.
///
/// Signals (case-insensitive, word-boundary aware):
/// 1. Mutation verbs (`push`, `pop`, `insert`, `remove`, `update`, `set`, `reset`, `append`,
///    `replace`, `clear`, `advance`, `increment`, `decrement`, `record`, `consume`, `capture`,
///    `drain`, `fill`, `swap`, `take`, `assign`, `write`) appearing as natural-language verbs.
/// 2. Explicit re-assignment in the description: `self.<ident> = …` (excluding `==`).
///
/// Deliberately avoids false positives from read-verbs like `match`, `check`, `return`.
fn flow_implies_mutation(flow: &[String], extensions: &[String], guarantee: &[String]) -> bool {
    const MUTATION_VERBS: &[&str] = &[
        "push",
        "pop",
        "insert",
        "remove",
        "delete",
        "update",
        "set",
        "reset",
        "append",
        "prepend",
        "replace",
        "clear",
        "advance",
        "increment",
        "decrement",
        "record",
        "consume",
        "capture",
        "drain",
        "fill",
        "swap",
        "take",
        "assign",
        "write",
        "emit",
        "extend",
        "flush",
        "rotate",
        "shift",
        "drop",
        "move",
        "mutate",
        "accumulate",
        "store",
    ];
    let combined: String = flow
        .iter()
        .chain(extensions)
        .chain(guarantee)
        .map(|line| line.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let lowered = combined.to_ascii_lowercase();

    for verb in MUTATION_VERBS {
        if contains_word(&lowered, verb) {
            return true;
        }
        // English inflections: `-s`, `-es`, `-ed`, `-ing`.
        for suffix in &["s", "es", "ed", "ing"] {
            let inflected = format!("{verb}{suffix}");
            if contains_word(&lowered, &inflected) {
                return true;
            }
        }
    }

    // Direct field re-assignment: `self.foo = …` (but not `==`).
    if let Some(mut idx) = combined.find("self.") {
        loop {
            let tail = &combined[idx..];
            if let Some(eq_pos) = tail.find('=') {
                let after = &tail[eq_pos + 1..];
                if !after.starts_with('=') && !tail[..eq_pos].contains('\n') {
                    return true;
                }
            }
            match combined[idx + "self.".len()..].find("self.") {
                Some(next) => idx += "self.".len() + next,
                None => break,
            }
        }
    }

    false
}

fn contains_word(haystack: &str, needle: &str) -> bool {
    let mut start = 0usize;
    let needle_bytes = needle.as_bytes();
    let bytes = haystack.as_bytes();
    while let Some(pos) = haystack[start..].find(needle) {
        let absolute = start + pos;
        let before_ok = absolute == 0 || !is_word_char(bytes[absolute - 1] as char);
        let after_end = absolute + needle_bytes.len();
        let after_ok = after_end >= bytes.len() || !is_word_char(bytes[after_end] as char);
        if before_ok && after_ok {
            return true;
        }
        start = absolute + 1;
    }
    false
}

fn is_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn render_signature(
    name: &str,
    receiver: Option<&str>,
    parameters: &[ParameterSpec],
    return_type: &str,
) -> String {
    let mut parts = Vec::new();
    if let Some(receiver) = receiver {
        parts.push(receiver.to_string());
    }
    parts.extend(parameters.iter().map(|parameter| {
        format!(
            "{}: {}",
            parameter.name,
            parameter.type_status.rust().unwrap_or("Unknown")
        )
    }));
    format!("{name}({}) -> {return_type}", parts.join(", "))
}

fn resolve_data_field_type(name: &str, texts: &[&str], catalog: &DraftCatalog) -> ValueStatus {
    resolve_type_with_defaults(name, texts, catalog, true, false)
}

fn resolve_named_type(name: &str, texts: &[&str], catalog: &DraftCatalog) -> ValueStatus {
    resolve_type_with_defaults(name, texts, catalog, false, true)
}

fn resolve_nominal_role_type(
    name: &str,
    explicit: &str,
    texts: &[&str],
    catalog: &DraftCatalog,
) -> ValueStatus {
    let normalized = crate::draft_parser::normalize_type_notation(explicit);
    if looks_like_nominal_role_type(&normalized, catalog) {
        return ValueStatus::resolved(normalized, "draft.role_player_type");
    }
    let mut evidence = texts
        .iter()
        .filter(|text| !text.trim().is_empty())
        .map(|text| Evidence {
            section: name.to_string(),
            text: text.trim().to_string(),
        })
        .collect::<Vec<_>>();
    evidence.push(Evidence {
        section: name.to_string(),
        text: format!("Explicit type: {explicit}"),
    });
    ValueStatus::missing(
        format!("role player `{name}` must resolve to a nominal concrete type"),
        evidence,
    )
}

fn resolve_nominal_role_type_from_texts(
    name: &str,
    texts: &[&str],
    catalog: &DraftCatalog,
) -> ValueStatus {
    // First, consult resolved types from previously-prepared artifacts — if a sibling draft has
    // already pinned this role's type, reuse it deterministically.
    let sanitized = sanitize_identifier(name);
    if let Some(rust) = catalog.resolved_role_types.get(&sanitized) {
        return ValueStatus::resolved(rust.clone(), "prepare.cross_artifact");
    }

    let status = resolve_named_type(name, texts, catalog);
    match status.rust.as_deref() {
        Some(rust) if looks_like_nominal_role_type(rust, catalog) => status,
        _ => ValueStatus::missing(
            format!("no nominal concrete Rust type could be inferred for `{name}`"),
            texts
                .iter()
                .filter(|text| !text.trim().is_empty())
                .map(|text| Evidence {
                    section: name.to_string(),
                    text: text.trim().to_string(),
                })
                .collect(),
        ),
    }
}

fn resolve_nominal_collaborator_type_from_texts(
    name: &str,
    texts: &[&str],
    catalog: &DraftCatalog,
) -> ValueStatus {
    let mut combined = texts
        .iter()
        .map(|text| text.trim())
        .filter(|text| !text.is_empty())
        .map(|text| text.to_string())
        .collect::<Vec<_>>();
    if let Some(hints) = catalog.role_hints.get(&sanitize_identifier(name)) {
        combined.extend(hints.iter().cloned());
    }
    let refs = combined
        .iter()
        .map(|text| text.as_str())
        .collect::<Vec<_>>();
    resolve_nominal_role_type_from_texts(name, &refs, catalog)
}

fn resolve_type_with_defaults(
    name: &str,
    texts: &[&str],
    catalog: &DraftCatalog,
    allow_scalar_defaults: bool,
    prefer_exact_name_match: bool,
) -> ValueStatus {
    let evidence = texts
        .iter()
        .filter(|text| !text.trim().is_empty())
        .map(|text| Evidence {
            section: name.to_string(),
            text: text.trim().to_string(),
        })
        .collect::<Vec<_>>();

    if prefer_exact_name_match {
        let normalized_name = normalize_symbol(name);
        if let Some(candidates) = catalog.normalized.get(&normalized_name) {
            if candidates.len() == 1 {
                return ValueStatus::resolved(candidates[0].clone(), "name_match");
            }
            return ValueStatus::ambiguous(
                candidates.clone(),
                format!("multiple exports match `{name}`"),
                evidence,
            );
        }
    }

    let explicit = extract_type_candidates(texts, catalog);
    if explicit.len() == 1 {
        return ValueStatus::resolved(explicit[0].clone(), "draft_hint");
    }
    if explicit.len() > 1 {
        return ValueStatus::ambiguous(
            explicit,
            format!("multiple type candidates were found for `{name}`"),
            evidence,
        );
    }

    if !prefer_exact_name_match {
        let normalized_name = normalize_symbol(name);
        if let Some(candidates) = catalog.normalized.get(&normalized_name) {
            if candidates.len() == 1 {
                return ValueStatus::resolved(candidates[0].clone(), "name_match");
            }
            return ValueStatus::ambiguous(
                candidates.clone(),
                format!("multiple exports match `{name}`"),
                evidence,
            );
        }
    }

    if allow_scalar_defaults {
        let lower = format!("{} {}", name, texts.join(" ")).to_ascii_lowercase();
        if lower.contains("string") || lower.contains("text") {
            return ValueStatus::defaulted("String", "scalar_default");
        }
        if lower.contains("millisecond") || lower.ends_with("_ms") {
            return ValueStatus::defaulted("u64", "scalar_default");
        }
        if lower.contains("whole number") || lower.contains("count") || lower.contains("score") {
            return ValueStatus::defaulted("u32", "scalar_default");
        }
        if lower.contains("true") || lower.contains("false") || lower.contains("boolean") {
            return ValueStatus::defaulted("bool", "scalar_default");
        }
    }

    ValueStatus::missing(
        format!("no concrete Rust type could be inferred for `{name}`"),
        evidence,
    )
}

fn extract_type_candidates(texts: &[&str], catalog: &DraftCatalog) -> Vec<String> {
    let backtick = Regex::new(r"`([^`]+)`").expect("type hint regex");
    let explicit_prefix =
        Regex::new(r"(?i)\btype is\s+`?([A-Za-z0-9_:<>, &\[\]]+)`?").expect("explicit type regex");
    let mut candidates = BTreeSet::new();
    for text in texts {
        for captures in explicit_prefix.captures_iter(text) {
            let raw = captures.get(1).unwrap().as_str().trim().to_string();
            if looks_like_type(&raw, catalog) {
                candidates.insert(raw);
            }
        }
        for captures in backtick.captures_iter(text) {
            let raw = captures.get(1).unwrap().as_str().trim().to_string();
            if looks_like_type(&raw, catalog) {
                candidates.insert(raw);
            }
        }
    }
    candidates.into_iter().collect()
}

fn looks_like_nominal_role_type(value: &str, catalog: &DraftCatalog) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.contains("fn(")
        || trimmed.starts_with("Vec<")
        || trimmed.starts_with("Option<")
        || trimmed.starts_with("HashMap<")
        || trimmed.starts_with("std::collections::")
        || matches!(
            trimmed,
            "String"
                | "str"
                | "bool"
                | "char"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "usize"
                | "i8"
                | "i16"
                | "i32"
                | "i64"
                | "isize"
                | "f32"
                | "f64"
                | "()"
        )
    {
        return false;
    }
    if catalog.exports.contains(trimmed) {
        return true;
    }
    trimmed.contains("::")
        && trimmed.rsplit("::").next().is_some_and(|segment| {
            segment
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_uppercase())
        })
}

fn looks_like_type(value: &str, catalog: &DraftCatalog) -> bool {
    let trimmed = strip_backticks(value);
    if trimmed.is_empty() {
        return false;
    }
    if matches!(trimmed.as_str(), "&self" | "&mut self" | "self") {
        return false;
    }
    if trimmed.contains(" -> ") || trimmed.contains('(') || trimmed.contains(')') {
        return false;
    }
    if catalog.exports.contains(&trimmed) {
        return true;
    }
    if trimmed.contains("::") || trimmed.contains('<') || trimmed.contains('[') {
        return true;
    }
    trimmed
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
        || matches!(
            trimmed.as_str(),
            "String"
                | "str"
                | "bool"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "usize"
                | "i8"
                | "i16"
                | "i32"
                | "i64"
                | "isize"
                | "char"
                | "()"
        )
}

fn infer_derives(notes: &[String]) -> Vec<String> {
    let mut derives = vec!["Debug".to_string(), "Clone".to_string()];
    let lower = notes.join(" ").to_ascii_lowercase();
    if lower.contains("copy") {
        derives.push("Copy".to_string());
    }
    derives.sort();
    derives.dedup();
    derives
}

fn getter_mode(rust: Option<&str>) -> &'static str {
    if copy_like_type(rust) { "copy" } else { "ref" }
}

fn copy_like_type(rust: Option<&str>) -> bool {
    matches!(
        rust.unwrap_or(""),
        "bool"
            | "char"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "usize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "isize"
            | "f32"
            | "f64"
    )
}

fn export_name(value: &str) -> String {
    let mut out = String::new();
    for token in value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
    {
        let mut chars = token.chars();
        if let Some(first) = chars.next() {
            out.push_str(&first.to_uppercase().collect::<String>());
            out.push_str(chars.as_str());
        }
    }
    if out.is_empty() {
        "Artifact".to_string()
    } else {
        out
    }
}

fn sanitize_identifier(value: &str) -> String {
    let mut out = String::new();
    let mut prev_underscore = false;
    for ch in value.chars() {
        let normalized = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            '_'
        };
        if normalized == '_' {
            if !prev_underscore && !out.is_empty() {
                out.push('_');
            }
            prev_underscore = true;
            continue;
        }
        prev_underscore = false;
        out.push(normalized);
    }
    out.trim_matches('_').to_string()
}

fn normalize_symbol(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn non_empty_lines(value: &str) -> Vec<String> {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn strip_backticks(value: &str) -> String {
    value.trim().trim_matches('`').to_string()
}

trait DraftInfoExt {
    fn relative_path(&self) -> &str;
    fn title(&self) -> &str;
    fn stem(&self) -> &str;
}

impl DraftInfoExt for crate::draft_parser::DraftInfo {
    fn relative_path(&self) -> &str {
        &self.relative_path
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn stem(&self) -> &str {
        self.relative_path
            .rsplit('/')
            .next()
            .unwrap_or(&self.relative_path)
            .trim_end_matches(".md")
    }
}

trait DraftDocumentInfoExt {
    fn info(&self) -> &crate::draft_parser::DraftInfo;
}

impl DraftDocumentInfoExt for DraftDocument {
    fn info(&self) -> &crate::draft_parser::DraftInfo {
        match self {
            DraftDocument::Data(data) => &data.info,
            DraftDocument::Projection(projection) => &projection.info,
            DraftDocument::Context(context) => &context.info,
            DraftDocument::App(app) => &app.info,
        }
    }
}

/// Scan a prepared artifact's resolved types for external module paths and ensure each one's
/// crate is registered in `drafts/dependencies.yml` and `drafts/types-manifest.yml`.
///
/// Uses `drafts/capability_registry.yml` as the source of truth for the crate metadata
/// (features, default_features, external_path_prefixes). Returns the number of crates added.
fn ensure_external_dependencies_from_prepared(
    workspace: &Workspace,
    prepared: &PreparedArtifact,
) -> Result<usize> {
    let mut added = 0usize;
    let mut seen_roots: BTreeSet<String> = BTreeSet::new();

    let consider = |ty: &str, added: &mut usize, seen_roots: &mut BTreeSet<String>| -> Result<()> {
        let trimmed = ty.trim_start_matches('&').trim_start();
        let trimmed = trimmed.strip_prefix("mut ").unwrap_or(trimmed);
        let Some(first_segment) = trimmed.split("::").next() else {
            return Ok(());
        };
        if first_segment.is_empty() || !trimmed.contains("::") {
            return Ok(());
        }
        let root = first_segment.trim().to_string();
        if !seen_roots.insert(root.clone()) {
            return Ok(());
        }
        if crate::manifest::ensure_external_dependency_for_type(workspace, trimmed)? {
            *added += 1;
        }
        Ok(())
    };

    for field in &prepared.fields {
        if let Some(rust) = field.type_status.rust() {
            consider(rust, &mut added, &mut seen_roots)?;
        }
    }
    for role in &prepared.roles {
        if let Some(rust) = role.type_status.rust() {
            consider(rust, &mut added, &mut seen_roots)?;
        }
        for method in &role.methods {
            if let Some(rust) = method.return_status.rust() {
                consider(rust, &mut added, &mut seen_roots)?;
            }
            for param in &method.parameters {
                if let Some(rust) = param.type_status.rust() {
                    consider(rust, &mut added, &mut seen_roots)?;
                }
            }
        }
    }
    for prop in &prepared.props {
        if let Some(rust) = prop.type_status.rust() {
            consider(rust, &mut added, &mut seen_roots)?;
        }
    }
    for collab in &prepared.collaborators {
        if let Some(rust) = collab.type_status.rust() {
            consider(rust, &mut added, &mut seen_roots)?;
        }
    }
    for functionality in &prepared.functionalities {
        if let Some(rust) = functionality.return_status.rust() {
            consider(rust, &mut added, &mut seen_roots)?;
        }
        for param in &functionality.parameters {
            if let Some(rust) = param.type_status.rust() {
                consider(rust, &mut added, &mut seen_roots)?;
            }
        }
    }

    Ok(added)
}

/// Second pass over all prepared artifacts: inspect type references and text across the workspace
/// and upgrade each Data artifact's `derives` list with the traits its usages imply.
///
/// Today we infer:
/// - `Eq`, `Hash`, `PartialEq` when a data type appears as the key of `HashMap`, `HashSet`, or
///   `DashMap`.
/// - `Ord`, `PartialOrd`, `Eq`, `PartialEq` when the type is the key of `BTreeMap` / `BTreeSet`.
///
/// These are the most common sources of post-scaffold `E0277`/`E0369` compile errors that our
/// deterministic compile-repair would otherwise have to patch round-by-round.
///
/// Returns the number of artifacts whose `derives` list was modified.
fn apply_cross_artifact_derive_inference(workspace: &Workspace) -> Result<usize> {
    if !workspace.prepared_dir.is_dir() {
        return Ok(0);
    }

    let prepared_paths = collect_all_prepared_yaml_paths(&workspace.prepared_dir)?;
    if prepared_paths.is_empty() {
        return Ok(0);
    }

    // Load everything up front so we can do cross-artifact analysis.
    let mut artifacts: Vec<(PathBuf, PreparedArtifact)> = Vec::with_capacity(prepared_paths.len());
    for path in &prepared_paths {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let parsed: PreparedArtifact = match serde_yaml::from_str(&raw) {
            Ok(value) => value,
            Err(_) => continue,
        };
        artifacts.push((path.clone(), parsed));
    }

    // Index data-kind artifacts by export name so we know which ones we're allowed to patch.
    let mut data_index: BTreeMap<String, usize> = BTreeMap::new();
    for (idx, (_, artifact)) in artifacts.iter().enumerate() {
        if artifact.source.kind == "data" {
            data_index.insert(artifact.export.name.clone(), idx);
        }
    }

    // Accumulate required-derive sets, keyed by the data export name.
    let mut required: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (_, artifact) in &artifacts {
        collect_derive_requirements(artifact, &data_index, &mut required);
    }

    let mut patched = 0usize;
    for (name, traits) in required {
        let Some(&idx) = data_index.get(&name) else {
            continue;
        };
        let (path, artifact) = &mut artifacts[idx];
        let mut existing: BTreeSet<String> = artifact.derives.iter().cloned().collect();
        let before = existing.len();
        for trait_name in &traits {
            existing.insert(trait_name.clone());
        }
        if existing.len() == before {
            continue;
        }
        let mut new_derives: Vec<String> = existing.into_iter().collect();
        new_derives.sort();
        artifact.derives = new_derives;
        let yaml = serde_yaml::to_string(artifact)
            .with_context(|| format!("Failed to serialize {}", path.display()))?;
        fs::write(&*path, yaml).with_context(|| format!("Failed to write {}", path.display()))?;
        patched += 1;
    }

    Ok(patched)
}

/// Second cross-artifact pass: align each role method's first parameter (the role player) with
/// the *collaborator's* matching method receiver.
///
/// For each context role method `<role>.<method>`:
/// 1. Look up the collaborator artifact by the role's exported Rust type name.
/// 2. Find the delegate method on the collaborator:
///    - For data collaborators: getters are always `&self` on immutable data; if the method
///      matches a getter, keep `&T`.
///    - For context/projection collaborators: match a functionality by name and read its
///      `receiver` (`&self`, `&mut self`, or `self`).
/// 3. Set the role-player parameter type to `&T` / `&mut T` / `T` accordingly, overwriting
///    even previously-resolved values because mutability is a structural fact, not a modelling
///    choice.
///
/// Returns the number of artifacts whose role method signatures changed.
fn apply_cross_artifact_role_method_sync(workspace: &Workspace) -> Result<usize> {
    if !workspace.prepared_dir.is_dir() {
        return Ok(0);
    }
    let prepared_paths = collect_all_prepared_yaml_paths(&workspace.prepared_dir)?;
    if prepared_paths.is_empty() {
        return Ok(0);
    }

    let mut artifacts: Vec<(PathBuf, PreparedArtifact)> = Vec::with_capacity(prepared_paths.len());
    for path in &prepared_paths {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let parsed: PreparedArtifact = match serde_yaml::from_str(&raw) {
            Ok(value) => value,
            Err(_) => continue,
        };
        artifacts.push((path.clone(), parsed));
    }

    // Build an index keyed by export name (Rust type name).
    let mut by_export: BTreeMap<String, usize> = BTreeMap::new();
    for (idx, (_, artifact)) in artifacts.iter().enumerate() {
        by_export.insert(artifact.export.name.clone(), idx);
    }

    // Collect sync plans. Each plan: which artifact index to patch + which role method + new
    // parameter type string.
    struct SyncPlan {
        artifact_idx: usize,
        role_idx: usize,
        method_idx: usize,
        new_param_type: String,
        new_receiver: &'static str,
    }
    let mut plans: Vec<SyncPlan> = Vec::new();

    for (idx, (_, artifact)) in artifacts.iter().enumerate() {
        if artifact.source.kind != "context" {
            continue;
        }
        for (ridx, role) in artifact.roles.iter().enumerate() {
            let Some(role_type_rust) = role.type_status.rust() else {
                continue;
            };
            let simple_role_type = simple_type_name(role_type_rust);
            let Some(&collab_idx) = by_export.get(&simple_role_type) else {
                continue;
            };
            let collaborator = &artifacts[collab_idx].1;
            for (midx, role_method) in role.methods.iter().enumerate() {
                let Some(collab_receiver) =
                    lookup_collaborator_receiver(collaborator, &role_method.name)
                else {
                    continue;
                };
                let desired = match collab_receiver {
                    CollabReceiver::RefSelf => ("&".to_string() + &simple_role_type, "&self"),
                    CollabReceiver::MutRefSelf => {
                        ("&mut ".to_string() + &simple_role_type, "&self")
                    }
                    CollabReceiver::OwnedSelf => (simple_role_type.clone(), "&self"),
                };
                let current = role_method
                    .parameters
                    .first()
                    .and_then(|p| p.type_status.rust())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if current == desired.0 {
                    continue;
                }
                plans.push(SyncPlan {
                    artifact_idx: idx,
                    role_idx: ridx,
                    method_idx: midx,
                    new_param_type: desired.0,
                    new_receiver: desired.1,
                });
            }
        }
    }

    // Track which artifacts actually changed so we only re-serialize those.
    let mut changed_idx: BTreeSet<usize> = BTreeSet::new();
    for plan in plans {
        let artifact = &mut artifacts[plan.artifact_idx].1;
        let role = &mut artifact.roles[plan.role_idx];
        let method = &mut role.methods[plan.method_idx];
        if let Some(param) = method.parameters.first_mut() {
            param.type_status =
                ValueStatus::resolved(plan.new_param_type.clone(), "prepare.collab_sync");
        }
        method.receiver = Some(plan.new_receiver.to_string());
        // Re-render the signature string so scaffold picks up the new shape deterministically.
        let return_type = method
            .return_status
            .rust()
            .map(str::to_string)
            .unwrap_or_else(|| "()".to_string());
        let rendered = render_signature(
            &sanitize_identifier(&method.name),
            Some(plan.new_receiver),
            &method.parameters,
            &return_type,
        );
        method.signature = ValueStatus::resolved(rendered, "prepare.collab_sync");
        changed_idx.insert(plan.artifact_idx);
    }

    for idx in &changed_idx {
        let (path, artifact) = &artifacts[*idx];
        let yaml = serde_yaml::to_string(artifact)
            .with_context(|| format!("Failed to serialize {}", path.display()))?;
        fs::write(path, yaml).with_context(|| format!("Failed to write {}", path.display()))?;
    }

    Ok(changed_idx.len())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CollabReceiver {
    RefSelf,
    MutRefSelf,
    OwnedSelf,
}

/// Look up the collaborator's matching method receiver.
///
/// Priority:
/// 1. A `functionality` with the same name → use its `receiver` field.
/// 2. A `getter` with the same name → `&self` (data type).
/// 3. Otherwise: `None` (unknown; leave the role method alone).
fn lookup_collaborator_receiver(
    collaborator: &PreparedArtifact,
    method_name: &str,
) -> Option<CollabReceiver> {
    for functionality in &collaborator.functionalities {
        if functionality.name == method_name {
            return Some(classify_receiver(functionality.receiver.as_deref()));
        }
    }
    for getter in &collaborator.getters {
        if getter.name == method_name {
            return Some(CollabReceiver::RefSelf);
        }
    }
    None
}

fn classify_receiver(receiver: Option<&str>) -> CollabReceiver {
    match receiver.map(str::trim) {
        Some("&mut self") => CollabReceiver::MutRefSelf,
        Some("self") | Some("mut self") => CollabReceiver::OwnedSelf,
        _ => CollabReceiver::RefSelf,
    }
}

fn collect_all_prepared_yaml_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    walk_yaml(root, &mut out)?;
    Ok(out)
}

fn walk_yaml(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_yaml(&path, out)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("yml") {
            out.push(path);
        }
    }
    Ok(())
}

/// Walk all resolved type strings in `artifact` and record required derives for any data export
/// mentioned as a container key.
fn collect_derive_requirements(
    artifact: &PreparedArtifact,
    data_index: &BTreeMap<String, usize>,
    required: &mut BTreeMap<String, BTreeSet<String>>,
) {
    // Collect all type strings worth scanning.
    let mut type_strings: Vec<String> = Vec::new();

    for field in &artifact.fields {
        if let Some(rust) = field.type_status.rust.as_ref() {
            type_strings.push(rust.clone());
        }
    }
    for role in &artifact.roles {
        if let Some(rust) = role.type_status.rust.as_ref() {
            type_strings.push(rust.clone());
        }
        for method in &role.methods {
            push_method_types(method, &mut type_strings);
        }
    }
    for prop in &artifact.props {
        if let Some(rust) = prop.type_status.rust.as_ref() {
            type_strings.push(rust.clone());
        }
    }
    for collaborator in &artifact.collaborators {
        if let Some(rust) = collaborator.type_status.rust.as_ref() {
            type_strings.push(rust.clone());
        }
    }
    for functionality in &artifact.functionalities {
        push_method_types(functionality, &mut type_strings);
    }

    for type_string in &type_strings {
        for (container, key_type) in extract_generic_container_keys(type_string) {
            let simple = simple_type_name(&key_type);
            if !data_index.contains_key(&simple) {
                continue;
            }
            let entry = required.entry(simple).or_default();
            match container.as_str() {
                "HashMap" | "HashSet" | "DashMap" | "IndexMap" | "IndexSet" => {
                    entry.insert("Eq".to_string());
                    entry.insert("Hash".to_string());
                    entry.insert("PartialEq".to_string());
                }
                "BTreeMap" | "BTreeSet" => {
                    entry.insert("Eq".to_string());
                    entry.insert("Ord".to_string());
                    entry.insert("PartialEq".to_string());
                    entry.insert("PartialOrd".to_string());
                }
                _ => {}
            }
        }
    }
}

fn push_method_types(method: &MethodSpec, out: &mut Vec<String>) {
    if let Some(rust) = method.return_status.rust.as_ref() {
        out.push(rust.clone());
    }
    for parameter in &method.parameters {
        if let Some(rust) = parameter.type_status.rust.as_ref() {
            out.push(rust.clone());
        }
    }
}

/// Extract `(container_name, key_type_string)` pairs from a Rust type expression.
///
/// Only recognises a fixed set of map/set containers where the first generic parameter is the
/// hash/order key. Nested occurrences are walked recursively.
fn extract_generic_container_keys(type_string: &str) -> Vec<(String, String)> {
    const CONTAINERS: &[&str] = &[
        "HashMap", "HashSet", "BTreeMap", "BTreeSet", "IndexMap", "IndexSet", "DashMap",
    ];
    let mut results = Vec::new();
    for container in CONTAINERS {
        let mut search_from = 0usize;
        while let Some(idx) = type_string[search_from..].find(container) {
            let absolute = search_from + idx;
            let before_ok = absolute == 0
                || !type_string.as_bytes()[absolute - 1].is_ascii_alphanumeric()
                    && type_string.as_bytes()[absolute - 1] != b'_';
            let after = absolute + container.len();
            if after >= type_string.len() || !before_ok {
                search_from = after;
                continue;
            }
            // Skip any whitespace and require a `<`.
            let tail = &type_string[after..];
            let mut tail_iter = tail.char_indices();
            let mut gen_start = None;
            for (offset, ch) in tail_iter.by_ref() {
                if ch == '<' {
                    gen_start = Some(offset + 1);
                    break;
                }
                if !ch.is_whitespace() {
                    break;
                }
            }
            let Some(gen_start) = gen_start else {
                search_from = after;
                continue;
            };
            let key_slice = tail[gen_start..].to_string();
            let Some(key_type) = first_generic_argument(&key_slice) else {
                search_from = after;
                continue;
            };
            results.push(((*container).to_string(), key_type.trim().to_string()));
            search_from = after;
        }
    }
    results
}

/// Given the contents after the opening `<` of a Rust generic list, return the first comma-
/// separated argument (respecting nested angle brackets).
fn first_generic_argument(inside: &str) -> Option<String> {
    let mut depth = 0i32;
    let mut out = String::new();
    for ch in inside.chars() {
        match ch {
            '<' => {
                depth += 1;
                out.push(ch);
            }
            '>' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                out.push(ch);
            }
            ',' if depth == 0 => break,
            _ => out.push(ch),
        }
    }
    let trimmed = out.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Strip reference markers, module paths, and lifetimes from a Rust type expression.
fn simple_type_name(raw: &str) -> String {
    let mut s = raw.trim();
    while let Some(rest) = s.strip_prefix('&') {
        s = rest.trim_start();
        if let Some(after_mut) = s.strip_prefix("mut ") {
            s = after_mut.trim_start();
        }
        // Skip a single lifetime token like `'a `.
        if let Some(after_tick) = s.strip_prefix('\'') {
            let end = after_tick
                .find(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
                .unwrap_or(after_tick.len());
            s = after_tick[end..].trim_start();
        }
    }
    let last_segment = s.rsplit("::").next().unwrap_or(s);
    // Drop generics (e.g. `Foo<Bar>` → `Foo`).
    let generic_start = last_segment.find('<').unwrap_or(last_segment.len());
    last_segment[..generic_start].trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_catalog(exports: &[&str]) -> DraftCatalog {
        let mut export_set = BTreeSet::new();
        let mut normalized = BTreeMap::new();
        for export in exports {
            let export = export.to_string();
            export_set.insert(export.clone());
            normalized.insert(normalize_symbol(&export), vec![export]);
        }
        DraftCatalog {
            exports: export_set,
            normalized,
            role_hints: BTreeMap::new(),
            resolved_role_types: BTreeMap::new(),
        }
    }

    #[test]
    fn resolve_named_type_prefers_exact_name_match_over_incidental_hint() {
        let catalog = test_catalog(&["TerminalRenderer", "StringRenderer"]);
        let status = resolve_named_type(
            "TerminalRenderer",
            &["Uses `StringRenderer` and shows the latest frame in the terminal."],
            &catalog,
        );
        assert_eq!(status.rust.as_deref(), Some("TerminalRenderer"));
        assert_eq!(status.source.as_deref(), Some("name_match"));
    }

    #[test]
    fn available_types_for_app_uses_manifest_allowlists_and_prefixes() {
        let catalog = test_catalog(&["GameLoopContext"]);
        let manifest = TypesManifest {
            primitives: vec!["u32".to_string()],
            external_path_prefixes: vec!["std::".to_string(), "rand::".to_string()],
            allowlists: ManifestAllowlists {
                data: vec!["Board".to_string()],
                projection: vec!["StringRenderer".to_string()],
                context: vec!["TerminalRenderer".to_string()],
            },
        };

        let available = available_types_for_draft(
            Some(&manifest),
            crate::draft_parser::ArtifactKind::App,
            &catalog,
        );

        assert!(available.contains(&"GameLoopContext".to_string()));
        assert!(available.contains(&"Board".to_string()));
        assert!(available.contains(&"StringRenderer".to_string()));
        assert!(available.contains(&"TerminalRenderer".to_string()));
        assert!(available.contains(&"u32".to_string()));
        assert!(available.contains(&"std::".to_string()));
        assert!(available.contains(&"rand::".to_string()));
        assert!(available.contains(&"rand::rngs::ThreadRng".to_string()));
        assert!(available.contains(&"rand::rngs::StdRng".to_string()));
        assert!(available.contains(&"rand::rngs::SmallRng".to_string()));
    }

    #[test]
    fn nominal_role_type_rejects_function_pointer_guess() {
        let catalog = test_catalog(&["Board", "Snake"]);
        let status = resolve_nominal_role_type(
            "food_dropper",
            "fn(&Board, &Snake) -> Option<Position>",
            &["Chooses food positions"],
            &catalog,
        );

        assert!(!status.is_resolved());
        assert_eq!(
            status.reason.as_deref(),
            Some("role player `food_dropper` must resolve to a nominal concrete type")
        );
    }

    #[test]
    fn role_methods_require_explicit_signature_even_when_role_type_resolves() {
        let methods = prepare_role_methods(
            &RoleMethodGroup {
                role: "command".to_string(),
                methods: vec![crate::draft_parser::RoleMethod {
                    name: "next_action".to_string(),
                    detail: "Returns the next gameplay action.".to_string(),
                }],
            },
            "command",
            &ValueStatus::resolved("CommandInputContext", "test"),
        );

        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].receiver.as_deref(), Some("&self"));
        assert_eq!(methods[0].parameters[0].name, "command_");
        assert_eq!(
            methods[0].parameters[0].type_status.rust.as_deref(),
            Some("&CommandInputContext")
        );
        assert_eq!(
            methods[0].signature.reason.as_deref(),
            Some("role method `next_action` on role `command` has no explicit signature")
        );
        assert!(methods[0].body.is_none());
    }

    #[test]
    fn explicit_role_method_signature_builds_context_wrapper_with_role_parameter() {
        let methods = prepare_role_methods(
            &RoleMethodGroup {
                role: "command".to_string(),
                methods: vec![crate::draft_parser::RoleMethod {
                    name: "next_action".to_string(),
                    detail: "Signature: `next_action() -> Option<UserAction>`".to_string(),
                }],
            },
            "command",
            &ValueStatus::resolved("CommandInputContext", "test"),
        );

        assert_eq!(methods.len(), 1);
        assert_eq!(
            methods[0].signature.rust.as_deref(),
            Some("next_action(&self, command_: &CommandInputContext) -> Option<UserAction>")
        );
        assert_eq!(methods[0].parameters.len(), 1);
        assert_eq!(methods[0].parameters[0].name, "command_");
    }

    #[test]
    fn role_type_is_not_inferred_from_matching_method_surface() {
        let catalog = test_catalog(&["CommandInputContext"]);
        let status = resolve_nominal_role_type_from_texts(
            "command",
            &[
                "Shared input stream",
                "Captures keys and returns the next gameplay action",
            ],
            &catalog,
        );

        assert!(!status.is_resolved());
        assert_eq!(
            status.reason.as_deref(),
            Some("no nominal concrete Rust type could be inferred for `command`")
        );
    }

    #[test]
    fn app_collaborator_type_uses_matching_context_role_hints_as_evidence() {
        let context = CompositeDraft {
            info: crate::draft_parser::DraftInfo {
                kind: crate::draft_parser::ArtifactKind::Context,
                relative_path: "contexts/game_loop.md".to_string(),
                title: "GameLoopContext".to_string(),
            },
            purpose: "game loop".to_string(),
            roles: vec![crate::draft_parser::RolePlayer {
                name: "food_dropper".to_string(),
                why_involved: "an RNG used to choose new food positions".to_string(),
                expected_behavior:
                    "Chooses a free interior food position, or no position if none is available"
                        .to_string(),
                explicit_type: None,
            }],
            role_methods: Vec::new(),
            props: Vec::new(),
            functionalities: Vec::new(),
            notes: Vec::new(),
        };
        let catalog = build_catalog(&[DraftDocument::Context(context)], BTreeMap::new());
        let status = resolve_nominal_collaborator_type_from_texts(
            "food_dropper",
            &["Chooses the next food placement on a free interior non-wall, non-snake cell."],
            &catalog,
        );

        assert!(!status.is_resolved());
        let evidence_texts = status
            .evidence
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>();
        assert!(evidence_texts.contains(&"an RNG used to choose new food positions"));
        assert!(evidence_texts.contains(
            &"Chooses a free interior food position, or no position if none is available"
        ));
    }

    #[test]
    fn prepare_app_keeps_prose_flow_when_body_normalization_fails() {
        let app = AppDraft {
            info: crate::draft_parser::DraftInfo {
                kind: crate::draft_parser::ArtifactKind::App,
                relative_path: "app.md".to_string(),
                title: "Example App".to_string(),
            },
            application_kind: Some("cli_app".to_string()),
            sections: BTreeMap::from([(
                "Main Flow".to_string(),
                "- Keep the greeting interaction running in the appropriate way for the current situation."
                    .to_string(),
            )]),
            collaborators: Vec::new(),
        };

        let prepared = prepare_app(&app, &test_catalog(&[])).expect("prepare app");
        let main = prepared
            .functionalities
            .first()
            .expect("prepared app main functionality");

        assert!(main.body.is_none());
        assert_eq!(
            main.flow,
            vec!["Keep the greeting interaction running in the appropriate way for the current situation.".to_string()]
        );
        assert!(prepared.ambiguities.is_empty());
    }

    #[test]
    fn snake_render_contracts_prepare_with_board_and_u32_signatures() {
        let drafts_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("snake")
            .join("drafts");

        let load = |relative: &str| {
            let path = drafts_root.join(relative);
            let content = std::fs::read_to_string(&path).expect("read snake draft");
            parse_draft_file(&path, &drafts_root, &content).expect("parse snake draft")
        };

        let board_doc = load("data/Board.md");
        let snake_doc = load("data/Snake.md");
        let food_doc = load("data/Food.md");
        let game_state_doc = load("data/GameState.md");
        let game_loop_doc = load("contexts/game_loop.md");
        let string_renderer_doc = load("projections/string_renderer.md");
        let terminal_renderer_doc = load("contexts/terminal_renderer.md");

        let docs = vec![
            board_doc.clone(),
            snake_doc,
            food_doc,
            game_state_doc,
            game_loop_doc.clone(),
            string_renderer_doc.clone(),
            terminal_renderer_doc.clone(),
        ];
        let catalog = build_catalog(&docs, BTreeMap::new());

        let board = prepare_document(&board_doc, &catalog).expect("prepare board");
        let game_loop = prepare_document(&game_loop_doc, &catalog).expect("prepare game loop");
        let string_renderer =
            prepare_document(&string_renderer_doc, &catalog).expect("prepare string renderer");
        let terminal_renderer =
            prepare_document(&terminal_renderer_doc, &catalog).expect("prepare terminal renderer");

        let cells = board
            .fields
            .iter()
            .find(|field| field.name == "cells")
            .expect("board cells field");
        assert_eq!(
            cells.type_status.rust.as_deref(),
            Some("std::collections::HashMap<Position, char>")
        );
        let symbol_at = board
            .functionalities
            .iter()
            .find(|method| method.name == "symbol_at")
            .expect("Board::symbol_at");
        assert_eq!(
            symbol_at.signature.rust.as_deref(),
            Some("symbol_at(&self, x: u32, y: u32) -> char")
        );

        let current_board = game_loop
            .functionalities
            .iter()
            .find(|method| method.name == "current_board")
            .expect("GameLoopContext::current_board");
        assert_eq!(current_board.return_status.rust.as_deref(), Some("Board"));

        let board_role = string_renderer
            .roles
            .iter()
            .find(|role| role.name == "board")
            .expect("StringRenderer board role");
        let width = board_role
            .methods
            .iter()
            .find(|method| method.name == "width")
            .expect("board.width");
        assert_eq!(
            width.signature.rust.as_deref(),
            Some("width(&self, board_: &Board) -> u32")
        );
        let symbol_at_role = board_role
            .methods
            .iter()
            .find(|method| method.name == "symbol_at")
            .expect("board.symbol_at");
        assert_eq!(
            symbol_at_role.signature.rust.as_deref(),
            Some("symbol_at(&self, board_: &Board, x: u32, y: u32) -> char")
        );
        let render = string_renderer
            .functionalities
            .iter()
            .find(|method| method.name == "render")
            .expect("StringRenderer::render");
        assert_eq!(
            render.signature.rust.as_deref(),
            Some("render(&self, board: &Board, score: u32) -> String")
        );

        let render_role = terminal_renderer
            .roles
            .iter()
            .find(|role| role.name == "string_renderer")
            .and_then(|role| role.methods.iter().find(|method| method.name == "render"))
            .expect("TerminalRenderer string_renderer.render");
        assert_eq!(
            render_role.signature.rust.as_deref(),
            Some(
                "render(&self, string_renderer_: &StringRenderer, board: &Board, score: u32) -> String"
            )
        );
        let terminal_render = terminal_renderer
            .functionalities
            .iter()
            .find(|method| method.name == "render")
            .expect("TerminalRenderer::render");
        assert_eq!(
            terminal_render.signature.rust.as_deref(),
            Some("render(&self, board: &Board, score: u32) -> anyhow::Result<()>")
        );
    }
}
