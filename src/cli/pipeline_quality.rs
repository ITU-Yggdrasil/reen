use anyhow::{Context, Result};
use regex::Regex;
use serde::Serialize;
use serde_json::json;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) enum SpecificationKind {
    App,
    Context,
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

    let mut warnings = Vec::new();
    for artifact in &required_artifacts {
        if !artifact.exists {
            warnings.push(format!(
                "Collaborator '{}' does not have a matching generated artifact yet",
                artifact.collaborator
            ));
        }
    }

    ImplementationPlanReport {
        contract,
        output_path: output_path.display().to_string(),
        required_artifacts,
        verification_targets,
        warnings,
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

    for env_var in &plan.contract.env_vars {
        let satisfied = code.contains(env_var)
            && (code.contains("env::var")
                || code.contains("std::env::var")
                || code.contains("var_os"));
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
        let satisfied = code.contains(collaborator);
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
        let satisfied = code.contains(&requirement.actor) && code.contains(&requirement.target);
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
        let satisfied = code.contains(&requirement.literal);
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

    for artifact in &plan.required_artifacts {
        if !artifact.exists {
            errors.push(format!(
                "Required collaborator artifact for '{}' is missing (checked: {})",
                artifact.collaborator,
                artifact.candidate_paths.join(", ")
            ));
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
        if pattern.is_match(&code) {
            if behavior_sensitive {
                high_risk_findings.push(format!(
                    "{} in behavior-sensitive implementation {}",
                    message,
                    output_path.display()
                ));
            } else {
                warnings.push(message.to_string());
            }
        }
    }
    for finding in detect_trivial_obligation_stubs(&code, &plan.contract.role_method_names) {
        if behavior_sensitive {
            high_risk_findings.push(format!(
                "{} in behavior-sensitive implementation {}",
                finding,
                output_path.display()
            ));
        } else {
            warnings.push(finding);
        }
    }
    for finding in detect_ignored_immutable_return_values(spec_content, &code) {
        high_risk_findings.push(format!(
            "{} in {}",
            finding,
            output_path.display()
        ));
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

fn detect_trivial_obligation_stubs(code: &str, role_method_names: &[String]) -> Vec<String> {
    if role_method_names.is_empty() {
        return Vec::new();
    }

    let fn_re = Regex::new(r"(?s)fn\s+([A-Za-z0-9_]+)[^{]*\{([^{}]*)\}").unwrap();
    let vec_new_re = Regex::new(r"^Vec(?:::<[^>]+>)?::new\s*\(\)\s*;?$").unwrap();
    let string_new_re = Regex::new(r"^String::new\s*\(\)\s*;?$").unwrap();
    let none_re = Regex::new(r"^(?:return\s+)?None\s*;?$").unwrap();

    let normalized_role_names = role_method_names
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .collect::<Vec<_>>();
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
            findings.push(message);
        }
    }

    findings
}

fn detect_ignored_immutable_return_values(spec_content: &str, code: &str) -> Vec<String> {
    if !spec_declares_immutable_value_updates(spec_content) {
        return Vec::new();
    }

    let methods = extract_immutable_transform_method_names(spec_content);
    if methods.is_empty() {
        return Vec::new();
    }

    let mut findings = Vec::new();
    for line in code.lines() {
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
                findings.push(format!(
                    "Immutable transform method '{}' appears to have its return value ignored",
                    method
                ));
            }
        }
    }

    findings.sort();
    findings.dedup();
    findings
}

fn spec_declares_immutable_value_updates(spec_content: &str) -> bool {
    let sections = parse_markdown_sections(spec_content);
    if let Some(section) = find_section(&sections, "Mutability") {
        if section.body.to_ascii_lowercase().contains("immutable") {
            return true;
        }
    }
    spec_content
        .to_ascii_lowercase()
        .contains("returns a new")
}

fn extract_immutable_transform_method_names(spec_content: &str) -> Vec<String> {
    let sections = parse_markdown_sections(spec_content);
    let Some(section) = find_section(&sections, "Functionalities") else {
        return Vec::new();
    };

    let mut methods = Vec::new();
    let mut current_method: Option<String> = None;
    let mut current_body = Vec::new();

    let flush =
        |methods: &mut Vec<String>, current_method: &mut Option<String>, current_body: &mut Vec<String>| {
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

fn extract_behavior_contract(spec_path: &Path, spec_content: &str) -> BehaviorContract {
    let title = spec_content
        .lines()
        .find_map(|line| line.trim().strip_prefix("# ").map(str::trim))
        .unwrap_or("")
        .to_string();
    let sections = parse_markdown_sections(spec_content);
    let kind = infer_kind(spec_path, spec_content, &sections);
    let collaborators = extract_collaborators(spec_content, &sections);
    let env_vars = extract_env_vars(spec_content);
    let delegation_requirements = extract_delegation_requirements(spec_content);
    let output_requirements = extract_output_requirements(spec_content);
    let shared_state_requirements = extract_shared_state_requirements(spec_content);
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

fn infer_kind(spec_path: &Path, spec_content: &str, sections: &[Section]) -> SpecificationKind {
    let path = spec_path.to_string_lossy().to_ascii_lowercase();
    if path.ends_with("/app.md") {
        return SpecificationKind::App;
    }

    if has_any_section(sections, &["Purpose", "Role Players", "Role Methods", "Props"]) {
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
        if let Some(title) = line.trim().strip_prefix("## ") {
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

fn extract_collaborators(content: &str, sections: &[Section]) -> Vec<String> {
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

fn extract_delegation_requirements(content: &str) -> Vec<DelegationRequirement> {
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
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        if ids.len() >= 2 {
            requirements.push(DelegationRequirement {
                actor: ids[0].clone(),
                target: ids[1].clone(),
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
    use super::{
        SpecificationKind, analyze_specification, compare_verifier_reports,
        determine_spec_path_for_output, verify_generated_implementation,
    };
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

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
