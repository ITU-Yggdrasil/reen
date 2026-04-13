use anyhow::{Context, Result};
use proc_macro2::Span;
use regex::Regex;
use serde::Serialize;
use serde_json::json;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use syn::{
    Expr, File, FnArg, ImplItem, ImplItemFn, Item, ItemExternCrate, ItemUse, Member, Pat,
    Path as SynPath, Stmt, Type, UseTree, Visibility,
    spanned::Spanned,
    visit::{self, Visit},
};

use super::capability_registry::{
    allowed_external_crate_roots, registry_provider_domains_by_crate,
};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub(crate) enum SpecificationKind {
    App,
    Context,
    Projection,
    Data,
    Unknown,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct DelegationRequirement {
    pub(crate) actor: String,
    pub(crate) target: String,
    pub(crate) source_line: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct OutputRequirement {
    pub(crate) literal: String,
    pub(crate) source_line: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct BehaviorContract {
    pub(crate) title: String,
    pub(crate) kind: SpecificationKind,
    pub(crate) source_path: String,
    pub(crate) collaborators: Vec<String>,
    pub(crate) env_vars: Vec<String>,
    pub(crate) delegation_requirements: Vec<DelegationRequirement>,
    pub(crate) output_requirements: Vec<OutputRequirement>,
    pub(crate) shared_state_requirements: Vec<String>,
    pub(crate) role_method_names: Vec<String>,
    pub(crate) external_behavior_clues: Vec<String>,
}

impl BehaviorContract {
    /// Returns true when the kind is always immutable by definition (Data or Projection).
    pub(crate) fn is_immutable(&self) -> bool {
        matches!(
            self.kind,
            SpecificationKind::Data | SpecificationKind::Projection
        )
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct SpecificationQualityReport {
    pub(crate) contract: BehaviorContract,
    pub(crate) errors: Vec<String>,
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct RequiredArtifact {
    pub(crate) collaborator: String,
    pub(crate) candidate_paths: Vec<String>,
    pub(crate) exists: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct VerificationTarget {
    pub(crate) kind: String,
    pub(crate) detail: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct ImplementationPlanReport {
    pub(crate) contract: BehaviorContract,
    pub(crate) output_path: String,
    pub(crate) required_artifacts: Vec<RequiredArtifact>,
    pub(crate) verification_targets: Vec<VerificationTarget>,
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct VerificationEvidence {
    pub(crate) obligation: String,
    pub(crate) satisfied: bool,
    pub(crate) evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodeLocation {
    line: usize,
    column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodeFinding {
    message: String,
    location: Option<CodeLocation>,
}

impl CodeFinding {
    fn without_location(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            location: None,
        }
    }

    fn with_span(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            location: location_from_span(span),
        }
    }

    fn with_offset(message: impl Into<String>, code: &str, offset: usize) -> Self {
        Self {
            message: message.into(),
            location: Some(location_from_offset(code, offset)),
        }
    }

    fn with_location(mut self, location: Option<CodeLocation>) -> Self {
        self.location = location;
        self
    }

    fn render_for_path(&self, output_path: &Path) -> String {
        if let Some(location) = &self.location {
            format!(
                "{}:{}:{}: {}",
                output_path.display(),
                location.line,
                location.column,
                self.message
            )
        } else {
            format!("{}: {}", output_path.display(), self.message)
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct StaticBehaviorVerifierReport {
    pub(crate) contract: BehaviorContract,
    pub(crate) output_path: String,
    pub(crate) errors: Vec<String>,
    pub(crate) warnings: Vec<String>,
    pub(crate) high_risk_findings: Vec<String>,
    pub(crate) evidence: Vec<VerificationEvidence>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct SemanticRegressionReport {
    pub(crate) worsened: bool,
    pub(crate) issues: Vec<String>,
    pub(crate) before: StaticBehaviorVerifierReport,
    pub(crate) after: StaticBehaviorVerifierReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Section {
    title: String,
    body: String,
}

pub(crate) fn analyze_specification(
    spec_path: &Path,
    spec_content: &str,
    dependency_context: Option<&HashMap<String, serde_json::Value>>,
) -> SpecificationQualityReport {
    let contract = extract_behavior_contract(spec_path, spec_content);
    let sections = parse_markdown_sections(spec_content);
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if !contract.env_vars.is_empty() {
        if let Some(env_section) = find_section(&sections, "Environment Variables") {
            let env_body = env_section.body.to_ascii_lowercase();
            if env_body.contains("no environment variables referenced")
                || env_body.contains("unspecified")
            {
                errors.push(format!(
                    "Environment variable section contradicts referenced env vars: {}",
                    contract.env_vars.join(", ")
                ));
            }
        }
    }

    let collaborator_set: HashSet<&str> =
        contract.collaborators.iter().map(String::as_str).collect();
    for requirement in &contract.delegation_requirements {
        if !collaborator_set.contains(requirement.actor.as_str()) {
            warnings.push(format!(
                "Delegation requirement references '{}' but it is not listed as a collaborator",
                requirement.actor
            ));
        }
        if !collaborator_set.contains(requirement.target.as_str()) {
            warnings.push(format!(
                "Delegation requirement references '{}' but it is not listed as a collaborator",
                requirement.target
            ));
        }
    }

    if let Some(ctx) = dependency_context {
        let known = dependency_names_from_context(ctx);
        for collaborator in &contract.collaborators {
            if is_probably_domain_type(collaborator)
                && !known.is_empty()
                && !known.contains(collaborator)
            {
                warnings.push(format!(
                    "Collaborator '{}' was not found in dependency context; verify dependency planning or spec references",
                    collaborator
                ));
            }
        }
    }

    if matches!(contract.kind, SpecificationKind::Context)
        && !contract.external_behavior_clues.is_empty()
        && contract.role_method_names.is_empty()
    {
        warnings.push(
            "Specification describes behavior with external interactions but exposes no role methods"
                .to_string(),
        );
    }

    SpecificationQualityReport {
        contract,
        errors,
        warnings,
    }
}

pub(crate) fn build_implementation_plan(
    spec_path: &Path,
    spec_content: &str,
    output_path: &Path,
    project_root: &Path,
) -> ImplementationPlanReport {
    let contract = extract_behavior_contract(spec_path, spec_content);
    let required_artifacts = contract
        .collaborators
        .iter()
        .filter(|name| is_probably_domain_type(name))
        .map(|name| {
            let candidate_paths = collaborator_candidate_paths(project_root, name);
            let exists = candidate_paths.iter().any(|path| Path::new(path).exists());
            RequiredArtifact {
                collaborator: name.clone(),
                candidate_paths,
                exists,
            }
        })
        .collect::<Vec<_>>();

    let mut verification_targets = Vec::new();
    verification_targets.extend(
        contract
            .env_vars
            .iter()
            .cloned()
            .map(|name| VerificationTarget {
                kind: "env_var".to_string(),
                detail: name,
            }),
    );
    verification_targets.extend(contract.collaborators.iter().cloned().map(|name| {
        VerificationTarget {
            kind: "collaborator".to_string(),
            detail: name,
        }
    }));
    verification_targets.extend(contract.delegation_requirements.iter().map(|req| {
        VerificationTarget {
            kind: "delegation".to_string(),
            detail: format!("{} -> {}", req.actor, req.target),
        }
    }));
    verification_targets.extend(
        contract
            .shared_state_requirements
            .iter()
            .cloned()
            .map(|detail| VerificationTarget {
                kind: "shared_state".to_string(),
                detail,
            }),
    );
    verification_targets.extend(contract.output_requirements.iter().map(|req| {
        VerificationTarget {
            kind: "output_literal".to_string(),
            detail: req.literal.clone(),
        }
    }));

    ImplementationPlanReport {
        contract,
        output_path: output_path.display().to_string(),
        required_artifacts,
        verification_targets,
        warnings: Vec::new(),
    }
}

pub(crate) fn verify_generated_implementation(
    project_root: &Path,
    spec_path: &Path,
    spec_content: &str,
    output_path: &Path,
) -> Result<StaticBehaviorVerifierReport> {
    let plan = build_implementation_plan(spec_path, spec_content, output_path, project_root);
    let code = fs::read_to_string(output_path).with_context(|| {
        format!(
            "Failed to read generated implementation: {}",
            output_path.display()
        )
    })?;

    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut high_risk_findings = Vec::new();
    let mut evidence = Vec::new();
    let searchable_code = verification_search_text(&code);
    let env_var_read_re = Regex::new(r"\b(?:std\s*::\s*)?env\s*::\s*(?:var|var_os)\b").unwrap();

    for env_var in &plan.contract.env_vars {
        let satisfied =
            searchable_code.contains(env_var) && env_var_read_re.is_match(&searchable_code);
        evidence.push(VerificationEvidence {
            obligation: format!("Read environment variable {}", env_var),
            satisfied,
            evidence: if satisfied {
                vec![format!("Found env-var read for {}", env_var)]
            } else {
                Vec::new()
            },
        });
        if !satisfied {
            errors.push(format!(
                "Implementation does not appear to read required environment variable '{}'",
                env_var
            ));
        }
    }

    for collaborator in &plan.contract.collaborators {
        let satisfied = searchable_code.contains(collaborator);
        evidence.push(VerificationEvidence {
            obligation: format!("Reference collaborator {}", collaborator),
            satisfied,
            evidence: if satisfied {
                vec![format!("Found collaborator identifier '{}'", collaborator)]
            } else {
                Vec::new()
            },
        });
        if is_probably_domain_type(collaborator) && !satisfied {
            warnings.push(format!(
                "Implementation does not reference collaborator '{}' directly",
                collaborator
            ));
        }
    }

    for requirement in &plan.contract.delegation_requirements {
        let satisfied = searchable_code.contains(&requirement.actor)
            && searchable_code.contains(&requirement.target);
        evidence.push(VerificationEvidence {
            obligation: format!("Delegation {} -> {}", requirement.actor, requirement.target),
            satisfied,
            evidence: if satisfied {
                vec![format!(
                    "Found identifiers '{}' and '{}'",
                    requirement.actor, requirement.target
                )]
            } else {
                Vec::new()
            },
        });
        if !satisfied {
            warnings.push(format!(
                "Could not confirm required delegation '{}' -> '{}'",
                requirement.actor, requirement.target
            ));
        }
    }

    for requirement in &plan.contract.output_requirements {
        let satisfied = searchable_code.contains(&requirement.literal);
        evidence.push(VerificationEvidence {
            obligation: format!("Emit required output literal {}", requirement.literal),
            satisfied,
            evidence: if satisfied {
                vec![format!("Found literal '{}'", requirement.literal)]
            } else {
                Vec::new()
            },
        });
        if !satisfied {
            warnings.push(format!(
                "Could not find required output literal '{}' in implementation",
                requirement.literal
            ));
        }
    }

    if !plan.contract.shared_state_requirements.is_empty()
        && code.contains(".clone()")
        && plan
            .contract
            .collaborators
            .iter()
            .any(|name| is_probably_domain_type(name))
    {
        high_risk_findings.push(
                "Implementation uses `.clone()` despite shared-state requirements; verify semantics are actually shared"
                .to_string(),
        );
    }
    for finding in detect_effectively_empty_implementation(&searchable_code) {
        errors.push(finding.render_for_path(output_path));
    }
    if matches!(plan.contract.kind, SpecificationKind::App) {
        for finding in detect_missing_binary_main_entrypoint(&code, &searchable_code) {
            errors.push(finding.render_for_path(output_path));
        }
    }

    let placeholder_patterns = [
        (
            Regex::new(r"\btodo!\s*\(").unwrap(),
            "Contains todo! placeholder",
        ),
        (
            Regex::new(r"\bunimplemented!\s*\(").unwrap(),
            "Contains unimplemented! placeholder",
        ),
        (
            Regex::new(r"//\s*no-op").unwrap(),
            "Contains explicit no-op comment",
        ),
        (
            Regex::new(r"//\s*stub").unwrap(),
            "Contains explicit stub comment",
        ),
    ];
    let behavior_sensitive = !plan.contract.external_behavior_clues.is_empty()
        || !plan.contract.env_vars.is_empty()
        || !plan.contract.output_requirements.is_empty();
    for (pattern, message) in placeholder_patterns {
        if let Some(found) = pattern.find(&code) {
            let finding = CodeFinding::with_offset(message, &code, found.start());
            if behavior_sensitive {
                high_risk_findings.push(
                    CodeFinding::without_location(format!(
                        "{} in behavior-sensitive implementation",
                        finding.message
                    ))
                    .with_location(finding.location)
                    .render_for_path(output_path),
                );
            } else {
                warnings.push(finding.render_for_path(output_path));
            }
        }
    }
    for finding in detect_trivial_obligation_stubs(&code, &plan.contract.role_method_names) {
        if behavior_sensitive {
            high_risk_findings.push(
                CodeFinding::without_location(format!(
                    "{} in behavior-sensitive implementation",
                    finding.message
                ))
                .with_location(finding.location.clone())
                .render_for_path(output_path),
            );
        } else {
            warnings.push(finding.render_for_path(output_path));
        }
    }
    for finding in detect_ignored_immutable_return_values(spec_content, &code) {
        high_risk_findings.push(finding.render_for_path(output_path));
    }
    for finding in detect_private_leaf_module_import_findings(&code) {
        high_risk_findings.push(finding.render_for_path(output_path));
    }
    for finding in detect_external_crate_policy_findings(project_root, &code)? {
        high_risk_findings.push(finding.render_for_path(output_path));
    }
    if matches!(
        plan.contract.kind,
        SpecificationKind::Context | SpecificationKind::Projection
    ) {
        for finding in
            detect_undeclared_collaborator_method_calls(project_root, &plan.contract.title, &code)?
        {
            high_risk_findings.push(finding.render_for_path(output_path));
        }
    }
    // Projections and Data are always immutable; apply the shared structural checks.
    if plan.contract.is_immutable() || matches!(plan.contract.kind, SpecificationKind::Projection) {
        for finding in detect_immutable_kind_findings(&code) {
            high_risk_findings.push(finding.render_for_path(output_path));
        }
    }
    // Projection-specific: must not import Context kinds.
    if matches!(plan.contract.kind, SpecificationKind::Projection) {
        for finding in detect_projection_kind_findings(&code) {
            high_risk_findings.push(finding.render_for_path(output_path));
        }
    }
    // Data-specific: must not import Context or Projection kinds.
    if matches!(plan.contract.kind, SpecificationKind::Data) {
        for finding in detect_data_kind_findings(&code) {
            high_risk_findings.push(finding.render_for_path(output_path));
        }
    }

    Ok(StaticBehaviorVerifierReport {
        contract: plan.contract,
        output_path: output_path.display().to_string(),
        errors,
        warnings,
        high_risk_findings,
        evidence,
    })
}

pub(crate) fn compare_verifier_reports(
    before: StaticBehaviorVerifierReport,
    after: StaticBehaviorVerifierReport,
) -> SemanticRegressionReport {
    let mut issues = Vec::new();
    if after.errors.len() > before.errors.len() {
        issues.push(format!(
            "Verifier errors increased from {} to {}",
            before.errors.len(),
            after.errors.len()
        ));
    }
    if after.high_risk_findings.len() > before.high_risk_findings.len() {
        issues.push(format!(
            "High-risk findings increased from {} to {}",
            before.high_risk_findings.len(),
            after.high_risk_findings.len()
        ));
    }
    for issue in &after.errors {
        if !before.errors.contains(issue) {
            issues.push(format!("New verifier error: {}", issue));
        }
    }
    for issue in &after.high_risk_findings {
        if !before.high_risk_findings.contains(issue) {
            issues.push(format!("New high-risk finding: {}", issue));
        }
    }

    SemanticRegressionReport {
        worsened: !issues.is_empty(),
        issues,
        before,
        after,
    }
}

fn location_from_span(span: Span) -> Option<CodeLocation> {
    let start = span.start();
    if start.line == 0 {
        None
    } else {
        Some(CodeLocation {
            line: start.line,
            column: start.column + 1,
        })
    }
}

fn location_from_offset(code: &str, offset: usize) -> CodeLocation {
    let mut line = 1usize;
    let mut column = 1usize;
    for ch in code[..offset.min(code.len())].chars() {
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    CodeLocation { line, column }
}

fn dedup_code_findings(findings: &mut Vec<CodeFinding>) {
    findings.sort_by(|left, right| {
        let left_key = (
            left.location.as_ref().map(|item| item.line).unwrap_or(0),
            left.location.as_ref().map(|item| item.column).unwrap_or(0),
            left.message.as_str(),
        );
        let right_key = (
            right.location.as_ref().map(|item| item.line).unwrap_or(0),
            right.location.as_ref().map(|item| item.column).unwrap_or(0),
            right.message.as_str(),
        );
        left_key.cmp(&right_key)
    });
    findings.dedup();
}

fn detect_trivial_obligation_stubs(code: &str, role_method_names: &[String]) -> Vec<CodeFinding> {
    if role_method_names.is_empty() {
        return Vec::new();
    }

    let normalized_role_names = role_method_names
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let mut findings = match syn::parse_file(code) {
        Ok(file) => detect_trivial_obligation_stubs_from_ast(&file, &normalized_role_names),
        Err(_) => detect_trivial_obligation_stubs_fallback(code, &normalized_role_names),
    };
    dedup_code_findings(&mut findings);
    findings
}

#[derive(Debug, Clone)]
struct InterfaceMethodSurface {
    draft_relative_path: String,
    allowed_methods: HashSet<String>,
}

fn detect_undeclared_collaborator_method_calls(
    project_root: &Path,
    primary_type_name: &str,
    code: &str,
) -> Result<Vec<CodeFinding>> {
    let interface_index = load_interface_method_surfaces(project_root)?;
    if interface_index.is_empty() {
        return Ok(Vec::new());
    }

    let Ok(file) = syn::parse_file(code) else {
        return Ok(Vec::new());
    };

    let self_field_types = collect_primary_struct_field_types(&file, primary_type_name);
    let local_type_names = collect_local_type_names(&file);
    let mut detector = UndeclaredCollaboratorMethodDetector {
        interface_index: &interface_index,
        self_field_types,
        local_type_names,
        scope_stack: Vec::new(),
        findings: Vec::new(),
    };
    detector.visit_file(&file);
    dedup_code_findings(&mut detector.findings);
    Ok(detector.findings)
}

fn load_interface_method_surfaces(
    project_root: &Path,
) -> Result<HashMap<String, InterfaceMethodSurface>> {
    let interfaces_root = project_root.join(".reen").join("interfaces");
    if !interfaces_root.exists() {
        return Ok(HashMap::new());
    }

    let mut interface_paths = Vec::new();
    collect_interface_ir_paths(&interfaces_root, &mut interface_paths)?;
    let mut surfaces = HashMap::new();

    for path in interface_paths {
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(interface_ir) = serde_json::from_str::<super::contract_store::InterfaceIr>(&content)
        else {
            continue;
        };
        if interface_ir.primary_export_name.trim().is_empty() {
            continue;
        }

        let mut allowed_methods = interface_ir
            .exported_methods
            .iter()
            .chain(interface_ir.role_method_exports.iter())
            .flat_map(|method| {
                [
                    method.export_name.clone(),
                    method.rust_name.clone(),
                    method.semantic_name.clone(),
                ]
            })
            .collect::<HashSet<_>>();

        // Field-only interfaces are still meaningful collaborator surfaces. Treat exported
        // field names as getter-shaped accessors so immutable data artifacts can be consumed
        // through `value.field_name()` helpers without suppressing undeclared method checks.
        allowed_methods.extend(
            interface_ir
                .exported_types
                .iter()
                .filter(|exported_type| {
                    exported_type.export_name == interface_ir.primary_export_name
                        || exported_type.rust_name == interface_ir.primary_export_name
                        || exported_type.semantic_name == interface_ir.primary_export_name
                })
                .flat_map(|exported_type| {
                    exported_type.fields.iter().flat_map(|field| {
                        [
                            field.export_name.clone(),
                            field.rust_name.clone(),
                            field.semantic_name.clone(),
                        ]
                    })
                }),
        );

        surfaces.insert(
            interface_ir.primary_export_name.clone(),
            InterfaceMethodSurface {
                draft_relative_path: interface_ir.draft_relative_path,
                allowed_methods,
            },
        );
    }

    Ok(surfaces)
}

fn collect_interface_ir_paths(root: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(root).with_context(|| format!("Failed to read {}", root.display()))? {
        let entry = entry.with_context(|| format!("Failed to read {}", root.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_interface_ir_paths(&path, out)?;
        } else if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value == "json")
            && path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.ends_with(".interface.json"))
        {
            out.push(path);
        }
    }
    Ok(())
}

fn collect_primary_struct_field_types(
    file: &File,
    primary_type_name: &str,
) -> HashMap<String, String> {
    let name_candidates = rust_type_name_candidates(primary_type_name);
    file.items
        .iter()
        .find_map(|item| {
            let Item::Struct(item_struct) = item else {
                return None;
            };
            if !name_candidates.contains(&item_struct.ident.to_string()) {
                return None;
            }

            let syn::Fields::Named(fields) = &item_struct.fields else {
                return Some(HashMap::new());
            };
            let mapped = fields
                .named
                .iter()
                .filter_map(|field| {
                    let field_name = field.ident.as_ref()?.to_string();
                    let type_name = base_type_name(&field.ty)?;
                    Some((field_name, type_name))
                })
                .collect::<HashMap<_, _>>();
            Some(mapped)
        })
        .unwrap_or_default()
}

fn rust_type_name_candidates(name: &str) -> HashSet<String> {
    let mut candidates = HashSet::new();
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return candidates;
    }

    candidates.insert(trimmed.to_string());

    let mut pascal = String::new();
    for raw in trimmed.split(|c: char| !c.is_ascii_alphanumeric()) {
        if raw.is_empty() {
            continue;
        }

        let has_lower = raw.chars().any(|c| c.is_ascii_lowercase());
        let has_upper = raw.chars().any(|c| c.is_ascii_uppercase());
        let token = if has_lower && has_upper {
            let mut chars = raw.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        } else {
            let lower = raw.to_ascii_lowercase();
            let mut chars = lower.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        };
        pascal.push_str(&token);
    }
    if !pascal.is_empty() {
        candidates.insert(pascal);
    }

    candidates
}

fn collect_local_type_names(file: &File) -> HashSet<String> {
    file.items
        .iter()
        .filter_map(|item| match item {
            Item::Struct(item_struct) => Some(item_struct.ident.to_string()),
            Item::Enum(item_enum) => Some(item_enum.ident.to_string()),
            Item::Trait(item_trait) => Some(item_trait.ident.to_string()),
            Item::Type(item_type) => Some(item_type.ident.to_string()),
            _ => None,
        })
        .collect()
}

fn function_param_type_names(sig: &syn::Signature) -> HashMap<String, String> {
    sig.inputs
        .iter()
        .filter_map(|arg| {
            let FnArg::Typed(pat_type) = arg else {
                return None;
            };
            let Pat::Ident(pat_ident) = pat_type.pat.as_ref() else {
                return None;
            };
            let type_name = base_type_name(&pat_type.ty)?;
            Some((pat_ident.ident.to_string(), type_name))
        })
        .collect()
}

fn base_type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Reference(reference) => base_type_name(&reference.elem),
        Type::Paren(paren) => base_type_name(&paren.elem),
        Type::Group(group) => base_type_name(&group.elem),
        Type::Path(type_path) => {
            let segment = type_path.path.segments.last()?;
            let ident = segment.ident.to_string();
            match ident.as_str() {
                "Box" | "Arc" | "Rc" | "Mutex" | "RwLock" | "RefCell" => {
                    if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                        for arg in &args.args {
                            if let syn::GenericArgument::Type(inner) = arg {
                                if let Some(inner_name) = base_type_name(inner) {
                                    return Some(inner_name);
                                }
                            }
                        }
                    }
                    None
                }
                _ => Some(ident),
            }
        }
        _ => None,
    }
}

fn unwrap_receiver_expr(mut expr: &Expr) -> &Expr {
    loop {
        match expr {
            Expr::Reference(reference) => expr = &reference.expr,
            Expr::Paren(paren) => expr = &paren.expr,
            Expr::Group(group) => expr = &group.expr,
            _ => return expr,
        }
    }
}

struct UndeclaredCollaboratorMethodDetector<'a> {
    interface_index: &'a HashMap<String, InterfaceMethodSurface>,
    self_field_types: HashMap<String, String>,
    local_type_names: HashSet<String>,
    scope_stack: Vec<HashMap<String, String>>,
    findings: Vec<CodeFinding>,
}

impl<'a> UndeclaredCollaboratorMethodDetector<'a> {
    fn push_scope(&mut self, values: HashMap<String, String>) {
        self.scope_stack.push(values);
    }

    fn pop_scope(&mut self) {
        self.scope_stack.pop();
    }

    fn resolve_receiver_binding(&self, expr: &Expr) -> Option<(String, String)> {
        let expr = unwrap_receiver_expr(expr);
        match expr {
            Expr::Field(field) => {
                let Expr::Path(path) = unwrap_receiver_expr(&field.base) else {
                    return None;
                };
                if !path.path.is_ident("self") {
                    return None;
                }
                let Member::Named(member) = &field.member else {
                    return None;
                };
                let field_name = member.to_string();
                let type_name = self.self_field_types.get(&field_name)?.clone();
                Some((field_name, type_name))
            }
            Expr::Path(path) => {
                let ident = path.path.get_ident()?.to_string();
                let type_name = self
                    .scope_stack
                    .iter()
                    .rev()
                    .find_map(|scope| scope.get(&ident))
                    .cloned()?;
                Some((ident, type_name))
            }
            _ => None,
        }
    }

    fn maybe_record_finding(&mut self, receiver: &Expr, method_call: &syn::ExprMethodCall) {
        let Some((binding_name, type_name)) = self.resolve_receiver_binding(receiver) else {
            return;
        };
        if self.local_type_names.contains(&type_name) {
            return;
        }

        let Some(surface) = self.interface_index.get(&type_name) else {
            return;
        };
        let method_name = method_call.method.to_string();
        if surface.allowed_methods.contains(&method_name) {
            return;
        }

        let receiver_label = match unwrap_receiver_expr(receiver) {
            Expr::Field(_) => format!("self.{binding_name}"),
            _ => binding_name,
        };
        self.findings.push(CodeFinding::with_span(
            format!(
                "Direct collaborator call '{}.{}' targets type '{}' but method '{}' is not declared on interface '{}'; keep this behavior in a private context role method or local helper over the collaborator instead of expanding collaborator API",
                receiver_label,
                method_name,
                type_name,
                method_name,
                surface.draft_relative_path
            ),
            method_call.method.span(),
        ));
    }
}

impl<'ast> Visit<'ast> for UndeclaredCollaboratorMethodDetector<'_> {
    fn visit_item_fn(&mut self, function: &'ast syn::ItemFn) {
        self.push_scope(function_param_type_names(&function.sig));
        visit::visit_item_fn(self, function);
        self.pop_scope();
    }

    fn visit_impl_item_fn(&mut self, method: &'ast ImplItemFn) {
        self.push_scope(function_param_type_names(&method.sig));
        visit::visit_impl_item_fn(self, method);
        self.pop_scope();
    }

    fn visit_expr_method_call(&mut self, method_call: &'ast syn::ExprMethodCall) {
        self.maybe_record_finding(&method_call.receiver, method_call);
        visit::visit_expr_method_call(self, method_call);
    }
}

fn verification_search_text(code: &str) -> String {
    code.parse::<proc_macro2::TokenStream>()
        .map(|tokens| tokens.to_string())
        .unwrap_or_else(|_| strip_comments_fallback(code))
}

fn strip_comments_fallback(code: &str) -> String {
    let without_block_comments = Regex::new(r"(?s)/\*.*?\*/").unwrap().replace_all(code, " ");
    Regex::new(r"(?m)//.*$")
        .unwrap()
        .replace_all(&without_block_comments, " ")
        .into_owned()
}

fn detect_effectively_empty_implementation(searchable_code: &str) -> Vec<CodeFinding> {
    if searchable_code.trim().is_empty() {
        vec![CodeFinding::without_location(
            "Implementation is effectively empty after removing comments and whitespace",
        )]
    } else {
        Vec::new()
    }
}

fn detect_missing_binary_main_entrypoint(code: &str, searchable_code: &str) -> Vec<CodeFinding> {
    let has_main = match syn::parse_file(code) {
        Ok(file) => file
            .items
            .iter()
            .any(|item| matches!(item, Item::Fn(function) if function.sig.ident == "main")),
        Err(_) => Regex::new(r"\bfn\s+main\b")
            .unwrap()
            .is_match(searchable_code),
    };

    if has_main {
        Vec::new()
    } else {
        vec![CodeFinding::without_location(
            "Application implementation does not define a top-level `main` function",
        )]
    }
}

fn detect_trivial_obligation_stubs_from_ast(
    file: &File,
    normalized_role_names: &[String],
) -> Vec<CodeFinding> {
    let mut findings = Vec::new();
    for item in &file.items {
        match item {
            Item::Fn(function) => {
                if let Some(message) = trivial_named_block_message(
                    &function.sig.ident.to_string(),
                    &function.block,
                    normalized_role_names,
                ) {
                    findings.push(CodeFinding::with_span(message, function.sig.ident.span()));
                }
            }
            Item::Impl(item_impl) => {
                if item_impl.trait_.is_some() {
                    continue;
                }
                for impl_item in &item_impl.items {
                    let ImplItem::Fn(method) = impl_item else {
                        continue;
                    };
                    if let Some(message) = trivial_named_block_message(
                        &method.sig.ident.to_string(),
                        &method.block,
                        normalized_role_names,
                    ) {
                        findings.push(CodeFinding::with_span(message, method.sig.ident.span()));
                    }
                }
            }
            _ => {}
        }
    }
    findings
}

fn detect_trivial_obligation_stubs_fallback(
    code: &str,
    normalized_role_names: &[String],
) -> Vec<CodeFinding> {
    let fn_re = Regex::new(r"(?s)fn\s+([A-Za-z0-9_]+)[^{]*\{([^{}]*)\}").unwrap();
    let vec_new_re = Regex::new(r"^Vec(?:::<[^>]+>)?::new\s*\(\)\s*;?$").unwrap();
    let string_new_re = Regex::new(r"^String::new\s*\(\)\s*;?$").unwrap();
    let none_re = Regex::new(r"^(?:return\s+)?None\s*;?$").unwrap();
    let mut findings = Vec::new();

    for captures in fn_re.captures_iter(code) {
        let Some(name_match) = captures.get(1) else {
            continue;
        };
        let Some(body_match) = captures.get(2) else {
            continue;
        };

        let fn_name = name_match.as_str();
        let fn_name_lower = fn_name.to_ascii_lowercase();
        let is_obligation_method = normalized_role_names.iter().any(|role_name| {
            fn_name_lower == *role_name
                || fn_name_lower.ends_with(&format!("_{}", role_name))
                || fn_name_lower.contains(role_name)
        });
        if !is_obligation_method {
            continue;
        }

        let normalized_body = normalize_stub_candidate_body(body_match.as_str());
        if normalized_body.is_empty() {
            continue;
        }

        let finding = if vec_new_re.is_match(&normalized_body) {
            Some(format!(
                "Role method '{}' has a trivial body returning Vec::new()",
                fn_name
            ))
        } else if string_new_re.is_match(&normalized_body) {
            Some(format!(
                "Role method '{}' has a trivial body returning String::new()",
                fn_name
            ))
        } else if none_re.is_match(&normalized_body) {
            Some(format!(
                "Role method '{}' has a trivial body returning None",
                fn_name
            ))
        } else {
            None
        };

        if let Some(message) = finding {
            findings.push(CodeFinding::with_offset(message, code, name_match.start()));
        }
    }

    findings
}

fn trivial_named_block_message(
    function_name: &str,
    block: &syn::Block,
    normalized_role_names: &[String],
) -> Option<String> {
    let fn_name_lower = function_name.to_ascii_lowercase();
    let is_obligation_method = normalized_role_names.iter().any(|role_name| {
        fn_name_lower == *role_name
            || fn_name_lower.ends_with(&format!("_{}", role_name))
            || fn_name_lower.contains(role_name)
    });
    if !is_obligation_method {
        return None;
    }

    let mut relevant = Vec::new();
    for stmt in &block.stmts {
        if is_logging_statement(stmt) {
            continue;
        }
        relevant.push(stmt);
    }

    if relevant.len() != 1 {
        return None;
    }

    let expr = stmt_expr(relevant[0])?;
    let expr = unwrap_return_expr(expr);
    if expr_is_vec_new(expr) {
        Some(format!(
            "Role method '{}' has a trivial body returning Vec::new()",
            function_name
        ))
    } else if expr_is_string_new(expr) {
        Some(format!(
            "Role method '{}' has a trivial body returning String::new()",
            function_name
        ))
    } else if expr_is_none(expr) {
        Some(format!(
            "Role method '{}' has a trivial body returning None",
            function_name
        ))
    } else {
        None
    }
}

fn is_logging_statement(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Macro(item) => macro_path_is_logging(&item.mac.path),
        Stmt::Expr(Expr::Macro(expr), _) => macro_path_is_logging(&expr.mac.path),
        _ => false,
    }
}

fn stmt_expr(stmt: &Stmt) -> Option<&Expr> {
    match stmt {
        Stmt::Expr(expr, _) => Some(expr),
        _ => None,
    }
}

fn unwrap_return_expr(expr: &Expr) -> &Expr {
    match expr {
        Expr::Return(item) => item.expr.as_deref().unwrap_or(expr),
        _ => expr,
    }
}

fn macro_path_is_logging(path: &SynPath) -> bool {
    let rendered = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>();
    rendered.as_slice() == ["tracing", "debug"]
        || rendered.as_slice() == ["tracing", "info"]
        || rendered.as_slice() == ["tracing", "warn"]
        || rendered.as_slice() == ["tracing", "error"]
        || rendered.as_slice() == ["log", "debug"]
        || rendered.as_slice() == ["log", "info"]
        || rendered.as_slice() == ["log", "warn"]
        || rendered.as_slice() == ["log", "error"]
        || rendered.as_slice() == ["println"]
        || rendered.as_slice() == ["eprintln"]
}

fn expr_is_none(expr: &Expr) -> bool {
    matches!(expr, Expr::Path(item) if item.path.is_ident("None"))
}

fn expr_is_vec_new(expr: &Expr) -> bool {
    expr_call_matches(expr, &["Vec", "new"])
}

fn expr_is_string_new(expr: &Expr) -> bool {
    expr_call_matches(expr, &["String", "new"])
}

fn expr_call_matches(expr: &Expr, expected: &[&str]) -> bool {
    let Expr::Call(item) = expr else {
        return false;
    };
    let Expr::Path(path) = item.func.as_ref() else {
        return false;
    };
    path_ends_with_segments(&path.path, expected)
}

fn path_ends_with_segments(path: &SynPath, expected: &[&str]) -> bool {
    if path.segments.len() < expected.len() {
        return false;
    }
    path.segments
        .iter()
        .rev()
        .zip(expected.iter().rev())
        .all(|(actual, expected)| actual.ident == *expected)
}

fn detect_ignored_immutable_return_values(spec_content: &str, code: &str) -> Vec<CodeFinding> {
    if !spec_declares_immutable_value_updates(spec_content) {
        return Vec::new();
    }

    let methods = extract_immutable_transform_method_names(spec_content);
    if methods.is_empty() {
        return Vec::new();
    }

    let mut findings = Vec::new();
    for (index, line) in code.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("//")
            || trimmed.starts_with("let ")
            || trimmed.starts_with("return ")
            || trimmed.starts_with("if ")
            || trimmed.starts_with("match ")
        {
            continue;
        }

        for method in &methods {
            let pattern = format!(".{}(", method);
            if trimmed.contains(&pattern) && trimmed.ends_with(';') {
                findings.push(CodeFinding {
                    message: format!(
                        "Immutable transform method '{}' appears to have its return value ignored",
                        method
                    ),
                    location: Some(CodeLocation {
                        line: index + 1,
                        column: line.find(&pattern).map(|value| value + 1).unwrap_or(1),
                    }),
                });
            }
        }
    }

    dedup_code_findings(&mut findings);
    findings
}

fn detect_private_leaf_module_import_findings(code: &str) -> Vec<CodeFinding> {
    let mut findings = Vec::new();
    let leaf_import_re = Regex::new(
        r"(?m)^\s*use\s+(?:crate|[A-Za-z_][A-Za-z0-9_]*)::(?:data|contexts)::[a-z_][A-Za-z0-9_]*::",
    )
    .unwrap();

    for (index, line) in code.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(found) = leaf_import_re.find(trimmed) {
            findings.push(CodeFinding {
                message: format!(
                    "Import uses a private leaf-module path; rewrite it to a public re-export path (`{}`)",
                    trimmed
                ),
                location: Some(CodeLocation {
                    line: index + 1,
                    column: found.start() + 1,
                }),
            });
        }
    }

    dedup_code_findings(&mut findings);
    findings
}

fn detect_external_crate_policy_findings(
    project_root: &Path,
    code: &str,
) -> Result<Vec<CodeFinding>> {
    let drafts_root = project_root.join("drafts");
    let Some(allowed) = allowed_external_crate_roots(&drafts_root)? else {
        return Ok(Vec::new());
    };
    let provider_domains = registry_provider_domains_by_crate(&drafts_root)?.unwrap_or_default();
    let planned_by_domain = provider_domains
        .iter()
        .filter(|(crate_name, _)| allowed.contains(crate_name.as_str()))
        .map(|(crate_name, domain)| (domain.clone(), crate_name.clone()))
        .collect::<HashMap<_, _>>();
    let local_crate_roots = detect_local_crate_roots(project_root)?;
    let external_roots = extract_external_crate_roots(code);
    let mut findings = Vec::new();

    for root in external_roots {
        if allowed.contains(&root)
            || local_crate_roots.contains(&root)
            || baseline_allowed_crate_roots().contains(root.as_str())
        {
            continue;
        }
        let location = find_first_root_path_location(code, &root);
        let known_domain = provider_domains
            .get(&root)
            .cloned()
            .or_else(|| known_capability_domain_for_crate(&root).map(str::to_string));
        if let Some(domain) = known_domain.as_deref() {
            if let Some(planned) = planned_by_domain.get(domain) {
                findings.push(CodeFinding {
                    message: format!(
                        "Code imports external crate '{}' but capability domain '{}' is planned to use '{}'",
                        root, domain, planned
                    ),
                    location: location.clone(),
                });
                continue;
            }
        }
        findings.push(CodeFinding {
            message: format!(
                "Code imports external crate '{}' which is not declared in the resolved dependency plan",
                root
            ),
            location,
        });
    }

    dedup_code_findings(&mut findings);
    Ok(findings)
}

fn detect_local_crate_roots(project_root: &Path) -> Result<HashSet<String>> {
    let cargo_toml = project_root.join("Cargo.toml");
    if !cargo_toml.exists() {
        return Ok(HashSet::new());
    }

    let content = fs::read_to_string(&cargo_toml)
        .with_context(|| format!("Failed to read {}", cargo_toml.display()))?;
    let package_name = extract_cargo_manifest_name(&content, "package");
    let lib_name = extract_cargo_manifest_name(&content, "lib");

    let mut roots = HashSet::new();
    if let Some(name) = package_name {
        roots.insert(normalize_cargo_crate_root(&name));
    }
    if let Some(name) = lib_name {
        roots.insert(normalize_cargo_crate_root(&name));
    }

    Ok(roots)
}

fn extract_cargo_manifest_name(content: &str, section_name: &str) -> Option<String> {
    let mut in_target_section = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_target_section = trimmed == format!("[{}]", section_name);
            continue;
        }

        if !in_target_section {
            continue;
        }

        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        if key.trim() != "name" {
            continue;
        }

        let raw = value.trim().trim_matches('"');
        if raw.is_empty() {
            return None;
        }
        return Some(raw.to_string());
    }

    None
}

fn normalize_cargo_crate_root(raw: &str) -> String {
    let mut out = String::new();
    let mut last_was_separator = false;

    for ch in raw.chars() {
        let lowered = ch.to_ascii_lowercase();
        if lowered.is_ascii_alphanumeric() {
            if out.is_empty() && lowered.is_ascii_digit() {
                out.push('_');
            }
            out.push(lowered);
            last_was_separator = false;
        } else if !out.is_empty() && !last_was_separator {
            out.push('_');
            last_was_separator = true;
        }
    }

    while out.ends_with('_') {
        out.pop();
    }

    if out.is_empty() {
        "generated_project".to_string()
    } else {
        out
    }
}

fn extract_external_crate_roots(code: &str) -> BTreeSet<String> {
    match syn::parse_file(code) {
        Ok(file) => extract_external_crate_roots_from_file(&file),
        Err(_) => extract_external_crate_roots_fallback(code),
    }
}

fn extract_external_crate_roots_from_file(file: &File) -> BTreeSet<String> {
    let mut collector = ExternalCrateRootCollector::new(collect_local_root_bindings(file));

    for item in &file.items {
        match item {
            Item::Use(item_use) => {
                collector.record_use_roots(item_use);
                collector.record_use_bindings(item_use);
            }
            Item::ExternCrate(item) => collector.record_extern_crate(item),
            _ => {}
        }
    }

    collector.visit_file(file);
    collector.roots
}

fn extract_external_crate_roots_fallback(code: &str) -> BTreeSet<String> {
    let mut roots = BTreeSet::new();
    let use_root_re =
        Regex::new(r"(?m)^\s*use\s+([a-z_][a-z0-9_]*)\b(?:\s*::|\s*;|\s+as\b)").unwrap();
    let extern_crate_re = Regex::new(r"(?m)^\s*extern\s+crate\s+([a-z_][a-z0-9_]*)\b").unwrap();

    for capture in use_root_re.captures_iter(code) {
        if let Some(root) = capture.get(1).map(|m| m.as_str()) {
            if is_possible_external_crate_root(root) {
                roots.insert(root.to_string());
            }
        }
    }

    for capture in extern_crate_re.captures_iter(code) {
        if let Some(root) = capture.get(1).map(|m| m.as_str()) {
            if is_possible_external_crate_root(root) {
                roots.insert(root.to_string());
            }
        }
    }

    roots
}

fn collect_local_root_bindings(file: &File) -> HashSet<String> {
    file.items
        .iter()
        .filter_map(local_item_binding)
        .collect::<HashSet<_>>()
}

fn local_item_binding(item: &Item) -> Option<String> {
    match item {
        Item::Const(item) => Some(item.ident.to_string()),
        Item::Enum(item) => Some(item.ident.to_string()),
        Item::Fn(item) => Some(item.sig.ident.to_string()),
        Item::Macro(item) => item.ident.as_ref().map(|ident| ident.to_string()),
        Item::Mod(item) => Some(item.ident.to_string()),
        Item::Static(item) => Some(item.ident.to_string()),
        Item::Struct(item) => Some(item.ident.to_string()),
        Item::Trait(item) => Some(item.ident.to_string()),
        Item::TraitAlias(item) => Some(item.ident.to_string()),
        Item::Type(item) => Some(item.ident.to_string()),
        Item::Union(item) => Some(item.ident.to_string()),
        _ => None,
    }
}

#[derive(Default)]
struct ExternalCrateRootCollector {
    roots: BTreeSet<String>,
    local_bindings: HashSet<String>,
    imported_bindings: HashSet<String>,
}

impl ExternalCrateRootCollector {
    fn new(local_bindings: HashSet<String>) -> Self {
        Self {
            roots: BTreeSet::new(),
            local_bindings,
            imported_bindings: HashSet::new(),
        }
    }

    fn record_extern_crate(&mut self, item: &ItemExternCrate) {
        self.record_use_root_candidate(&item.ident.to_string());
        let binding = item
            .rename
            .as_ref()
            .map(|(_, ident)| ident)
            .unwrap_or(&item.ident)
            .to_string();
        self.imported_bindings.insert(binding);
    }

    fn record_use_roots(&mut self, item: &ItemUse) {
        self.record_use_tree_roots(&item.tree, None);
    }

    fn record_use_bindings(&mut self, item: &ItemUse) {
        self.record_use_tree_bindings(&item.tree, None);
    }

    fn record_use_tree_roots(&mut self, tree: &UseTree, top_root: Option<&str>) {
        match tree {
            UseTree::Path(path) => {
                let ident = path.ident.to_string();
                let next_root = top_root.unwrap_or(&ident);
                if top_root.is_none() {
                    self.record_use_root_candidate(next_root);
                }
                self.record_use_tree_roots(&path.tree, Some(next_root));
            }
            UseTree::Name(name) => {
                if let Some(root) = top_root {
                    self.record_use_root_candidate(root);
                } else {
                    let root = name.ident.to_string();
                    self.record_use_root_candidate(&root);
                }
            }
            UseTree::Rename(rename) => {
                if let Some(root) = top_root {
                    self.record_use_root_candidate(root);
                } else {
                    let root = rename.ident.to_string();
                    self.record_use_root_candidate(&root);
                }
            }
            UseTree::Glob(_) => {
                if let Some(root) = top_root {
                    self.record_use_root_candidate(root);
                }
            }
            UseTree::Group(group) => {
                for item in &group.items {
                    self.record_use_tree_roots(item, top_root);
                }
            }
        }
    }

    fn record_use_tree_bindings(&mut self, tree: &UseTree, current_leaf: Option<&str>) {
        match tree {
            UseTree::Path(path) => {
                let ident = path.ident.to_string();
                self.record_use_tree_bindings(&path.tree, Some(&ident));
            }
            UseTree::Name(name) => {
                if name.ident == "self" {
                    if let Some(binding) = current_leaf {
                        self.imported_bindings.insert(binding.to_string());
                    }
                } else {
                    self.imported_bindings.insert(name.ident.to_string());
                }
            }
            UseTree::Rename(rename) => {
                self.imported_bindings.insert(rename.rename.to_string());
            }
            UseTree::Glob(_) => {}
            UseTree::Group(group) => {
                for item in &group.items {
                    self.record_use_tree_bindings(item, current_leaf);
                }
            }
        }
    }

    fn record_use_root_candidate(&mut self, root: &str) {
        if is_possible_external_crate_root(root) && !self.local_bindings.contains(root) {
            self.roots.insert(root.to_string());
        }
    }

    fn record_path_root(&mut self, path: &SynPath) {
        if path.segments.len() < 2 {
            return;
        }

        let Some(root_segment) = path.segments.first() else {
            return;
        };
        let root = root_segment.ident.to_string();

        if !is_possible_external_crate_root(&root) {
            return;
        }
        if self.local_bindings.contains(&root) || self.imported_bindings.contains(&root) {
            return;
        }

        self.roots.insert(root);
    }
}

impl<'ast> Visit<'ast> for ExternalCrateRootCollector {
    fn visit_item_use(&mut self, _node: &'ast ItemUse) {}

    fn visit_item_extern_crate(&mut self, _node: &'ast ItemExternCrate) {}

    fn visit_path(&mut self, path: &'ast SynPath) {
        self.record_path_root(path);
        visit::visit_path(self, path);
    }
}

fn is_possible_external_crate_root(root: &str) -> bool {
    let first = root.chars().next();
    first.map(|ch| ch.is_ascii_lowercase()).unwrap_or(false)
        && root
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
        && !baseline_allowed_crate_roots().contains(root)
        && !primitive_path_roots().contains(root)
}

fn primitive_path_roots() -> HashSet<&'static str> {
    HashSet::from([
        "bool", "char", "f32", "f64", "i8", "i16", "i32", "i64", "i128", "isize", "str", "u8",
        "u16", "u32", "u64", "u128", "usize",
    ])
}

fn baseline_allowed_crate_roots() -> HashSet<&'static str> {
    HashSet::from([
        "crate",
        "self",
        "super",
        "std",
        "core",
        "alloc",
        "data",
        "contexts",
        "projections",
        "tracing",
    ])
}

fn known_capability_domain_for_crate(crate_name: &str) -> Option<&'static str> {
    match crate_name {
        "crossterm" | "termion" => Some("terminal"),
        "serialport" => Some("serial"),
        _ => None,
    }
}

fn find_first_root_path_location(code: &str, root: &str) -> Option<CodeLocation> {
    let pattern = format!(
        r"(?m)\b{}\b(?:(?:\s*::)|(?:\s*;)|(?:\s+as\b))",
        regex::escape(root)
    );
    let regex = Regex::new(&pattern).ok()?;
    regex
        .find(code)
        .map(|matched| location_from_offset(code, matched.start()))
}

fn method_receiver_mut_span(method: &ImplItemFn) -> Option<Span> {
    method.sig.inputs.iter().find_map(|arg| match arg {
        FnArg::Receiver(receiver) if receiver.mutability.is_some() => Some(receiver.span()),
        _ => None,
    })
}

fn is_public_visibility(visibility: &Visibility) -> bool {
    !matches!(visibility, Visibility::Inherited)
}

fn detect_immutable_kind_findings(code: &str) -> Vec<CodeFinding> {
    let mut findings = Vec::new();

    if let Ok(file) = syn::parse_file(code) {
        for item in &file.items {
            let Item::Impl(item_impl) = item else {
                continue;
            };
            for impl_item in &item_impl.items {
                let ImplItem::Fn(method) = impl_item else {
                    continue;
                };
                if is_public_visibility(&method.vis) {
                    if let Some(span) = method_receiver_mut_span(method) {
                        findings.push(CodeFinding::with_span(
                            "Immutable kind (Data or Projection) exposes a public mutable receiver (`&mut self`/`mut self`)",
                            span,
                        ));
                    }
                }
            }
        }
    } else {
        let public_mut_re =
            Regex::new(r"\bpub(?:\([^)]*\))?\s+fn\s+[A-Za-z0-9_]+\s*\([^)]*(?:&mut self|mut self)")
                .unwrap();
        if let Some(found) = public_mut_re.find(code) {
            findings.push(CodeFinding::with_offset(
                "Immutable kind (Data or Projection) exposes a public mutable receiver (`&mut self`/`mut self`)",
                code,
                found.start(),
            ));
        }
    }

    let mutability_patterns = [
        (r"\bRefCell\s*<", "RefCell"),
        (r"\bCell\s*<", "Cell"),
        (r"\bMutex\s*<", "Mutex"),
        (r"\bRwLock\s*<", "RwLock"),
        (r"\bAtomic[A-Za-z0-9_]+\b", "atomic state"),
    ];
    for (pattern, label) in mutability_patterns {
        let re = Regex::new(pattern).unwrap();
        if let Some(found) = re.find(code) {
            findings.push(CodeFinding::with_offset(
                format!(
                    "Immutable kind (Data or Projection) uses interior mutability/storage pattern `{}`",
                    label
                ),
                code,
                found.start(),
            ));
        }
    }

    let spawn_patterns = [
        (r"\bthread::spawn\s*\(", "thread::spawn"),
        (r"\bstd::thread::spawn\s*\(", "std::thread::spawn"),
        (r"\btokio::spawn\s*\(", "tokio::spawn"),
        (r"\bspawn_blocking\s*\(", "spawn_blocking"),
        (r"\basync_std::task::spawn\s*\(", "async_std::task::spawn"),
    ];
    for (pattern, label) in spawn_patterns {
        let re = Regex::new(pattern).unwrap();
        if let Some(found) = re.find(code) {
            findings.push(CodeFinding::with_offset(
                format!(
                    "Immutable kind (Data or Projection) starts background lifecycle work via `{}`",
                    label
                ),
                code,
                found.start(),
            ));
        }
    }

    dedup_code_findings(&mut findings);
    findings
}

/// Additional lint checks specific to Projection kinds.
/// Projections are CQRS read models: they must never depend on Context kinds.
fn use_tree_references_module(tree: &UseTree, target_module: &str) -> bool {
    fn walk(tree: &UseTree, prefix: &mut Vec<String>, target_module: &str) -> bool {
        match tree {
            UseTree::Path(path) => {
                prefix.push(path.ident.to_string());
                let found = walk(&path.tree, prefix, target_module);
                prefix.pop();
                found
            }
            UseTree::Name(_) | UseTree::Rename(_) | UseTree::Glob(_) => {
                prefix.first().is_some_and(|item| item == target_module)
                    || prefix.get(1).is_some_and(|item| item == target_module)
            }
            UseTree::Group(group) => group
                .items
                .iter()
                .any(|item| walk(item, prefix, target_module)),
        }
    }

    walk(tree, &mut Vec::new(), target_module)
}

fn detect_projection_kind_findings(code: &str) -> Vec<CodeFinding> {
    let mut findings = Vec::new();

    if let Ok(file) = syn::parse_file(code) {
        for item in &file.items {
            match item {
                Item::Use(item_use) if use_tree_references_module(&item_use.tree, "contexts") => {
                    findings.push(CodeFinding::with_span(
                        "Projection imports from `contexts` module; Projections must not depend on Context kinds",
                        item_use.span(),
                    ));
                }
                Item::Impl(item_impl) => {
                    for impl_item in &item_impl.items {
                        let ImplItem::Fn(method) = impl_item else {
                            continue;
                        };
                        if let Some(span) = method_receiver_mut_span(method) {
                            findings.push(CodeFinding::with_span(
                                "Projection contains a `&mut self`/`mut self` method; all Projection methods must be immutable",
                                span,
                            ));
                        }
                    }
                }
                _ => {}
            }
        }
    } else {
        let context_import_re = Regex::new(r"\buse\s+[A-Za-z0-9_]+::contexts::").unwrap();
        if let Some(found) = context_import_re.find(code) {
            findings.push(CodeFinding::with_offset(
                "Projection imports from `contexts` module; Projections must not depend on Context kinds",
                code,
                found.start(),
            ));
        }

        let private_mut_re =
            Regex::new(r"\bfn\s+[A-Za-z0-9_]+\s*\([^)]*(?:&mut self|mut self)").unwrap();
        if let Some(found) = private_mut_re.find(code) {
            findings.push(CodeFinding::with_offset(
                "Projection contains a `&mut self`/`mut self` method; all Projection methods must be immutable",
                code,
                found.start(),
            ));
        }
    }

    dedup_code_findings(&mut findings);
    findings
}

/// Additional lint checks specific to Data kinds.
/// Data types must not reference Context or Projection kinds.
fn detect_data_kind_findings(code: &str) -> Vec<CodeFinding> {
    let mut findings = Vec::new();

    if let Ok(file) = syn::parse_file(code) {
        for item in &file.items {
            let Item::Use(item_use) = item else {
                continue;
            };
            if use_tree_references_module(&item_use.tree, "contexts") {
                findings.push(CodeFinding::with_span(
                    "Data type imports from `contexts` module; Data kinds must only depend on other Data kinds and primitives",
                    item_use.span(),
                ));
            }
            if use_tree_references_module(&item_use.tree, "projections") {
                findings.push(CodeFinding::with_span(
                    "Data type imports from `projections` module; Data kinds must only depend on other Data kinds and primitives",
                    item_use.span(),
                ));
            }
        }
    } else {
        let context_import_re = Regex::new(r"\buse\s+[A-Za-z0-9_]+::contexts::").unwrap();
        if let Some(found) = context_import_re.find(code) {
            findings.push(CodeFinding::with_offset(
                "Data type imports from `contexts` module; Data kinds must only depend on other Data kinds and primitives",
                code,
                found.start(),
            ));
        }
        let projection_import_re = Regex::new(r"\buse\s+[A-Za-z0-9_]+::projections::").unwrap();
        if let Some(found) = projection_import_re.find(code) {
            findings.push(CodeFinding::with_offset(
                "Data type imports from `projections` module; Data kinds must only depend on other Data kinds and primitives",
                code,
                found.start(),
            ));
        }
    }

    dedup_code_findings(&mut findings);
    findings
}

fn spec_declares_immutable_value_updates(spec_content: &str) -> bool {
    let sections = parse_markdown_sections(spec_content);
    if let Some(section) = find_section(&sections, "Mutability") {
        if section.body.to_ascii_lowercase().contains("immutable") {
            return true;
        }
    }
    spec_content.to_ascii_lowercase().contains("returns a new")
}

fn extract_immutable_transform_method_names(spec_content: &str) -> Vec<String> {
    let sections = parse_markdown_sections(spec_content);
    let Some(section) = find_section(&sections, "Functionalities") else {
        return Vec::new();
    };

    let mut methods = Vec::new();
    let mut current_method: Option<String> = None;
    let mut current_body = Vec::new();

    let flush = |methods: &mut Vec<String>,
                 current_method: &mut Option<String>,
                 current_body: &mut Vec<String>| {
        let Some(name) = current_method.take() else {
            current_body.clear();
            return;
        };
        let body = current_body.join("\n").to_ascii_lowercase();
        if body.contains("returns a new") {
            methods.push(normalize_symbol_name(&name));
        }
        current_body.clear();
    };

    for line in section.body.lines() {
        let trimmed = line.trim();
        let next_name = trimmed
            .strip_prefix("### ")
            .map(normalize_symbol_name)
            .filter(|value| !value.is_empty())
            .or_else(|| {
                trimmed.strip_prefix("- **").and_then(|rest| {
                    rest.find("**")
                        .map(|end| normalize_symbol_name(&rest[..end]))
                })
            });

        if let Some(name) = next_name {
            flush(&mut methods, &mut current_method, &mut current_body);
            current_method = Some(name);
        } else if current_method.is_some() && !trimmed.is_empty() {
            current_body.push(trimmed.to_string());
        }
    }

    flush(&mut methods, &mut current_method, &mut current_body);
    methods.sort();
    methods.dedup();
    methods
}

fn normalize_stub_candidate_body(body: &str) -> String {
    body.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with("//"))
        .filter(|line| !line.starts_with("tracing::"))
        .filter(|line| !line.starts_with("log::"))
        .filter(|line| !line.starts_with("println!"))
        .filter(|line| !line.starts_with("eprintln!"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn determine_spec_path_for_output(
    output_path: &Path,
    specifications_root: &Path,
) -> Option<PathBuf> {
    let normalized = output_path.to_string_lossy().replace('\\', "/");
    if normalized.ends_with("/src/main.rs") || normalized == "src/main.rs" {
        let app = specifications_root.join("app.md");
        if app.exists() {
            return Some(app);
        }
    }
    if normalized.ends_with("/src/lib.rs") || normalized == "src/lib.rs" {
        return None;
    }

    let relative = if let Some(idx) = normalized.find("/src/") {
        &normalized[idx + 5..]
    } else if let Some(rest) = normalized.strip_prefix("src/") {
        rest
    } else {
        return None;
    };
    let candidate = specifications_root.join(relative).with_extension("md");
    if candidate.exists() {
        Some(candidate)
    } else {
        None
    }
}

pub(crate) fn report_dir(project_root: &Path) -> PathBuf {
    project_root.join(".reen").join("pipeline_quality")
}

pub(crate) fn write_json_report<T: Serialize>(
    project_root: &Path,
    stage: &str,
    artifact_path: &Path,
    file_name: &str,
    value: &T,
) -> Result<PathBuf> {
    let rel = artifact_path.to_string_lossy().replace('\\', "/");
    let safe_rel = rel.trim_start_matches('/').replace('/', "__");
    let dir = report_dir(project_root).join(stage).join(safe_rel);
    fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create report directory {}", dir.display()))?;
    let report_path = dir.join(file_name);
    let payload = serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string());
    fs::write(&report_path, payload)
        .with_context(|| format!("Failed to write report {}", report_path.display()))?;
    Ok(report_path)
}

pub(crate) fn contract_to_context_value(contract: &BehaviorContract) -> serde_json::Value {
    json!(contract)
}

/// Section titles that are documentation/analysis artifacts rather than behavioral
/// specification. These sections must not be fed to heuristic extraction functions
/// because their prose can match extraction patterns (e.g. "must", "uses ") and
/// produce false delegation requirements or output obligations.
///
/// "Role Methods" is included because role method descriptions (e.g. "Chooses a food
/// placement using the current board and the current snake.") contain terms that look
/// like delegation actors/targets but are private implementation helpers, not external
/// call obligations. Stripping this section prevents role method names (e.g. `drop`,
/// `symbol_at`) from being misread as required call-edge participants.
const METADATA_SECTION_TITLES: &[&str] = &[
    "Blocking Ambiguities",
    "Implementation Choices Left Open",
    "Inferred Types or Structures",
    "Inferred Types",
    "Decision Sources",
    "Role Methods",
];

/// Reconstruct spec content with metadata-only sections removed so that heuristic
/// extractors only see behavioral content.
fn behavioral_content(spec_content: &str) -> String {
    let mut result = String::with_capacity(spec_content.len());
    let mut skip = false;
    for line in spec_content.lines() {
        let trimmed = line.trim();
        if let Some(title) = trimmed.strip_prefix("## ") {
            let title = title.trim();
            skip = METADATA_SECTION_TITLES
                .iter()
                .any(|t| t.eq_ignore_ascii_case(title));
        }
        if !skip {
            result.push_str(line);
            result.push('\n');
        }
    }
    result
}

fn extract_behavior_contract(spec_path: &Path, spec_content: &str) -> BehaviorContract {
    let title = spec_content
        .lines()
        .find_map(|line| line.trim().strip_prefix("# ").map(str::trim))
        .unwrap_or("")
        .to_string();
    let sections = parse_markdown_sections(spec_content);
    let kind = infer_kind(spec_path, spec_content, &sections);
    let collaborators = extract_collaborators(kind, spec_content, &sections);
    let env_vars = extract_env_vars(spec_content);
    // Strip metadata sections before running heuristic extractors so that prose in
    // "Blocking Ambiguities" / "Implementation Choices Left Open" / etc. cannot
    // accidentally produce false delegation requirements or output obligations.
    let behavioral = behavioral_content(spec_content);
    let delegation_requirements =
        extract_delegation_requirements(kind, &behavioral, &collaborators);
    let output_requirements = extract_output_requirements(&behavioral);
    let shared_state_requirements = extract_shared_state_requirements(&behavioral);
    let role_method_names = extract_role_method_names(&sections);
    let external_behavior_clues = extract_external_behavior_clues(spec_content);

    BehaviorContract {
        title,
        kind,
        source_path: spec_path.display().to_string(),
        collaborators,
        env_vars,
        delegation_requirements,
        output_requirements,
        shared_state_requirements,
        role_method_names,
        external_behavior_clues,
    }
}

fn is_markdown_thematic_break(line: &str) -> bool {
    let compact = line
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    if compact.len() < 3 {
        return false;
    }
    let mut chars = compact.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    matches!(first, '-' | '*' | '_') && chars.all(|ch| ch == first)
}

fn infer_kind(spec_path: &Path, spec_content: &str, sections: &[Section]) -> SpecificationKind {
    let path = spec_path.to_string_lossy().to_ascii_lowercase();
    if path.ends_with("/app.md") {
        return SpecificationKind::App;
    }
    // Projection specs live under specifications/projections/
    if path.contains("/projections/") {
        return SpecificationKind::Projection;
    }

    if has_any_section(
        sections,
        &["Purpose", "Role Players", "Role Methods", "Props"],
    ) {
        return SpecificationKind::Context;
    }
    if has_any_section(sections, &["Description", "Fields", "Variants"]) {
        return SpecificationKind::Data;
    }

    let title = spec_content
        .lines()
        .find_map(|line| line.trim().strip_prefix("# ").map(str::trim))
        .unwrap_or("")
        .to_ascii_lowercase();
    if title == "app"
        || title == "application"
        || title.contains("primary application")
        || has_any_section(
            sections,
            &[
                "Application Kind",
                "Behavior",
                "Runtime Topology",
                "Configuration Surface",
                "Command Interface",
                "Transport Surface",
                "Static Surface",
                "Startup",
                "Startup Sequence",
                "Main Loop Behavior",
                "Collaborators and Wiring",
                "Exit Codes",
                "Error Handling",
                "Shutdown",
            ],
        )
    {
        return SpecificationKind::App;
    }

    SpecificationKind::Unknown
}

fn parse_markdown_sections(content: &str) -> Vec<Section> {
    let mut sections = Vec::new();
    let mut current_title: Option<String> = None;
    let mut current_body = String::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(title) = trimmed.strip_prefix("## ") {
            if let Some(existing) = current_title.take() {
                sections.push(Section {
                    title: existing,
                    body: current_body.trim().to_string(),
                });
                current_body.clear();
            }
            current_title = Some(title.trim().to_string());
            continue;
        }
        if current_title.is_some() && is_markdown_thematic_break(trimmed) {
            continue;
        }
        if current_title.is_some() {
            current_body.push_str(line);
            current_body.push('\n');
        }
    }
    if let Some(existing) = current_title {
        sections.push(Section {
            title: existing,
            body: current_body.trim().to_string(),
        });
    }
    sections
}

fn find_section<'a>(sections: &'a [Section], title: &str) -> Option<&'a Section> {
    sections.iter().find(|section| section.title == title)
}

fn has_section(sections: &[Section], title: &str) -> bool {
    find_section(sections, title).is_some()
}

fn has_any_section(sections: &[Section], titles: &[&str]) -> bool {
    titles.iter().any(|title| has_section(sections, title))
}

fn extract_collaborators(
    kind: SpecificationKind,
    content: &str,
    sections: &[Section],
) -> Vec<String> {
    if matches!(kind, SpecificationKind::Data) {
        return Vec::new();
    }

    let mut names = BTreeSet::new();

    if let Ok(depends_re) = Regex::new(r"(?i)^depends on:\s*(.+)$") {
        for line in content.lines() {
            if let Some(captures) = depends_re.captures(line.trim()) {
                if let Some(matched) = captures.get(1) {
                    for part in matched.as_str().split(',') {
                        let value = normalize_symbol_name(part);
                        if !value.is_empty() {
                            names.insert(value);
                        }
                    }
                }
            }
        }
    }

    if let Some(section) = find_section(sections, "Role Players") {
        let role_lines = section.body.lines().collect::<Vec<_>>();
        let uses_subheadings = role_lines
            .iter()
            .any(|line| line.trim().starts_with("### "));

        for line in role_lines {
            let trimmed = line.trim();
            let candidate = if uses_subheadings {
                trimmed
                    .strip_prefix("### ")
                    .map(normalize_symbol_name)
                    .unwrap_or_default()
            } else if let Some(cell) = extract_table_cell_name(trimmed, &["role player"]) {
                cell
            } else if trimmed.starts_with('-') {
                extract_bullet_name(trimmed)
            } else {
                String::new()
            };
            if collaborator_name_is_actionable(&candidate) {
                names.insert(candidate);
            }
        }
    }

    for title in ["Collaborators", "Collaborators and Wiring"] {
        if let Some(section) = find_section(sections, title) {
            for line in section.body.lines() {
                let trimmed = line.trim();
                let candidate = if let Some(cell) =
                    extract_table_cell_name(trimmed, &["helper", "collaborator"])
                {
                    cell
                } else if trimmed.starts_with('-') {
                    extract_bullet_name(trimmed)
                } else {
                    String::new()
                };
                if collaborator_name_is_actionable(&candidate) {
                    names.insert(candidate);
                }
            }
        }
    }

    names.into_iter().collect()
}

fn extract_env_vars(content: &str) -> Vec<String> {
    let mut vars = BTreeSet::new();
    let token_re = Regex::new(r"\b[A-Z][A-Z0-9_]{2,}\b").unwrap();
    let assignment_re = Regex::new(r"\b([A-Z][A-Z0-9_]{2,})=").unwrap();
    let quoted_re = Regex::new(r#"`([^`]+)`|"([^"]+)"|'([^']+)'"#).unwrap();
    let env_context_markers = [
        "environment variable",
        "environment variables",
        "env var",
        "env vars",
        "process environment",
        "read environment",
        "reads environment",
        "from environment",
        ".env",
        "override precedence",
        "variable `",
    ];

    for line in content.lines() {
        for captures in assignment_re.captures_iter(line) {
            if let Some(matched) = captures.get(1) {
                vars.insert(matched.as_str().to_string());
            }
        }

        let lowered = line.to_ascii_lowercase();
        let has_env_context = env_context_markers
            .iter()
            .any(|marker| lowered.contains(marker));
        if !has_env_context {
            continue;
        }

        for captures in quoted_re.captures_iter(line) {
            let candidate = captures
                .get(1)
                .or_else(|| captures.get(2))
                .or_else(|| captures.get(3))
                .map(|m| m.as_str().trim())
                .unwrap_or("");
            if token_re.is_match(candidate) && is_env_var_like(candidate) {
                vars.insert(candidate.to_string());
            }
        }

        for mat in token_re.find_iter(line) {
            let value = mat.as_str();
            if is_env_var_like(value) {
                vars.insert(value.to_string());
            }
        }
    }
    vars.into_iter().collect()
}

fn is_env_var_like(value: &str) -> bool {
    let common_acronyms = [
        "API", "ANSI", "ASCII", "FIFO", "HTTP", "HTTPS", "JSON", "SQL", "CSV", "XML", "UTF",
        "UUID", "CLI", "GUI", "TUI", "TCP", "UDP",
    ];
    if common_acronyms.contains(&value) {
        return false;
    }
    value.contains('_') || value.len() > 6
}

fn extract_delegation_requirements(
    kind: SpecificationKind,
    content: &str,
    collaborators: &[String],
) -> Vec<DelegationRequirement> {
    if matches!(kind, SpecificationKind::Data) {
        return Vec::new();
    }

    let collaborator_set: HashSet<String> = collaborators
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .collect();

    let mut requirements = Vec::new();
    let code_re = Regex::new(r"`([^`]+)`").unwrap();
    for line in content.lines() {
        let lowered = line.to_ascii_lowercase();
        if !lowered.contains("must") && !lowered.contains("uses ") && !lowered.contains("delegated")
        {
            continue;
        }
        let ids = code_re
            .captures_iter(line)
            .filter_map(|capture| capture.get(1).map(|m| normalize_symbol_name(m.as_str())))
            .filter(|value| delegation_entity_name_is_actionable(value))
            .collect::<Vec<_>>();
        if ids.len() >= 2 {
            let actor = ids[0].clone();
            let target = ids[1].clone();
            if !collaborator_set.is_empty()
                && (!collaborator_set.contains(&actor.to_ascii_lowercase())
                    || !collaborator_set.contains(&target.to_ascii_lowercase()))
            {
                continue;
            }
            requirements.push(DelegationRequirement {
                actor,
                target,
                source_line: line.trim().to_string(),
            });
        }
    }
    requirements
}

fn extract_output_requirements(content: &str) -> Vec<OutputRequirement> {
    let mut requirements = Vec::new();
    let code_re = Regex::new(r"`([^`]+)`").unwrap();
    for line in content.lines() {
        let lowered = line.to_ascii_lowercase();
        if !(lowered.contains("print")
            || lowered.contains("render")
            || lowered.contains("stderr")
            || lowered.contains("stdout"))
        {
            continue;
        }
        for capture in code_re.captures_iter(line) {
            if let Some(matched) = capture.get(1) {
                let literal = matched.as_str().trim();
                if literal.contains(' ') || literal.contains('_') || literal.contains(':') {
                    requirements.push(OutputRequirement {
                        literal: literal.to_string(),
                        source_line: line.trim().to_string(),
                    });
                }
            }
        }
    }
    requirements
}

fn extract_shared_state_requirements(content: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(|line| {
            let lowered = line.to_ascii_lowercase();
            if lowered.contains("one shared")
                || lowered.contains("same shared")
                || lowered.contains("process lifetime")
                || lowered.contains("without resetting or replacing")
            {
                Some(line.trim().to_string())
            } else {
                None
            }
        })
        .collect()
}

fn extract_role_method_names(sections: &[Section]) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(section) = find_section(sections, "Role Methods") {
        let table_method_re = Regex::new(r"^\|\s*\*\*([^*|`]+)\*\*").unwrap();
        for line in section.body.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("- **") && trimmed.contains("**") {
                let name = extract_bullet_name(trimmed);
                if !name.is_empty() {
                    names.push(name);
                }
            } else if trimmed.starts_with('|') && !trimmed.contains("---") {
                let name = table_method_re
                    .captures(trimmed)
                    .and_then(|captures| {
                        captures
                            .get(1)
                            .map(|matched| normalize_symbol_name(matched.as_str()))
                    })
                    .unwrap_or_default();
                if !name.is_empty() {
                    names.push(name);
                }
            }
        }
    }
    names
}

fn extract_external_behavior_clues(content: &str) -> Vec<String> {
    let keywords = [
        "stdin",
        "standard input",
        "non-blocking",
        "render",
        "terminal",
        "stdout",
        "stderr",
        "environment variable",
        "shared input stream",
        "capture",
    ];
    content
        .lines()
        .filter_map(|line| {
            let lowered = line.to_ascii_lowercase();
            if keywords.iter().any(|keyword| lowered.contains(keyword)) {
                Some(line.trim().to_string())
            } else {
                None
            }
        })
        .collect()
}

fn dependency_names_from_context(ctx: &HashMap<String, serde_json::Value>) -> HashSet<String> {
    let mut names = HashSet::new();
    for key in [
        "direct_dependencies",
        "dependency_closure",
        "implemented_dependencies",
    ] {
        if let Some(entries) = ctx.get(key).and_then(|value| value.as_array()) {
            for entry in entries {
                if let Some(name) = entry.get("name").and_then(|value| value.as_str()) {
                    names.insert(normalize_symbol_name(name));
                }
                if let Some(path) = entry.get("path").and_then(|value| value.as_str()) {
                    if let Some(stem) = Path::new(path).file_stem().and_then(|value| value.to_str())
                    {
                        names.insert(normalize_symbol_name(stem));
                    }
                }
            }
        }
    }
    names
}

fn extract_bullet_name(line: &str) -> String {
    if let Some(rest) = line.strip_prefix("- **") {
        if let Some(end) = rest.find("**") {
            return normalize_symbol_name(&rest[..end]);
        }
    }
    let trimmed = line.trim_start_matches('-').trim();
    let candidate = trimmed
        .split_once(':')
        .map(|(head, _)| head)
        .unwrap_or(trimmed);
    normalize_symbol_name(candidate)
}

fn extract_table_cell_name(line: &str, header_labels: &[&str]) -> Option<String> {
    if !line.starts_with('|') || line.contains("---") {
        return None;
    }

    let cells = line
        .trim_matches('|')
        .split('|')
        .map(|cell| strip_markdown_markup(cell.trim()))
        .collect::<Vec<_>>();
    if cells.is_empty() {
        return None;
    }

    let first = cells[0].trim();
    if first.is_empty() {
        return None;
    }

    let first_lower = first.to_ascii_lowercase();
    if header_labels.iter().any(|label| *label == first_lower) {
        return None;
    }

    let name = normalize_symbol_name(first);
    if name.is_empty() { None } else { Some(name) }
}

fn normalize_symbol_name(value: &str) -> String {
    let trimmed = strip_markdown_markup(value);
    if trimmed.is_empty() {
        return String::new();
    }
    trimmed
        .split_whitespace()
        .next()
        .unwrap_or(trimmed)
        .trim_matches(|c: char| matches!(c, '(' | ')' | ',' | '.'))
        .to_string()
}

fn strip_markdown_markup(value: &str) -> &str {
    value
        .trim()
        .trim_matches('`')
        .trim_matches('*')
        .trim_matches('|')
}

fn collaborator_name_is_actionable(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    !is_documentation_label(name)
}

/// Rust primitive scalar types and common stdlib types that can never be DCI
/// collaborators. They appear in type-annotation prose ("`score` must be `i32`")
/// and must not be treated as delegation targets.
const RUST_PRIMITIVE_TYPES: &[&str] = &[
    "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize",
    "f32", "f64", "bool", "char", "str", "String", "Vec", "Option", "Result", "Box", "Arc",
    "Rc", "Cell", "RefCell", "Mutex", "RwLock",
];

fn is_rust_primitive(name: &str) -> bool {
    RUST_PRIMITIVE_TYPES.contains(&name)
}

fn delegation_entity_name_is_actionable(name: &str) -> bool {
    collaborator_name_is_actionable(name)
        && identifier_like_entity_name(name)
        && !is_rust_primitive(name)
}

fn identifier_like_entity_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn is_documentation_label(name: &str) -> bool {
    matches!(
        name,
        "Behavior"
            | "Basis"
            | "Description"
            | "Ends"
            | "Example"
            | "Examples"
            | "Functionalities"
            | "Inference"
            | "Input"
            | "Inputs"
            | "Location"
            | "Methods"
            | "Note"
            | "Notes"
            | "Output"
            | "Outputs"
            | "Produces"
            | "Purpose"
            | "Returns"
            | "Role"
            | "Rules"
    )
}

fn is_probably_domain_type(name: &str) -> bool {
    if is_documentation_label(name) {
        return false;
    }
    let first = name.chars().next();
    first.map(|c| c.is_ascii_uppercase()).unwrap_or(false)
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn collaborator_candidate_paths(project_root: &Path, name: &str) -> Vec<String> {
    let stem = to_snake_case(name);
    let candidates = [
        project_root
            .join("src")
            .join("contexts")
            .join(format!("{stem}.rs")),
        project_root
            .join("src")
            .join("data")
            .join(format!("{stem}.rs")),
        project_root.join("src").join(format!("{stem}.rs")),
    ];
    candidates
        .iter()
        .map(|path| path.display().to_string())
        .collect()
}

fn to_snake_case(name: &str) -> String {
    let mut out = String::new();
    for (idx, ch) in name.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if idx > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

#[cfg(test)]
mod tests {
    use super::super::capability_registry::{
        add_capability_mapping_to_registry, empty_registry, write_capability_registry,
    };
    use super::{
        SpecificationKind, analyze_specification, compare_verifier_reports,
        determine_spec_path_for_output, extract_external_crate_roots,
        verify_generated_implementation,
    };
    use serde_json::json;
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn write_interface_ir(
        root: &Path,
        category: &str,
        type_name: &str,
        draft_relative_path: &str,
        methods: &[&str],
    ) {
        let path = root
            .join(".reen")
            .join("interfaces")
            .join(category)
            .join(format!("{type_name}.interface.json"));
        fs::create_dir_all(path.parent().expect("interface parent")).expect("mkdir interface dir");
        let exported_methods = methods
            .iter()
            .map(|method| {
                json!({
                    "semantic_name": method,
                    "rust_name": method,
                    "export_name": method,
                    "receiver": "&self",
                    "parameters": [],
                    "return_type": "()",
                    "failure_shape": "plain",
                    "signature": format!("pub fn {method}(&self)")
                })
            })
            .collect::<Vec<_>>();
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "version": "reen.interface-ir/v1",
                "draft_identity": type_name,
                "draft_relative_path": draft_relative_path,
                "specification_kind": category.trim_end_matches('s'),
                "artifact_kind": format!("{}_module", category.trim_end_matches('s')),
                "interface_fingerprint": "test-fingerprint",
                "primary_export_name": type_name,
                "exported_types": [],
                "exported_methods": exported_methods,
                "role_method_exports": [],
                "name_bindings": [],
                "dependency_bindings": [],
                "resolved_types": []
            }))
            .expect("serialize interface"),
        )
        .expect("write interface");
    }

    fn write_field_only_interface_ir(
        root: &Path,
        category: &str,
        type_name: &str,
        draft_relative_path: &str,
        fields: &[&str],
    ) {
        let path = root
            .join(".reen")
            .join("interfaces")
            .join(category)
            .join(format!("{type_name}.interface.json"));
        fs::create_dir_all(path.parent().expect("interface parent")).expect("mkdir interface dir");
        let exported_fields = fields
            .iter()
            .map(|field| {
                json!({
                    "semantic_name": field,
                    "rust_name": field,
                    "export_name": field,
                    "type_ref": "u32"
                })
            })
            .collect::<Vec<_>>();
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "version": "reen.interface-ir/v1",
                "draft_identity": type_name,
                "draft_relative_path": draft_relative_path,
                "specification_kind": category.trim_end_matches('s'),
                "artifact_kind": format!("{}_module", category.trim_end_matches('s')),
                "interface_fingerprint": "test-fingerprint",
                "primary_export_name": type_name,
                "exported_types": [{
                    "semantic_name": type_name,
                    "rust_name": type_name,
                    "export_name": type_name,
                    "kind": "struct",
                    "fields": exported_fields
                }],
                "exported_methods": [],
                "role_method_exports": [],
                "name_bindings": [],
                "dependency_bindings": [],
                "resolved_types": []
            }))
            .expect("serialize interface"),
        )
        .expect("write interface");
    }

    #[test]
    fn spec_lint_flags_env_var_contradictions() {
        let content = r#"# App

## Behavior
- Read environment variable `FOO_MODE`

## Environment Variables
Unspecified in draft - no environment variables referenced.
"#;
        let report = analyze_specification(Path::new("specifications/app.md"), content, None);
        assert_eq!(report.contract.kind, SpecificationKind::App);
        assert!(report.errors.iter().any(|item| item.contains("FOO_MODE")));
    }

    #[test]
    fn data_spec_has_no_spurious_delegations_or_collaborators() {
        let content = r#"# Board

## Description

The board describes the rectangular play area for a round of Snake.

## Fields

| Field | Meaning | Notes |
|---|---|---|
| width | Width | Positive whole number |
| height | Height | Positive whole number |

## Rules

- `width` must be greater than `0`.
- `height` must be greater than `0`.
"#;
        let report =
            analyze_specification(Path::new("specifications/data/Board.md"), content, None);
        assert_eq!(report.contract.kind, SpecificationKind::Data);
        assert!(
            report.contract.collaborators.is_empty(),
            "collaborators: {:?}",
            report.contract.collaborators
        );
        assert!(
            report.contract.delegation_requirements.is_empty(),
            "delegation_requirements: {:?}",
            report.contract.delegation_requirements
        );
    }

    #[test]
    fn projection_output_literals_do_not_become_delegation_requirements() {
        let content = r#"# String Renderer

## Purpose

StringRenderer turns one board picture and its score into plain text.

## Role Players

| Role player | Why involved | Expected behaviour |
|---|---|---|
| Board | Supplies the picture of the current board | Allows the renderer to read what symbol should appear at each visible position |

## Role Methods

### Board

- **width**
  Returns the number of columns in the picture.

- **height**
  Returns the number of rows in the picture.

- **symbol_at**
  Returns the symbol to show at a given coordinate.

## Props

| Prop | Meaning | Notes |
|---|---|---|
| score | Score shown below the board | Same score the user sees during play |

## Functionalities

### render

| Started by | Uses | Result |
|---|---|---|
| TerminalRenderer or the application | board, score | one text frame is returned |

Rules:
- Reads the current board picture and the score.
- Uses score line format `Score: <score>`.
- The score line also ends with a newline (`\n`).
"#;

        let report = analyze_specification(
            Path::new("specifications/projections/string_renderer.md"),
            content,
            None,
        );

        assert_eq!(report.contract.collaborators, vec!["Board".to_string()]);
        assert!(
            report.contract.delegation_requirements.is_empty(),
            "delegation_requirements: {:?}",
            report.contract.delegation_requirements
        );
    }

    #[test]
    fn primitive_rust_types_never_become_delegation_requirements() {
        // Prose like "`score` must be `i32`" or "`value` uses `u32`" appears
        // in synthesised contracts as type annotations. Rust primitives must
        // never be interpreted as delegation targets.
        let content = r#"# ScoreDisplay

## Purpose

Shows the score.

## Role Players

| Role player | Why involved | Expected behaviour |
|---|---|---|
| Board | Supplies the picture | Read-only grid access |

## Props

| Prop | Meaning | Notes |
|------|---------|-------|
| score | The score value | Must be `i32` |

## Functionalities

### render

Rules:
- The `score` must be provided as `i32`.
- Uses `score` of type `i32` to format the line.
"#;

        let report = analyze_specification(
            Path::new("specifications/projections/score_display.md"),
            content,
            None,
        );
        assert!(
            report.contract.delegation_requirements.is_empty(),
            "primitive types produced false delegation requirements: {:?}",
            report.contract.delegation_requirements
        );
    }

    #[test]
    fn delegation_skips_backtick_pairs_when_either_side_is_not_a_collaborator() {
        let content = r#"# Loop

## Purpose

Test context.

## Role Players

| Role player | Why involved | Expected behaviour |
|---|---|---|
| food_dropper | Chooses food | Chooses |

## Functionalities

### tick

Rules:
- must record `food_dropper` then `drop` on the same line for extraction.
"#;

        let report = analyze_specification(
            Path::new("specifications/contexts/loop_ctx.md"),
            content,
            None,
        );
        assert_eq!(report.contract.kind, SpecificationKind::Context);
        assert!(
            report.contract.delegation_requirements.is_empty(),
            "expected role/method backtick pairs to be ignored: {:?}",
            report.contract.delegation_requirements
        );
    }

    #[test]
    fn verifier_flags_missing_env_var_without_flagging_normal_vec_new_usage() {
        let root = make_temp_dir("pipeline_quality_verifier");
        let specs = root.join("specifications");
        let src = root.join("src");
        fs::create_dir_all(&specs).expect("mkdir specs");
        fs::create_dir_all(&src).expect("mkdir src");

        let spec_path = specs.join("app.md");
        fs::write(
            &spec_path,
            r#"# App

## Behavior
- Read environment variable `FOO_MODE`
- Print `READY`
"#,
        )
        .expect("write spec");

        let output = src.join("main.rs");
        fs::write(
            &output,
            "fn main() { let _x = Vec::<u8>::new(); println!(\"READY\"); }",
        )
        .expect("write impl");

        let report = verify_generated_implementation(
            &root,
            &spec_path,
            &fs::read_to_string(&spec_path).unwrap(),
            &output,
        )
        .expect("verify");
        assert!(report.errors.iter().any(|item| item.contains("FOO_MODE")));
        assert!(report.high_risk_findings.is_empty());

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn verifier_rejects_comment_only_app_implementations() {
        let root = make_temp_dir("pipeline_quality_comment_only_app");
        let specs = root.join("specifications");
        let src = root.join("src");
        fs::create_dir_all(&specs).expect("mkdir specs");
        fs::create_dir_all(&src).expect("mkdir src");

        let spec_path = specs.join("app.md");
        fs::write(
            &spec_path,
            r#"# Snake App

## Behavior
- Read environment variable `SNAKE_RENDERER`
- Print `REEN_SNAKE_TEST_RESULT game_over score=<score>`

## Collaborators and Wiring
| Collaborator | Responsibility |
|---|---|
| `CommandInputContext` | Captures key presses |
"#,
        )
        .expect("write spec");

        let output = src.join("main.rs");
        fs::write(
            &output,
            r#"// Read environment variable SNAKE_RENDERER
// CommandInputContext
// REEN_SNAKE_TEST_RESULT game_over score=<score>
"#,
        )
        .expect("write impl");

        let report = verify_generated_implementation(
            &root,
            &spec_path,
            &fs::read_to_string(&spec_path).unwrap(),
            &output,
        )
        .expect("verify");

        assert!(
            report
                .errors
                .iter()
                .any(|item| item.contains("effectively empty")),
            "errors: {:?}",
            report.errors
        );
        assert!(
            report
                .errors
                .iter()
                .any(|item| item.contains("top-level `main` function")),
            "errors: {:?}",
            report.errors
        );
        assert!(
            report
                .errors
                .iter()
                .any(|item| item.contains("SNAKE_RENDERER")),
            "errors: {:?}",
            report.errors
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn verifier_does_not_error_on_missing_collaborator_artifact_files() {
        let root = make_temp_dir("pipeline_quality_missing_artifact_rule_removed");
        let specs = root.join("specifications");
        let src = root.join("src");
        fs::create_dir_all(&specs).expect("mkdir specs");
        fs::create_dir_all(&src).expect("mkdir src");

        let spec_path = specs.join("app.md");
        fs::write(
            &spec_path,
            r#"# App

## Collaborators and Wiring
| Collaborator | Responsibility |
|---|---|
| `ImaginaryRenderer` | Renders output |
"#,
        )
        .expect("write spec");

        let output = src.join("main.rs");
        fs::write(
            &output,
            r#"struct ImaginaryRenderer;

fn main() {
    let _ = core::mem::size_of::<ImaginaryRenderer>();
}"#,
        )
        .expect("write impl");

        let report = verify_generated_implementation(
            &root,
            &spec_path,
            &fs::read_to_string(&spec_path).unwrap(),
            &output,
        )
        .expect("verify");

        assert!(
            !report
                .errors
                .iter()
                .any(|item| item.contains("Required collaborator artifact")),
            "errors: {:?}",
            report.errors
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn verifier_flags_trivial_role_method_stub_returns() {
        let root = make_temp_dir("pipeline_quality_role_stub");
        let specs = root.join("specifications").join("contexts");
        let src = root.join("src").join("contexts");
        fs::create_dir_all(&specs).expect("mkdir specs");
        fs::create_dir_all(&src).expect("mkdir src");

        let spec_path = specs.join("command_input.md");
        fs::write(
            &spec_path,
            r#"# CommandInputContext

## Purpose
Used for one shared input stream across the whole application session.

## Role Players
| Role Player | Why Involved | Expected Behaviour |
|---|---|---|
| stdin_source | Supplies keyboard input to the context | Provides non-blocking reads from standard input. |

## Role Methods
### stdin_source
- **read_available**
  Returns all currently available keystrokes in arrival order without blocking.
"#,
        )
        .expect("write spec");

        let output = src.join("command_input.rs");
        fs::write(
            &output,
            r#"fn stdin_source_read_available(&self) -> Vec<char> { Vec::new() }"#,
        )
        .expect("write impl");

        let report = verify_generated_implementation(
            &root,
            &spec_path,
            &fs::read_to_string(&spec_path).unwrap(),
            &output,
        )
        .expect("verify");
        assert!(
            report
                .high_risk_findings
                .iter()
                .any(|item| item.contains("stdin_source_read_available"))
        );
        assert!(
            report
                .high_risk_findings
                .iter()
                .any(|item| item.contains("command_input.rs:1:")),
            "findings: {:?}",
            report.high_risk_findings
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn verifier_ignores_trivial_trait_impl_for_collaborator_role_methods() {
        let root = make_temp_dir("pipeline_quality_trait_role_impl");
        let specs = root.join("specifications").join("contexts");
        let src = root.join("src").join("contexts");
        fs::create_dir_all(&specs).expect("mkdir specs");
        fs::create_dir_all(&src).expect("mkdir src");

        let spec_path = specs.join("game_loop.md");
        fs::write(
            &spec_path,
            r#"# GameLoopContext

## Purpose
Runs the game loop.

## Role Methods
### food_dropper
- **drop**
  Returns `Some(food)` or `None`.
"#,
        )
        .expect("write spec");

        let output = src.join("game_loop.rs");
        fs::write(
            &output,
            r#"trait FoodDropper {
    fn drop(&mut self) -> Option<u32>;
}

struct EmptyFoodDropper;

impl FoodDropper for EmptyFoodDropper {
    fn drop(&mut self) -> Option<u32> {
        None
    }
}

struct GameLoopContext {
    food_dropper: EmptyFoodDropper,
}

impl GameLoopContext {
    fn food_dropper_drop(&mut self) -> Option<u32> {
        self.food_dropper.drop()
    }
}"#,
        )
        .expect("write impl");

        let report = verify_generated_implementation(
            &root,
            &spec_path,
            &fs::read_to_string(&spec_path).unwrap(),
            &output,
        )
        .expect("verify");
        assert!(
            !report
                .high_risk_findings
                .iter()
                .any(|item| item.contains("trivial body returning None")),
            "findings: {:?}",
            report.high_risk_findings
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn verifier_flags_direct_collaborator_calls_missing_from_interface() {
        let root = make_temp_dir("pipeline_quality_collaborator_method_leak");
        let specs = root.join("specifications").join("contexts");
        let src = root.join("src").join("contexts");
        fs::create_dir_all(&specs).expect("mkdir specs");
        fs::create_dir_all(&src).expect("mkdir src");

        write_interface_ir(
            &root,
            "data",
            "Snake",
            "data/Snake.md",
            &["body", "direction"],
        );
        write_interface_ir(
            &root,
            "data",
            "GameState",
            "data/GameState.md",
            &["score", "food", "increment_score", "place_food"],
        );

        let spec_path = specs.join("game_loop.md");
        fs::write(
            &spec_path,
            r#"# GameLoopContext

## Purpose
Runs the game loop.

## Role Players
| Role player | Why involved | Expected behaviour |
|---|---|---|
| snake | Tracks the snake | Exposes snake state |
| game_state | Tracks score and food | Exposes game state |

## Role Methods
### snake
- **body**
  Returns positions.

### game_state
- **score**
  Returns the score.

## Functionalities
### tick
| Started by | Uses | Result |
|---|---|---|
| scheduler | snake, game_state | one game tick is processed |
"#,
        )
        .expect("write spec");

        let output = src.join("game_loop.rs");
        fs::write(
            &output,
            r#"use crate::data::{GameState, Position, Snake};

impl Position {
    fn new(x: u32, y: u32) -> Self {
        let _ = (x, y);
        Self
    }
}

struct GameLoopContext {
    snake: Snake,
    game_state: GameState,
}

impl GameLoopContext {
    pub fn tick(&mut self) {
        let _ = self.snake.grow(Position::new(1, 2));
        let _ = self.game_state.with_score(10);
    }
}"#,
        )
        .expect("write impl");

        let report = verify_generated_implementation(
            &root,
            &spec_path,
            &fs::read_to_string(&spec_path).unwrap(),
            &output,
        )
        .expect("verify");
        assert!(
            report
                .high_risk_findings
                .iter()
                .any(|item| item.contains("self.snake.grow") && item.contains("data/Snake.md")),
            "findings: {:?}",
            report.high_risk_findings
        );
        assert!(
            report.high_risk_findings.iter().any(|item| {
                item.contains("self.game_state.with_score") && item.contains("data/GameState.md")
            }),
            "findings: {:?}",
            report.high_risk_findings
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn verifier_allows_local_helper_that_uses_declared_collaborator_methods() {
        let root = make_temp_dir("pipeline_quality_context_helper_allowed");
        let specs = root.join("specifications").join("contexts");
        let src = root.join("src").join("contexts");
        fs::create_dir_all(&specs).expect("mkdir specs");
        fs::create_dir_all(&src).expect("mkdir src");

        write_interface_ir(
            &root,
            "data",
            "Snake",
            "data/Snake.md",
            &["body", "direction"],
        );

        let spec_path = specs.join("game_loop.md");
        fs::write(
            &spec_path,
            r#"# GameLoopContext

## Purpose
Runs the game loop.

## Role Players
| Role player | Why involved | Expected behaviour |
|---|---|---|
| snake | Tracks the snake | Exposes snake state |

## Role Methods
### snake
- **body**
  Returns positions.

- **direction**
  Returns direction.

## Functionalities
### head
| Started by | Uses | Result |
|---|---|---|
| caller | snake | current head is returned |
"#,
        )
        .expect("write spec");

        let output = src.join("game_loop.rs");
        fs::write(
            &output,
            r#"struct Snake;
struct Position;
struct Direction;

impl GameLoopContext {
    pub fn head(&self) -> Position {
        snake_head(&self.snake)
    }
}

struct GameLoopContext {
    snake: Snake,
}

fn snake_head(snake: &Snake) -> Position {
    let _ = snake.direction();
    snake.body()[0]
}"#,
        )
        .expect("write impl");

        let report = verify_generated_implementation(
            &root,
            &spec_path,
            &fs::read_to_string(&spec_path).unwrap(),
            &output,
        )
        .expect("verify");
        assert!(
            !report
                .high_risk_findings
                .iter()
                .any(|item| item.contains("not declared on interface")),
            "findings: {:?}",
            report.high_risk_findings
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn verifier_flags_direct_collaborator_calls_for_spaced_projection_titles() {
        let root = make_temp_dir("pipeline_quality_projection_spaced_title");
        let specs = root.join("drafts").join("projections");
        let src = root.join("src").join("projections");
        fs::create_dir_all(&specs).expect("mkdir specs");
        fs::create_dir_all(&src).expect("mkdir src");

        write_interface_ir(
            &root,
            "data",
            "Board",
            "data/board.md",
            &["width", "height"],
        );

        let spec_path = specs.join("string_renderer.md");
        fs::write(
            &spec_path,
            r#"# String Renderer

## Purpose
Formats the board.

## Role Players
| Role player | Why involved | Expected behaviour |
|---|---|---|
| board | Source picture | Provides board state |

## Role Methods
### board
- **width**
  Returns width.
- **symbol_at**
  Returns a symbol.

## Props
| Prop | Meaning | Notes |
|---|---|---|
| score | Visible score | Stable |

## Functionalities
### render
| Started by | Uses | Result |
|---|---|---|
| caller | board, score | one frame |
"#,
        )
        .expect("write spec");

        let output = src.join("string_renderer.rs");
        fs::write(
            &output,
            r#"use crate::data::Board;

pub struct StringRenderer {
    board: Board,
    score: u32,
}

impl StringRenderer {
    pub fn render(&self) -> char {
        self.board_symbol_at()
    }

    fn board_symbol_at(&self) -> char {
        let _ = self.score;
        self.board.symbol_at()
    }
}"#,
        )
        .expect("write impl");

        let report = verify_generated_implementation(
            &root,
            &spec_path,
            &fs::read_to_string(&spec_path).unwrap(),
            &output,
        )
        .expect("verify");
        assert!(
            report
                .high_risk_findings
                .iter()
                .any(|item| item.contains("self.board.symbol_at") && item.contains("data/board.md")),
            "findings: {:?}",
            report.high_risk_findings
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn verifier_allows_field_getter_access_but_flags_other_calls_on_field_only_interfaces() {
        let root = make_temp_dir("pipeline_quality_projection_field_only_interface");
        let specs = root.join("drafts").join("projections");
        let src = root.join("src").join("projections");
        fs::create_dir_all(&specs).expect("mkdir specs");
        fs::create_dir_all(&src).expect("mkdir src");

        write_field_only_interface_ir(
            &root,
            "data",
            "Board",
            "data/Board.md",
            &["width", "height"],
        );

        let spec_path = specs.join("string_renderer.md");
        fs::write(
            &spec_path,
            r#"# String Renderer

## Purpose
Formats the board.

## Role Players
| Role player | Why involved | Expected behaviour |
|---|---|---|
| board | Source picture | Provides board state |

## Role Methods
### board
- **width**
  Returns width.
- **height**
  Returns height.
- **symbol_at**
  Returns a symbol.

## Props
| Prop | Meaning | Notes |
|---|---|---|
| score | Visible score | Stable |

## Functionalities
### render
| Started by | Uses | Result |
|---|---|---|
| caller | board, score | one frame |
"#,
        )
        .expect("write spec");

        let output = src.join("string_renderer.rs");
        fs::write(
            &output,
            r#"use crate::data::Board;

struct Position;

impl Position {
    fn new(x: u32, y: u32) -> Self {
        let _ = (x, y);
        Self
    }
}

pub struct StringRenderer {
    board: Board,
    score: u32,
}

impl StringRenderer {
    pub fn render(&self) -> char {
        let _ = self.board.width();
        let _ = self.board.height();
        let _ = self.score;
        self.board.symbol_at(Position::new(0, 0))
    }
}"#,
        )
        .expect("write impl");

        let report = verify_generated_implementation(
            &root,
            &spec_path,
            &fs::read_to_string(&spec_path).unwrap(),
            &output,
        )
        .expect("verify");
        assert!(
            report
                .high_risk_findings
                .iter()
                .any(|item| item.contains("self.board.symbol_at") && item.contains("data/Board.md")),
            "findings: {:?}",
            report.high_risk_findings
        );
        assert!(
            !report
                .high_risk_findings
                .iter()
                .any(|item| item.contains("self.board.width") || item.contains("self.board.height")),
            "findings: {:?}",
            report.high_risk_findings
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn verifier_flags_ignored_immutable_transform_results() {
        let root = make_temp_dir("pipeline_quality_immutable_ignore");
        let specs = root.join("specifications").join("data");
        let src = root.join("src").join("data");
        fs::create_dir_all(&specs).expect("mkdir specs");
        fs::create_dir_all(&src).expect("mkdir src");

        let spec_path = specs.join("gamestate.md");
        fs::write(
            &spec_path,
            r#"# GameState

## Mutability
Immutable. All mutation-like operations return a new GameState rather than modifying the existing instance.

## Functionalities
- **place_food**
  Takes Some(food) or None and returns a new GameState with food updated.
- **increment_score**
  Takes a positive whole number and returns a new GameState with score increased.
"#,
        )
        .expect("write spec");

        let output = src.join("gamestate_usage.rs");
        fs::write(
            &output,
            r#"fn build(mut game_state: GameState, food: Option<Food>) {
    game_state.place_food(food);
    game_state.increment_score(10);
}"#,
        )
        .expect("write impl");

        let report = verify_generated_implementation(
            &root,
            &spec_path,
            &fs::read_to_string(&spec_path).unwrap(),
            &output,
        )
        .expect("verify");
        assert!(
            report
                .high_risk_findings
                .iter()
                .any(|item| item.contains("place_food"))
        );
        assert!(
            report
                .high_risk_findings
                .iter()
                .any(|item| item.contains("increment_score"))
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn verifier_flags_private_leaf_module_import_paths() {
        let root = make_temp_dir("pipeline_quality_leaf_imports");
        let specs = root.join("specifications");
        let src = root.join("src");
        fs::create_dir_all(&specs).expect("mkdir specs");
        fs::create_dir_all(&src).expect("mkdir src");

        let spec_path = specs.join("app.md");
        fs::write(
            &spec_path,
            r#"# Snake App

## Behavior
- Starts the game.
"#,
        )
        .expect("write spec");

        let output = src.join("main.rs");
        fs::write(
            &output,
            r#"use snake::data::direction::Direction;

fn main() {
    let _ = Direction::Right;
}"#,
        )
        .expect("write impl");

        let report = verify_generated_implementation(
            &root,
            &spec_path,
            &fs::read_to_string(&spec_path).unwrap(),
            &output,
        )
        .expect("verify");
        assert!(
            report
                .high_risk_findings
                .iter()
                .any(|item| item.contains("private leaf-module path")),
            "findings: {:?}",
            report.high_risk_findings
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn verifier_flags_unplanned_external_crates_when_registry_exists() {
        let root = make_temp_dir("pipeline_quality_unplanned_crate");
        let specs = root.join("specifications");
        let drafts = root.join("drafts");
        let src = root.join("src");
        fs::create_dir_all(&specs).expect("mkdir specs");
        fs::create_dir_all(drafts.join("contexts")).expect("mkdir drafts");
        fs::create_dir_all(&src).expect("mkdir src");

        let mut registry = empty_registry();
        add_capability_mapping_to_registry(
            &mut registry,
            "terminal_raw_input",
            "crossterm",
            "terminal",
            "0.27",
            &[],
            true,
        )
        .expect("map capability");
        write_capability_registry(&drafts.join("capability_registry.yml"), &registry)
            .expect("write registry");
        fs::write(
            drafts.join("contexts/terminal_renderer.md"),
            "# TerminalRenderer\n\nReads key presses in raw mode.\n",
        )
        .expect("write draft");

        let spec_path = specs.join("app.md");
        fs::write(&spec_path, "# App\n\n## Behavior\n- Runs.\n").expect("write spec");

        let output = src.join("main.rs");
        fs::write(
            &output,
            r#"use libc::STDIN_FILENO;

fn main() {
    let _ = STDIN_FILENO;
}"#,
        )
        .expect("write impl");

        let report = verify_generated_implementation(
            &root,
            &spec_path,
            &fs::read_to_string(&spec_path).unwrap(),
            &output,
        )
        .expect("verify");
        assert!(
            report
                .high_risk_findings
                .iter()
                .any(|item| item.contains("not declared in the resolved dependency plan")),
            "findings: {:?}",
            report.high_risk_findings
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn external_crate_extractor_collapses_nested_use_trees_to_top_level_crates() {
        let roots = extract_external_crate_roots(
            r#"use std::io::{self, Write};
use crossterm::{
    cursor::{MoveToColumn, MoveUp},
    queue,
    style::Print,
};

fn render() -> Result<(), io::Error> {
    let _line_count = u16::MAX;
    let mut out = io::stdout();
    queue!(out, MoveUp(1), MoveToColumn(0), Print("x"))?;
    out.flush()?;
    Ok(())
}"#,
        );

        assert_eq!(roots, BTreeSet::from(["crossterm".to_string()]));
    }

    #[test]
    fn external_crate_extractor_ignores_imported_module_bindings() {
        let roots = extract_external_crate_roots(
            r#"use crossterm::event::{self, Event, KeyCode, KeyEvent};
use std::time::Duration;

fn read_one() {
    let _ = event::poll(Duration::from_secs(0));
}"#,
        );

        assert_eq!(roots, BTreeSet::from(["crossterm".to_string()]));
    }

    #[test]
    fn verifier_flags_conflicting_domain_provider() {
        let root = make_temp_dir("pipeline_quality_conflicting_provider");
        let specs = root.join("specifications");
        let drafts = root.join("drafts");
        let src = root.join("src");
        fs::create_dir_all(&specs).expect("mkdir specs");
        fs::create_dir_all(drafts.join("contexts")).expect("mkdir drafts");
        fs::create_dir_all(&src).expect("mkdir src");

        let mut registry = empty_registry();
        add_capability_mapping_to_registry(
            &mut registry,
            "terminal_raw_input",
            "crossterm",
            "terminal",
            "0.27",
            &[],
            true,
        )
        .expect("map capability");
        write_capability_registry(&drafts.join("capability_registry.yml"), &registry)
            .expect("write registry");
        fs::write(
            drafts.join("contexts/terminal_renderer.md"),
            "# TerminalRenderer\n\nReads key presses in raw mode.\n",
        )
        .expect("write draft");

        let spec_path = specs.join("app.md");
        fs::write(&spec_path, "# App\n\n## Behavior\n- Runs.\n").expect("write spec");

        let output = src.join("main.rs");
        fs::write(
            &output,
            r#"use termion::raw::IntoRawMode;

fn main() {}"#,
        )
        .expect("write impl");

        let report = verify_generated_implementation(
            &root,
            &spec_path,
            &fs::read_to_string(&spec_path).unwrap(),
            &output,
        )
        .expect("verify");
        assert!(
            report.high_risk_findings.iter().any(|item| {
                item.contains("capability domain 'terminal' is planned to use 'crossterm'")
            }),
            "findings: {:?}",
            report.high_risk_findings
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn verifier_allows_planned_terminal_crate_with_nested_use_tree_and_primitives() {
        let root = make_temp_dir("pipeline_quality_planned_crossterm_nested");
        let specs = root.join("specifications");
        let drafts = root.join("drafts");
        let src = root.join("src");
        fs::create_dir_all(&specs).expect("mkdir specs");
        fs::create_dir_all(drafts.join("contexts")).expect("mkdir drafts");
        fs::create_dir_all(&src).expect("mkdir src");

        let mut registry = empty_registry();
        add_capability_mapping_to_registry(
            &mut registry,
            "terminal_screen_control",
            "crossterm",
            "terminal",
            "0.27",
            &[],
            true,
        )
        .expect("map screen control");
        add_capability_mapping_to_registry(
            &mut registry,
            "terminal_raw_input",
            "crossterm",
            "terminal",
            "0.27",
            &[],
            true,
        )
        .expect("map raw input");
        write_capability_registry(&drafts.join("capability_registry.yml"), &registry)
            .expect("write registry");
        fs::write(
            drafts.join("contexts/terminal_renderer.md"),
            "# TerminalRenderer\n\nUses terminal render and keypress handling.\n",
        )
        .expect("write draft");

        let spec_path = specs.join("app.md");
        fs::write(&spec_path, "# App\n\n## Behavior\n- Runs.\n").expect("write spec");

        let output = src.join("main.rs");
        fs::write(
            &output,
            r#"use std::io::{self, Write};
use crossterm::{
    cursor::{MoveToColumn, MoveUp},
    queue,
    style::Print,
};
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use std::time::Duration;

fn main() -> Result<(), io::Error> {
    let _line_count = u16::MAX;
    let _ = event::poll(Duration::from_secs(0));
    let mut out = io::stdout();
    queue!(out, MoveUp(1), MoveToColumn(0), Print("x"))?;
    out.flush()?;
    Ok(())
}"#,
        )
        .expect("write impl");

        let report = verify_generated_implementation(
            &root,
            &spec_path,
            &fs::read_to_string(&spec_path).unwrap(),
            &output,
        )
        .expect("verify");
        assert!(
            !report
                .high_risk_findings
                .iter()
                .any(|item| item.contains("resolved dependency plan")),
            "findings: {:?}",
            report.high_risk_findings
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn verifier_allows_local_library_crate_imports_from_main() {
        let root = make_temp_dir("pipeline_quality_local_main_import");
        let specs = root.join("specifications");
        let drafts = root.join("drafts");
        let src = root.join("src");
        fs::create_dir_all(&specs).expect("mkdir specs");
        fs::create_dir_all(drafts.join("contexts")).expect("mkdir drafts");
        fs::create_dir_all(&src).expect("mkdir src");

        let mut registry = empty_registry();
        add_capability_mapping_to_registry(
            &mut registry,
            "terminal_screen_control",
            "crossterm",
            "terminal",
            "0.27",
            &[],
            true,
        )
        .expect("map screen control");
        write_capability_registry(&drafts.join("capability_registry.yml"), &registry)
            .expect("write registry");
        fs::write(
            drafts.join("contexts/terminal_renderer.md"),
            "# TerminalRenderer\n\nUses terminal rendering.\n",
        )
        .expect("write draft");
        fs::write(
            root.join("Cargo.toml"),
            r#"[package]
name = "generated-project"
version = "0.1.0"
edition = "2021"

[lib]
name = "generated_project"
path = "src/lib.rs"
"#,
        )
        .expect("write cargo toml");

        let spec_path = specs.join("app.md");
        fs::write(&spec_path, "# App\n\n## Behavior\n- Runs.\n").expect("write spec");

        let output = src.join("main.rs");
        fs::write(
            &output,
            r#"use generated_project::contexts::TerminalRenderer;

fn main() {
    let _ = core::mem::size_of::<TerminalRenderer>();
}"#,
        )
        .expect("write impl");

        let report = verify_generated_implementation(
            &root,
            &spec_path,
            &fs::read_to_string(&spec_path).unwrap(),
            &output,
        )
        .expect("verify");
        assert!(
            !report.high_risk_findings.iter().any(|item| {
                item.contains("external crate 'generated_project'")
                    || item.contains("not declared in the resolved dependency plan")
            }),
            "findings: {:?}",
            report.high_risk_findings
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn verifier_allows_used_immutable_transform_results() {
        let root = make_temp_dir("pipeline_quality_immutable_used");
        let specs = root.join("specifications").join("data");
        let src = root.join("src").join("data");
        fs::create_dir_all(&specs).expect("mkdir specs");
        fs::create_dir_all(&src).expect("mkdir src");

        let spec_path = specs.join("gamestate.md");
        fs::write(
            &spec_path,
            r#"# GameState

## Mutability
Immutable. All mutation-like operations return a new GameState rather than modifying the existing instance.

## Functionalities
- **place_food**
  Takes Some(food) or None and returns a new GameState with food updated.
"#,
        )
        .expect("write spec");

        let output = src.join("gamestate_usage.rs");
        fs::write(
            &output,
            r#"fn build(game_state: GameState, food: Option<Food>) -> GameState {
    let game_state = game_state.place_food(food);
    game_state
}"#,
        )
        .expect("write impl");

        let report = verify_generated_implementation(
            &root,
            &spec_path,
            &fs::read_to_string(&spec_path).unwrap(),
            &output,
        )
        .expect("verify");
        assert!(
            !report
                .high_risk_findings
                .iter()
                .any(|item| item.contains("place_food"))
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn env_var_extraction_ignores_casing_examples_without_env_context() {
        let content = r#"# CollisionType

## Implementation Choices Left Open
- Exact casing/style of variant names (e.g., `Obstacle` vs `OBSTACLE` vs `obstacle`) is non-blocking.
- JSON and ANSI output details are non-blocking.
"#;
        let report = analyze_specification(
            Path::new("specifications/data/CollisionType.md"),
            content,
            None,
        );
        assert!(report.contract.env_vars.is_empty());
    }

    #[test]
    fn env_var_extraction_uses_environment_context() {
        let content = r#"# App

## Behavior
- Read environment variable `SNAKE_RENDERER`
- If `SNAKE_RENDERER=string`, print `READY`
"#;
        let report = analyze_specification(Path::new("specifications/app.md"), content, None);
        assert_eq!(report.contract.env_vars, vec!["SNAKE_RENDERER".to_string()]);
    }

    #[test]
    fn collaborator_extraction_ignores_role_metadata_labels() {
        let content = r#"# Terminal Renderer

## Role Players

### `string_renderer`
- **Purpose**: Formats the current game frame as plain text.
- **Methods**:
  - `render(board, score)`
    - **Behavior**:
      - Produces a fully formatted frame string.
    - **Input**:
      - `board`: A 2D char grid.
    - **Output**: Returns a single string.
"#;
        let report = analyze_specification(
            Path::new("specifications/contexts/terminal_renderer.md"),
            content,
            None,
        );
        assert_eq!(
            report.contract.collaborators,
            vec!["string_renderer".to_string()]
        );
    }

    #[test]
    fn context_specs_with_role_players_tables_stay_contexts() {
        let content = r#"# CommandInputContext

## Purpose
Used for one shared input stream across the whole application session.

## Message Receiver
yes

## Role Players
| Role Player | Why Involved | Expected Behaviour |
|---|---|---|
| stdin_source | Supplies keyboard input to the context | Provides non-blocking reads from standard input |

## Role Methods
### stdin_source
- **read_available**
  Returns all currently available keystrokes in arrival order without blocking.

## Props
| Prop | Meaning | Notes |
|---|---|---|
| buffer | FIFO queue of captured keystrokes | Shared for the whole application session |

## Functionalities
### capture
- Reads available keys without blocking.
"#;
        let report = analyze_specification(
            Path::new("specifications/contexts/command_input.md"),
            content,
            None,
        );
        assert_eq!(report.contract.kind, SpecificationKind::Context);
        assert_eq!(
            report.contract.collaborators,
            vec!["stdin_source".to_string()]
        );
    }

    #[test]
    fn context_specs_produce_no_errors_without_message_receiver_section() {
        // Message Receiver is no longer part of the context schema; its absence is not an error.
        let content = r#"# TransferContext

## Purpose
Executes a money transfer.

## Role Players
| Role Player | Why Involved | Expected Behaviour |
|---|---|---|
| source | Funds provider | Can withdraw |

## Role Methods
### source
- **withdraw** Removes the specified amount from the source account.

## Props
| Prop | Meaning | Notes |
|---|---|---|
| amount | Transfer amount | Positive |

## Functionalities
### execute
| Started by | Uses | Result |
|---|---|---|
| caller | source | funds moved |

Rules:
- Amount must be positive.

| Given | When | Then |
|---|---|---|
| valid amount | execute runs | funds are moved |
"#;
        let report = analyze_specification(
            Path::new("specifications/contexts/transfer_context.md"),
            content,
            None,
        );
        assert!(
            report.errors.is_empty(),
            "unexpected errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn verifier_flags_immutable_kind_hidden_mutability_patterns() {
        let root = make_temp_dir("pipeline_quality_immutable_kind");
        let specs = root.join("specifications").join("projections");
        let src = root.join("src").join("projections");
        fs::create_dir_all(&specs).expect("mkdir specs");
        fs::create_dir_all(&src).expect("mkdir src");

        let spec_path = specs.join("projection_context.md");
        fs::write(
            &spec_path,
            r#"# ProjectionContext

## Purpose
Derived read model.

## Role Players
| Role Player | Why Involved | Expected Behaviour |
|---|---|---|
| ledger | Source data | Provides entries |

## Role Methods
### ledger
- **entries**
  Returns entries.

## Props
| Prop | Meaning | Notes |
|---|---|---|
| account_id | Target account | Stable |

## Functionalities
### refresh
- Returns a refreshed projection.
"#,
        )
        .expect("write spec");

        let output = src.join("projection_context.rs");
        fs::write(
            &output,
            r#"use std::cell::RefCell;
use std::thread;

pub struct ProjectionContext {
    cache: RefCell<Vec<String>>,
}

impl ProjectionContext {
    pub fn refresh(&mut self) {
        thread::spawn(|| {});
        self.cache.borrow_mut().push(String::new());
    }
}"#,
        )
        .expect("write impl");

        let report = verify_generated_implementation(
            &root,
            &spec_path,
            &fs::read_to_string(&spec_path).unwrap(),
            &output,
        )
        .expect("verify");
        assert!(
            report
                .high_risk_findings
                .iter()
                .any(|item| item.contains("public mutable receiver"))
        );
        assert!(
            report
                .high_risk_findings
                .iter()
                .any(|item| item.contains("projection_context.rs:")),
            "findings: {:?}",
            report.high_risk_findings
        );
        assert!(
            report
                .high_risk_findings
                .iter()
                .any(|item| item.contains("RefCell"))
        );
        assert!(
            report
                .high_risk_findings
                .iter()
                .any(|item| item.contains("thread::spawn"))
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn collaborator_extraction_reads_collaborators_and_wiring_table_rows() {
        let content = r#"# App

## Collaborators and Wiring
| Collaborator | Responsibility |
|---|---|
| `CommandInputContext` | Captures key presses into one shared FIFO stream |
| `GameLoopContext` | Holds the game rules and advances the game one tick at a time |
| `StringRenderer` | Formats the board and score into a plain-text frame string |
"#;
        let report = analyze_specification(Path::new("specifications/app.md"), content, None);
        assert_eq!(
            report.contract.collaborators,
            vec![
                "CommandInputContext".to_string(),
                "GameLoopContext".to_string(),
                "StringRenderer".to_string()
            ]
        );
    }

    #[test]
    fn determines_spec_path_for_main_output() {
        let specs = Path::new("/tmp/example/specifications");
        let output = Path::new("/tmp/example/src/main.rs");
        let candidate = determine_spec_path_for_output(output, specs);
        assert_eq!(candidate, None);
    }

    #[test]
    fn semantic_regression_reports_worsening() {
        let before = super::StaticBehaviorVerifierReport {
            contract: super::BehaviorContract {
                title: "App".to_string(),
                kind: SpecificationKind::App,
                source_path: "specifications/app.md".to_string(),
                collaborators: Vec::new(),
                env_vars: Vec::new(),
                delegation_requirements: Vec::new(),
                output_requirements: Vec::new(),
                shared_state_requirements: Vec::new(),
                role_method_names: Vec::new(),
                external_behavior_clues: Vec::new(),
            },
            output_path: "src/main.rs".to_string(),
            errors: Vec::new(),
            warnings: Vec::new(),
            high_risk_findings: Vec::new(),
            evidence: Vec::new(),
        };
        let mut after = before.clone();
        after.errors.push("missing env".to_string());
        let regression = compare_verifier_reports(before, after);
        assert!(regression.worsened);
        assert!(!regression.issues.is_empty());
    }

    fn make_temp_dir(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("{}_{}", prefix, nanos));
        fs::create_dir_all(&dir).expect("mkdir temp");
        dir
    }
}
