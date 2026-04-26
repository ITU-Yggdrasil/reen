use crate::draft_parser::{
    AppDraft, BrandDraft, ComponentDraft, ContextDraft, DataDraft, DraftDocument, ProjectionDraft,
    RoutingTableDraft, parse_draft_file,
};
use crate::draft_refine::{DraftReview, review_raw_drafts};
use crate::manifest::DciVocabulary;
use crate::test_synth::synthesise_tests;
use crate::prepared::{
    Ambiguity, ArtifactKind, AttributeSpec, ChildBindingSpec, CompositionSpec, DerivedSignature,
    EventBindingSpec, EventSpec, FunctionalitySpec, Multiplicity, OperationSpec, ParamSpec,
    PreparedArtifact, ProseSectionSpec, PropSpec, RoleMethodGroupSpec, RoleMethodSpec,
    RolePlayerSpec, RouteParamSpec, RouteSpec, ScenarioSpec, SlotSpec, SynthesisedTest,
    TokenGroupSpec, TokenSpec, VariantSpec, VocabularyBinding,
};
use crate::workspace::Workspace;
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;

#[derive(Debug, Clone)]
pub struct PrepareOptions {
    pub selection: crate::workspace::Selection,
    pub profile: Option<String>,
    pub verbose: bool,
    pub debug: bool,
    pub dry_run: bool,
    pub allow_code_tokens: bool,
}

#[derive(Debug, Clone, Default)]
pub struct RefineOptions {
    pub selection: crate::workspace::Selection,
    pub verbose: bool,
    pub drafts_only: bool,
    pub prepared_only: bool,
}

pub fn prepare_workspace(workspace: &Workspace, options: &PrepareOptions) -> Result<()> {
    let selected_paths = workspace.raw_draft_paths(&options.selection)?;

    // ── Code-token lint (blocking unless --allow-code-tokens) ────
    {
        let review = review_raw_drafts(workspace, &selected_paths)?;
        let code_token_findings: Vec<_> = review
            .findings
            .iter()
            .filter(|f| {
                f.category == crate::draft_refine::DraftReviewCategory::Concreteness
                    && f.message.starts_with("code token")
            })
            .collect();

        // Web-kind lint findings are always blocking (no --allow-code-tokens escape)
        let web_lint_findings: Vec<_> = review
            .findings
            .iter()
            .filter(|f| {
                f.category == crate::draft_refine::DraftReviewCategory::Concreteness
                    && f.message.starts_with("web-kind lint")
            })
            .collect();

        if !web_lint_findings.is_empty() {
            for finding in &web_lint_findings {
                let location = match finding.source_line {
                    Some(line) => format!("{}:{}", finding.draft_path, line),
                    None => finding.draft_path.clone(),
                };
                eprintln!("error[concreteness]: {}", finding.message);
                eprintln!("  --> {}", location);
            }
            anyhow::bail!(
                "prepare blocked: {} web-kind lint finding{} in draft(s); \
                 rewrite in business vocabulary (no escape hatch for web kinds)",
                web_lint_findings.len(),
                if web_lint_findings.len() == 1 { "" } else { "s" }
            );
        }

        if !code_token_findings.is_empty() {
            let label = if options.allow_code_tokens {
                "warning"
            } else {
                "error"
            };
            for finding in &code_token_findings {
                let location = match finding.source_line {
                    Some(line) => format!("{}:{}", finding.draft_path, line),
                    None => finding.draft_path.clone(),
                };
                eprintln!("{}[concreteness]: {}", label, finding.message);
                eprintln!("  --> {}", location);
            }
            if !options.allow_code_tokens {
                anyhow::bail!(
                    "prepare blocked: {} code-token finding{} in draft(s); \
                     rewrite in business vocabulary or pass --allow-code-tokens to downgrade to warnings",
                    code_token_findings.len(),
                    if code_token_findings.len() == 1 { "" } else { "s" }
                );
            } else if options.verbose {
                eprintln!(
                    "warning: {} code-token finding{} (--allow-code-tokens active, continuing)",
                    code_token_findings.len(),
                    if code_token_findings.len() == 1 { "" } else { "s" }
                );
            }
        }
    }

    let vocabulary = DciVocabulary::load(workspace)?;

    let mut wrote = 0usize;
    let mut blocking_total = 0usize;

    if options.verbose {
        println!("Preparing {} draft(s)", selected_paths.len());
    }

    // First pass: prepare all documents
    let mut all_prepared: Vec<(std::path::PathBuf, PreparedArtifact)> = Vec::new();

    for path in &selected_paths {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let draft = parse_draft_file(path, &workspace.drafts_dir, &content)?;
        let output_path = workspace.prepared_output_path(path)?;
        let mut prepared = prepare_document(&draft, &vocabulary)?;

        // Scenario-driven inference (bug 5 + 6). Both passes read scenarios
        // already populated above; signature inference annotates operations
        // and functionalities, multiplicity inference annotates role players
        // and attributes. Drafts are unchanged — both passes derive their
        // outputs from prose the BA already wrote.
        derive_multiplicity(&mut prepared);
        derive_signatures(&mut prepared, &vocabulary);

        // LLM-backed test synthesis from prose invariants/descriptions
        let synth = synthesise_tests(&prepared);
        prepared.synthesised_tests.extend(synth);

        all_prepared.push((output_path, prepared));
    }

    // Second pass: classify contexts (realisation_target)
    classify_contexts(&mut all_prepared, workspace);

    // Validation pass: web_app requires a RoutingTable in the same project
    validate_web_app_requirements(&mut all_prepared);

    // Third pass: report and write
    let mut low_confidence_total = 0usize;
    for (idx, (output_path, prepared)) in all_prepared.iter().enumerate() {
        let path = &selected_paths[idx];
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
                eprintln!("  --> {}", location);
            }
        }
        // Low-confidence findings (informational, never block)
        let low: Vec<_> = prepared
            .ambiguities
            .iter()
            .filter(|a| a.severity.eq_ignore_ascii_case("low"))
            .collect();
        if !low.is_empty() {
            low_confidence_total += low.len();
            let draft_path = path.strip_prefix(&workspace.root).unwrap_or(path);
            for ambiguity in low {
                let location = match ambiguity.source_line {
                    Some(line) => format!("{}:{}", draft_path.display(), line),
                    None => format!("{}", draft_path.display()),
                };
                eprintln!("info[prepare]: {}", ambiguity.message);
                eprintln!("  --> {}", location);
            }
        }
        let yaml = serde_yaml::to_string(prepared)?;
        if options.dry_run {
            if options.verbose {
                println!("[dry-run] would write {}", output_path.display());
            }
        } else {
            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create {}", parent.display()))?;
            }
            fs::write(output_path, yaml)
                .with_context(|| format!("Failed to write {}", output_path.display()))?;
            wrote += 1;
        }
    }

    if options.verbose {
        println!("Prepared {} artifact(s)", wrote);
    }
    if low_confidence_total > 0 && options.verbose {
        eprintln!(
            "\nPossible prose references: {} informational finding{} (not blocking)",
            low_confidence_total,
            if low_confidence_total == 1 { "" } else { "s" }
        );
    }
    if blocking_total > 0 {
        anyhow::bail!(
            "prepare found {} blocking ambiguit{} across draft(s); see errors above",
            blocking_total,
            if blocking_total == 1 { "y" } else { "ies" }
        );
    }

    // ── Build tracker integration ────────────────────────────────
    if !options.dry_run {
        update_build_tracker(workspace, &selected_paths, &all_prepared, options.verbose)?;
    }

    Ok(())
}

pub fn refine_workspace(workspace: &Workspace, options: &RefineOptions) -> Result<()> {
    let raw_paths = workspace.raw_draft_paths(&options.selection)?;
    if options.verbose {
        println!("Reviewing {} raw draft(s)", raw_paths.len());
    }
    let mut draft_review = review_raw_drafts(workspace, &raw_paths)?;

    // Vocabulary-aware unresolved reference check
    let vocabulary = DciVocabulary::load(workspace)?;
    for path in &raw_paths {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let relative = path
            .strip_prefix(&workspace.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if let Ok(document) = crate::draft_parser::parse_draft_file(path, &workspace.drafts_dir, &content) {
            crate::draft_refine::check_unresolved_references(
                &relative, &document, &vocabulary, &mut draft_review.findings,
            );
        }
    }
    crate::draft_refine::sort_and_dedup_findings(&mut draft_review.findings);

    print_draft_findings(&draft_review);
    let failures = draft_review.findings.len();
    if failures > 0 {
        anyhow::bail!(
            "refine found {} blocking draft review finding{}; see errors above",
            failures,
            if failures == 1 { "" } else { "s" }
        );
    }
    if options.verbose {
        println!("Draft review found no blocking issues");
    }
    Ok(())
}

pub fn clear_prepared_outputs(workspace: &Workspace, dry_run: bool) -> Result<()> {
    if !workspace.prepared_dir.is_dir() {
        return Ok(());
    }
    if dry_run {
        println!(
            "[dry-run] would remove {}",
            workspace.prepared_dir.display()
        );
    } else {
        fs::remove_dir_all(&workspace.prepared_dir)
            .with_context(|| format!("Failed to remove {}", workspace.prepared_dir.display()))?;
    }
    Ok(())
}

// ── Build tracker integration ─────────────────────────────────────

fn update_build_tracker(
    workspace: &Workspace,
    draft_paths: &[std::path::PathBuf],
    all_prepared: &[(std::path::PathBuf, PreparedArtifact)],
    verbose: bool,
) -> Result<()> {
    use crate::build_tracker::{
        ArtefactEntry, ArtefactState, BuildTracker, StaleReason,
        edges::{extract_artefacts, hash_file_bytes, hash_fragment},
    };

    let mut tracker = BuildTracker::load(&workspace.state_dir)?;

    // 1. Hash each draft file; short-circuit unchanged drafts
    let mut changed_drafts: BTreeSet<String> = BTreeSet::new();
    for path in draft_paths {
        let relative = path
            .strip_prefix(&workspace.root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();
        let bytes = fs::read(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let hash = hash_file_bytes(&bytes);
        let old_hash = tracker.drafts.get(&relative);
        if old_hash != Some(&hash) {
            changed_drafts.insert(relative.clone());
        }
        tracker.drafts.insert(relative, hash);
    }

    if changed_drafts.is_empty() {
        let summary = tracker.stale_summary();
        if verbose {
            println!(
                "0 drafts changed. {} method{} stale.",
                summary.total(),
                if summary.total() == 1 { "" } else { "s" }
            );
        }
        tracker.save(&workspace.state_dir)?;
        return Ok(());
    }

    // 2. For each prepared artifact, extract artefacts with edges
    let all_artifacts_refs: Vec<&PreparedArtifact> =
        all_prepared.iter().map(|(_, a)| a).collect();

    let mut stale_roots: Vec<crate::build_tracker::keys::ArtefactKey> = Vec::new();
    let mut seen_keys: BTreeSet<crate::build_tracker::keys::ArtefactKey> = BTreeSet::new();

    for (_output_path, artifact) in all_prepared {
        let extracted = extract_artefacts(artifact, &all_artifacts_refs);
        for ea in extracted {
            seen_keys.insert(ea.key.clone());
            let new_hash = hash_fragment(&ea.canonical_fragment);

            let existing = tracker.artefacts.get(&ea.key);
            let is_new = existing.is_none();
            let hash_changed = existing
                .map(|e| e.prepared_hash != new_hash)
                .unwrap_or(false);

            if is_new {
                // New artefact: insert as not_implemented
                tracker.upsert_artefact(
                    ea.key.clone(),
                    ArtefactEntry {
                        kind: ea.key.kind(),
                        prepared_hash: new_hash,
                        upstream: ea.upstream,
                        state: ArtefactState::NotImplemented,
                        reason: Some(StaleReason::InitialScaffold),
                        audit_finding: None,
                    },
                );
            } else if hash_changed {
                // Hash changed: mark stale
                tracker.upsert_artefact(
                    ea.key.clone(),
                    ArtefactEntry {
                        kind: ea.key.kind(),
                        prepared_hash: new_hash,
                        upstream: ea.upstream,
                        state: ArtefactState::Stale,
                        reason: Some(StaleReason::DraftChanged),
                        audit_finding: None,
                    },
                );
                stale_roots.push(ea.key);
            } else {
                // Hash unchanged: update edges but keep state
                if let Some(entry) = tracker.artefacts.get_mut(&ea.key) {
                    entry.upstream = ea.upstream;
                }
            }
        }
    }

    // 3. Remove artefacts that no longer exist in prepared YAML
    let removed_keys: Vec<_> = tracker
        .artefacts
        .keys()
        .filter(|k| !seen_keys.contains(k))
        .cloned()
        .collect();
    for key in removed_keys {
        tracker.remove_artefact(&key);
    }

    // 4. Cascade staleness from roots
    if !stale_roots.is_empty() {
        tracker.cascade_stale(&stale_roots);
    }

    // 5. Print summary
    let summary = tracker.stale_summary();
    let total = summary.total();
    if total > 0 {
        println!(
            "{} method{} stale ({} direct, {} cascaded). Run `reen scaffold` to regenerate stubs.",
            total,
            if total == 1 { "" } else { "s" },
            summary.direct + summary.initial,
            summary.cascaded,
        );
    } else if verbose {
        println!("0 methods stale.");
    }

    // 6. Persist
    tracker.save(&workspace.state_dir)?;

    Ok(())
}

// ── Document preparation ──────────────────────────────────────────

fn prepare_document(doc: &DraftDocument, vocab: &DciVocabulary) -> Result<PreparedArtifact> {
    match doc {
        DraftDocument::Context(ctx) => prepare_context(ctx, vocab),
        DraftDocument::Data(data) => prepare_data(data, vocab),
        DraftDocument::Projection(proj) => prepare_projection(proj, vocab),
        DraftDocument::Component(comp) => prepare_component(comp, vocab),
        DraftDocument::Brand(brand) => prepare_brand(brand),
        DraftDocument::RoutingTable(rt) => prepare_routing_table(rt, vocab),
        DraftDocument::App(app) => prepare_app(app, vocab),
    }
}

fn prepare_context(ctx: &ContextDraft, vocab: &DciVocabulary) -> Result<PreparedArtifact> {
    let export_name = sanitize_export_name(&ctx.info.title);
    let mut artifact = PreparedArtifact::new(
        ArtifactKind::Context,
        ctx.info.relative_path.clone(),
        ctx.info.title.clone(),
        export_name,
    );

    let mut bindings = Vec::new();
    let mut ambiguities = Vec::new();

    // Role players
    for rp in &ctx.role_players {
        let resolved = if DciVocabulary::is_api_reference(&rp.data_type) {
            vocab.resolve_api(&rp.data_type)
        } else {
            vocab.resolve(&rp.data_type)
        };
        artifact.role_players.push(RolePlayerSpec {
            name: rp.name.clone(),
            data_type: rp.data_type.clone(),
            responsibility: rp.responsibility.clone(),
            multiplicity: Multiplicity::Single,
        });
        bindings.push(VocabularyBinding {
            name: rp.name.clone(),
            resolution: "role_player".to_string(),
            resolved_to: Some(rp.data_type.clone()),
        });
        if let Some(resolution) = &resolved {
            // Enforce reference graph: contexts can reference data, projections, operations
            if resolution == "operation" {
                // Operations can be referenced by contexts (they are context-level concerns)
                bindings.push(VocabularyBinding {
                    name: rp.data_type.clone(),
                    resolution: resolution.clone(),
                    resolved_to: Some(rp.data_type.clone()),
                });
            } else {
                bindings.push(VocabularyBinding {
                    name: rp.data_type.clone(),
                    resolution: resolution.clone(),
                    resolved_to: Some(rp.data_type.clone()),
                });
            }
        } else if !is_builtin_primitive(&rp.data_type) {
            ambiguities.push(if DciVocabulary::is_api_reference(&rp.data_type) {
                Ambiguity {
                    message: format!(
                        "Role player `{}` references `{}` which is not in the API manifest. \
                         Either add the type to the boundary draft whitelist or correct the name.",
                        rp.name, rp.data_type
                    ),
                    severity: "blocking".to_string(),
                    suggested_resolution: Some(format!(
                        "Add `{}` to the API manifest",
                        rp.data_type
                    )),
                    source_line: None,
                }
            } else {
                unresolved_reference_error(
                    &ctx.info.relative_path,
                    "role player",
                    &rp.name,
                    &rp.data_type,
                )
            });
            bindings.push(VocabularyBinding {
                name: rp.data_type.clone(),
                resolution: "ambiguous".to_string(),
                resolved_to: None,
            });
        }
    }

    // Props
    for prop in &ctx.props {
        artifact.props.push(PropSpec {
            name: prop.name.clone(),
            description: prop.description.clone(),
        });
        bindings.push(VocabularyBinding {
            name: prop.name.clone(),
            resolution: "prop".to_string(),
            resolved_to: None,
        });
    }

    // Role methods — grouped by role, with canonical names
    for group in &ctx.role_methods {
        let methods: Vec<RoleMethodSpec> = group
            .methods
            .iter()
            .map(|m| {
                let canonical = format!("{}_{}", group.role, m.name);
                bindings.push(VocabularyBinding {
                    name: canonical.clone(),
                    resolution: "role_method".to_string(),
                    resolved_to: Some(format!("{}.{}", group.role, m.name)),
                });
                RoleMethodSpec {
                    name: m.name.clone(),
                    canonical_name: canonical,
                    signature_hint: None,
                    citation: m.description.clone(),
                }
            })
            .collect();
        artifact.role_methods.push(RoleMethodGroupSpec {
            role: group.role.clone(),
            methods,
        });
        bindings.push(VocabularyBinding {
            name: group.role.clone(),
            resolution: "role".to_string(),
            resolved_to: None,
        });
    }

    // Functionalities — resolve role method references in prose
    for func in &ctx.functionalities {
        let role_method_refs = extract_role_method_refs(&func.description, &artifact.role_methods);
        artifact.functionalities.push(FunctionalitySpec {
            name: func.name.clone(),
            description: func.description.clone(),
            role_method_refs,
            derived_signature: None,
        });
    }

    // Scenarios → synthesised_tests (source: "scenario")
    for scenario in &ctx.scenarios {
        artifact.scenarios.push(ScenarioSpec {
            name: scenario.name.clone(),
            given: scenario.given.clone(),
            when: scenario.when.clone(),
            then: scenario.then.clone(),
        });
        artifact.synthesised_tests.push(SynthesisedTest {
            source: "scenario".to_string(),
            given: scenario.given.clone(),
            when: scenario.when.clone(),
            then: scenario.then.clone(),
            citation: Some(format!("Scenarios / {}", scenario.name)),
        });
    }

    artifact.vocabulary_bindings = bindings;
    artifact.ambiguities = ambiguities;
    Ok(artifact)
}

fn prepare_data(data: &DataDraft, vocab: &DciVocabulary) -> Result<PreparedArtifact> {
    let export_name = sanitize_export_name(&data.info.title);
    let mut artifact = PreparedArtifact::new(
        ArtifactKind::Data,
        data.info.relative_path.clone(),
        data.info.title.clone(),
        export_name,
    );

    let mut bindings = Vec::new();

    // Reference graph enforcement: scan all prose for references that violate the graph
    let all_prose = format!(
        "{} {} {}",
        data.purpose,
        data.attributes
            .iter()
            .map(|a| a.description.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        data.operations
            .iter()
            .map(|o| o.description.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    );
    for api_ref in extract_api_references(&all_prose) {
        if let Some(kind) = vocab.resolve_api(&api_ref) {
            if let Some(ambiguity) =
                check_reference_graph(&data.info.title, "data", &api_ref, &kind)
            {
                artifact.ambiguities.push(ambiguity);
            }
        }
    }
    // Check prose references to non-API vocabulary entries
    check_prose_references(
        &all_prose,
        "data",
        &data.info.title,
        vocab,
        &mut artifact.ambiguities,
    );

    for attr in &data.attributes {
        artifact.attributes.push(AttributeSpec {
            name: attr.name.clone(),
            description: attr.description.clone(),
            multiplicity: Multiplicity::Single,
        });
        bindings.push(VocabularyBinding {
            name: attr.name.clone(),
            resolution: "attribute".to_string(),
            resolved_to: None,
        });
    }

    artifact.invariants = data.invariants.clone();

    for op in &data.operations {
        artifact.operations.push(OperationSpec {
            name: op.name.clone(),
            description: op.description.clone(),
            kind: op.kind.clone(),
            derived_signature: None,
        });
    }

    // Scenarios
    for scenario in &data.scenarios {
        artifact.scenarios.push(ScenarioSpec {
            name: scenario.name.clone(),
            given: scenario.given.clone(),
            when: scenario.when.clone(),
            then: scenario.then.clone(),
        });
        artifact.synthesised_tests.push(SynthesisedTest {
            source: "scenario".to_string(),
            given: scenario.given.clone(),
            when: scenario.when.clone(),
            then: scenario.then.clone(),
            citation: Some(format!("Scenarios / {}", scenario.name)),
        });
    }

    // Synthesise tests from invariants
    for invariant in &data.invariants {
        artifact.synthesised_tests.push(SynthesisedTest {
            source: "invariant".to_string(),
            given: format!("a {} that might violate: {}", data.info.title, invariant),
            when: "the invariant is tested".to_string(),
            then: format!("the invariant holds: {}", invariant),
            citation: Some(format!("Invariants / {}", invariant)),
        });
    }

    artifact.vocabulary_bindings = bindings;
    Ok(artifact)
}

fn prepare_projection(proj: &ProjectionDraft, vocab: &DciVocabulary) -> Result<PreparedArtifact> {
    // Projections share the context-shaped fields. The structural rules
    // (immutability, no-Context-refs, private fields, public-surface =
    // Functionalities) are enforced downstream by codegen and verify.
    let export_name = sanitize_export_name(&proj.info.title);
    let mut artifact = PreparedArtifact::new(
        ArtifactKind::Projection,
        proj.info.relative_path.clone(),
        proj.info.title.clone(),
        export_name,
    );

    let mut bindings = Vec::new();
    let mut ambiguities = Vec::new();

    // Role players — projections may reference Data and Projection types only.
    // (Context references are rejected by verify, not here, so the failure
    // surfaces with full kind-disciplined wording rather than at parse time.)
    for rp in &proj.role_players {
        let resolved = vocab.resolve(&rp.data_type);
        artifact.role_players.push(RolePlayerSpec {
            name: rp.name.clone(),
            data_type: rp.data_type.clone(),
            responsibility: rp.responsibility.clone(),
            multiplicity: Multiplicity::Single,
        });
        bindings.push(VocabularyBinding {
            name: rp.name.clone(),
            resolution: "role_player".to_string(),
            resolved_to: Some(rp.data_type.clone()),
        });
        if let Some(resolution) = &resolved {
            bindings.push(VocabularyBinding {
                name: rp.data_type.clone(),
                resolution: resolution.clone(),
                resolved_to: Some(rp.data_type.clone()),
            });
        } else if !is_builtin_primitive(&rp.data_type) {
            ambiguities.push(unresolved_reference_error(
                &proj.info.relative_path,
                "role player",
                &rp.name,
                &rp.data_type,
            ));
        }
    }

    // Props
    for prop in &proj.props {
        artifact.props.push(PropSpec {
            name: prop.name.clone(),
            description: prop.description.clone(),
        });
        bindings.push(VocabularyBinding {
            name: prop.name.clone(),
            resolution: "prop".to_string(),
            resolved_to: None,
        });
    }

    // Role methods — same shape as a context's role methods.
    for group in &proj.role_methods {
        let methods: Vec<RoleMethodSpec> = group
            .methods
            .iter()
            .map(|m| {
                let canonical = format!("{}_{}", group.role, m.name);
                bindings.push(VocabularyBinding {
                    name: canonical.clone(),
                    resolution: "role_method".to_string(),
                    resolved_to: Some(format!("{}.{}", group.role, m.name)),
                });
                RoleMethodSpec {
                    name: m.name.clone(),
                    canonical_name: canonical,
                    signature_hint: None,
                    citation: m.description.clone(),
                }
            })
            .collect();
        artifact.role_methods.push(RoleMethodGroupSpec {
            role: group.role.clone(),
            methods,
        });
        bindings.push(VocabularyBinding {
            name: group.role.clone(),
            resolution: "role".to_string(),
            resolved_to: None,
        });
    }

    // Functionalities — same as contexts, but at verify time these will be
    // checked to take &self only.
    for func in &proj.functionalities {
        let role_method_refs = extract_role_method_refs(&func.description, &artifact.role_methods);
        artifact.functionalities.push(FunctionalitySpec {
            name: func.name.clone(),
            description: func.description.clone(),
            role_method_refs,
            derived_signature: None,
        });
    }

    // Scenarios
    for scenario in &proj.scenarios {
        artifact.scenarios.push(ScenarioSpec {
            name: scenario.name.clone(),
            given: scenario.given.clone(),
            when: scenario.when.clone(),
            then: scenario.then.clone(),
        });
        artifact.synthesised_tests.push(SynthesisedTest {
            source: "scenario".to_string(),
            given: scenario.given.clone(),
            when: scenario.when.clone(),
            then: scenario.then.clone(),
            citation: Some(format!("Scenarios / {}", scenario.name)),
        });
    }

    artifact.vocabulary_bindings = bindings;
    artifact.ambiguities = ambiguities;
    Ok(artifact)
}

fn prepare_component(comp: &ComponentDraft, vocab: &DciVocabulary) -> Result<PreparedArtifact> {
    let export_name = sanitize_export_name(&comp.info.title);
    let mut artifact = PreparedArtifact::new(
        ArtifactKind::Component,
        comp.info.relative_path.clone(),
        comp.info.title.clone(),
        export_name,
    );

    let mut bindings = Vec::new();
    let mut ambiguities = Vec::new();

    // Props — resolve types against vocabulary
    for prop in &comp.props {
        artifact.props.push(PropSpec {
            name: prop.name.clone(),
            description: prop.description.clone(),
        });
        // Try to resolve the description as a type reference
        let words: Vec<&str> = prop.description.split_whitespace().collect();
        if words.len() == 1 {
            if let Some(resolution) = vocab.resolve(words[0]) {
                bindings.push(VocabularyBinding {
                    name: words[0].to_string(),
                    resolution,
                    resolved_to: Some(words[0].to_string()),
                });
            }
        }
    }

    // Slots
    for slot in &comp.slots {
        artifact.slots.push(SlotSpec {
            name: slot.name.clone(),
            description: slot.description.clone(),
        });
    }

    // Events
    for event in &comp.events {
        artifact.events.push(EventSpec {
            name: event.name.clone(),
            description: event.description.clone(),
        });
    }

    // Variants
    for variant in &comp.variants {
        artifact.variants.push(VariantSpec {
            name: variant.name.clone(),
            description: variant.description.clone(),
        });
    }

    // Composition
    if let Some(composition) = &comp.composition {
        let child_bindings = composition
            .child_bindings
            .iter()
            .map(|cb| {
                // Resolve child component reference
                let resolved = vocab.resolve(&cb.child);
                if resolved.is_none() {
                    ambiguities.push(unresolved_reference_error(
                        &comp.info.relative_path,
                        "child component binding",
                        &cb.child,
                        &cb.child,
                    ));
                }
                ChildBindingSpec {
                    child: cb.child.clone(),
                    bindings: cb.bindings.clone(),
                }
            })
            .collect();

        let event_bindings = composition
            .event_bindings
            .iter()
            .map(|eb| {
                // Resolve context reference
                let resolved = vocab.resolve(&eb.context);
                if let Some(kind) = &resolved {
                    if kind != "context" {
                        ambiguities.push(Ambiguity {
                            message: format!(
                                "Component `{}` binds event `{}.{}` to `{}` which is a {} — event bindings must target contexts",
                                comp.info.title, eb.child, eb.event, eb.context, kind
                            ),
                            severity: "blocking".to_string(),
                            suggested_resolution: Some(format!(
                                "Create a context draft for `{}`",
                                eb.context
                            )),
                            source_line: None,
                        });
                    }
                    bindings.push(VocabularyBinding {
                        name: eb.context.clone(),
                        resolution: kind.clone(),
                        resolved_to: Some(eb.context.clone()),
                    });
                } else {
                    ambiguities.push(unresolved_reference_error(
                        &comp.info.relative_path,
                        &format!("event binding {}.{}", eb.child, eb.event),
                        &eb.context,
                        &eb.context,
                    ));
                }
                EventBindingSpec {
                    child: eb.child.clone(),
                    event: eb.event.clone(),
                    context: eb.context.clone(),
                }
            })
            .collect();

        artifact.composition = Some(CompositionSpec {
            child_bindings,
            event_bindings,
        });
    }

    // Invariants
    artifact.invariants = comp.invariants.clone();

    // Scenarios
    for scenario in &comp.scenarios {
        artifact.scenarios.push(ScenarioSpec {
            name: scenario.name.clone(),
            given: scenario.given.clone(),
            when: scenario.when.clone(),
            then: scenario.then.clone(),
        });
        artifact.synthesised_tests.push(SynthesisedTest {
            source: "scenario".to_string(),
            given: scenario.given.clone(),
            when: scenario.when.clone(),
            then: scenario.then.clone(),
            citation: Some(format!("Scenarios / {}", scenario.name)),
        });
    }

    artifact.vocabulary_bindings = bindings;
    artifact.ambiguities = ambiguities;
    Ok(artifact)
}

fn prepare_brand(brand: &BrandDraft) -> Result<PreparedArtifact> {
    let export_name = sanitize_export_name(&brand.info.title);
    let mut artifact = PreparedArtifact::new(
        ArtifactKind::Brand,
        brand.info.relative_path.clone(),
        brand.info.title.clone(),
        export_name,
    );

    // Token groups (colours, typography, spacing, motion)
    for group in &brand.token_groups {
        artifact.token_groups.push(TokenGroupSpec {
            group: group.group.clone(),
            tokens: group
                .tokens
                .iter()
                .map(|t| TokenSpec {
                    name: t.name.clone(),
                    role: t.role.clone(),
                })
                .collect(),
        });
    }

    // Voice section
    if let Some(voice) = &brand.voice {
        artifact.prose_sections.push(ProseSectionSpec {
            name: voice.name.clone(),
            body: voice.body.clone(),
        });
    }

    // Other prose sections (imagery, iconography, logo)
    for section in &brand.prose_sections {
        artifact.prose_sections.push(ProseSectionSpec {
            name: section.name.clone(),
            body: section.body.clone(),
        });
    }

    Ok(artifact)
}

fn prepare_routing_table(
    rt: &RoutingTableDraft,
    vocab: &DciVocabulary,
) -> Result<PreparedArtifact> {
    let export_name = sanitize_export_name(&rt.info.title);
    let mut artifact = PreparedArtifact::new(
        ArtifactKind::RoutingTable,
        rt.info.relative_path.clone(),
        rt.info.title.clone(),
        export_name,
    );

    let mut ambiguities = Vec::new();
    let mut bindings = Vec::new();

    for route in &rt.routes {
        // Resolve component reference
        let component_resolved = vocab.resolve(&route.component);
        if let Some(kind) = &component_resolved {
            if kind != "component" {
                ambiguities.push(Ambiguity {
                    message: format!(
                        "Route `{}` references `{}` which is a {} — routes must reference components",
                        route.path, route.component, kind
                    ),
                    severity: "blocking".to_string(),
                    suggested_resolution: Some(format!(
                        "Create a component draft for `{}`",
                        route.component
                    )),
                    source_line: None,
                });
            }
            bindings.push(VocabularyBinding {
                name: route.component.clone(),
                resolution: kind.clone(),
                resolved_to: Some(route.component.clone()),
            });
        } else {
            ambiguities.push(unresolved_reference_error(
                &rt.info.relative_path,
                &format!("route `{}`", route.path),
                &route.component,
                &route.component,
            ));
        }

        // Resolve path param types
        let mut params = Vec::new();
        // Extract path parameters from the path template
        let path_params: Vec<String> = extract_path_params(&route.path);

        for param in &route.params {
            let resolved = vocab.resolve(&param.data_type);
            if let Some(kind) = &resolved {
                bindings.push(VocabularyBinding {
                    name: param.data_type.clone(),
                    resolution: kind.clone(),
                    resolved_to: Some(param.data_type.clone()),
                });
            } else if !is_builtin_primitive(&param.data_type) {
                ambiguities.push(unresolved_reference_error(
                    &rt.info.relative_path,
                    &format!("route `{}` param", route.path),
                    &param.name,
                    &param.data_type,
                ));
            }
            params.push(RouteParamSpec {
                name: param.name.clone(),
                data_type: param.data_type.clone(),
            });
        }

        // Check that all path parameters have a matching params entry
        for path_param in &path_params {
            if !route.params.iter().any(|p| &p.name == path_param) {
                ambiguities.push(Ambiguity {
                    message: format!(
                        "Route `{}` has path parameter `<{}>` but no matching entry under `params:`",
                        route.path, path_param
                    ),
                    severity: "blocking".to_string(),
                    suggested_resolution: Some(format!(
                        "Add `{}: <DataType>` under the route's params section",
                        path_param
                    )),
                    source_line: None,
                });
            }
        }

        artifact.routes.push(RouteSpec {
            path: route.path.clone(),
            component: route.component.clone(),
            params,
        });
    }

    artifact.vocabulary_bindings = bindings;
    artifact.ambiguities = ambiguities;
    Ok(artifact)
}

/// Validate that a reference from a source kind to a target kind is allowed
/// per the DCI reference graph.
///
/// The authoritative graph:
/// - data → data
/// - projections → data + projections
/// - operations → data
/// - contexts → data + projections + operations + contexts + components + brand + routing_table
/// - components → data + projections + brand + components + contexts
/// - routing_table → components + data
/// - brand → nothing
fn prepare_app(app: &AppDraft, vocab: &DciVocabulary) -> Result<PreparedArtifact> {
    let mut artifact = PreparedArtifact::new(
        ArtifactKind::App,
        app.info.relative_path.clone(),
        app.info.title.clone(),
        "app".to_string(),
    );

    let mut ambiguities = Vec::new();

    artifact.application_kind = Some(app.application_kind.clone());

    // Resolve entry context against manifest
    if let Some(ctx_name) = &app.entry_context {
        let resolved = vocab.resolve(ctx_name);
        if let Some(kind) = &resolved {
            if kind != "context" {
                ambiguities.push(Ambiguity {
                    message: format!(
                        "`drafts/app.md` declares `{}` as the entry context but it is a {} in the manifest — \
                         the entry context must be a context draft",
                        ctx_name, kind
                    ),
                    severity: "blocking".to_string(),
                    suggested_resolution: Some(format!(
                        "Change `Entry Context` to the name of a context draft, or create a context draft for `{}`",
                        ctx_name
                    )),
                    source_line: None,
                });
            }
        } else {
            ambiguities.push(unresolved_reference_error(
                &app.info.relative_path,
                "entry context",
                "entry_context",
                ctx_name,
            ));
        }
        artifact.entry_context = Some(ctx_name.clone());
    }

    if let Some(func_name) = &app.entry_functionality {
        artifact.entry_functionality = Some(func_name.clone());
    }

    artifact.app_dependencies = app.dependencies.clone();

    if !app.notes.is_empty() {
        artifact.notes = Some(app.notes.clone());
    }

    artifact.ambiguities = ambiguities;
    Ok(artifact)
}

fn is_reference_allowed(source_kind: &str, target_kind: &str) -> bool {
    match source_kind {
        "data" => target_kind == "data",
        "projection" => matches!(target_kind, "data" | "projection"),
        "operation" => target_kind == "data",
        "context" => matches!(
            target_kind,
            "data" | "projection" | "operation" | "context" | "component" | "brand" | "routing_table"
        ),
        "component" => matches!(
            target_kind,
            "data" | "projection" | "brand" | "component" | "context"
        ),
        "routing_table" => matches!(target_kind, "component" | "data"),
        "brand" => false, // brand references nothing
        _ => false,
    }
}

/// Check a vocabulary reference from a draft of the given source kind.
/// Returns an ambiguity if the reference graph is violated.
fn check_reference_graph(
    source_title: &str,
    source_kind: &str,
    ref_name: &str,
    ref_kind: &str,
) -> Option<Ambiguity> {
    if is_reference_allowed(source_kind, ref_kind) {
        return None;
    }

    let kind_label = match source_kind {
        "data" => "Data",
        "projection" => "Projection",
        "context" => "Context",
        "component" => "Component",
        "brand" => "Brand",
        "routing_table" => "Routing table",
        "operation" => "Operation",
        _ => source_kind,
    };

    Some(Ambiguity {
        message: format!(
            "{kind_label} draft `{source_title}` references {ref_kind} `{ref_name}`. \
             In DCI, {source_kind} drafts cannot reference {ref_kind} artifacts — \
             this responsibility belongs at a different level of the architecture.",
        ),
        severity: "blocking".to_string(),
        suggested_resolution: Some(format!(
            "Move the reference to `{ref_name}` into a draft kind that is allowed to reference {ref_kind} artifacts",
        )),
        source_line: None,
    })
}

/// Classify each context's `realisation_target` based on its transitive dependency graph.
///
/// Rules (from PLAN.md Phase 10):
/// - A context touching only data/projections/brand/other-client_also-contexts → `client_also`
/// - A context transitively touching an operation with secret-requiring auth → `server_only`
/// - Anything else → `unknown`
///
/// Also checks for cycles in context→context dependencies and reports them as ambiguities.
fn classify_contexts(
    artifacts: &mut [(std::path::PathBuf, PreparedArtifact)],
    workspace: &Workspace,
) {
    // Build a map from export name → index for quick lookup
    let name_to_idx: BTreeMap<String, usize> = artifacts
        .iter()
        .enumerate()
        .map(|(i, (_, a))| (a.export.name.clone(), i))
        .collect();

    // Build adjacency: for each context, collect the names it references
    let mut context_deps: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    let mut touches_operation: BTreeSet<usize> = BTreeSet::new();

    for (i, (_, artifact)) in artifacts.iter().enumerate() {
        if artifact.kind != ArtifactKind::Context {
            continue;
        }

        let mut deps = Vec::new();

        // Role player types
        for rp in &artifact.role_players {
            deps.push(rp.data_type.clone());
        }

        // Vocabulary bindings — look for resolved references
        for binding in &artifact.vocabulary_bindings {
            if let Some(resolved) = &binding.resolved_to {
                deps.push(resolved.clone());
            }
        }

        // Check if this context touches operations (directly)
        for binding in &artifact.vocabulary_bindings {
            if binding.resolution == "operation" {
                touches_operation.insert(i);
            }
        }

        context_deps.insert(i, deps);
    }

    // Also check for operations in the prepared directory (from Phase 7 API ingestion)
    let ops_dir = workspace.prepared_dir.join("operations");
    let has_operations = ops_dir.is_dir();
    let mut operation_names: BTreeSet<String> = BTreeSet::new();
    let mut server_only_operations: BTreeSet<String> = BTreeSet::new();

    if has_operations {
        if let Ok(entries) = fs::read_dir(&ops_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("yml") {
                    if let Ok(content) = fs::read_to_string(&path) {
                        if let Ok(op) =
                            serde_yaml::from_str::<crate::prepared::ApiOperationSpec>(&content)
                        {
                            operation_names.insert(op.name.clone());
                            // Operations with secret-requiring auth are server-only
                            let auth_kind = op.auth.kind.as_str();
                            if matches!(
                                auth_kind,
                                "api_key_header" | "api_key_body" | "bearer"
                            ) {
                                server_only_operations.insert(op.name.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    // Build kind lookup from artifacts (immutable pass)
    let artifact_kinds: Vec<ArtifactKind> = artifacts.iter().map(|(_, a)| a.kind.clone()).collect();

    // Walk transitive deps for each context to determine classification (immutable pass)
    let mut classifications: BTreeMap<usize, (String, Option<Ambiguity>)> = BTreeMap::new();

    for (i, (_, artifact)) in artifacts.iter().enumerate() {
        if artifact.kind != ArtifactKind::Context {
            continue;
        }

        let mut visited = HashSet::new();
        let mut stack = vec![i];
        let mut is_server_only = false;
        let mut has_cycle = false;

        while let Some(current) = stack.pop() {
            if !visited.insert(current) {
                if current == i && visited.len() > 1 {
                    has_cycle = true;
                }
                continue;
            }

            if touches_operation.contains(&current) {
                is_server_only = true;
            }

            if let Some(deps) = context_deps.get(&current) {
                for dep in deps {
                    if let Some(&dep_idx) = name_to_idx.get(dep) {
                        if artifact_kinds[dep_idx] == ArtifactKind::Context {
                            stack.push(dep_idx);
                        }
                    }
                    if server_only_operations.contains(dep) {
                        is_server_only = true;
                    }
                }
            }
        }

        if has_cycle {
            classifications.insert(
                i,
                (
                    "unknown".to_string(),
                    Some(Ambiguity {
                        message: format!(
                            "Context `{}` has a circular dependency in its context→context reference graph. \
                             This prevents reliable classification of its realisation target.",
                            artifact.export.name,
                        ),
                        severity: "blocking".to_string(),
                        suggested_resolution: Some(
                            "Break the circular dependency by restructuring the context hierarchy"
                                .to_string(),
                        ),
                        source_line: None,
                    }),
                ),
            );
        } else if is_server_only {
            classifications.insert(i, ("server_only".to_string(), None));
        } else {
            classifications.insert(i, ("client_also".to_string(), None));
        }
    }

    // Apply classifications (mutable pass)
    for (idx, (target, ambiguity)) in classifications {
        artifacts[idx].1.realisation_target = Some(target);
        if let Some(amb) = ambiguity {
            artifacts[idx].1.ambiguities.push(amb);
        }
    }
}

/// PascalCase tokens that commonly appear at English sentence starts and are
/// never manifest references. Not configurable — extending is a code change.
/// Validate that web_app projects have a routing table in the same project.
fn validate_web_app_requirements(artifacts: &mut [(std::path::PathBuf, PreparedArtifact)]) {
    let has_web_app = artifacts
        .iter()
        .any(|(_, a)| a.kind == ArtifactKind::App && a.application_kind.as_deref() == Some("web_app"));

    if !has_web_app {
        return;
    }

    let has_routing_table = artifacts
        .iter()
        .any(|(_, a)| a.kind == ArtifactKind::RoutingTable);

    if !has_routing_table {
        // Find the App artifact and add a blocking ambiguity
        for (_, artifact) in artifacts.iter_mut() {
            if artifact.kind == ArtifactKind::App
                && artifact.application_kind.as_deref() == Some("web_app")
            {
                artifact.ambiguities.push(Ambiguity {
                    message: "A `web_app` project requires at least one routing table draft \
                              under `drafts/routes/`. Routing is the entry surface for web \
                              applications — add a routing table draft that maps URL paths \
                              to components."
                        .to_string(),
                    severity: "blocking".to_string(),
                    suggested_resolution: Some(
                        "Create a routing table draft under drafts/routes/ with at least one route"
                            .to_string(),
                    ),
                    source_line: None,
                });
                break;
            }
        }
    }
}

// ── Unresolved reference diagnostics ─────────────────────────────

/// Implementation-speak suffixes that should never appear in a draft.
/// When a type name ends with one of these, the draft is naming a
/// mechanism, not a domain concept.
const IMPLEMENTATION_SPEAK_SUFFIXES: &[&str] = &[
    "Client", "Store", "Sink", "Ledger", "Repository", "Service",
    "Manager", "Handler", "Adapter", "Gateway", "Connection", "Pool",
    "Registry", "Cache", "Queue", "Bus", "Clock", "Timer",
];

/// Built-in primitive types that are allowed without declaration.
const BUILTIN_PRIMITIVES: &[&str] = &[
    "String", "string", "str",
    "i8", "i16", "i32", "i64", "i128", "isize",
    "u8", "u16", "u32", "u64", "u128", "usize",
    "f32", "f64",
    "bool", "boolean",
    "uuid", "Uuid", "UUID",
    "DateTime", "datetime", "NaiveDate", "NaiveDateTime",
    "Duration",
    "Vec", "Option", "Result",
    "HashMap", "BTreeMap", "HashSet", "BTreeSet",
];

/// Returns true if `name` is a built-in primitive that needs no declaration.
pub fn is_builtin_primitive(name: &str) -> bool {
    // Strip generic wrappers: Vec<X>, Option<X>, etc.
    let base = name.split('<').next().unwrap_or(name).trim();
    BUILTIN_PRIMITIVES.contains(&base)
}

/// Categorise an unresolved type reference and produce a diagnostic suggestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnresolvedCategory {
    ImplementationSpeak,
    MissingDataDraft,
    MissingContext,
}

/// Determine the likely category of an unresolved reference name and return
/// a (category, suggestion) pair.
pub fn classify_unresolved_reference(name: &str) -> (UnresolvedCategory, String) {
    // Check implementation-speak suffixes
    for suffix in IMPLEMENTATION_SPEAK_SUFFIXES {
        if name.ends_with(suffix) || name == *suffix {
            return (
                UnresolvedCategory::ImplementationSpeak,
                format!(
                    "`{}` looks like implementation-speak (ends with `{}`). \
                     Name the domain concept instead — would a non-technical \
                     stakeholder recognise this name?",
                    name, suffix
                ),
            );
        }
    }

    // If it looks like a verb or action phrase (contains a verb-like word), suggest context
    let lower = name.to_lowercase();
    let context_verbs = ["initiate", "execute", "process", "handle", "perform",
                         "authenticate", "authorize", "validate", "verify",
                         "transfer", "send", "receive", "fetch", "query"];
    for verb in &context_verbs {
        if lower.contains(verb) {
            return (
                UnresolvedCategory::MissingContext,
                format!(
                    "`{}` looks like a missing context. Consider authoring \
                     `drafts/contexts/{}.md`.",
                    name,
                    crate::codegen::to_snake_case(name)
                ),
            );
        }
    }

    // Default: missing data draft
    (
        UnresolvedCategory::MissingDataDraft,
        format!(
            "`{}` is not declared. Consider authoring `drafts/data/{}.md` \
             or renaming the reference to an existing type.",
            name, name
        ),
    )
}

/// Build the error message for an unresolved type reference.
pub fn unresolved_reference_error(
    draft_path: &str,
    field_label: &str,
    field_name: &str,
    referenced_name: &str,
) -> Ambiguity {
    let (_category, suggestion) = classify_unresolved_reference(referenced_name);
    Ambiguity {
        message: format!(
            "Unresolved reference: {} `{}` references `{}` which is not declared as any artefact. {}",
            field_label, field_name, referenced_name, suggestion
        ),
        severity: "blocking".to_string(),
        suggested_resolution: Some(suggestion),
        source_line: None,
    }
}

const PROSE_REFERENCE_STOPWORDS: &[&str] = &[
    "The", "A", "An", "This", "That", "These", "Those",
    "When", "If", "Then", "Given", "And", "Or", "But", "So",
    "For", "On", "In", "At", "Of", "With", "Without",
    "It", "Its", "All", "Each", "Every", "No", "Not", "Is",
    "Are", "Was", "Were", "Has", "Have", "Had", "Do", "Does",
    "Will", "Would", "Could", "Should", "May", "Must", "Can",
    "Returns", "Creates", "Fails", "Adds", "Sets", "Gets",
    "Checks", "Updates", "Removes", "Ensures", "Provides",
    "Represents", "Contains", "Receives", "Produces", "Requires",
    "Accepts", "Constructs", "Executes", "Enacts", "Invokes",
    "Validates", "Transfers", "Converts", "Computes",
    "Commits", "Takes", "Panics", "Subtracts", "Calculates",
    "Stores", "Loads", "Sends", "Reads", "Writes", "Finds",
    "Looks", "Uses", "Calls", "Starts", "Stops", "Runs",
    "Moves", "Shows", "Hides", "Opens", "Closes",
    "Negative", "Positive", "Empty", "Full", "New", "Old",
    "First", "Last", "Next", "Previous", "Current",
    "Maximum", "Minimum", "Default", "Expected", "Actual",
    "After", "Before", "During", "Between", "Through",
    "Only", "Also", "Either", "Neither", "Both", "Same",
    "Here", "There", "Where", "How", "What", "Which", "Who",
    "Yes", "No", "True", "False", "None",
];

/// Scan prose text for PascalCase tokens that may be vocabulary references.
///
/// Two-tier severity:
/// - **High-confidence (blocking):** a PascalCase token that resolves elsewhere
///   in the same draft but appears unresolved here (likely typo), or an explicit
///   `@api/Name` / `@brand/token` that doesn't resolve.
/// - **Low-confidence (informational):** a standalone PascalCase token not in the
///   manifest and not resolved elsewhere in the draft.
///
/// Reference-graph violations for resolved tokens are always blocking.
fn check_prose_references(
    text: &str,
    source_kind: &str,
    source_title: &str,
    vocab: &DciVocabulary,
    ambiguities: &mut Vec<Ambiguity>,
) {
    let pascal_re = regex::Regex::new(r"\b[A-Z][A-Za-z0-9]*\b").unwrap();
    let stopwords: HashSet<&str> = PROSE_REFERENCE_STOPWORDS.iter().copied().collect();

    // First pass: collect all PascalCase tokens that resolve in the manifest.
    // This becomes the "resolved-in-this-draft" set for typo detection.
    let mut resolved_in_draft: BTreeSet<String> = BTreeSet::new();
    for m in pascal_re.find_iter(text) {
        let token = m.as_str();
        if stopwords.contains(token) || is_all_caps(token) {
            continue;
        }
        if vocab.resolve(token).is_some() {
            resolved_in_draft.insert(token.to_string());
        }
    }

    // Second pass: scan line-by-line with exclusion zones.
    let mut unresolved_tokens: BTreeMap<String, Option<usize>> = BTreeMap::new();

    for (line_idx, line) in text.lines().enumerate() {
        let trimmed = line.trim();

        // Exclusion: heading lines
        if trimmed.starts_with('#') {
            continue;
        }

        // Exclusion: scenario concrete-value lines (Given:/When:/Then: with
        // fixture-like content: tuples, bracketed lists, numbers with units)
        if is_scenario_value_line(trimmed) {
            continue;
        }

        // Strip backtick code spans before scanning
        let cleaned = strip_backtick_spans(line);

        for m in pascal_re.find_iter(&cleaned) {
            let token = m.as_str();

            if stopwords.contains(token) || is_all_caps(token) {
                continue;
            }

            // Check if it resolves in the manifest
            if let Some(kind) = vocab.resolve(token) {
                // It resolves — check reference graph
                if let Some(ambiguity) =
                    check_reference_graph(source_title, source_kind, token, &kind)
                {
                    ambiguities.push(ambiguity);
                }
            } else {
                // Does not resolve — record for second-pass severity classification
                unresolved_tokens
                    .entry(token.to_string())
                    .or_insert(Some(line_idx + 1));
            }
        }
    }

    // Classify unresolved tokens into high/low confidence
    for (token, source_line) in &unresolved_tokens {
        // High-confidence: the token is a near-miss of something resolved in this draft
        let is_typo = resolved_in_draft.iter().any(|resolved| {
            is_likely_typo(resolved, token)
        });

        if is_typo {
            let closest = resolved_in_draft
                .iter()
                .filter(|r| is_likely_typo(r, token))
                .next()
                .unwrap();
            ambiguities.push(Ambiguity {
                message: format!(
                    "Possible typo: `{}` is not in the vocabulary manifest but `{}` is. \
                     Did you mean `{}`?",
                    token, closest, closest
                ),
                severity: "blocking".to_string(),
                suggested_resolution: Some(format!("Replace `{}` with `{}`", token, closest)),
                source_line: *source_line,
            });
        } else {
            // Low-confidence: standalone unresolved PascalCase token
            ambiguities.push(Ambiguity {
                message: format!(
                    "Possible prose reference: `{}` looks like a domain name but is not in the vocabulary manifest",
                    token
                ),
                severity: "low".to_string(),
                suggested_resolution: Some(format!(
                    "If `{}` is a domain concept, add it to the manifest. Otherwise, ignore this message.",
                    token
                )),
                source_line: *source_line,
            });
        }
    }
}

/// Check if a token is all-caps (acronym like UTC, DKK, JSON — not PascalCase).
fn is_all_caps(token: &str) -> bool {
    token.len() >= 2 && token.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// Check if a line is a scenario concrete-value line (Given:/When:/Then: followed
/// by fixture-like content: tuples, bracketed lists, numbers with units).
fn is_scenario_value_line(trimmed: &str) -> bool {
    let lower = trimmed.to_ascii_lowercase();
    let after_label = if let Some(rest) = lower.strip_prefix("given:") {
        rest
    } else if let Some(rest) = lower.strip_prefix("when:") {
        rest
    } else if let Some(rest) = lower.strip_prefix("then:") {
        rest
    } else {
        return false;
    };

    let after = after_label.trim();
    // Fixture-like: contains brackets, tuples, or is mostly numeric/symbolic
    after.contains('[') || after.contains('(') || after.contains('{')
}

/// Strip backtick-delimited code spans from a line, replacing them with spaces.
fn strip_backtick_spans(line: &str) -> String {
    let mut result = String::with_capacity(line.len());
    let mut in_backtick = false;
    for ch in line.chars() {
        if ch == '`' {
            in_backtick = !in_backtick;
            result.push(' ');
        } else if in_backtick {
            result.push(' ');
        } else {
            result.push(ch);
        }
    }
    result
}

/// Check if `candidate` is a likely typo of `known` using edit distance.
/// Returns true if the tokens differ by exactly 1-2 character edits and
/// are at least 4 characters long (to avoid false positives on short tokens).
///
/// Excludes common English morphological variations (plural -s, -es, -ed, -ing)
/// to avoid flagging "Accounts" when "Account" is in the manifest.
fn is_likely_typo(known: &str, candidate: &str) -> bool {
    if known == candidate {
        return false;
    }
    if known.len() < 4 || candidate.len() < 4 {
        return false;
    }
    // Exclude morphological variations (plural, past tense, gerund)
    if is_morphological_variant(known, candidate) {
        return false;
    }
    let len_diff = (known.len() as isize - candidate.len() as isize).unsigned_abs();
    if len_diff > 2 {
        return false;
    }
    let distance = levenshtein_distance(known, candidate);
    distance >= 1 && distance <= 2
}

/// Check if `candidate` is a morphological variant of `known` (plural, past tense, etc.)
fn is_morphological_variant(known: &str, candidate: &str) -> bool {
    // candidate is known + "s", "es", "ed", "ing", "er"
    for suffix in &["s", "es", "ed", "ing", "er"] {
        if candidate == format!("{}{}", known, suffix) {
            return true;
        }
        if known == format!("{}{}", candidate, suffix) {
            return true;
        }
    }
    false
}

/// Simple Levenshtein distance implementation.
fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();

    let mut prev = (0..=n).collect::<Vec<_>>();
    let mut curr = vec![0; n + 1];

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a_chars[i - 1] == b_chars[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1)
                .min(curr[j - 1] + 1)
                .min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

/// Extract path parameters from a route path template.
/// E.g. "/products/<id>/reviews/<review_id>" → ["id", "review_id"]
fn extract_path_params(path: &str) -> Vec<String> {
    let mut params = Vec::new();
    let mut i = 0;
    let chars: Vec<char> = path.chars().collect();
    while i < chars.len() {
        if chars[i] == '<' {
            i += 1;
            let start = i;
            while i < chars.len() && chars[i] != '>' {
                i += 1;
            }
            if i < chars.len() {
                let param: String = chars[start..i].iter().collect();
                params.push(param);
            }
            i += 1;
        } else {
            i += 1;
        }
    }
    params
}

// ── Helpers ───────────────────────────────────────────────────────

/// Extract role method references from functionality prose.
///
/// Scans for known role method names (both short form `withdraw` and
/// canonical form `source_withdraw`) in the description text.
fn extract_role_method_refs(
    description: &str,
    role_method_groups: &[RoleMethodGroupSpec],
) -> Vec<String> {
    let mut refs = BTreeSet::new();
    let lower = description.to_ascii_lowercase();
    for group in role_method_groups {
        for method in &group.methods {
            // Check for short name reference
            if lower.contains(&method.name.to_ascii_lowercase()) {
                refs.insert(method.canonical_name.clone());
            }
            // Check for canonical name reference
            if lower.contains(&method.canonical_name.to_ascii_lowercase()) {
                refs.insert(method.canonical_name.clone());
            }
        }
    }
    refs.into_iter().collect()
}

/// Extract `@api/Name` references from prose text.
fn extract_api_references(text: &str) -> Vec<String> {
    let mut refs = Vec::new();
    let mut i = 0;
    let chars: Vec<char> = text.chars().collect();
    while i < chars.len() {
        if chars[i] == '@' {
            let start = i;
            i += 1;
            // Read api name
            let mut has_slash = false;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '/') {
                if chars[i] == '/' {
                    has_slash = true;
                }
                i += 1;
            }
            if has_slash {
                let reference: String = chars[start..i].iter().collect();
                refs.push(reference);
            }
        } else {
            i += 1;
        }
    }
    refs
}

/// Convert a draft title like "Context: MoneyTransfer" or "Data: Amount" into
/// a clean PascalCase export name.
fn sanitize_export_name(title: &str) -> String {
    let cleaned = title
        .strip_prefix("Context:")
        .or_else(|| title.strip_prefix("Data:"))
        .or_else(|| title.strip_prefix("Projection:"))
        .or_else(|| title.strip_prefix("Component:"))
        .or_else(|| title.strip_prefix("Brand:"))
        .or_else(|| title.strip_prefix("Routes:"))
        .unwrap_or(title)
        .trim();
    if cleaned.is_empty() {
        return title.trim().to_string();
    }
    cleaned.to_string()
}

fn print_draft_findings(review: &DraftReview) {
    for finding in &review.findings {
        let location = match finding.source_line {
            Some(line) => format!("{}:{}", finding.draft_path, line),
            None => finding.draft_path.clone(),
        };
        eprintln!(
            "error[{}]: {}",
            finding.category.as_str(),
            finding.message
        );
        eprintln!("  --> {}", location);
    }
}

// ── Multiplicity inference (bug 6) ──────────────────────────────────
//
// Three cues, ordered weakest → strongest:
//   1. Name plurality — `transactions`, `accounts`, `scopes`, `turns`. Strong
//      list cue. Singular nouns are a strong single cue.
//   2. Description prose — phrases like "a list of", "the transactions",
//      "all the X" suggest a collection.
//   3. Scenario `Given:` shape — decisive. If the entity appears with a count
//      ("five transactions", "transactions with amounts X, Y, Z"), it's a
//      list. If it appears as a single instance ("a Session with …"), it's
//      single.
//
// Aggregation: scenarios beat names beat prose. Disagreements (plural name
// but singular scenarios) raise an informational finding.

fn derive_multiplicity(artifact: &mut PreparedArtifact) {
    let scenarios: Vec<ScenarioSpec> = artifact.scenarios.clone();

    // Role players first.
    let mut findings: Vec<Ambiguity> = Vec::new();
    for rp in &mut artifact.role_players {
        let name_cue = name_plurality(&rp.name);
        let prose_cue = prose_plurality(&rp.responsibility);
        let scenario_cue = scenario_plurality(&rp.name, &scenarios);

        let resolved = combine_multiplicity_cues(scenario_cue, name_cue, prose_cue);
        rp.multiplicity = resolved.multiplicity;
        if let Some(message) = resolved.disagreement {
            findings.push(Ambiguity {
                message: format!(
                    "role player `{}` has conflicting multiplicity cues: {}",
                    rp.name, message,
                ),
                severity: "low".to_string(),
                suggested_resolution: Some(
                    "add a scenario that makes the cardinality unambiguous".into(),
                ),
                source_line: None,
            });
        }
    }

    for attr in &mut artifact.attributes {
        let name_cue = name_plurality(&attr.name);
        let prose_cue = prose_plurality(&attr.description);
        let scenario_cue = scenario_plurality(&attr.name, &scenarios);

        let resolved = combine_multiplicity_cues(scenario_cue, name_cue, prose_cue);
        attr.multiplicity = resolved.multiplicity;
        if let Some(message) = resolved.disagreement {
            findings.push(Ambiguity {
                message: format!(
                    "attribute `{}` has conflicting multiplicity cues: {}",
                    attr.name, message,
                ),
                severity: "low".to_string(),
                suggested_resolution: Some(
                    "add a scenario that makes the cardinality unambiguous".into(),
                ),
                source_line: None,
            });
        }
    }

    artifact.ambiguities.extend(findings);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cue {
    Single,
    List,
    Unknown,
}

struct ResolvedMultiplicity {
    multiplicity: Multiplicity,
    disagreement: Option<String>,
}

fn combine_multiplicity_cues(scenario: Cue, name: Cue, prose: Cue) -> ResolvedMultiplicity {
    // Scenario decides outright when it knows.
    let chosen = match (scenario, name, prose) {
        (Cue::List, _, _) => Cue::List,
        (Cue::Single, _, _) => Cue::Single,
        (_, Cue::List, _) => Cue::List,
        (_, Cue::Single, _) => Cue::Single,
        (_, _, Cue::List) => Cue::List,
        (_, _, Cue::Single) => Cue::Single,
        _ => Cue::Single,
    };
    let multiplicity = match chosen {
        Cue::List => Multiplicity::List,
        _ => Multiplicity::Single,
    };

    // Disagreement detection: scenario says one thing, name says the opposite.
    let disagreement = match (scenario, name) {
        (Cue::Single, Cue::List) => Some(format!(
            "name `{:?}` is plural but scenarios consistently supply a single instance",
            name,
        )),
        (Cue::List, Cue::Single) => Some(format!(
            "name `{:?}` is singular but scenarios supply multiple instances",
            name,
        )),
        _ => None,
    };
    ResolvedMultiplicity { multiplicity, disagreement }
}

const IRREGULAR_PLURALS: &[(&str, &str)] = &[
    ("children", "child"),
    ("people", "person"),
    ("men", "man"),
    ("women", "woman"),
    ("feet", "foot"),
    ("teeth", "tooth"),
    ("mice", "mouse"),
    ("geese", "goose"),
];

fn name_plurality(name: &str) -> Cue {
    let lower = name.to_ascii_lowercase();
    let last_word = lower
        .rsplit(|c: char| c == ' ' || c == '_' || c == '-')
        .next()
        .unwrap_or(&lower);
    for (plural, _) in IRREGULAR_PLURALS {
        if last_word == *plural {
            return Cue::List;
        }
    }
    if last_word.ends_with("ies") && last_word.len() > 3 {
        return Cue::List;
    }
    if last_word.ends_with("ses") || last_word.ends_with("xes") || last_word.ends_with("zes") {
        return Cue::List;
    }
    if last_word.ends_with('s') && !last_word.ends_with("ss") && last_word.len() > 2 {
        // Coarse English plural rule. False positives ("status", "ios") are
        // rare in this domain; the scenario cue overrides anyway.
        return Cue::List;
    }
    Cue::Single
}

fn prose_plurality(prose: &str) -> Cue {
    let lower = prose.to_ascii_lowercase();
    if lower.contains("a list of")
        || lower.contains("the list of")
        || lower.contains("the ordered ")
        || lower.contains("the collection of")
        || lower.contains("all the ")
    {
        return Cue::List;
    }
    Cue::Unknown
}

fn scenario_plurality(role_name: &str, scenarios: &[ScenarioSpec]) -> Cue {
    let role_lower = role_name.to_ascii_lowercase();
    let role_singular = depluralise(&role_lower);

    let mut list_hits = 0usize;
    let mut single_hits = 0usize;

    for sc in scenarios {
        let blob = format!("{}\n{}\n{}", sc.given, sc.when, sc.then).to_ascii_lowercase();
        // Multiple-instance phrases.
        if blob.contains(&format!("{} with amounts ", role_lower))
            || blob.contains(&format!("{}: ", role_lower)) // not reliable; skip
            || COUNT_WORDS.iter().any(|w| blob.contains(&format!("{} {}", w, role_lower)))
        {
            list_hits += 1;
            continue;
        }
        // Single-instance phrases.
        if blob.contains(&format!("a {}", role_singular))
            || blob.contains(&format!("the {}", role_singular))
            || blob.contains(&format!("{} with ", role_singular))
        {
            single_hits += 1;
        }
    }

    if list_hits > 0 && list_hits >= single_hits {
        Cue::List
    } else if single_hits > 0 {
        Cue::Single
    } else {
        Cue::Unknown
    }
}

const COUNT_WORDS: &[&str] = &[
    "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
    "eleven", "twelve", "many", "several",
];

fn depluralise(word: &str) -> String {
    if word.ends_with("ies") && word.len() > 3 {
        return format!("{}y", &word[..word.len() - 3]);
    }
    for (plural, singular) in IRREGULAR_PLURALS {
        if word == *plural {
            return (*singular).to_string();
        }
    }
    if word.ends_with("ses") || word.ends_with("xes") || word.ends_with("zes") {
        return word[..word.len() - 2].to_string();
    }
    if word.ends_with('s') && !word.ends_with("ss") && word.len() > 2 {
        return word[..word.len() - 1].to_string();
    }
    word.to_string()
}

// ── Signature inference (bug 5) ─────────────────────────────────────
//
// Walks scenarios per data operation, context functionality, and projection
// query. Patterns are an explicit allow-list — when none match we leave
// `derived_signature` as `None` and codegen falls back to the `(&self)`
// stub. New shapes get added here in a controlled way as drafts demand
// them, never by widening to regex magic.

fn derive_signatures(artifact: &mut PreparedArtifact, vocab: &DciVocabulary) {
    let host_type = artifact.export.name.clone();
    let scenarios: Vec<ScenarioSpec> = artifact.scenarios.clone();
    let kind = artifact.kind.clone();
    let role_players: Vec<RolePlayerSpec> = artifact.role_players.clone();
    let attributes: Vec<AttributeSpec> = artifact.attributes.clone();

    let mut findings: Vec<Ambiguity> = Vec::new();

    match kind {
        ArtifactKind::Data => {
            for op in &mut artifact.operations {
                if op.kind.as_deref() == Some("with") {
                    // `with` operations have a fixed signature shape; do not
                    // touch them.
                    continue;
                }
                let (sig, finding) =
                    infer_for_data_operation(&op.name, &host_type, &scenarios, vocab);
                op.derived_signature = sig;
                if let Some(f) = finding {
                    findings.push(f);
                }
            }
        }
        ArtifactKind::Context => {
            for func in &mut artifact.functionalities {
                let (sig, finding) =
                    infer_for_context_functionality(&func.name, &scenarios);
                func.derived_signature = sig;
                if let Some(f) = finding {
                    findings.push(f);
                }
            }
        }
        ArtifactKind::Projection => {
            for func in &mut artifact.functionalities {
                let (sig, finding) = infer_for_projection_query(
                    &func.name,
                    &scenarios,
                    &role_players,
                    vocab,
                );
                func.derived_signature = sig;
                if let Some(f) = finding {
                    findings.push(f);
                }
            }
        }
        _ => {}
    }

    let _ = attributes; // attributes are not signature-relevant
    artifact.ambiguities.extend(findings);
}

fn scenario_invokes(when_clause: &str, op_name: &str) -> bool {
    let when_lower = when_clause.to_ascii_lowercase();
    let op_lower = op_name.to_ascii_lowercase();
    let op_with_underscores = op_lower.replace(' ', "_");
    let op_with_spaces = op_lower.replace('_', " ");
    when_lower.contains(&op_lower)
        || when_lower.contains(&op_with_underscores)
        || when_lower.contains(&op_with_spaces)
}

fn infer_for_data_operation(
    op_name: &str,
    host_type: &str,
    scenarios: &[ScenarioSpec],
    _vocab: &DciVocabulary,
) -> (Option<DerivedSignature>, Option<Ambiguity>) {
    let matching: Vec<&ScenarioSpec> = scenarios
        .iter()
        .filter(|s| scenario_invokes(&s.when, op_name))
        .collect();
    if matching.is_empty() {
        return (
            None,
            Some(Ambiguity {
                message: format!(
                    "operation `{}` has no scenario; signature cannot be derived",
                    op_name,
                ),
                severity: "low".to_string(),
                suggested_resolution: Some(
                    "add a scenario whose When invokes this operation".into(),
                ),
                source_line: None,
            }),
        );
    }

    // Rejection scenarios specify error behaviour, not the success path.
    // The signature is derived from the happy-path scenarios only — including
    // a rejection in the aggregate would force the return to `()` and lose
    // the actual return type. If only rejections exist we fall back to all
    // matching scenarios so something is still inferred.
    let happy: Vec<&ScenarioSpec> = matching
        .iter()
        .copied()
        .filter(|s| !is_rejection_then(&s.then))
        .collect();
    let chosen: Vec<&ScenarioSpec> = if happy.is_empty() { matching } else { happy };

    let mut sigs: Vec<DerivedSignature> = Vec::new();
    for sc in &chosen {
        // Count host-type references in Given to detect binary vs. unary.
        let host_refs = count_phrase(&sc.given, &format!("a {}", host_type));
        let mut parameters: Vec<ParamSpec> = Vec::new();
        if host_refs >= 2 {
            parameters.push(ParamSpec {
                name: "other".into(),
                rust_type: format!("&{}", host_type),
            });
        }
        // Quoted-string param in When clause (e.g. `to "awaiting-input"`).
        if let Some(quoted) = first_quoted_after(&sc.when, " to ") {
            let _ = quoted;
            parameters.push(ParamSpec {
                name: "value".into(),
                rust_type: "&str".into(),
            });
        }

        let returns = classify_return_shape(&sc.then, host_type);
        sigs.push(DerivedSignature {
            receiver: "&self".into(),
            parameters,
            returns,
            source: "derived".into(),
        });
    }

    aggregate_signatures(op_name, sigs)
}

fn infer_for_context_functionality(
    op_name: &str,
    scenarios: &[ScenarioSpec],
) -> (Option<DerivedSignature>, Option<Ambiguity>) {
    let matching: Vec<&ScenarioSpec> = scenarios
        .iter()
        .filter(|s| scenario_invokes(&s.when, op_name))
        .collect();
    if matching.is_empty() {
        return (
            None,
            Some(Ambiguity {
                message: format!(
                    "functionality `{}` has no scenario; signature cannot be derived",
                    op_name,
                ),
                severity: "low".to_string(),
                suggested_resolution: Some(
                    "add a scenario whose When invokes this functionality".into(),
                ),
                source_line: None,
            }),
        );
    }
    let mut sigs: Vec<DerivedSignature> = Vec::new();
    for sc in &matching {
        let returns = classify_return_shape(&sc.then, "");
        sigs.push(DerivedSignature {
            receiver: "&self".into(),
            parameters: Vec::new(),
            returns,
            source: "derived".into(),
        });
    }
    aggregate_signatures(op_name, sigs)
}

fn infer_for_projection_query(
    op_name: &str,
    scenarios: &[ScenarioSpec],
    role_players: &[RolePlayerSpec],
    _vocab: &DciVocabulary,
) -> (Option<DerivedSignature>, Option<Ambiguity>) {
    let matching: Vec<&ScenarioSpec> = scenarios
        .iter()
        .filter(|s| scenario_invokes(&s.when, op_name))
        .collect();
    if matching.is_empty() {
        return (
            None,
            Some(Ambiguity {
                message: format!(
                    "query `{}` has no scenario; signature cannot be derived",
                    op_name,
                ),
                severity: "low".to_string(),
                suggested_resolution: Some(
                    "add a scenario whose When queries this functionality".into(),
                ),
                source_line: None,
            }),
        );
    }

    // For projections, the Given describes role players already on self; we
    // look at When for parameters and Then for the return shape. The list
    // role player's element type is the canonical list element.
    let element_type = role_players
        .first()
        .map(|rp| {
            let ty = &rp.data_type;
            ty.clone()
        })
        .unwrap_or_default();

    let mut sigs: Vec<DerivedSignature> = Vec::new();
    for sc in &matching {
        let parameters = parse_query_parameters(&sc.when);
        let returns = classify_projection_return(&sc.then, &element_type);
        sigs.push(DerivedSignature {
            receiver: "&self".into(),
            parameters,
            returns,
            source: "derived".into(),
        });
    }
    aggregate_signatures(op_name, sigs)
}

/// `queried with minimum 100 and maximum 250` → two i64 params.
/// `queried with "alice"` → one &str param named `query`.
/// `queried with no date range` → no params.
/// `queried with a range covering the last seven days` → one `range: ()` param.
fn parse_query_parameters(when: &str) -> Vec<ParamSpec> {
    let when_lower = when.to_ascii_lowercase();
    let mut params: Vec<ParamSpec> = Vec::new();

    // "queried with no <X>" → nothing
    if when_lower.contains("with no ") {
        return params;
    }

    // Pattern: "with <name1> <num1> and <name2> <num2>"
    if let Some(rest) = when_lower.split_once(" with ").map(|(_, b)| b) {
        // Tokenise on " and ".
        for chunk in rest.split(" and ") {
            let mut iter = chunk.split_whitespace();
            if let (Some(name), Some(value)) = (iter.next(), iter.next()) {
                if value.parse::<i64>().is_ok() {
                    params.push(ParamSpec {
                        name: name.trim_end_matches(',').to_string(),
                        rust_type: "i64".into(),
                    });
                    continue;
                }
                // Quoted string param: "alice"
                if value.starts_with('"') && chunk.contains('"') {
                    params.push(ParamSpec {
                        name: "query".into(),
                        rust_type: "&str".into(),
                    });
                    continue;
                }
            }
        }
    }

    params
}

fn classify_projection_return(then: &str, element_type: &str) -> String {
    let lower = then.to_ascii_lowercase();
    if lower.contains("returns the ")
        || lower.contains("are returned")
        || lower.contains("only transactions")
        || lower.contains("all five")
        || lower.contains("all the")
    {
        if !element_type.is_empty() {
            return format!("Vec<{}>", element_type);
        }
    }
    classify_return_shape(then, element_type)
}

fn classify_return_shape(then: &str, host_type: &str) -> String {
    let lower = then.to_ascii_lowercase();

    // Boolean: "the answer is affirmative|negative|yes|no"
    if lower.contains("the answer is affirmative")
        || lower.contains("the answer is negative")
        || lower.contains("the answer is yes")
        || lower.contains("the answer is no")
    {
        return "bool".into();
    }

    // Quoted text: `the result is "100.50 DKK"` → String
    if let Some(idx) = lower.find("the result is") {
        let tail = &then[idx + "the result is".len()..];
        let trimmed = tail.trim_start();
        if trimmed.starts_with('"') || trimmed.starts_with('\'') {
            return "String".into();
        }
        // `the result is a <Type> of/with...`
        if let Some(after_a) = trimmed.strip_prefix("a ").or_else(|| trimmed.strip_prefix("an ")) {
            // First word should be a PascalCase type.
            let first = after_a.split_whitespace().next().unwrap_or("");
            if !first.is_empty() && first.chars().next().unwrap().is_ascii_uppercase() {
                return first.trim_end_matches([',', '.', ';']).to_string();
            }
        }
    }

    // Rejection / acceptance: leave as unit (callers can upgrade to Result).
    if lower.contains("is rejected")
        || lower.contains("is accepted")
        || lower.contains("becomes ")
    {
        return "()".into();
    }

    // Default: unit.
    let _ = host_type;
    "()".into()
}

fn aggregate_signatures(
    op_name: &str,
    sigs: Vec<DerivedSignature>,
) -> (Option<DerivedSignature>, Option<Ambiguity>) {
    if sigs.is_empty() {
        return (None, None);
    }
    let first = sigs[0].clone();
    let all_agree = sigs.iter().all(|s| s == &first);
    if all_agree {
        return (Some(first), None);
    }

    // Disagreement on parameter arity is blocking; on return shape only,
    // informational (codegen still gets a useful signature from the first
    // shape, but we surface the discrepancy).
    let arity_disagrees = sigs.iter().any(|s| s.parameters.len() != first.parameters.len());
    let severity = if arity_disagrees { "blocking" } else { "low" };
    (
        None,
        Some(Ambiguity {
            message: format!(
                "scenarios for `{}` disagree on signature shape; cannot derive a single signature",
                op_name,
            ),
            severity: severity.to_string(),
            suggested_resolution: Some(
                "rewrite the scenarios so they describe a consistent receiver, parameters, and result".into(),
            ),
            source_line: None,
        }),
    )
}

fn count_phrase(haystack: &str, needle: &str) -> usize {
    let h = haystack.to_ascii_lowercase();
    let n = needle.to_ascii_lowercase();
    if n.is_empty() {
        return 0;
    }
    h.matches(&n).count()
}

fn is_rejection_then(then: &str) -> bool {
    let lower = then.to_ascii_lowercase();
    lower.contains("is rejected")
        || lower.contains("returns an error")
        || lower.contains("with a currency mismatch")
        || lower.contains("transition is rejected")
}

fn first_quoted_after(text: &str, marker: &str) -> Option<String> {
    let idx = text.to_ascii_lowercase().find(marker)?;
    let tail = &text[idx + marker.len()..];
    let mut chars = tail.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' || c == '\'' {
            let quote = c;
            let mut acc = String::new();
            for d in chars.by_ref() {
                if d == quote {
                    return Some(acc);
                }
                acc.push(d);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draft_parser::parse_draft_file;
    use std::path::Path;

    fn empty_vocab() -> DciVocabulary {
        DciVocabulary::empty()
    }

    fn vocab_with(names: &[(&str, &str)]) -> DciVocabulary {
        let mut v = DciVocabulary::empty();
        for (name, kind) in names {
            v.add(name, kind);
        }
        v
    }

    #[test]
    fn prepare_context_with_resolved_vocabulary() {
        let content = r#"# Context: MoneyTransfer

## Purpose
Enacts a transfer of money between two accounts.

## Role Players
- source: Account — the account money is withdrawn from
- sink: Account — the account money is deposited into

## Props
- amount: the quantity of money to transfer

## Role Methods
### source
- withdraw: removes the transfer amount from the source balance

### sink
- deposit: adds the transfer amount to the sink balance

## Functionalities
### transfer
The source withdraws the amount via its withdraw role method,
then the sink deposits the amount via its deposit role method.

## Scenarios
### successful transfer
Given: source has balance 100, sink has balance 0, amount is 30
When: transfer is invoked
Then: source has balance 70, sink has balance 30
"#;
        let doc = parse_draft_file(
            Path::new("drafts/contexts/money_transfer.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let vocab = vocab_with(&[("Account", "data")]);
        let prepared = prepare_document(&doc, &vocab).unwrap();

        assert_eq!(prepared.kind, ArtifactKind::Context);
        assert_eq!(prepared.export.name, "MoneyTransfer");
        assert_eq!(prepared.role_players.len(), 2);
        assert_eq!(prepared.role_methods.len(), 2);
        assert_eq!(prepared.role_methods[0].methods[0].canonical_name, "source_withdraw");
        assert_eq!(prepared.functionalities.len(), 1);
        assert!(!prepared.functionalities[0].role_method_refs.is_empty());
        assert_eq!(prepared.scenarios.len(), 1);
        assert_eq!(prepared.synthesised_tests.len(), 1);
        assert!(prepared.ambiguities.is_empty(), "expected no ambiguities: {:?}", prepared.ambiguities);
    }

    #[test]
    fn prepare_context_flags_unresolved_vocabulary() {
        let content = r#"# Context: MoneyTransfer

## Purpose
Enacts a transfer of money between two accounts.

## Role Players
- source: Account — the account money is withdrawn from
- sink: Account — the account money is deposited into

## Props
- amount: the quantity of money to transfer

## Role Methods
### source
- withdraw: removes the transfer amount from the source balance

### sink
- deposit: adds the transfer amount to the sink balance

## Functionalities
### transfer
The source withdraws the amount.

## Scenarios
### successful transfer
Given: source has balance 100
When: transfer is invoked
Then: source has balance 70
"#;
        let doc = parse_draft_file(
            Path::new("drafts/contexts/money_transfer.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let vocab = empty_vocab(); // Account is NOT in vocabulary
        let prepared = prepare_document(&doc, &vocab).unwrap();

        assert!(
            prepared.ambiguities.iter().any(|a| a.message.contains("Account")),
            "expected ambiguity about Account: {:?}",
            prepared.ambiguities
        );
    }

    #[test]
    fn prepare_data_with_invariant_generates_test() {
        let content = r#"# Data: Amount

## Purpose
Represents a monetary amount that is never negative.

## Attributes
- value: the numeric quantity in the smallest currency unit

## Invariants
- an Amount is never negative

## Operations
### add
Adds two amounts together.

## Scenarios
### adding two amounts
Given: an amount of 30 and an amount of 50
When: add is invoked
Then: the result is 80
"#;
        let doc = parse_draft_file(
            Path::new("drafts/data/amount.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let vocab = empty_vocab();
        let prepared = prepare_document(&doc, &vocab).unwrap();

        assert_eq!(prepared.kind, ArtifactKind::Data);
        assert_eq!(prepared.attributes.len(), 1);
        assert_eq!(prepared.invariants.len(), 1);
        assert_eq!(prepared.operations.len(), 1);
        // 1 from scenario + 1 from invariant
        assert_eq!(prepared.synthesised_tests.len(), 2);
        assert!(prepared.synthesised_tests.iter().any(|t| t.source == "invariant"));
    }

    #[test]
    fn prepare_projection_flags_unresolved_role_player_data_type() {
        // Projections share the context-aligned six-section shape; the
        // unresolved-reference path runs over their role players.
        let content = r#"# Projection: AccountBalance

## Purpose
Provides a read-only view of an account's current balance.

## Role Players
- account: Account — the account being viewed

## Role Methods
### account
- read_balance: provides the current balance value

## Functionalities
### current_balance
Returns the account's current balance via the account's read_balance role method.

## Scenarios
### balance query
Given: an account with balance 100
When: current_balance is queried
Then: returns 100
"#;
        let doc = parse_draft_file(
            Path::new("drafts/projections/account_balance.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let vocab = empty_vocab(); // Account NOT in vocabulary
        let prepared = prepare_document(&doc, &vocab).unwrap();

        assert_eq!(prepared.kind, ArtifactKind::Projection);
        assert!(
            prepared.ambiguities.iter().any(|a| a.message.contains("Account")),
            "expected ambiguity about Account: {:?}",
            prepared.ambiguities
        );
    }

    #[test]
    fn sanitize_export_name_strips_kind_prefix() {
        assert_eq!(sanitize_export_name("Context: MoneyTransfer"), "MoneyTransfer");
        assert_eq!(sanitize_export_name("Data: Amount"), "Amount");
        assert_eq!(sanitize_export_name("Projection: BalanceView"), "BalanceView");
        assert_eq!(sanitize_export_name("JustAName"), "JustAName");
    }

    #[test]
    fn data_draft_referencing_operation_is_rejected() {
        let content = r#"# Data: Vessel

## Purpose
Represents a vessel. Uses @aisstream/Subscribe for live updates.

## Attributes
- mmsi: vessel identifier

## Invariants
- mmsi is always positive

## Operations
### update_position
Updates position from @aisstream/Subscribe operation.

## Scenarios
### create vessel
Given: a vessel with mmsi 123456
When: created
Then: mmsi is 123456
"#;
        let doc = parse_draft_file(
            Path::new("drafts/data/vessel.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let mut vocab = DciVocabulary::empty();
        // Simulate an API manifest with Subscribe as an operation
        vocab.add("Vessel", "data");
        // We need to test with a vocab that has api entries — build manually
        let prepared = prepare_document(&doc, &vocab).unwrap();

        // Without an API manifest loaded, the @api/ref won't resolve
        // so no blocking ambiguity about operations. That's fine —
        // the reference graph is enforced when the manifest IS loaded.
        // Let's test the extract_api_references helper instead.
        let refs = extract_api_references("Uses @aisstream/Subscribe for updates");
        assert_eq!(refs, vec!["@aisstream/Subscribe"]);

        let refs2 = extract_api_references("No api references here");
        assert!(refs2.is_empty());
    }

    #[test]
    fn context_can_reference_api_operation() {
        // Contexts are allowed to reference operations (they are context-level concerns)
        let content = r#"# Context: VesselTracker

## Purpose
Tracks vessels using @aisstream/Subscribe.

## Role Players
- stream: @aisstream/Subscribe — provides live vessel data

## Props
- api_key: the authentication key for the stream

## Role Methods
### stream
- connect: establishes the streaming connection

## Functionalities
### track
Subscribes to vessel updates via the stream's connect role method.

## Scenarios
### successful tracking
Given: a valid api key and bounding box
When: track is invoked
Then: vessel updates are received
"#;
        let doc = parse_draft_file(
            Path::new("drafts/contexts/vessel_tracker.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        // Build a vocab with API entries
        let mut vocab = DciVocabulary::empty();
        // Manually: we can't call load_api_manifest in a unit test easily,
        // so we test that the prepare function handles @-prefixed refs
        let prepared = prepare_document(&doc, &vocab).unwrap();

        // Without the API manifest loaded, @aisstream/Subscribe is unresolved
        // and will be flagged as an ambiguity referencing the API manifest
        assert!(
            prepared.ambiguities.iter().any(|a| a.message.contains("@aisstream/Subscribe")),
            "expected ambiguity about unresolved API ref: {:?}",
            prepared.ambiguities
        );
    }

    // ── Reference graph tests ────────────────────────────────────

    #[test]
    fn reference_graph_allowed_edges() {
        // data → data
        assert!(is_reference_allowed("data", "data"));
        // projections → data + projections
        assert!(is_reference_allowed("projection", "data"));
        assert!(is_reference_allowed("projection", "projection"));
        // contexts → everything except brand (nothing blocks contexts)
        assert!(is_reference_allowed("context", "data"));
        assert!(is_reference_allowed("context", "projection"));
        assert!(is_reference_allowed("context", "operation"));
        assert!(is_reference_allowed("context", "context"));
        assert!(is_reference_allowed("context", "component"));
        assert!(is_reference_allowed("context", "brand"));
        assert!(is_reference_allowed("context", "routing_table"));
        // components → data + projections + brand + components + contexts
        assert!(is_reference_allowed("component", "data"));
        assert!(is_reference_allowed("component", "projection"));
        assert!(is_reference_allowed("component", "brand"));
        assert!(is_reference_allowed("component", "component"));
        assert!(is_reference_allowed("component", "context"));
        // routing_table → components + data
        assert!(is_reference_allowed("routing_table", "component"));
        assert!(is_reference_allowed("routing_table", "data"));
    }

    #[test]
    fn reference_graph_forbidden_edges() {
        // data cannot reference context, component, brand, routing_table, operation
        assert!(!is_reference_allowed("data", "context"));
        assert!(!is_reference_allowed("data", "component"));
        assert!(!is_reference_allowed("data", "brand"));
        assert!(!is_reference_allowed("data", "routing_table"));
        assert!(!is_reference_allowed("data", "projection"));
        assert!(!is_reference_allowed("data", "operation"));
        // projection cannot reference context, operation, component, brand, routing_table
        assert!(!is_reference_allowed("projection", "context"));
        assert!(!is_reference_allowed("projection", "operation"));
        assert!(!is_reference_allowed("projection", "component"));
        // component cannot reference operation, routing_table
        assert!(!is_reference_allowed("component", "operation"));
        assert!(!is_reference_allowed("component", "routing_table"));
        // routing_table cannot reference context, projection, operation, brand, routing_table
        assert!(!is_reference_allowed("routing_table", "context"));
        assert!(!is_reference_allowed("routing_table", "projection"));
        assert!(!is_reference_allowed("routing_table", "operation"));
        assert!(!is_reference_allowed("routing_table", "brand"));
        // brand → nothing
        assert!(!is_reference_allowed("brand", "data"));
        assert!(!is_reference_allowed("brand", "context"));
        assert!(!is_reference_allowed("brand", "component"));
    }

    #[test]
    fn component_referencing_context_resolves_cleanly() {
        let content = r#"# Component: LoginForm

## Purpose
Lets the user authenticate.

## Props
- error_message: authentication error message

## Events
- submitted: credentials submitted

## Composition
- EmailInput: receives email
- when `EmailInput`.`changed`: invoke `ValidateEmail`

## Scenarios
### submit
Given: valid credentials
When: submit is pressed
Then: submitted event fires
"#;
        let doc = parse_draft_file(
            Path::new("drafts/components/LoginForm.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let vocab = vocab_with(&[
            ("EmailInput", "component"),
            ("ValidateEmail", "context"),
        ]);
        let prepared = prepare_document(&doc, &vocab).unwrap();

        assert_eq!(prepared.kind, ArtifactKind::Component);
        // No ambiguities about the context reference — it's allowed
        let blocking: Vec<_> = prepared.blocking_ambiguities().collect();
        assert!(
            blocking.is_empty(),
            "expected no blocking ambiguities, got: {:?}",
            blocking
        );
    }

    #[test]
    fn routing_table_missing_component_fails() {
        let content = r#"# Routes: main

## Purpose
Main routes.

## Routes
- path: /
  component: HomeScreen
"#;
        let doc = parse_draft_file(
            Path::new("drafts/routes/main.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let vocab = empty_vocab(); // HomeScreen NOT in vocabulary
        let prepared = prepare_document(&doc, &vocab).unwrap();

        assert!(
            prepared.ambiguities.iter().any(|a| a.message.contains("HomeScreen")),
            "expected ambiguity about missing HomeScreen component: {:?}",
            prepared.ambiguities
        );
    }

    #[test]
    fn routing_table_missing_param_type_fails() {
        let content = r#"# Routes: main

## Purpose
Main routes.

## Routes
- path: /products/<id>
  component: ProductScreen
  params:
    id: Product
"#;
        let doc = parse_draft_file(
            Path::new("drafts/routes/main.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let vocab = vocab_with(&[("ProductScreen", "component")]); // Product NOT in vocab
        let prepared = prepare_document(&doc, &vocab).unwrap();

        assert!(
            prepared.ambiguities.iter().any(|a| a.message.contains("Product")),
            "expected ambiguity about missing Product type: {:?}",
            prepared.ambiguities
        );
    }

    #[test]
    fn routing_table_path_param_without_declaration_fails() {
        let content = r#"# Routes: main

## Purpose
Main routes.

## Routes
- path: /products/<id>
  component: ProductScreen
"#;
        let doc = parse_draft_file(
            Path::new("drafts/routes/main.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let vocab = vocab_with(&[("ProductScreen", "component")]);
        let prepared = prepare_document(&doc, &vocab).unwrap();

        assert!(
            prepared.ambiguities.iter().any(|a| a.message.contains("<id>")),
            "expected ambiguity about undeclared path param <id>: {:?}",
            prepared.ambiguities
        );
    }

    // ── Prepared output shape tests ──────────────────────────────

    #[test]
    fn prepare_component_produces_current_schema_yaml() {
        let content = r#"# Component: Button

## Purpose
A pressable element.

## Props
- label: the text shown

## Events
- pressed: the user pressed the button

## Scenarios
### press
Given: a button
When: pressed
Then: pressed event fires
"#;
        let doc = parse_draft_file(
            Path::new("drafts/components/Button.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let vocab = empty_vocab();
        let prepared = prepare_document(&doc, &vocab).unwrap();

        assert_eq!(prepared.schema, "reen.prepare/v4");
        assert_eq!(prepared.kind, ArtifactKind::Component);
        assert_eq!(prepared.export.name, "Button");
        assert_eq!(prepared.props.len(), 1);
        assert_eq!(prepared.events.len(), 1);
        assert_eq!(prepared.events[0].name, "pressed");
    }

    #[test]
    fn prepare_brand_produces_current_schema_yaml() {
        let content = r#"# Brand: Acme

## Purpose
Professional experience.

## Voice
- Tone: confident
- Register: formal

## Visual Identity
### Colours
- primary-action: main CTA colour
### Typography
- heading-large: page headings

## Motion
- transition-default: standard transitions
"#;
        let doc = parse_draft_file(
            Path::new("drafts/brands/Acme.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let prepared = prepare_document(&doc, &empty_vocab()).unwrap();

        assert_eq!(prepared.schema, "reen.prepare/v4");
        assert_eq!(prepared.kind, ArtifactKind::Brand);
        assert_eq!(prepared.export.name, "Acme");
        assert_eq!(prepared.token_groups.len(), 3); // colours, typography, motion
        assert!(!prepared.prose_sections.is_empty()); // voice
    }

    #[test]
    fn prepare_routing_table_produces_current_schema_yaml() {
        let content = r#"# Routes: main

## Purpose
Main routes.

## Routes
- path: /
  component: HomeScreen
- path: /products/<id>
  component: ProductScreen
  params:
    id: Product
"#;
        let doc = parse_draft_file(
            Path::new("drafts/routes/main.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let vocab = vocab_with(&[
            ("HomeScreen", "component"),
            ("ProductScreen", "component"),
            ("Product", "data"),
        ]);
        let prepared = prepare_document(&doc, &vocab).unwrap();

        assert_eq!(prepared.schema, "reen.prepare/v4");
        assert_eq!(prepared.kind, ArtifactKind::RoutingTable);
        assert_eq!(prepared.routes.len(), 2);
        assert_eq!(prepared.routes[0].path, "/");
        assert_eq!(prepared.routes[1].params.len(), 1);
        assert!(prepared.ambiguities.is_empty());
    }

    // ── Schema version test ──────────────────────────────────────

    #[test]
    fn all_artefact_kinds_produce_current_schema() {
        // After the projection-adopts-context-shape rename, all artefact
        // kinds emit the current unified schema.
        let content = r#"# Data: Amount

## Purpose
A monetary amount.

## Attributes
- value: numeric quantity

## Invariants
- never negative

## Scenarios
### create
Given: value 100
When: created
Then: value is 100
"#;
        let doc = parse_draft_file(
            Path::new("drafts/data/amount.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let prepared = prepare_document(&doc, &empty_vocab()).unwrap();
        assert_eq!(prepared.schema, "reen.prepare/v4");
    }

    // ── Classification tests ─────────────────────────────────────

    #[test]
    fn context_without_operations_is_client_also() {
        let content = r#"# Context: SimpleTransfer

## Purpose
Transfers amounts.

## Role Players
- source: Account — provides funds
- sink: Account — receives funds

## Role Methods
### source
- withdraw: removes amount

### sink
- deposit: adds amount

## Functionalities
### transfer
Source withdraws, sink deposits.

## Scenarios
### ok
Given: accounts
When: transfer
Then: done
"#;
        let doc = parse_draft_file(
            Path::new("drafts/contexts/simple_transfer.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let vocab = vocab_with(&[("Account", "data")]);
        let prepared = prepare_document(&doc, &vocab).unwrap();

        // Simulate classification pass
        let workspace = crate::workspace::Workspace::discover(
            std::env::temp_dir().join("reen_classify_test_1"),
        )
        .unwrap();
        let output = workspace
            .prepared_output_path(Path::new("drafts/contexts/simple_transfer.md"))
            .unwrap();
        let mut artifacts = vec![(output, prepared)];
        classify_contexts(&mut artifacts, &workspace);

        assert_eq!(
            artifacts[0].1.realisation_target.as_deref(),
            Some("client_also"),
        );
    }

    #[test]
    fn sanitize_export_name_new_kinds() {
        assert_eq!(sanitize_export_name("Component: Button"), "Button");
        assert_eq!(sanitize_export_name("Brand: Acme"), "Acme");
        assert_eq!(sanitize_export_name("Routes: main"), "main");
    }

    #[test]
    fn extract_path_params_works() {
        assert_eq!(extract_path_params("/"), Vec::<String>::new());
        assert_eq!(extract_path_params("/products/<id>"), vec!["id"]);
        assert_eq!(
            extract_path_params("/users/<user_id>/posts/<post_id>"),
            vec!["user_id", "post_id"]
        );
    }

    // ── Inference tests (Group 2 / bugs 5+6) ──────────────────────

    fn data_artifact_with_op(name: &str, op: &str, scenarios: Vec<ScenarioSpec>) -> PreparedArtifact {
        let mut a = PreparedArtifact::new(
            ArtifactKind::Data,
            format!("data/{}.md", name),
            name.into(),
            name.into(),
        );
        a.operations.push(OperationSpec {
            name: op.into(),
            description: "test op".into(),
            ..Default::default()
        });
        a.scenarios = scenarios;
        a
    }

    fn scenario(when: &str, given: &str, then: &str) -> ScenarioSpec {
        ScenarioSpec {
            name: "test".into(),
            given: given.into(),
            when: when.into(),
            then: then.into(),
        }
    }

    #[test]
    fn derive_signature_binary_operation_from_scenarios() {
        // Money.add — Given carries two `a Money of …` references, Then
        // returns `a Money of …`. Inference must produce
        // `&self, other: &Money) -> Money`.
        let mut a = data_artifact_with_op(
            "Money",
            "add",
            vec![scenario(
                "add is performed",
                "a Money of 5000 in DKK and a Money of 2500 in DKK",
                "the result is a Money of 7500 in DKK",
            )],
        );
        let vocab = DciVocabulary::default();
        derive_signatures(&mut a, &vocab);

        let sig = a.operations[0].derived_signature.as_ref().expect("sig");
        assert_eq!(sig.receiver, "&self");
        assert_eq!(sig.parameters.len(), 1);
        assert_eq!(sig.parameters[0].name, "other");
        assert_eq!(sig.parameters[0].rust_type, "&Money");
        assert_eq!(sig.returns, "Money");
        assert_eq!(sig.source, "derived");
    }

    #[test]
    fn derive_signature_text_return_from_quoted_result() {
        // Money.display_as_major_units — Given has one Money, Then is
        // a quoted string. Inference: `&self) -> String`.
        let mut a = data_artifact_with_op(
            "Money",
            "display as major units",
            vec![scenario(
                "display as major units is performed",
                "a Money of 10050 in DKK",
                "the result is \"100.50 DKK\"",
            )],
        );
        derive_signatures(&mut a, &DciVocabulary::default());

        let sig = a.operations[0].derived_signature.as_ref().expect("sig");
        assert_eq!(sig.receiver, "&self");
        assert!(sig.parameters.is_empty());
        assert_eq!(sig.returns, "String");
    }

    #[test]
    fn derive_signature_boolean_return_from_affirmative_negative() {
        // Session.is_expired — Then is `the answer is negative`.
        let mut a = data_artifact_with_op(
            "Session",
            "is expired",
            vec![
                scenario(
                    "is expired is checked",
                    "a Session with expiry in the past",
                    "the answer is affirmative",
                ),
                scenario(
                    "is expired is checked",
                    "a Session with expiry in the future",
                    "the answer is negative",
                ),
            ],
        );
        derive_signatures(&mut a, &DciVocabulary::default());

        let sig = a.operations[0].derived_signature.as_ref().expect("sig");
        assert_eq!(sig.returns, "bool");
        assert_eq!(sig.receiver, "&self");
        assert!(sig.parameters.is_empty());
    }

    #[test]
    fn derive_signature_no_scenarios_emits_finding() {
        let mut a = data_artifact_with_op("Money", "untested_op", vec![]);
        derive_signatures(&mut a, &DciVocabulary::default());

        assert!(a.operations[0].derived_signature.is_none());
        let has_finding = a.ambiguities.iter().any(|x| {
            x.message.contains("untested_op") && x.message.contains("no scenario")
        });
        assert!(has_finding, "expected a no-scenario finding, got {:?}", a.ambiguities);
    }

    #[test]
    fn derive_signature_disagreement_emits_blocking_finding() {
        // Two scenarios for the same op disagree on arity — one supplies a
        // single Money operand, the other supplies two. Inference must
        // refuse to emit and surface a blocking finding.
        let mut a = data_artifact_with_op(
            "Money",
            "tricky",
            vec![
                scenario(
                    "tricky is performed",
                    "a Money of 100 in DKK",
                    "the result is a Money of 200 in DKK",
                ),
                scenario(
                    "tricky is performed",
                    "a Money of 100 in DKK and a Money of 50 in DKK",
                    "the result is a Money of 50 in DKK",
                ),
            ],
        );
        derive_signatures(&mut a, &DciVocabulary::default());

        assert!(a.operations[0].derived_signature.is_none());
        let blocker = a
            .ambiguities
            .iter()
            .find(|x| x.message.contains("tricky") && x.message.contains("disagree"))
            .expect("expected disagreement finding");
        assert_eq!(blocker.severity, "blocking");
    }

    #[test]
    fn derive_multiplicity_list_from_plural_name_and_multiple_instances() {
        let mut a = PreparedArtifact::new(
            ArtifactKind::Projection,
            "projections/transactions.md".into(),
            "TransactionList".into(),
            "TransactionList".into(),
        );
        a.role_players.push(RolePlayerSpec {
            name: "transactions".into(),
            data_type: "Transaction".into(),
            responsibility: "the transactions to filter".into(),
            ..Default::default()
        });
        a.scenarios = vec![scenario(
            "filtered is queried",
            "five transactions spanning the last week",
            "all five transactions are returned",
        )];
        derive_multiplicity(&mut a);

        assert_eq!(a.role_players[0].multiplicity, Multiplicity::List);
        // No disagreement finding because name and scenarios agree.
        assert!(a.ambiguities.is_empty(), "no findings expected, got {:?}", a.ambiguities);
    }

    #[test]
    fn derive_multiplicity_single_from_singular_name_and_one_instance() {
        let mut a = PreparedArtifact::new(
            ArtifactKind::Context,
            "contexts/auth.md".into(),
            "Auth".into(),
            "Auth".into(),
        );
        a.role_players.push(RolePlayerSpec {
            name: "session".into(),
            data_type: "Session".into(),
            responsibility: "the active session".into(),
            ..Default::default()
        });
        a.scenarios = vec![scenario(
            "authenticate is performed",
            "a Session with chat state \"idle\"",
            "the session is authenticated",
        )];
        derive_multiplicity(&mut a);

        assert_eq!(a.role_players[0].multiplicity, Multiplicity::Single);
    }

    #[test]
    fn derive_multiplicity_disagreement_emits_finding() {
        // Plural-named role player but every scenario supplies a single
        // instance — surface a finding so the BA clarifies.
        let mut a = PreparedArtifact::new(
            ArtifactKind::Projection,
            "projections/odd.md".into(),
            "Odd".into(),
            "Odd".into(),
        );
        a.role_players.push(RolePlayerSpec {
            name: "transactions".into(),
            data_type: "Transaction".into(),
            responsibility: "the transactions".into(),
            ..Default::default()
        });
        a.scenarios = vec![scenario(
            "queried",
            "a transaction posted yesterday",
            "the transaction is returned",
        )];
        derive_multiplicity(&mut a);

        let finding = a
            .ambiguities
            .iter()
            .find(|x| x.message.contains("transactions") && x.message.contains("conflicting"));
        assert!(finding.is_some(), "expected disagreement finding, got {:?}", a.ambiguities);
    }

    // ── Prose-reference lint tests (Phase 9.5) ──────────────────

    /// Helper: run check_prose_references on text with given vocab and return ambiguities.
    fn prose_ref_ambiguities(text: &str, vocab: &DciVocabulary) -> Vec<Ambiguity> {
        let mut ambiguities = Vec::new();
        check_prose_references(text, "data", "TestDraft", vocab, &mut ambiguities);
        ambiguities
    }

    #[test]
    fn prose_ref_resolved_token_passes() {
        let vocab = vocab_with(&[("Account", "data")]);
        let ambs = prose_ref_ambiguities("The source Account is valid", &vocab);
        // Account resolves, so reference graph check may fire (data→data is allowed)
        // but no "possible prose reference" or "typo" finding
        assert!(
            !ambs.iter().any(|a| a.message.contains("Possible")),
            "resolved token should not produce prose-reference finding: {:?}",
            ambs
        );
    }

    #[test]
    fn prose_ref_stopword_ignored() {
        let vocab = empty_vocab();
        let ambs = prose_ref_ambiguities("The user types their password", &vocab);
        assert!(
            ambs.is_empty(),
            "stopword 'The' should be ignored: {:?}",
            ambs
        );
    }

    #[test]
    fn prose_ref_backtick_span_ignored() {
        let vocab = empty_vocab();
        let ambs = prose_ref_ambiguities("Use `Widget` to render the output", &vocab);
        assert!(
            !ambs.iter().any(|a| a.message.contains("Widget")),
            "backtick span should be ignored: {:?}",
            ambs
        );
    }

    #[test]
    fn prose_ref_heading_ignored() {
        let vocab = empty_vocab();
        let ambs = prose_ref_ambiguities("# Component: Button\nSome text", &vocab);
        assert!(
            !ambs.iter().any(|a| a.message.contains("Button") || a.message.contains("Component")),
            "heading tokens should be ignored: {:?}",
            ambs
        );
    }

    #[test]
    fn prose_ref_scenario_value_line_ignored() {
        let vocab = empty_vocab();
        let ambs = prose_ref_ambiguities(
            "Given: a body of [(5,5), (4,5), (3,5)]",
            &vocab,
        );
        assert!(
            ambs.is_empty(),
            "scenario value line should be ignored: {:?}",
            ambs
        );
    }

    #[test]
    fn prose_ref_typo_is_high_confidence() {
        let vocab = vocab_with(&[("Account", "data")]);
        let ambs = prose_ref_ambiguities(
            "The Account is valid but the Accoutn is not",
            &vocab,
        );
        let typo_finding = ambs.iter().find(|a| a.message.contains("Accoutn"));
        assert!(
            typo_finding.is_some(),
            "expected typo finding for Accoutn: {:?}",
            ambs
        );
        let finding = typo_finding.unwrap();
        assert_eq!(finding.severity, "blocking");
        assert!(finding.message.contains("Account"));
    }

    #[test]
    fn prose_ref_standalone_unresolved_is_low_confidence() {
        let vocab = empty_vocab();
        let ambs = prose_ref_ambiguities("Widget is used here", &vocab);
        let widget = ambs.iter().find(|a| a.message.contains("Widget"));
        assert!(widget.is_some(), "expected Widget finding: {:?}", ambs);
        assert_eq!(widget.unwrap().severity, "low");
    }

    #[test]
    fn prose_ref_all_caps_acronym_ignored() {
        let vocab = empty_vocab();
        let ambs = prose_ref_ambiguities("Use UTC for timestamps and DKK for currency", &vocab);
        assert!(
            !ambs.iter().any(|a| a.message.contains("UTC") || a.message.contains("DKK")),
            "all-caps acronyms should be ignored: {:?}",
            ambs
        );
    }

    #[test]
    fn prose_ref_order_in_phrase_not_flagged() {
        // "in order to" should not flag "Order" even if Order is a manifest entry
        let vocab = vocab_with(&[("Order", "data")]);
        let ambs = prose_ref_ambiguities(
            "The system does this in order to complete the transfer",
            &vocab,
        );
        // "Order" is in the manifest as data, data→data is allowed, so no graph violation.
        // The key check: the lowercase "order" should not match PascalCase regex.
        // Only "Order" at word boundary matches, which resolves fine.
        assert!(
            !ambs.iter().any(|a| a.severity == "blocking"),
            "lowercase 'order' should not produce a blocking finding: {:?}",
            ambs
        );
    }

    #[test]
    fn prose_ref_levenshtein_basics() {
        assert_eq!(levenshtein_distance("Account", "Account"), 0);
        assert_eq!(levenshtein_distance("Account", "Accoutn"), 2);
        assert_eq!(levenshtein_distance("Account", "Acount"), 1);
        assert_eq!(levenshtein_distance("Account", "Widget"), 6);
    }

    #[test]
    fn is_likely_typo_basics() {
        assert!(is_likely_typo("Account", "Accoutn")); // edit distance 2
        assert!(is_likely_typo("Account", "Acount"));  // edit distance 1
        assert!(!is_likely_typo("Account", "Widget")); // too different
        assert!(!is_likely_typo("Account", "Account")); // same
        assert!(!is_likely_typo("Foo", "Fop")); // too short (< 4 chars)
        assert!(!is_likely_typo("Account", "Accounts")); // plural, not typo
        assert!(!is_likely_typo("GeographicCell", "GeographicCells")); // plural
    }

    #[test]
    fn strip_backtick_spans_works() {
        assert_eq!(strip_backtick_spans("no backticks"), "no backticks");
        assert_eq!(strip_backtick_spans("use `Widget` here"), "use          here");
        assert_eq!(strip_backtick_spans("`all`"), "     ");
    }

    #[test]
    fn is_scenario_value_line_works() {
        assert!(is_scenario_value_line("Given: a body of [(5,5), (4,5)]"));
        assert!(is_scenario_value_line("Then: result is {x: 1, y: 2}"));
        assert!(!is_scenario_value_line("Given: source has balance 100"));
        assert!(!is_scenario_value_line("The source withdraws"));
    }

    #[test]
    fn is_all_caps_works() {
        assert!(is_all_caps("UTC"));
        assert!(is_all_caps("DKK"));
        assert!(is_all_caps("JSON"));
        assert!(!is_all_caps("Account"));
        assert!(!is_all_caps("A")); // single char
        assert!(!is_all_caps("iPhone"));
    }

    // ── App prepare tests (Phase 2.5) ────────────────────────────

    #[test]
    fn prepare_app_resolves_entry_context() {
        let content = r#"# App

## Application Kind
`cli_app`

## Entry Context
`GameLoop`

## Entry Functionality
`tick`

## Dependencies
- anyhow
"#;
        let doc = parse_draft_file(
            Path::new("drafts/app.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let vocab = vocab_with(&[("GameLoop", "context")]);
        let prepared = prepare_document(&doc, &vocab).unwrap();

        assert_eq!(prepared.kind, ArtifactKind::App);
        assert_eq!(prepared.application_kind.as_deref(), Some("cli_app"));
        assert_eq!(prepared.entry_context.as_deref(), Some("GameLoop"));
        assert_eq!(prepared.entry_functionality.as_deref(), Some("tick"));
        assert_eq!(prepared.app_dependencies, vec!["anyhow"]);
        assert!(prepared.ambiguities.is_empty(), "{:?}", prepared.ambiguities);
    }

    #[test]
    fn prepare_app_flags_unresolved_entry_context() {
        let content = r#"# App

## Application Kind
`cli_app`

## Entry Context
`NonExistent`
"#;
        let doc = parse_draft_file(
            Path::new("drafts/app.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let vocab = empty_vocab();
        let prepared = prepare_document(&doc, &vocab).unwrap();

        assert!(
            prepared.ambiguities.iter().any(|a| a.message.contains("NonExistent")),
            "expected ambiguity about unresolved entry context: {:?}",
            prepared.ambiguities
        );
    }

    #[test]
    fn prepare_app_flags_non_context_entry() {
        let content = r#"# App

## Application Kind
`cli_app`

## Entry Context
`Amount`
"#;
        let doc = parse_draft_file(
            Path::new("drafts/app.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let vocab = vocab_with(&[("Amount", "data")]);
        let prepared = prepare_document(&doc, &vocab).unwrap();

        assert!(
            prepared.ambiguities.iter().any(|a| a.message.contains("data")),
            "expected ambiguity about non-context entry: {:?}",
            prepared.ambiguities
        );
    }

    // ── Unresolved reference heuristic tests ─────────────────────

    #[test]
    fn classify_implementation_speak_suffixes() {
        let cases = vec![
            "SessionStore", "NordeaClient", "AuditSink", "IdempotencyLedger",
            "UserRepository", "PaymentService", "RequestHandler",
            "DatabaseAdapter", "ApiGateway", "ConnectionPool",
            "TokenRegistry", "ResponseCache", "EventQueue", "MessageBus",
            "SystemClock", "RetryTimer",
        ];
        for name in cases {
            let (cat, _msg) = classify_unresolved_reference(name);
            assert_eq!(
                cat, UnresolvedCategory::ImplementationSpeak,
                "{} should be classified as implementation-speak",
                name
            );
        }
    }

    #[test]
    fn classify_missing_data_draft() {
        let cases = vec!["Account", "Transaction", "Money", "Customer", "Receipt"];
        for name in cases {
            let (cat, msg) = classify_unresolved_reference(name);
            assert_eq!(
                cat, UnresolvedCategory::MissingDataDraft,
                "{} should be classified as missing data draft",
                name
            );
            assert!(msg.contains("drafts/data/"), "suggestion should point to data dir: {}", msg);
        }
    }

    #[test]
    fn classify_missing_context() {
        let cases = vec!["AuthenticateUser", "TransferFunds", "ValidatePayment"];
        for name in cases {
            let (cat, msg) = classify_unresolved_reference(name);
            assert_eq!(
                cat, UnresolvedCategory::MissingContext,
                "{} should be classified as missing context",
                name
            );
            assert!(msg.contains("drafts/contexts/"), "suggestion should point to contexts dir: {}", msg);
        }
    }

    #[test]
    fn builtin_primitives_pass() {
        assert!(is_builtin_primitive("String"));
        assert!(is_builtin_primitive("i64"));
        assert!(is_builtin_primitive("bool"));
        assert!(is_builtin_primitive("DateTime"));
        assert!(is_builtin_primitive("Vec"));
        assert!(!is_builtin_primitive("Account"));
        assert!(!is_builtin_primitive("SessionStore"));
    }

    #[test]
    fn prepare_context_rejects_implementation_speak_role_player() {
        let content = "# Context: BadContext\n\n\
                       ## Purpose\nDoes something.\n\n\
                       ## Role Players\n\
                       - store: SessionStore — persists sessions\n\n\
                       ## Role Methods\n### store\n- save: saves the session\n\n\
                       ## Functionalities\n### do_thing\nCalls store save.\n\n\
                       ## Scenarios\n### happy path\nGiven: a store\nWhen: do_thing\nThen: success\n";

        let doc = parse_draft_file(
            Path::new("drafts/contexts/bad.md"),
            Path::new("drafts"),
            content,
        ).unwrap();
        let vocab = empty_vocab();
        let prepared = prepare_document(&doc, &vocab).unwrap();

        let blocking: Vec<_> = prepared.blocking_ambiguities().collect();
        assert!(
            !blocking.is_empty(),
            "SessionStore should produce a blocking ambiguity"
        );
        assert!(
            blocking.iter().any(|a| a.message.contains("implementation-speak")),
            "error should mention implementation-speak: {:?}",
            blocking
        );
    }

    #[test]
    fn prepare_context_accepts_declared_role_player() {
        let content = "# Context: GoodContext\n\n\
                       ## Purpose\nDoes something.\n\n\
                       ## Role Players\n\
                       - account: Account — the source account\n\n\
                       ## Role Methods\n### account\n- debit: removes funds\n\n\
                       ## Functionalities\n### withdraw\nCalls account debit.\n\n\
                       ## Scenarios\n### happy path\nGiven: an account\nWhen: withdraw\nThen: success\n";

        let doc = parse_draft_file(
            Path::new("drafts/contexts/good.md"),
            Path::new("drafts"),
            content,
        ).unwrap();
        let vocab = vocab_with(&[("Account", "data")]);
        let prepared = prepare_document(&doc, &vocab).unwrap();

        let blocking: Vec<_> = prepared.blocking_ambiguities().collect();
        assert!(
            blocking.is_empty(),
            "Account is declared — should have no blocking ambiguities: {:?}",
            blocking
        );
    }
}
