use crate::draft_parser::{
    ContextDraft, DataDraft, DraftDocument, ProjectionDraft, parse_draft_file,
};
use crate::draft_refine::{DraftReview, review_raw_drafts};
use crate::manifest::DciVocabulary;
use crate::test_synth::synthesise_tests;
use crate::prepared::{
    Ambiguity, ArtifactKind, AttributeSpec, FunctionalitySpec, OperationSpec, PreparedArtifact,
    PropSpec, QuerySpec, RoleMethodGroupSpec, RoleMethodSpec, RolePlayerSpec, ScenarioSpec,
    SynthesisedTest, VocabularyBinding,
};
use crate::workspace::Workspace;
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::fs;

#[derive(Debug, Clone)]
pub struct PrepareOptions {
    pub selection: crate::workspace::Selection,
    pub profile: Option<String>,
    pub verbose: bool,
    pub debug: bool,
    pub dry_run: bool,
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
    let vocabulary = DciVocabulary::load(workspace)?;

    let mut wrote = 0usize;
    let mut blocking_total = 0usize;

    if options.verbose {
        println!("Preparing {} draft(s)", selected_paths.len());
    }

    for path in &selected_paths {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let draft = parse_draft_file(path, &workspace.drafts_dir, &content)?;
        let output_path = workspace.prepared_output_path(path)?;
        let mut prepared = prepare_document(&draft, &vocabulary)?;

        // LLM-backed test synthesis from prose invariants/descriptions
        let synth = synthesise_tests(&prepared);
        prepared.synthesised_tests.extend(synth);

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
            wrote += 1;
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
    let raw_paths = workspace.raw_draft_paths(&options.selection)?;
    if options.verbose {
        println!("Reviewing {} raw draft(s)", raw_paths.len());
    }
    let draft_review = review_raw_drafts(workspace, &raw_paths)?;
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

// ── Document preparation ──────────────────────────────────────────

fn prepare_document(doc: &DraftDocument, vocab: &DciVocabulary) -> Result<PreparedArtifact> {
    match doc {
        DraftDocument::Context(ctx) => prepare_context(ctx, vocab),
        DraftDocument::Data(data) => prepare_data(data, vocab),
        DraftDocument::Projection(proj) => prepare_projection(proj, vocab),
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
        let resolved = vocab.resolve(&rp.data_type);
        artifact.role_players.push(RolePlayerSpec {
            name: rp.name.clone(),
            data_type: rp.data_type.clone(),
            responsibility: rp.responsibility.clone(),
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
        } else {
            ambiguities.push(Ambiguity {
                message: format!(
                    "Role player `{}` references `{}` which is not in the vocabulary manifest. \
                     Is it a data type or projection that should be added to the manifest?",
                    rp.name, rp.data_type
                ),
                severity: "blocking".to_string(),
                suggested_resolution: Some(format!(
                    "Add `{}` to the vocabulary manifest as a data or projection type",
                    rp.data_type
                )),
                source_line: None,
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

fn prepare_data(data: &DataDraft, _vocab: &DciVocabulary) -> Result<PreparedArtifact> {
    let export_name = sanitize_export_name(&data.info.title);
    let mut artifact = PreparedArtifact::new(
        ArtifactKind::Data,
        data.info.relative_path.clone(),
        data.info.title.clone(),
        export_name,
    );

    let mut bindings = Vec::new();

    for attr in &data.attributes {
        artifact.attributes.push(AttributeSpec {
            name: attr.name.clone(),
            description: attr.description.clone(),
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
    let export_name = sanitize_export_name(&proj.info.title);
    let mut artifact = PreparedArtifact::new(
        ArtifactKind::Projection,
        proj.info.relative_path.clone(),
        proj.info.title.clone(),
        export_name,
    );

    let mut bindings = Vec::new();
    let mut ambiguities = Vec::new();

    // Source resolution
    artifact.source_data = Some(proj.source.clone());
    if !proj.source.is_empty() {
        let resolved = vocab.resolve(&proj.source);
        if let Some(resolution) = &resolved {
            bindings.push(VocabularyBinding {
                name: proj.source.clone(),
                resolution: resolution.clone(),
                resolved_to: Some(proj.source.clone()),
            });
        } else {
            ambiguities.push(Ambiguity {
                message: format!(
                    "Projection source `{}` is not in the vocabulary manifest",
                    proj.source
                ),
                severity: "blocking".to_string(),
                suggested_resolution: Some(format!(
                    "Add `{}` to the vocabulary manifest as a data type",
                    proj.source
                )),
                source_line: None,
            });
        }
    }

    for query in &proj.queries {
        artifact.queries.push(QuerySpec {
            name: query.name.clone(),
            description: query.description.clone(),
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

/// Convert a draft title like "Context: MoneyTransfer" or "Data: Amount" into
/// a clean PascalCase export name.
fn sanitize_export_name(title: &str) -> String {
    let cleaned = title
        .strip_prefix("Context:")
        .or_else(|| title.strip_prefix("Data:"))
        .or_else(|| title.strip_prefix("Projection:"))
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
    fn prepare_projection_flags_unresolved_source() {
        let content = r#"# Projection: AccountBalance

## Purpose
Provides a read-only view of an account's current balance.

## Source
- Account

## Queries
### current_balance
Returns the current balance of the underlying account.

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
}
