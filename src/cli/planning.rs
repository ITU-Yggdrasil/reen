use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::pipeline_quality::{
    BehaviorContract, RequiredArtifact, SpecificationKind, build_implementation_plan,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PlanKind {
    Implementation,
    SemanticRepair,
}

impl PlanKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Implementation => "implementation",
            Self::SemanticRepair => "semantic_repair",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SharingConstraint {
    pub(crate) subject: String,
    pub(crate) identity_semantics: String,
    pub(crate) mutation_semantics: String,
    pub(crate) rust_guidance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct PlanTask {
    pub(crate) order: usize,
    pub(crate) title: String,
    pub(crate) detail: String,
    pub(crate) target_paths: Vec<String>,
    pub(crate) verification_targets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ExecutionPlan {
    pub(crate) plan_kind: PlanKind,
    pub(crate) target_spec_path: String,
    pub(crate) target_output_paths: Vec<String>,
    pub(crate) title: String,
    pub(crate) required_behaviors: Vec<String>,
    pub(crate) required_collaborators: Vec<String>,
    pub(crate) cross_component_integrations: Vec<String>,
    pub(crate) identity_and_sharing_constraints: Vec<SharingConstraint>,
    pub(crate) ordered_tasks: Vec<PlanTask>,
    pub(crate) verification_targets: Vec<String>,
    pub(crate) risks: Vec<String>,
    pub(crate) forbidden_regressions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct PlanValidationReport {
    pub(crate) ok: bool,
    pub(crate) errors: Vec<String>,
    pub(crate) warnings: Vec<String>,
}

pub(crate) fn build_default_plan(
    plan_kind: PlanKind,
    spec_path: &Path,
    spec_content: &str,
    output_paths: &[PathBuf],
    dependency_context: &HashMap<String, serde_json::Value>,
    diagnostic_text: Option<&str>,
) -> ExecutionPlan {
    let normalized_output_paths = if output_paths.is_empty() {
        vec![PathBuf::from("src/lib.rs")]
    } else {
        output_paths.to_vec()
    };
    let base_report =
        build_implementation_plan(spec_path, spec_content, &normalized_output_paths[0], Path::new("."));
    let contract = &base_report.contract;

    let mut required_behaviors = Vec::new();
    required_behaviors.extend(contract.external_behavior_clues.iter().cloned());
    required_behaviors.extend(
        contract
            .output_requirements
            .iter()
            .map(|req| format!("emit {}", req.literal)),
    );
    required_behaviors.extend(
        contract
            .env_vars
            .iter()
            .map(|name| format!("read env {}", name)),
    );
    dedupe_preserve(&mut required_behaviors);

    let cross_component_integrations = contract
        .delegation_requirements
        .iter()
        .map(|req| format!("{} -> {}", req.actor, req.target))
        .collect::<Vec<_>>();

    let identity_and_sharing_constraints =
        build_sharing_constraints(contract, dependency_context, plan_kind);

    let mut verification_targets = base_report
        .verification_targets
        .iter()
        .map(|target| format!("{}: {}", target.kind, target.detail))
        .collect::<Vec<_>>();
    if let Some(diag) = diagnostic_text {
        if !diag.trim().is_empty() {
            verification_targets.push("diagnostics resolved".to_string());
        }
    }
    dedupe_preserve(&mut verification_targets);

    let mut ordered_tasks = build_tasks(
        plan_kind,
        spec_path,
        &normalized_output_paths,
        contract,
        &verification_targets,
        &cross_component_integrations,
        diagnostic_text,
    );

    if ordered_tasks.is_empty() {
        ordered_tasks.push(PlanTask {
            order: 1,
            title: "Implement contract".to_string(),
            detail: "Satisfy the specification contract using the provided collaborators and outputs."
                .to_string(),
            target_paths: output_paths
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
            verification_targets: verification_targets.clone(),
        });
    }

    let mut risks = build_risks(contract, &base_report.required_artifacts, plan_kind, diagnostic_text);
    dedupe_preserve(&mut risks);
    let mut forbidden_regressions = build_forbidden_regressions(contract, plan_kind);
    dedupe_preserve(&mut forbidden_regressions);

    ExecutionPlan {
        plan_kind,
        target_spec_path: spec_path.display().to_string(),
        target_output_paths: normalized_output_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect(),
        title: if contract.title.is_empty() {
            spec_path
                .file_stem()
                .and_then(|v| v.to_str())
                .unwrap_or("Plan")
                .to_string()
        } else {
            contract.title.clone()
        },
        required_behaviors,
        required_collaborators: contract.collaborators.clone(),
        cross_component_integrations,
        identity_and_sharing_constraints,
        ordered_tasks,
        verification_targets,
        risks,
        forbidden_regressions,
    }
}

pub(crate) fn parse_plan_output(output: &str) -> Result<ExecutionPlan> {
    let candidate = extract_json_object(output)
        .ok_or_else(|| anyhow::anyhow!("Planning agent did not return a JSON object"))?;
    serde_json::from_str::<ExecutionPlan>(&candidate)
        .context("Planning agent output was not valid plan JSON")
}

pub(crate) fn validate_plan(
    plan: &ExecutionPlan,
    contract: &BehaviorContract,
    output_paths: &[PathBuf],
) -> PlanValidationReport {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if plan.target_spec_path.trim().is_empty() {
        errors.push("Plan is missing target_spec_path".to_string());
    }
    if plan.target_output_paths.is_empty() {
        errors.push("Plan is missing target_output_paths".to_string());
    }
    if plan.ordered_tasks.is_empty() {
        errors.push("Plan does not contain any ordered tasks".to_string());
    }
    if plan.verification_targets.is_empty() {
        warnings.push("Plan does not declare any verification targets".to_string());
    }

    for collaborator in &contract.collaborators {
        if !plan.required_collaborators.contains(collaborator) {
            warnings.push(format!(
                "Plan does not explicitly include collaborator '{}'",
                collaborator
            ));
        }
    }

    for expected in output_paths {
        let rendered = expected.display().to_string();
        if !plan.target_output_paths.contains(&rendered) {
            errors.push(format!(
                "Plan does not target expected output path '{}'",
                rendered
            ));
        }
    }

    for shared in &contract.shared_state_requirements {
        if !plan
            .identity_and_sharing_constraints
            .iter()
            .any(|constraint| constraint.rust_guidance.contains(shared))
        {
            warnings.push(format!(
                "Plan may be missing explicit handling for shared-state requirement '{}'",
                shared
            ));
        }
    }

    PlanValidationReport {
        ok: errors.is_empty(),
        errors,
        warnings,
    }
}

pub(crate) fn plan_to_context_value(plan: &ExecutionPlan) -> serde_json::Value {
    json!(plan)
}

pub(crate) fn validation_to_context_value(report: &PlanValidationReport) -> serde_json::Value {
    json!(report)
}

fn build_sharing_constraints(
    contract: &BehaviorContract,
    dependency_context: &HashMap<String, serde_json::Value>,
    plan_kind: PlanKind,
) -> Vec<SharingConstraint> {
    let mut constraints = Vec::new();
    let shared_required = !contract.shared_state_requirements.is_empty();
    for collaborator in &contract.collaborators {
        let guidance = if shared_required {
            format!(
                "Preserve logical sharing/identity when handling {}. {}",
                collaborator,
                contract.shared_state_requirements.join(" ")
            )
        } else if matches!(contract.kind, SpecificationKind::Data) {
            format!(
                "Prefer immutable value-style handling for data-like collaborator {}.",
                collaborator
            )
        } else {
            format!(
                "Choose the most idiomatic ownership model for {} consistent with the specification.",
                collaborator
            )
        };
        let identity_semantics = if shared_required {
            "shared_identity".to_string()
        } else if collaborator_is_dependency(collaborator, dependency_context) {
            "owned".to_string()
        } else {
            "replaceable".to_string()
        };
        let mutation_semantics = if matches!(contract.kind, SpecificationKind::Data) {
            "immutable".to_string()
        } else if shared_required && matches!(plan_kind, PlanKind::SemanticRepair) {
            "preserve_existing".to_string()
        } else {
            "infer_from_behavior".to_string()
        };
        constraints.push(SharingConstraint {
            subject: collaborator.clone(),
            identity_semantics,
            mutation_semantics,
            rust_guidance: guidance,
        });
    }
    constraints
}

fn collaborator_is_dependency(
    collaborator: &str,
    dependency_context: &HashMap<String, serde_json::Value>,
) -> bool {
    for key in ["direct_dependencies", "dependency_closure", "implemented_dependencies"] {
        if let Some(values) = dependency_context.get(key).and_then(|value| value.as_array()) {
            for item in values {
                if item
                    .get("name")
                    .and_then(|value| value.as_str())
                    .map(|name| name == collaborator)
                    .unwrap_or(false)
                {
                    return true;
                }
            }
        }
    }
    false
}

fn build_tasks(
    plan_kind: PlanKind,
    spec_path: &Path,
    output_paths: &[PathBuf],
    contract: &BehaviorContract,
    verification_targets: &[String],
    cross_component_integrations: &[String],
    diagnostic_text: Option<&str>,
) -> Vec<PlanTask> {
    let mut tasks = Vec::new();
    let rendered_targets = output_paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();

    match plan_kind {
        PlanKind::Implementation => {
            tasks.push(PlanTask {
                order: 1,
                title: "Map required behaviors".to_string(),
                detail: format!(
                    "Translate the contract from {} into concrete implementation work without changing observable behavior.",
                    spec_path.display()
                ),
                target_paths: rendered_targets.clone(),
                verification_targets: verification_targets.to_vec(),
            });
            if !contract.collaborators.is_empty() {
                tasks.push(PlanTask {
                    order: 2,
                    title: "Wire collaborators".to_string(),
                    detail: format!(
                        "Integrate collaborators: {}.",
                        contract.collaborators.join(", ")
                    ),
                    target_paths: rendered_targets.clone(),
                    verification_targets: cross_component_integrations.to_vec(),
                });
            }
            tasks.push(PlanTask {
                order: 3,
                title: "Verify behavior".to_string(),
                detail: "Confirm the generated code satisfies the declared outputs, integration points, and verifier targets.".to_string(),
                target_paths: rendered_targets,
                verification_targets: verification_targets.to_vec(),
            });
        }
        PlanKind::SemanticRepair => {
            tasks.push(PlanTask {
                order: 1,
                title: "Diagnose minimal fix scope".to_string(),
                detail: diagnostic_text
                    .map(|text| format!("Use the reported diagnostics as the minimal repair scope. {}", summarize_diagnostics(text)))
                    .unwrap_or_else(|| "Use the reported diagnostics as the minimal repair scope.".to_string()),
                target_paths: rendered_targets.clone(),
                verification_targets: verification_targets.to_vec(),
            });
            tasks.push(PlanTask {
                order: 2,
                title: "Preserve semantic invariants".to_string(),
                detail: "Repair compilation or integration issues without removing required collaborators, outputs, or shared-state semantics.".to_string(),
                target_paths: rendered_targets.clone(),
                verification_targets: cross_component_integrations.to_vec(),
            });
            tasks.push(PlanTask {
                order: 3,
                title: "Re-run behavioral verification".to_string(),
                detail: "Ensure the repaired code does not regress required behavior or verifier obligations.".to_string(),
                target_paths: rendered_targets,
                verification_targets: verification_targets.to_vec(),
            });
        }
    }
    tasks
}

fn summarize_diagnostics(text: &str) -> String {
    text.lines()
        .take(3)
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn build_risks(
    contract: &BehaviorContract,
    required_artifacts: &[RequiredArtifact],
    plan_kind: PlanKind,
    diagnostic_text: Option<&str>,
) -> Vec<String> {
    let mut risks = Vec::new();
    for artifact in required_artifacts {
        if !artifact.exists {
            risks.push(format!(
                "Collaborator artifact '{}' is not currently present in the generated source tree.",
                artifact.collaborator
            ));
        }
    }
    if !contract.shared_state_requirements.is_empty() {
        risks.push("Shared identity semantics are required; careless cloning may change behavior.".to_string());
    }
    if !contract.env_vars.is_empty() {
        risks.push("Environment/config behavior is specified and must not be omitted during implementation or repair.".to_string());
    }
    if matches!(plan_kind, PlanKind::SemanticRepair)
        && diagnostic_text.map(|text| !text.trim().is_empty()).unwrap_or(false)
    {
        risks.push("Semantic repair must stay within the minimal diagnostic scope and preserve behavior.".to_string());
    }
    risks
}

fn build_forbidden_regressions(contract: &BehaviorContract, plan_kind: PlanKind) -> Vec<String> {
    let mut regressions = Vec::new();
    if !contract.collaborators.is_empty() {
        regressions.push(format!(
            "Do not remove required collaborator usage: {}.",
            contract.collaborators.join(", ")
        ));
    }
    if !contract.output_requirements.is_empty() {
        regressions.push(format!(
            "Do not remove required outputs: {}.",
            contract
                .output_requirements
                .iter()
                .map(|req| req.literal.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !contract.shared_state_requirements.is_empty() {
        regressions.push("Do not break required shared-state or stable-identity behavior.".to_string());
    }
    if matches!(plan_kind, PlanKind::SemanticRepair) {
        regressions.push("Do not replace required behavior with stubs, placeholders, or compile-only shells.".to_string());
    }
    regressions
}

fn dedupe_preserve(values: &mut Vec<String>) {
    let mut seen = HashMap::new();
    values.retain(|value| seen.insert(value.clone(), ()).is_none());
}

fn extract_json_object(output: &str) -> Option<String> {
    let fenced = Regex::new(r"(?s)```json\s*(\{.*\})\s*```").ok();
    if let Some(re) = fenced {
        if let Some(captures) = re.captures(output) {
            if let Some(matched) = captures.get(1) {
                return Some(matched.as_str().trim().to_string());
            }
        }
    }

    let trimmed = output.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return Some(trimmed.to_string());
    }

    let start = output.find('{')?;
    let end = output.rfind('}')?;
    if start < end {
        Some(output[start..=end].trim().to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{PlanKind, build_default_plan, parse_plan_output, validate_plan};
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    #[test]
    fn parses_plan_output_from_json_fence() {
        let output = r#"```json
{"plan_kind":"implementation","target_spec_path":"specifications/app.md","target_output_paths":["src/main.rs"],"title":"App","required_behaviors":["do thing"],"required_collaborators":["Renderer"],"cross_component_integrations":["TerminalRenderer -> StringRenderer"],"identity_and_sharing_constraints":[{"subject":"Renderer","identity_semantics":"owned","mutation_semantics":"infer_from_behavior","rust_guidance":"Prefer owned."}],"ordered_tasks":[{"order":1,"title":"Task","detail":"Do it","target_paths":["src/main.rs"],"verification_targets":["collaborator: Renderer"]}],"verification_targets":["collaborator: Renderer"],"risks":["risk"],"forbidden_regressions":["no stub"]}
```"#;
        let plan = parse_plan_output(output).expect("parse plan");
        assert_eq!(plan.plan_kind, PlanKind::Implementation);
        assert_eq!(plan.target_output_paths, vec!["src/main.rs".to_string()]);
    }

    #[test]
    fn default_plan_captures_shared_constraints() {
        let content = r#"# App

## Collaborators
- **CommandInputContext**

## Behavior
- Keep the same shared input stream for the process lifetime.
"#;
        let plan = build_default_plan(
            PlanKind::Implementation,
            Path::new("specifications/app.md"),
            content,
            &[PathBuf::from("src/main.rs")],
            &HashMap::new(),
            None,
        );
        assert!(!plan.identity_and_sharing_constraints.is_empty());
        assert!(plan
            .identity_and_sharing_constraints
            .iter()
            .any(|item| item.identity_semantics == "shared_identity"));
    }

    #[test]
    fn validation_requires_tasks_and_outputs() {
        let plan = build_default_plan(
            PlanKind::Implementation,
            Path::new("specifications/app.md"),
            "# App",
            &[PathBuf::from("src/main.rs")],
            &HashMap::new(),
            None,
        );
        let contract = super::super::pipeline_quality::analyze_specification(
            Path::new("specifications/app.md"),
            "# App",
            None,
        )
        .contract;
        let report = validate_plan(&plan, &contract, &[PathBuf::from("src/main.rs")]);
        assert!(report.ok);
    }
}
