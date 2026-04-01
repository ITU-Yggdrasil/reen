use anyhow::{Context, Result};
use chrono::Utc;
use reen::execution::estimate_request_tokens;
use regex::Regex;
use serde::Serialize;
use serde_json::json;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use super::Config;
use super::agent_executor::{AgentExecutor, AgentResponse};
use super::contracts::{
    build_contract_artifact, compact_contract_artifact_value, contract_artifact_to_context_value,
    contract_validation_to_context_value, validate_contract_artifact,
};
use super::patch_service::{
    HunkLineKind, apply_unified_diff, extract_unified_diff_from_agent_output, parse_unified_diff,
    validate_unified_diff,
};
use super::pipeline_quality::{
    analyze_specification, compare_verifier_reports, contract_to_context_value,
    determine_spec_path_for_output, verify_generated_implementation,
};
use super::planning::{PlanKind, build_default_plan, parse_plan_output, plan_to_context_value};
use super::progress::print_timed_status;
use super::project_structure::ProjectInfo;
use reen::execution::NativeExecutionControl;

/// Configuration for compilation fix context size, read from env vars:
/// - REEN_COMPILE_FIX_MAX_ERRORS (default 10): max errors per agent round
/// - REEN_COMPILE_FIX_MAX_TOKENS (default 120000): token budget for context
/// - REEN_COMPILE_FIX_SNIPPET_LINES (default 20): lines around each error span
/// - REEN_COMPILE_FIX_MAX_FILES (default 20): max files to include
fn compile_fix_config() -> CompileFixConfig {
    CompileFixConfig {
        max_errors: env::var("REEN_COMPILE_FIX_MAX_ERRORS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(10),
        max_tokens: env::var("REEN_COMPILE_FIX_MAX_TOKENS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(120_000),
        snippet_lines: env::var("REEN_COMPILE_FIX_SNIPPET_LINES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(20),
        max_files: env::var("REEN_COMPILE_FIX_MAX_FILES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(20),
    }
}

#[derive(Debug, Clone)]
struct CompileFixConfig {
    max_errors: usize,
    max_tokens: usize,
    snippet_lines: usize,
    max_files: usize,
}

#[derive(Debug, Clone)]
struct CompileFixRoundContext {
    truncated_stderr: String,
    diagnostics: Vec<DiagnosticSpan>,
    relevant_paths: Vec<PathBuf>,
    specs_json: BTreeMap<String, String>,
    additional_context: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticSpan {
    pub file: String,
    pub line: u32,
    pub col: u32,
    pub code: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CompilationOutput {
    pub status_success: bool,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone)]
pub struct GuardrailReport {
    pub ok: bool,
    pub issues: Vec<String>,
    pub touched_files: Vec<String>,
    pub touches_cargo_toml: bool,
    pub adds_files: Vec<String>,
    pub modifies_public_fn_lines: bool,
    pub adds_stub_macros: bool,
    pub deleted_files: Vec<String>,
    pub total_deleted_lines: usize,
}

pub async fn ensure_compiles_with_auto_fix(
    config: &Config,
    max_attempts: usize,
    project_root: &Path,
    artifact_root: &Path,
    project_info: &ProjectInfo,
    recent_generated_files: &[PathBuf],
    request_token_limit: Option<f64>,
    ignore_cache_reads: bool,
    execution_control: Option<&dyn NativeExecutionControl>,
) -> Result<()> {
    if config.dry_run {
        return Ok(());
    }

    let mut output = run_cargo_build(project_root)?;
    if output.status_success {
        if config.verbose {
            println!("✓ Build successful");
        }
        return Ok(());
    }

    if let Some(message) = explicit_implementation_failure_message_from_stderr(&output.stderr) {
        anyhow::bail!(
            "Build failed because generated implementation reported a specification blocker. Automatic compilation fixes will not override that refusal.\n{}",
            message
        );
    }

    let session_dir = create_session_dir(project_root)?;
    let session_dir_display = session_dir.display().to_string();
    eprintln!(
        "error[compile]: build failed; attempting automatic compilation fixes (max_attempts={}). Logs: {}",
        max_attempts, session_dir_display
    );

    let cfg = compile_fix_config();
    let request_budget = effective_request_token_budget(cfg.max_tokens, request_token_limit);

    for attempt in 1..=max_attempts {
        let attempt_dir = session_dir.join(format!("attempt_{}", attempt));
        fs::create_dir_all(&attempt_dir)
            .with_context(|| format!("Failed to create {}", attempt_dir.display()))?;
        fs::write(
            attempt_dir.join("request_budget.txt"),
            format!("budget={request_budget}\n"),
        )
        .ok();

        write_attempt_compile_output(&attempt_dir, &output)?;

        let all_diagnostics = parse_rustc_diagnostics(&output.stderr);
        let mut max_paths = Some(cfg.max_files);
        let mut snippet_lines = cfg.snippet_lines;
        let mut max_errors_used = cfg.max_errors;
        let mut include_specs = true;

        let mut round_context = loop {
            let diagnostics = prioritize_and_cap_diagnostics(&all_diagnostics, max_errors_used);
            let truncated_stderr = truncate_stderr_for_diagnostics(&output.stderr, &diagnostics);

            let relevant_paths = collect_relevant_paths(
                project_root,
                &diagnostics,
                &output.stderr,
                recent_generated_files,
                max_paths,
            )?;

            let files_json = snapshot_files_json_snippets(
                project_root,
                &relevant_paths,
                &diagnostics,
                snippet_lines,
            )?;
            let specs_json = if include_specs {
                snapshot_specs_json(project_root, artifact_root, &relevant_paths)?
            } else {
                BTreeMap::new()
            };

            let ctx = build_agent_context(
                &output,
                &truncated_stderr,
                &diagnostics,
                &files_json,
                &specs_json,
                recent_generated_files,
                project_info,
            )?;

            let estimated =
                estimate_request_tokens("Compilation failed; propose minimal fix patch.", &ctx);
            if estimated <= request_budget {
                break CompileFixRoundContext {
                    truncated_stderr,
                    diagnostics,
                    relevant_paths,
                    specs_json,
                    additional_context: ctx,
                };
            }
            if max_errors_used > 3 {
                max_errors_used = max_errors_used / 2;
            } else if snippet_lines > 5 {
                snippet_lines = snippet_lines / 2;
            } else if include_specs {
                include_specs = false;
            } else if max_paths.map(|m| m > 1).unwrap_or(false) {
                max_paths = max_paths.map(|m| (m / 2).max(1));
            } else {
                anyhow::bail!(
                    "Compilation fix context exceeds token budget ({} > {}). \
                     Try REEN_COMPILE_FIX_MAX_TOKENS or fix errors manually. Logs: {}",
                    estimated,
                    request_budget,
                    attempt_dir.display()
                );
            }
        };

        let plan_diagnostics = prioritize_and_cap_diagnostics(&all_diagnostics, max_errors_used);
        let plan_truncated_stderr =
            truncate_stderr_for_diagnostics(&output.stderr, &plan_diagnostics);
        let plan_relevant_paths = collect_relevant_paths(
            project_root,
            &plan_diagnostics,
            &output.stderr,
            recent_generated_files,
            max_paths,
        )?;
        let plan_specs_json = if include_specs {
            snapshot_specs_json(project_root, artifact_root, &plan_relevant_paths)?
        } else {
            BTreeMap::new()
        };
        let semantic_repair_plan = generate_semantic_repair_plan(
            config,
            &plan_relevant_paths,
            &plan_specs_json,
            &plan_truncated_stderr,
            &plan_diagnostics,
            request_token_limit,
            ignore_cache_reads,
            execution_control,
        )
        .await?;
        round_context.additional_context.insert(
            "semantic_repair_plan".to_string(),
            semantic_repair_plan.clone(),
        );

        if let Some(serde_json::Value::String(s)) =
            round_context.additional_context.get("diagnostics_json")
        {
            fs::write(attempt_dir.join("diagnostics.json"), s).ok();
        }
        if let Some(serde_json::Value::String(s)) =
            round_context.additional_context.get("files_json")
        {
            fs::write(attempt_dir.join("context_files.json"), s).ok();
        }
        if let Some(serde_json::Value::String(s)) =
            round_context.additional_context.get("specs_json")
        {
            if !s.is_empty() && s != "null" {
                fs::write(attempt_dir.join("context_specs.json"), s).ok();
            }
        }
        fs::write(
            attempt_dir.join("semantic_repair_plan.json"),
            serde_json::to_string_pretty(&semantic_repair_plan)
                .unwrap_or_else(|_| "{}".to_string()),
        )
        .ok();

        let executor = AgentExecutor::new("resolve_compilation_errors", config)
            .context("Failed to create compilation error resolver agent")?;

        let agent_response = executor
            .execute_with_context_options(
                "Compilation failed; propose minimal fix patch.",
                round_context.additional_context.clone(),
                execution_control,
                ignore_cache_reads,
            )
            .await
            .context("Failed to execute compilation error resolver agent")?;

        let patch_text = match agent_response {
            AgentResponse::Final(s) => s,
            AgentResponse::Questions(q) => {
                fs::write(attempt_dir.join("agent_questions.txt"), &q).ok();
                anyhow::bail!(
                    "Compilation resolver requested clarification; escalating. See: {}",
                    attempt_dir.display()
                );
            }
        };

        fs::write(attempt_dir.join("proposed.patch"), &patch_text).ok();

        let mut extracted = extract_unified_diff_from_agent_output(&patch_text)
            .context("Resolver output did not contain a unified diff starting with 'diff --git'")?;

        let mut guardrail = check_guardrails(project_root, artifact_root, &extracted)?;
        fs::write(
            attempt_dir.join("guardrail_report.json"),
            serde_json::to_string_pretty(&guardrail_to_json(&guardrail))
                .unwrap_or_else(|_| "{}".to_string()),
        )
        .ok();

        if !guardrail.ok {
            anyhow::bail!(
                "Auto-fix blocked by guardrails:\n{}\nEscalating. Logs: {}",
                guardrail.issues.join("\n"),
                attempt_dir.display()
            );
        }

        let mut touched_paths = parse_unified_diff(&extracted)
            .context("Invalid unified diff from compilation resolver")?
            .into_iter()
            .filter_map(|fp| fp.new_path.or(fp.old_path))
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        let validation_result = validate_unified_diff(project_root, &extracted);
        if let Err(apply_err) = &validation_result {
            fs::write(
                attempt_dir.join("patch_validation_error.txt"),
                format!("{apply_err:#}"),
            )
            .ok();
        }
        match validation_result {
            Ok(()) => {}
            Err(apply_err) if patch_apply_error_requires_regeneration(&apply_err) => {
                let retry_patch = regenerate_patch_after_apply_failure(
                    &executor,
                    project_root,
                    &output,
                    &round_context,
                    recent_generated_files,
                    project_info,
                    &touched_paths,
                    &extracted,
                    &apply_err,
                    &attempt_dir,
                    request_token_limit,
                    ignore_cache_reads,
                    execution_control,
                )
                .await?;
                fs::write(attempt_dir.join("proposed_retry.patch"), &retry_patch).ok();

                extracted = extract_unified_diff_from_agent_output(&retry_patch).context(
                    "Regenerated resolver output did not contain a unified diff starting with 'diff --git'",
                )?;
                guardrail = check_guardrails(project_root, artifact_root, &extracted)?;
                fs::write(
                    attempt_dir.join("guardrail_retry_report.json"),
                    serde_json::to_string_pretty(&guardrail_to_json(&guardrail))
                        .unwrap_or_else(|_| "{}".to_string()),
                )
                .ok();

                if !guardrail.ok {
                    anyhow::bail!(
                        "Auto-fix blocked by guardrails after patch regeneration:\n{}\nEscalating. Logs: {}",
                        guardrail.issues.join("\n"),
                        attempt_dir.display()
                    );
                }

                touched_paths = parse_unified_diff(&extracted)
                    .context("Invalid regenerated unified diff from compilation resolver")?
                    .into_iter()
                    .filter_map(|fp| fp.new_path.or(fp.old_path))
                    .map(PathBuf::from)
                    .collect::<Vec<_>>();
                validate_unified_diff(project_root, &extracted)
                    .context("Failed to apply regenerated patch")?;
            }
            Err(apply_err) => {
                return Err(apply_err).context("Failed to apply proposed patch");
            }
        }

        let semantic_baseline = capture_verifier_reports(
            project_root,
            artifact_root,
            &touched_paths,
            &attempt_dir.join("semantic_before"),
        )?;
        let backups = snapshot_file_backups(project_root, &touched_paths)?;

        let applied_patch = apply_unified_diff(project_root, &extracted)
            .context("Failed to apply proposed patch")?;
        fs::write(attempt_dir.join("applied.patch"), &applied_patch).ok();

        let semantic_after = capture_verifier_reports(
            project_root,
            artifact_root,
            &touched_paths,
            &attempt_dir.join("semantic_after"),
        )?;
        let regressions = compare_semantic_reports(semantic_baseline, semantic_after);
        if !regressions.is_empty() {
            restore_file_backups(project_root, backups)?;
            fs::write(
                attempt_dir.join("semantic_regressions.txt"),
                regressions.join("\n"),
            )
            .ok();
            anyhow::bail!(
                "Compilation fix introduced semantic regressions:\n{}\nEscalating. Logs: {}",
                regressions.join("\n"),
                attempt_dir.display()
            );
        }

        output = run_cargo_build(project_root)?;
        if output.status_success {
            fs::write(attempt_dir.join("cargo_stdout_after.txt"), &output.stdout).ok();
            fs::write(attempt_dir.join("cargo_stderr_after.txt"), &output.stderr).ok();
            println!(
                "✓ Build restored after {} compilation fix attempt(s). Logs: {}",
                attempt, session_dir_display
            );
            return Ok(());
        }

        fs::write(attempt_dir.join("cargo_stdout_after.txt"), &output.stdout).ok();
        fs::write(attempt_dir.join("cargo_stderr_after.txt"), &output.stderr).ok();
    }

    anyhow::bail!(
        "Compilation still failing after {} attempt(s). Escalating to human review. Logs: {}",
        max_attempts,
        session_dir_display
    );
}

fn patch_apply_error_requires_regeneration(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        let msg = cause.to_string();
        msg.contains("Could not locate hunk context")
            || msg.contains("Context mismatch at pos")
            || msg.contains("Remove mismatch at pos")
            || msg.contains("Context line beyond EOF")
            || msg.contains("Remove line beyond EOF")
    })
}

fn build_patch_retry_paths(
    project_root: &Path,
    round_context: &CompileFixRoundContext,
    touched_paths: &[PathBuf],
) -> Vec<PathBuf> {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut paths = Vec::new();

    let source_paths: Vec<PathBuf> = if touched_paths.is_empty() {
        round_context.relevant_paths.clone()
    } else {
        touched_paths.to_vec()
    };

    for path in source_paths {
        let full = if path.is_absolute() {
            path
        } else {
            project_root.join(path)
        };
        if seen.insert(full.clone()) {
            paths.push(full);
        }
    }

    paths.sort();
    paths
}

fn dedupe_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for path in paths {
        if seen.insert(path.clone()) {
            deduped.push(path.clone());
        }
    }
    deduped
}

fn summarize_paths(paths: &[PathBuf]) -> String {
    let deduped = dedupe_paths(paths);
    if deduped.is_empty() {
        return "compilation diagnostics".to_string();
    }
    let shown = deduped
        .iter()
        .take(5)
        .map(|path| path.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    let remaining = deduped.len().saturating_sub(shown.len());
    if remaining == 0 {
        shown.join(", ")
    } else {
        format!("{}, +{} more", shown.join(", "), remaining)
    }
}

fn effective_request_token_budget(
    compile_fix_budget: usize,
    request_token_limit: Option<f64>,
) -> usize {
    let request_limit = request_token_limit
        .filter(|value| value.is_finite() && *value > 0.0)
        .map(|value| value.floor() as usize);
    request_limit
        .map(|limit| limit.min(compile_fix_budget))
        .unwrap_or(compile_fix_budget)
        .max(1)
}

fn trim_retry_context_to_budget(
    retry_context: &mut HashMap<String, serde_json::Value>,
    budget: usize,
) -> usize {
    let estimate =
        |ctx: &HashMap<String, serde_json::Value>| estimate_request_tokens(RETRY_PATCH_PROMPT, ctx);

    let mut estimated = estimate(retry_context);
    if estimated <= budget {
        return estimated;
    }

    if let Some(previous_patch) = retry_context
        .get("previous_patch")
        .and_then(|value| value.as_str())
        .filter(|value| value.chars().count() > 4000)
        .map(|value| {
            format!(
                "{}\n...[truncated previous patch by reen]",
                value.chars().take(4000).collect::<String>()
            )
        })
    {
        retry_context.insert("previous_patch".to_string(), json!(previous_patch));
        estimated = estimate(retry_context);
        if estimated <= budget {
            return estimated;
        }
    }

    for key in [
        "previous_patch",
        "contract_artifacts_json",
        "specs_json",
        "recent_changes",
        "semantic_repair_plan",
        "compiler_stdout",
        "diagnostics_json",
    ] {
        if retry_context.remove(key).is_some() {
            estimated = estimate(retry_context);
            if estimated <= budget {
                return estimated;
            }
        }
    }

    estimated
}

async fn regenerate_patch_after_apply_failure(
    executor: &AgentExecutor,
    project_root: &Path,
    output: &CompilationOutput,
    round_context: &CompileFixRoundContext,
    recent_generated_files: &[PathBuf],
    project_info: &ProjectInfo,
    touched_paths: &[PathBuf],
    previous_patch: &str,
    apply_error: &anyhow::Error,
    attempt_dir: &Path,
    request_token_limit: Option<f64>,
    ignore_cache_reads: bool,
    execution_control: Option<&dyn NativeExecutionControl>,
) -> Result<String> {
    let touched_paths = dedupe_paths(touched_paths);
    let retry_paths = build_patch_retry_paths(project_root, round_context, &touched_paths);
    let target_summary = summarize_paths(&touched_paths);
    print_timed_status("Regenerating patch", &target_summary);

    fs::write(
        attempt_dir.join("patch_apply_error.txt"),
        format!("{apply_error:#}"),
    )
    .ok();

    let files_json = snapshot_files_json(project_root, &retry_paths)?;
    let mut retry_context = build_agent_context(
        output,
        &round_context.truncated_stderr,
        &round_context.diagnostics,
        &files_json,
        &round_context.specs_json,
        recent_generated_files,
        project_info,
    )?;
    retry_context.insert("previous_patch".to_string(), json!(previous_patch));
    retry_context.insert(
        "patch_apply_error".to_string(),
        json!(format!("{apply_error:#}")),
    );
    let retry_budget =
        effective_request_token_budget(compile_fix_config().max_tokens, request_token_limit);
    let retry_estimated = trim_retry_context_to_budget(&mut retry_context, retry_budget);

    if let Some(serde_json::Value::String(s)) = retry_context.get("files_json") {
        fs::write(attempt_dir.join("context_files_retry.json"), s).ok();
    }
    fs::write(
        attempt_dir.join("retry_request_budget.txt"),
        format!("budget={retry_budget}\nestimated={retry_estimated}\n"),
    )
    .ok();

    let agent_response = executor
        .execute_with_context_options(
            RETRY_PATCH_PROMPT,
            retry_context,
            execution_control,
            ignore_cache_reads,
        )
        .await
        .context("Failed to regenerate compilation patch after apply failure")?;

    match agent_response {
        AgentResponse::Final(s) => Ok(s),
        AgentResponse::Questions(q) => {
            fs::write(attempt_dir.join("agent_questions_retry.txt"), &q).ok();
            anyhow::bail!(
                "Compilation resolver requested clarification while regenerating an unappliable patch; escalating. See: {}",
                attempt_dir.display()
            );
        }
    }
}

fn run_cargo_build(project_root: &Path) -> Result<CompilationOutput> {
    let output = Command::new("cargo")
        .arg("build")
        .current_dir(project_root)
        .output()
        .context("Failed to execute cargo build")?;
    Ok(CompilationOutput {
        status_success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

fn create_session_dir(project_root: &Path) -> Result<PathBuf> {
    let base = project_root.join(".reen").join("compilation_fixes");
    fs::create_dir_all(&base).with_context(|| format!("Failed to create {}", base.display()))?;
    let ts = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let dir = base.join(ts);
    fs::create_dir_all(&dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    Ok(dir)
}

fn write_attempt_compile_output(attempt_dir: &Path, output: &CompilationOutput) -> Result<()> {
    fs::write(attempt_dir.join("cargo_stdout.txt"), &output.stdout)
        .context("Failed to write cargo stdout")?;
    fs::write(attempt_dir.join("cargo_stderr.txt"), &output.stderr)
        .context("Failed to write cargo stderr")?;
    Ok(())
}

async fn generate_semantic_repair_plan(
    config: &Config,
    relevant_paths: &[PathBuf],
    specs_json: &BTreeMap<String, String>,
    truncated_stderr: &str,
    diagnostics: &[DiagnosticSpan],
    request_token_limit: Option<f64>,
    ignore_cache_reads: bool,
    execution_control: Option<&dyn NativeExecutionControl>,
) -> Result<serde_json::Value> {
    let target_summary = summarize_paths(relevant_paths);
    print_timed_status("Planning repair", &target_summary);

    let (spec_path, spec_content) = specs_json
        .iter()
        .next()
        .map(|(path, content)| (path.clone(), content.clone()))
        .unwrap_or_else(|| ("specifications/repair_bundle.md".to_string(), String::new()));
    let fallback = build_default_plan(
        PlanKind::SemanticRepair,
        Path::new(&spec_path),
        &spec_content,
        relevant_paths,
        &HashMap::new(),
        Some(truncated_stderr),
    );

    let mut context = HashMap::new();
    context.insert(
        "plan_kind".to_string(),
        json!(PlanKind::SemanticRepair.as_str()),
    );
    context.insert("context_content".to_string(), json!(spec_content));
    let contract_artifact = build_contract_artifact(
        Path::new(&spec_path),
        &spec_content,
        relevant_paths.first().map(PathBuf::as_path),
        None,
    );
    let contract_validation = validate_contract_artifact(
        &contract_artifact,
        Path::new(&spec_path),
        &spec_content,
        None,
    );
    let behavior_contract =
        analyze_specification(Path::new(&spec_path), &spec_content, None).contract;
    context.insert(
        "contract_artifact".to_string(),
        contract_artifact_to_context_value(&contract_artifact),
    );
    context.insert(
        "contract_validation".to_string(),
        contract_validation_to_context_value(&contract_validation),
    );
    context.insert(
        "behavior_contract".to_string(),
        contract_to_context_value(&behavior_contract),
    );
    context.insert("default_plan".to_string(), plan_to_context_value(&fallback));
    context.insert(
        "target_output_paths".to_string(),
        json!(
            relevant_paths
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
        ),
    );
    context.insert("diagnostics_text".to_string(), json!(truncated_stderr));
    context.insert(
        "diagnostics_json".to_string(),
        json!(serde_json::to_string_pretty(diagnostics).unwrap_or_else(|_| "[]".to_string())),
    );

    let planner = AgentExecutor::new("create_plan", config)?;
    let (context, _) = super::pipeline_context::fit_context_to_token_limit(
        &planner,
        "",
        context,
        request_token_limit,
    )?;
    let response = planner
        .execute_with_conversation_with_seed_options(
            "",
            "semantic_repair_plan",
            context,
            execution_control,
            ignore_cache_reads,
        )
        .await?;
    let plan = parse_plan_output(&response).unwrap_or(fallback);
    Ok(plan_to_context_value(&plan))
}

fn capture_verifier_reports(
    project_root: &Path,
    artifact_root: &Path,
    touched_paths: &[PathBuf],
    report_dir: &Path,
) -> Result<HashMap<PathBuf, super::pipeline_quality::StaticBehaviorVerifierReport>> {
    fs::create_dir_all(report_dir)
        .with_context(|| format!("Failed to create {}", report_dir.display()))?;
    let mut reports = HashMap::new();
    for rel in touched_paths {
        let full = project_root.join(rel);
        if !full.exists() {
            continue;
        }
        let Some(spec_path) =
            determine_spec_path_for_output(&full, &artifact_root.join("specifications"))
        else {
            continue;
        };
        if !spec_path.exists() {
            continue;
        }
        let spec_content = fs::read_to_string(&spec_path)
            .with_context(|| format!("Failed to read {}", spec_path.display()))?;
        let report =
            verify_generated_implementation(project_root, &spec_path, &spec_content, &full)
                .with_context(|| format!("Failed semantic verification for {}", full.display()))?;
        let report_path =
            report_dir.join(rel.to_string_lossy().replace('\\', "__").replace('/', "__") + ".json");
        fs::write(
            &report_path,
            serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string()),
        )
        .ok();
        reports.insert(rel.clone(), report);
    }
    Ok(reports)
}

fn snapshot_file_backups(
    project_root: &Path,
    touched_paths: &[PathBuf],
) -> Result<HashMap<PathBuf, Option<String>>> {
    let mut backups = HashMap::new();
    for rel in touched_paths {
        let full = project_root.join(rel);
        let content = if full.exists() {
            Some(
                fs::read_to_string(&full)
                    .with_context(|| format!("Failed to read {}", full.display()))?,
            )
        } else {
            None
        };
        backups.insert(rel.clone(), content);
    }
    Ok(backups)
}

fn restore_file_backups(
    project_root: &Path,
    backups: HashMap<PathBuf, Option<String>>,
) -> Result<()> {
    for (rel, content) in backups {
        let full = project_root.join(rel);
        match content {
            Some(value) => {
                if let Some(parent) = full.parent() {
                    fs::create_dir_all(parent).ok();
                }
                fs::write(&full, value)
                    .with_context(|| format!("Failed restoring {}", full.display()))?;
            }
            None => {
                if full.exists() {
                    fs::remove_file(&full)
                        .with_context(|| format!("Failed removing {}", full.display()))?;
                }
            }
        }
    }
    Ok(())
}

fn compare_semantic_reports(
    before: HashMap<PathBuf, super::pipeline_quality::StaticBehaviorVerifierReport>,
    after: HashMap<PathBuf, super::pipeline_quality::StaticBehaviorVerifierReport>,
) -> Vec<String> {
    let mut issues = Vec::new();
    let mut keys: HashSet<PathBuf> = before.keys().cloned().collect();
    keys.extend(after.keys().cloned());
    let mut ordered: Vec<PathBuf> = keys.into_iter().collect();
    ordered.sort();
    for path in ordered {
        match (before.get(&path), after.get(&path)) {
            (Some(old), Some(new)) => {
                let regression = compare_verifier_reports(old.clone(), new.clone());
                if regression.worsened {
                    for issue in regression.issues {
                        issues.push(format!("{}: {}", path.display(), issue));
                    }
                }
            }
            (None, Some(new)) if !new.errors.is_empty() || !new.high_risk_findings.is_empty() => {
                issues.push(format!(
                    "{}: new semantic report contains verifier issues",
                    path.display()
                ));
            }
            _ => {}
        }
    }
    issues
}

fn parse_rustc_diagnostics(stderr: &str) -> Vec<DiagnosticSpan> {
    // Typical rustc span line:
    //   --> src/foo.rs:12:34
    let re_span = Regex::new(r"(?m)^\s*-->\s+([^\s:][^:]*):(\d+):(\d+)\s*$").ok();
    let re_code = Regex::new(r"(?m)^\s*error\[(E\d+)\]:").ok();

    let mut spans = Vec::new();
    let mut current_code: Option<String> = None;
    for line in stderr.lines() {
        if let Some(re) = &re_code {
            if let Some(cap) = re.captures(line) {
                current_code = cap.get(1).map(|m| m.as_str().to_string());
                continue;
            }
        }
        if let Some(re) = &re_span {
            if let Some(cap) = re.captures(line) {
                let file = cap.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
                let line = cap
                    .get(2)
                    .and_then(|m| m.as_str().parse::<u32>().ok())
                    .unwrap_or(0);
                let col = cap
                    .get(3)
                    .and_then(|m| m.as_str().parse::<u32>().ok())
                    .unwrap_or(0);
                spans.push(DiagnosticSpan {
                    file,
                    line,
                    col,
                    code: current_code.clone(),
                });
            }
        }
    }

    spans
}

/// Error codes that often cause cascading errors; prioritize these first.
const ROOT_CAUSE_ERROR_CODES: &[&str] = &["E0412", "E0433", "E0583", "E0407", "E0405"];
const RETRY_PATCH_PROMPT: &str = "The previous patch did not apply. Re-emit an exact unified diff against the current full file contents. Do not abbreviate or invent omitted code. Every context and removed line must match the provided files verbatim.";

/// File path priority: root/mod files before leaf modules.
fn file_priority(path: &str) -> u8 {
    if path == "src/lib.rs" || path == "src/main.rs" {
        0
    } else if path.ends_with("/mod.rs") {
        1
    } else {
        2
    }
}

fn prioritize_and_cap_diagnostics(
    diagnostics: &[DiagnosticSpan],
    max_errors: usize,
) -> Vec<DiagnosticSpan> {
    if diagnostics.len() <= max_errors {
        return diagnostics.to_vec();
    }
    let mut indexed: Vec<(usize, &DiagnosticSpan)> = diagnostics.iter().enumerate().collect();
    indexed.sort_by(|(i_a, a), (i_b, b)| {
        let code_pri_a = a
            .code
            .as_ref()
            .map(|c| {
                if ROOT_CAUSE_ERROR_CODES.contains(&c.as_str()) {
                    0
                } else {
                    1
                }
            })
            .unwrap_or(1);
        let code_pri_b = b
            .code
            .as_ref()
            .map(|c| {
                if ROOT_CAUSE_ERROR_CODES.contains(&c.as_str()) {
                    0
                } else {
                    1
                }
            })
            .unwrap_or(1);
        code_pri_a
            .cmp(&code_pri_b)
            .then_with(|| file_priority(&a.file).cmp(&file_priority(&b.file)))
            .then_with(|| i_a.cmp(i_b))
    });
    indexed
        .into_iter()
        .take(max_errors)
        .map(|(_, d)| d.clone())
        .collect()
}

/// Truncates stderr to keep only error blocks matching the prioritized diagnostics.
fn truncate_stderr_for_diagnostics(stderr: &str, diagnostics: &[DiagnosticSpan]) -> String {
    if diagnostics.is_empty() {
        return stderr.to_string();
    }
    let diagnostic_set: HashSet<(String, u32)> = diagnostics
        .iter()
        .map(|d| (d.file.clone(), d.line))
        .collect();
    let re_span = Regex::new(r"(?m)^\s*-->\s+([^\s:][^:]*):(\d+):(\d+)\s*$").ok();
    let re_error = Regex::new(r"(?m)^\s*error\[(E\d+)\]:").ok();
    let mut blocks: Vec<String> = Vec::new();
    let mut current_block = String::new();
    let mut block_matches = false;
    let mut in_block = false;

    for line in stderr.lines() {
        let is_error_start = re_error.as_ref().map_or(false, |r| r.is_match(line));
        let is_span = re_span.as_ref().map_or(false, |r| r.is_match(line));

        if is_error_start {
            if in_block && block_matches {
                blocks.push(current_block.clone());
            }
            current_block = format!("{}\n", line);
            in_block = true;
            block_matches = false;
        } else if in_block {
            current_block.push_str(line);
            current_block.push('\n');
            if is_span {
                if let Some(re) = &re_span {
                    if let Some(cap) = re.captures(line) {
                        let file = cap.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
                        let line_no = cap
                            .get(2)
                            .and_then(|m| m.as_str().parse::<u32>().ok())
                            .unwrap_or(0);
                        if diagnostic_set.contains(&(file, line_no)) {
                            block_matches = true;
                        }
                    }
                }
            }
        }
    }
    if in_block && block_matches {
        blocks.push(current_block);
    } else if blocks.is_empty() {
        return stderr.to_string();
    }
    blocks.join("")
}

/// Extracts paths from `mod` lines in Rust source (best-effort).
/// For `mod foo;` we add parent/foo.rs and parent/foo/mod.rs.
fn extract_imports_from_file(content: &str, current_file_rel: &str) -> Vec<String> {
    let re_mod = Regex::new(r"\bmod\s+([a-zA-Z0-9_]+)\s*;").ok();
    let mut out: Vec<String> = Vec::new();
    let parent = Path::new(current_file_rel)
        .parent()
        .unwrap_or(Path::new("."));
    for line in content.lines() {
        if let Some(re) = &re_mod {
            if let Some(cap) = re.captures(line) {
                if let Some(m) = cap.get(1) {
                    let module = m.as_str();
                    let sibling = parent.join(format!("{}.rs", module));
                    out.push(sibling.to_string_lossy().to_string());
                    let submod = parent.join(module).join("mod.rs");
                    out.push(submod.to_string_lossy().to_string());
                }
            }
        }
    }
    out.into_iter().filter(|s| !s.is_empty()).collect()
}

fn collect_relevant_paths(
    project_root: &Path,
    diagnostics: &[DiagnosticSpan],
    stderr: &str,
    recent_generated_files: &[PathBuf],
    max_paths: Option<usize>,
) -> Result<Vec<PathBuf>> {
    let mut paths: HashSet<PathBuf> = HashSet::new();
    let diagnostic_files: HashSet<String> = diagnostics
        .iter()
        .map(|d| d.file.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let recent_set: HashSet<String> = recent_generated_files
        .iter()
        .map(|p| {
            p.strip_prefix(project_root)
                .unwrap_or(p)
                .to_string_lossy()
                .to_string()
        })
        .collect();

    // Cargo.toml always (small, needed for deps).
    let cargo = project_root.join("Cargo.toml");
    if cargo.exists() {
        paths.insert(cargo);
    }

    // Only include lib.rs, mod.rs if in diagnostics or recent.
    for candidate in [
        "src/lib.rs",
        "src/main.rs",
        "src/execution/mod.rs",
        "src/contexts/mod.rs",
        "src/data/mod.rs",
    ] {
        if diagnostic_files.contains(candidate) || recent_set.contains(candidate) {
            let p = project_root.join(candidate);
            if p.exists() {
                paths.insert(p);
            }
        }
    }

    for p in recent_generated_files {
        let full = if p.is_absolute() {
            p.clone()
        } else {
            project_root.join(p)
        };
        if full.exists() {
            paths.insert(full);
        }
    }

    for d in diagnostics {
        if d.file.trim().is_empty() {
            continue;
        }
        let cleaned = d.file.trim();
        let full = project_root.join(cleaned);
        if full.exists() {
            paths.insert(full);
        }
    }

    // Also try to extract any src/**.rs mentioned in stderr (e.g. "src/foo.rs")
    let re_rs = Regex::new(r"(src/[A-Za-z0-9_\-/]+\.rs)").ok();
    if let Some(re) = re_rs {
        for cap in re.captures_iter(stderr) {
            if let Some(m) = cap.get(1) {
                let p = project_root.join(m.as_str());
                if p.exists() {
                    paths.insert(p);
                }
            }
        }
    }

    // Import-based: for files with errors, parse use/mod and add dependencies.
    for path in paths.clone() {
        let rel = path
            .strip_prefix(project_root)
            .unwrap_or(&path)
            .to_string_lossy();
        if !rel.ends_with(".rs") {
            continue;
        }
        let content = fs::read_to_string(&path).unwrap_or_default();
        for dep in extract_imports_from_file(&content, &rel) {
            let full = project_root.join(&dep);
            if full.exists() {
                paths.insert(full);
            }
        }
    }

    let mut out: Vec<PathBuf> = paths.into_iter().collect();
    out.sort();

    if let Some(max) = max_paths {
        if out.len() > max {
            let diagnostic_paths: HashSet<PathBuf> = diagnostics
                .iter()
                .filter_map(|d| {
                    let p = project_root.join(d.file.trim());
                    if p.exists() { Some(p) } else { None }
                })
                .collect();
            let recent_paths: HashSet<PathBuf> = recent_generated_files
                .iter()
                .map(|p| {
                    if p.is_absolute() {
                        p.clone()
                    } else {
                        project_root.join(p)
                    }
                })
                .filter(|p| p.exists())
                .collect();
            let priority: HashSet<PathBuf> =
                diagnostic_paths.union(&recent_paths).cloned().collect();
            out.sort_by(|a, b| {
                let a_pri = priority.contains(a);
                let b_pri = priority.contains(b);
                b_pri.cmp(&a_pri).then_with(|| a.cmp(b))
            });
            out.truncate(max);
        }
    }
    Ok(out)
}

#[allow(dead_code)]
fn snapshot_files_json(project_root: &Path, paths: &[PathBuf]) -> Result<BTreeMap<String, String>> {
    let mut map = BTreeMap::new();
    for p in paths {
        if !p.exists() || p.is_dir() {
            continue;
        }
        let rel = p
            .strip_prefix(project_root)
            .unwrap_or(p)
            .to_string_lossy()
            .to_string();
        let content =
            fs::read_to_string(p).with_context(|| format!("Failed to read {}", p.display()))?;
        map.insert(rel, content);
    }
    Ok(map)
}

const SMALL_FILE_LINE_THRESHOLD: usize = 100;
const HEAD_LINES_FOR_NON_ERROR_FILE: usize = 50;

/// Snapshot files as snippets: around error spans for files with diagnostics,
/// first N lines for others. Small files and Cargo.toml get full content.
fn snapshot_files_json_snippets(
    project_root: &Path,
    paths: &[PathBuf],
    diagnostics: &[DiagnosticSpan],
    snippet_lines: usize,
) -> Result<BTreeMap<String, String>> {
    let diagnostics_by_file: HashMap<String, Vec<u32>> =
        diagnostics.iter().fold(HashMap::new(), |mut acc, d| {
            if !d.file.trim().is_empty() {
                acc.entry(d.file.trim().to_string())
                    .or_default()
                    .push(d.line);
            }
            acc
        });

    let mut map = BTreeMap::new();
    for p in paths {
        if !p.exists() || p.is_dir() {
            continue;
        }
        let rel = p
            .strip_prefix(project_root)
            .unwrap_or(p)
            .to_string_lossy()
            .to_string();
        let content =
            fs::read_to_string(p).with_context(|| format!("Failed to read {}", p.display()))?;
        let lines: Vec<&str> = content.lines().collect();
        let line_count = lines.len();

        let snippet = if rel == "Cargo.toml" || line_count <= SMALL_FILE_LINE_THRESHOLD {
            content
        } else if let Some(error_lines) = diagnostics_by_file.get(&rel) {
            let mut ranges: Vec<(usize, usize)> = error_lines
                .iter()
                .map(|&ln| {
                    let ln_usize = ln as usize;
                    let start = ln_usize.saturating_sub(snippet_lines + 1);
                    let end = (ln_usize + snippet_lines).min(line_count);
                    (start, end)
                })
                .collect();
            ranges.sort_by_key(|(a, _)| *a);
            let mut merged: Vec<(usize, usize)> = Vec::new();
            for (start, end) in ranges {
                if let Some(last) = merged.last_mut() {
                    if start <= last.1 + 1 {
                        last.1 = last.1.max(end);
                    } else {
                        merged.push((start, end));
                    }
                } else {
                    merged.push((start, end));
                }
            }
            let mut result = String::new();
            for (start, end) in merged {
                let end = end.min(line_count);
                for (i, line) in lines.iter().enumerate().take(end).skip(start) {
                    result.push_str(&format!("{:4} | {}\n", i + 1, line));
                }
                if !result.ends_with('\n') {
                    result.push('\n');
                }
            }
            if result.is_empty() {
                content
            } else {
                format!("(snippet around error lines)\n{}", result)
            }
        } else {
            let take = HEAD_LINES_FOR_NON_ERROR_FILE.min(line_count);
            let head: String = lines
                .iter()
                .take(take)
                .enumerate()
                .map(|(i, l)| format!("{:4} | {}\n", i + 1, l))
                .collect();
            format!("(first {} lines)\n{}", take, head)
        };
        map.insert(rel, snippet);
    }
    Ok(map)
}

fn snapshot_specs_json(
    project_root: &Path,
    artifact_root: &Path,
    src_paths: &[PathBuf],
) -> Result<BTreeMap<String, String>> {
    let mut spec_paths: HashSet<PathBuf> = HashSet::new();
    for p in src_paths {
        let rel = p.strip_prefix(project_root).unwrap_or(p);
        let rel_s = rel.to_string_lossy();
        let spec_rel = map_src_to_spec(&rel_s);
        if let Some(spec_rel) = spec_rel {
            if let Some(spec_full) = resolve_spec_path(artifact_root, &spec_rel) {
                spec_paths.insert(spec_full);
            }
        }
    }

    let mut map = BTreeMap::new();
    let mut list: Vec<PathBuf> = spec_paths.into_iter().collect();
    list.sort();
    for p in list {
        let rel = p
            .strip_prefix(artifact_root)
            .unwrap_or(&p)
            .to_string_lossy()
            .to_string();
        let content =
            fs::read_to_string(&p).with_context(|| format!("Failed to read {}", p.display()))?;
        map.insert(rel, content);
    }
    Ok(map)
}

fn map_src_to_spec(src_rel: &str) -> Option<String> {
    // Best-effort mapping. This is intentionally conservative.
    // src/contexts/x.rs -> specifications/contexts/x.md
    // src/contexts/ui/x.rs -> specifications/contexts/ui/x.md
    // src/data/x.rs -> specifications/data/x.md
    // src/data/payments/x.rs -> specifications/data/payments/x.md
    // src/main.rs -> specifications/app.md
    if let Some(stem) = src_rel
        .strip_prefix("src/contexts/")
        .and_then(|s| s.strip_suffix(".rs"))
    {
        return Some(format!("specifications/contexts/{}.md", stem));
    }
    if let Some(stem) = src_rel
        .strip_prefix("src/data/")
        .and_then(|s| s.strip_suffix(".rs"))
    {
        return Some(format!("specifications/data/{}.md", stem));
    }
    if src_rel == "src/main.rs" {
        return Some("specifications/app.md".to_string());
    }
    None
}

/// Resolve a logical spec path (`specifications/...`) under the active artifact workspace root
/// (`ArtifactStore::artifact_workspace_root`: repo root for file backend, GitHub projection root
/// for `--github`). Falls back to the parallel `drafts/...` path when the specification file is absent.
fn resolve_spec_path(artifact_root: &Path, spec_rel: &str) -> Option<PathBuf> {
    let spec_path = artifact_root.join(spec_rel);
    if spec_path.is_file() {
        return Some(spec_path);
    }
    let draft_rel = spec_rel.replacen("specifications/", "drafts/", 1);
    let draft_path = artifact_root.join(draft_rel);
    if draft_path.is_file() {
        return Some(draft_path);
    }
    None
}

fn build_agent_context(
    output: &CompilationOutput,
    truncated_stderr: &str,
    diagnostics: &[DiagnosticSpan],
    files_json: &BTreeMap<String, String>,
    specs_json: &BTreeMap<String, String>,
    recent_generated_files: &[PathBuf],
    project_info: &ProjectInfo,
) -> Result<HashMap<String, serde_json::Value>> {
    let mut ctx = HashMap::new();
    ctx.insert("compiler_stdout".to_string(), json!(output.stdout));
    ctx.insert("compiler_stderr".to_string(), json!(truncated_stderr));
    ctx.insert(
        "diagnostics_json".to_string(),
        json!(serde_json::to_string_pretty(diagnostics).unwrap_or_else(|_| "[]".to_string())),
    );
    ctx.insert(
        "files_json".to_string(),
        json!(serde_json::to_string_pretty(files_json).unwrap_or_else(|_| "{}".to_string())),
    );
    if !specs_json.is_empty() {
        ctx.insert(
            "specs_json".to_string(),
            json!(serde_json::to_string_pretty(specs_json).unwrap_or_else(|_| "{}".to_string())),
        );
        let contract_artifacts = specs_json
            .iter()
            .map(|(spec_path, spec_content)| {
                let artifact =
                    build_contract_artifact(Path::new(spec_path), spec_content, None, None);
                (
                    spec_path.clone(),
                    compact_contract_artifact_value(&artifact),
                )
            })
            .collect::<BTreeMap<_, _>>();
        ctx.insert(
            "contract_artifacts_json".to_string(),
            json!(
                serde_json::to_string_pretty(&contract_artifacts)
                    .unwrap_or_else(|_| "{}".to_string())
            ),
        );
    }
    ctx.insert(
        "recent_changes".to_string(),
        json!(
            recent_generated_files
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join("\n")
        ),
    );
    ctx.insert("project_info".to_string(), json!(project_info.package_name));
    Ok(ctx)
}

fn guardrail_to_json(r: &GuardrailReport) -> serde_json::Value {
    json!({
        "ok": r.ok,
        "issues": r.issues,
        "touched_files": r.touched_files,
        "touches_cargo_toml": r.touches_cargo_toml,
        "adds_files": r.adds_files,
        "deleted_files": r.deleted_files,
        "total_deleted_lines": r.total_deleted_lines,
        "modifies_public_fn_lines": r.modifies_public_fn_lines,
        "adds_stub_macros": r.adds_stub_macros,
    })
}

fn explicit_implementation_failure_message_from_stderr(stderr: &str) -> Option<String> {
    if !stderr.contains(super::IMPLEMENTATION_FAILURE_MARKER) {
        return None;
    }

    let mut excerpt = Vec::new();
    let mut capture = false;
    for line in stderr.lines() {
        if line.contains(super::IMPLEMENTATION_FAILURE_MARKER) {
            capture = true;
        }
        if !capture {
            continue;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !excerpt.is_empty() {
                break;
            }
            continue;
        }

        excerpt.push(trimmed.to_string());
        if excerpt.len() >= 8 {
            break;
        }
    }

    if excerpt.is_empty() {
        Some(super::IMPLEMENTATION_FAILURE_MARKER.to_string())
    } else {
        Some(excerpt.join("\n"))
    }
}

fn check_guardrails(
    project_root: &Path,
    artifact_root: &Path,
    diff: &str,
) -> Result<GuardrailReport> {
    let file_patches = parse_unified_diff(diff).context("Invalid unified diff")?;

    let mut issues = Vec::new();
    let mut touched_files = Vec::new();
    let mut adds_files = Vec::new();
    let mut deleted_files = Vec::new();
    let mut touches_cargo_toml = false;
    let mut modifies_public_fn_lines = false;
    let mut adds_stub_macros = false;
    let mut total_deleted_lines = 0usize;

    for fp in &file_patches {
        let path = fp.new_path.as_deref().unwrap_or("").to_string();
        let old = fp.old_path.as_deref().unwrap_or("").to_string();

        if fp.is_deletion {
            deleted_files.push(old.clone());
            issues.push(format!("File deletion is not allowed: {}", old));
            continue;
        }

        let target = if !path.is_empty() {
            path.clone()
        } else {
            old.clone()
        };
        if target.is_empty() {
            issues.push("Patch contains an empty path".to_string());
            continue;
        }

        // Block edits outside repo root (../) or absolute paths.
        if target.starts_with('/') || target.contains("..") {
            issues.push(format!("Blocked path (outside repo): {}", target));
            continue;
        }

        // Block touching drafts/specifications.
        if target.starts_with("drafts/") || target.starts_with("specifications/") {
            issues.push(format!("Blocked path (protected directory): {}", target));
            continue;
        }

        // Only allow patching src/** and Cargo.toml to prevent scope creep.
        if target != "Cargo.toml" && !target.starts_with("src/") {
            issues.push(format!(
                "Blocked path (outside allowed surface area): {} (only src/** and Cargo.toml permitted)",
                target
            ));
            continue;
        }

        if target == "Cargo.toml" {
            touches_cargo_toml = true;
        }

        if fp.is_new_file {
            if !target.starts_with("src/") {
                issues.push(format!(
                    "New file outside src/ is not allowed (path={}): only src/** additions are permitted",
                    target
                ));
            } else {
                adds_files.push(target.clone());
            }
        }

        // Heuristics: constrain public API changes and block stub/bypass macros.
        let mut removed_pub_fns: Vec<FnSig> = Vec::new();
        let mut added_pub_fns: Vec<FnSig> = Vec::new();
        let mut removes_impl_failure_marker = false;
        let mut adds_placeholder_text = false;
        for hl in &fp.hunk_lines {
            match hl.kind {
                HunkLineKind::Remove => {
                    total_deleted_lines += 1;
                    if hl.text.contains(super::IMPLEMENTATION_FAILURE_MARKER) {
                        removes_impl_failure_marker = true;
                    }
                    if public_fn_re().is_match(&hl.text) {
                        modifies_public_fn_lines = true;
                        if let Some(sig) = parse_pub_fn_signature(&hl.text) {
                            removed_pub_fns.push(sig);
                        }
                    }
                }
                HunkLineKind::Add => {
                    if public_fn_re().is_match(&hl.text) {
                        modifies_public_fn_lines = true;
                        if let Some(sig) = parse_pub_fn_signature(&hl.text) {
                            added_pub_fns.push(sig);
                        }
                    }
                    if stub_macro_re().is_match(&hl.text) {
                        adds_stub_macros = true;
                    }
                    if placeholder_text_re().is_match(&hl.text) {
                        adds_placeholder_text = true;
                    }
                }
                _ => {}
            }
        }

        if removes_impl_failure_marker {
            issues.push(format!(
                "Patch removes an explicit implementation-refusal marker from {}; escalation required.",
                target
            ));
        }
        if adds_placeholder_text {
            issues.push(format!(
                "Patch adds placeholder-style implementation text in {}; escalation required.",
                target
            ));
        }

        let allowed_public_methods = spec_declared_public_methods(artifact_root, &target);

        issues.extend(evaluate_public_api_changes(
            &target,
            &removed_pub_fns,
            &added_pub_fns,
            &allowed_public_methods,
        ));

        // Also ensure target resolves within root when joined.
        let full = project_root.join(&target);
        if let Ok(canon) = full.canonicalize() {
            if let Ok(root) = project_root.canonicalize() {
                if !canon.starts_with(root) {
                    issues.push(format!(
                        "Blocked path (outside repo after resolve): {}",
                        target
                    ));
                }
            }
        }

        touched_files.push(target);
    }

    touched_files.sort();
    touched_files.dedup();
    adds_files.sort();
    adds_files.dedup();

    if adds_stub_macros {
        issues.push("Patch introduces stub/bypass macros (todo!/unimplemented!/compile_error!); escalation required.".to_string());
    }
    if total_deleted_lines > 200 {
        issues.push(format!(
            "Patch deletes too many lines ({} > 200); escalation required.",
            total_deleted_lines
        ));
    }

    Ok(GuardrailReport {
        ok: issues.is_empty(),
        issues,
        touched_files,
        touches_cargo_toml,
        adds_files,
        modifies_public_fn_lines,
        adds_stub_macros,
        deleted_files,
        total_deleted_lines,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReceiverKind {
    RefSelf,
    ValSelf,
    MutRefSelf,
    MutValSelf,
    Other,
}

#[derive(Debug, Clone)]
struct FnSig {
    name: String,
    receiver: ReceiverKind,
    non_self_param_types: Vec<String>,
    return_type: Option<String>,
}

fn parse_pub_fn_signature(line: &str) -> Option<FnSig> {
    // Parse a single-line function signature.
    // Examples:
    // - pub fn foo(&self) -> &T {
    // - pub(crate) fn bar(self, x: T) -> Result<T> {
    // This is intentionally heuristic: it is used only for guardrails.
    let trimmed = line.trim();
    if !public_fn_re().is_match(trimmed) {
        return None;
    }

    let fn_pos = trimmed.find("fn ")?;
    let after_fn = &trimmed[fn_pos + 3..];
    let name_end = after_fn.find('(')?;
    let name = after_fn[..name_end].trim().to_string();
    if name.is_empty() {
        return None;
    }

    let open_paren = trimmed.find('(')?;
    let close_paren = trimmed[open_paren + 1..].find(')')? + open_paren + 1;
    let params_raw = trimmed[open_paren + 1..close_paren].trim();
    let mut params: Vec<String> = if params_raw.is_empty() {
        Vec::new()
    } else {
        params_raw
            .split(',')
            .map(|p| p.trim().to_string())
            .collect()
    };

    let receiver = match params.first().map(|s| s.as_str()) {
        Some("&self") => ReceiverKind::RefSelf,
        Some("self") => ReceiverKind::ValSelf,
        Some("&mut self") => ReceiverKind::MutRefSelf,
        Some("mut self") => ReceiverKind::MutValSelf,
        Some(p) if p.contains("self") => ReceiverKind::Other,
        Some(_) => ReceiverKind::Other,
        None => ReceiverKind::Other,
    };

    if !params.is_empty() {
        // Drop the receiver position if it looks like self.
        if matches!(
            receiver,
            ReceiverKind::RefSelf
                | ReceiverKind::ValSelf
                | ReceiverKind::MutRefSelf
                | ReceiverKind::MutValSelf
        ) {
            params.remove(0);
        }
    }

    let mut non_self_param_types = Vec::new();
    for p in params {
        // param may be "x: Type" or patterns; take rhs of ':' if present.
        let ty = p
            .split_once(':')
            .map(|(_, rhs)| rhs.trim().to_string())
            .unwrap_or_else(|| p.trim().to_string());
        if !ty.is_empty() {
            non_self_param_types.push(ty);
        }
    }

    let mut return_type: Option<String> = None;
    let after_params = &trimmed[close_paren + 1..];
    if let Some(arrow_pos) = after_params.find("->") {
        let rt_raw = after_params[arrow_pos + 2..].trim();
        // Trim trailing "where ..." or "{".
        let rt_end = rt_raw
            .find('{')
            .or_else(|| rt_raw.find(" where "))
            .unwrap_or(rt_raw.len());
        let rt = rt_raw[..rt_end].trim();
        if !rt.is_empty() {
            return_type = Some(rt.to_string());
        }
    }

    Some(FnSig {
        name,
        receiver,
        non_self_param_types,
        return_type,
    })
}

fn spec_declared_public_methods(artifact_root: &Path, target: &str) -> HashSet<String> {
    let Some(spec_rel) = map_src_to_spec(target) else {
        return HashSet::new();
    };
    let Some(spec_path) = resolve_spec_path(artifact_root, &spec_rel) else {
        return HashSet::new();
    };
    let Ok(spec_text) = fs::read_to_string(spec_path) else {
        return HashSet::new();
    };

    let mut methods = HashSet::new();
    for caps in spec_method_code_re().captures_iter(&spec_text) {
        if let Some(name) = caps.get(1) {
            methods.insert(name.as_str().to_string());
        }
    }
    for caps in spec_method_bold_re().captures_iter(&spec_text) {
        if let Some(name) = caps.get(1) {
            methods.insert(name.as_str().to_string());
        }
    }
    methods
}

fn evaluate_public_api_changes(
    target: &str,
    removed: &[FnSig],
    added: &[FnSig],
    allowed_public_methods: &HashSet<String>,
) -> Vec<String> {
    let mut issues = Vec::new();

    let mut removed_by_name: HashMap<&str, Vec<&FnSig>> = HashMap::new();
    for r in removed {
        removed_by_name.entry(r.name.as_str()).or_default().push(r);
    }
    let mut added_by_name: HashMap<&str, Vec<&FnSig>> = HashMap::new();
    for a in added {
        added_by_name.entry(a.name.as_str()).or_default().push(a);
    }

    let mut all_names: HashSet<&str> = HashSet::new();
    all_names.extend(removed_by_name.keys().copied());
    all_names.extend(added_by_name.keys().copied());

    for name in all_names {
        let rs = removed_by_name.get(name).cloned().unwrap_or_default();
        let as_ = added_by_name.get(name).cloned().unwrap_or_default();

        if rs.len() > 1 || as_.len() > 1 {
            issues.push(format!(
                "{}: multiple signature edits detected for public function `{}`; escalation required.",
                target, name
            ));
            continue;
        }

        match (rs.first().copied(), as_.first().copied()) {
            (Some(_r), None) => {
                // Removing public functions is considered behavior/API stripping.
                issues.push(format!(
                    "{}: patch removes public function `{}`; escalation required.",
                    target, name
                ));
            }
            (None, Some(a)) => {
                let allowed = is_allowed_new_public_method(a, allowed_public_methods);
                // Allow adding getters; block setters and other public additions.
                if !allowed {
                    issues.push(format!(
                        "{}: patch adds new public function `{}` outside allowed patterns (getters; spec-declared methods; constructors; or owned `mut self -> (Self, …)` / `Self` transitions — not `set_*` or `&mut self` unless the name is declared in the module spec).",
                        target, name
                    ));
                }
            }
            (Some(r), Some(a)) => {
                // Allow limited signature adjustments: &T <-> T (including receiver &self <-> self).
                if let Some(reason) = disallowed_pub_fn_modification(r, a) {
                    issues.push(format!(
                        "{}: patch modifies public function `{}` in a disallowed way: {}",
                        target, name, reason
                    ));
                }
            }
            (None, None) => {}
        }
    }

    issues
}

/// True when the return type is an owned state-passing shape (`(Self, …)`, `Self`, or the same
/// inside common enums/result wrappers). This matches the "non-destructive" / functional-update
/// style as an alternative to `&mut self` polling APIs.
fn return_type_implies_owned_state_transition(sig: &FnSig) -> bool {
    let Some(rt) = &sig.return_type else {
        return false;
    };
    let compact: String = rt.chars().filter(|c| !c.is_whitespace()).collect();
    if !compact.contains("Self") {
        return false;
    }
    if compact.starts_with("(Self,") || compact.starts_with("(Self)") {
        return true;
    }
    if compact == "Self" {
        return true;
    }
    for prefix in [
        "Result<(Self,",
        "Option<(Self,",
        "ControlFlow<(Self,",
        "Poll<(Self,",
    ] {
        if compact.starts_with(prefix) {
            return true;
        }
    }
    false
}

fn is_allowed_new_public_method(sig: &FnSig, allowed_public_methods: &HashSet<String>) -> bool {
    if is_constructor_name(&sig.name) {
        // Constructors are always safe to add as associated functions.
        return matches!(sig.receiver, ReceiverKind::Other);
    }
    if is_spec_allowed_public_method_name(&sig.name, allowed_public_methods) {
        return true;
    }
    // Allowed:
    // - adding getters (no args except receiver, no mut receiver), and not a setter
    // - adding constructors: `new` / `try_new` (associated functions; no `self` receiver)
    // - adding owned-receiver state transitions: `mut self -> (Self, T)` (etc.)
    if sig.name.starts_with("set_") {
        return false;
    }
    if matches!(sig.receiver, ReceiverKind::MutRefSelf) {
        return false;
    }
    if matches!(sig.receiver, ReceiverKind::MutValSelf) {
        return sig.non_self_param_types.is_empty()
            && return_type_implies_owned_state_transition(sig);
    }
    // RefSelf, ValSelf, or Other: allow only getter-shaped additions.
    sig.non_self_param_types.is_empty()
}

fn is_spec_allowed_public_method_name(
    name: &str,
    allowed_public_methods: &HashSet<String>,
) -> bool {
    if allowed_public_methods.contains(name) {
        return true;
    }
    if let Some(raw_name) = name.strip_prefix("r#") {
        return allowed_public_methods.contains(raw_name);
    }
    if let Some((base, _alias_suffix)) = name.split_once('_') {
        if rust_keyword_names().contains(base) && allowed_public_methods.contains(base) {
            return true;
        }
    }
    false
}

fn rust_keyword_names() -> &'static HashSet<&'static str> {
    static KEYWORDS: OnceLock<HashSet<&'static str>> = OnceLock::new();
    KEYWORDS.get_or_init(|| {
        HashSet::from([
            "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn",
            "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
            "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
            "unsafe", "use", "where", "while", "async", "await", "dyn", "abstract", "become",
            "box", "do", "final", "macro", "override", "priv", "try", "typeof", "unsized",
            "virtual", "yield",
        ])
    })
}

fn disallowed_pub_fn_modification(old: &FnSig, new: &FnSig) -> Option<String> {
    if old.name != new.name {
        return Some("function name changed".to_string());
    }
    if matches!(
        new.receiver,
        ReceiverKind::MutRefSelf | ReceiverKind::MutValSelf
    ) {
        return Some("introduces mutable receiver (`&mut self`/`mut self`)".to_string());
    }
    if matches!(
        old.receiver,
        ReceiverKind::MutRefSelf | ReceiverKind::MutValSelf
    ) && !is_allowed_immutable_receiver_transition(old, new)
    {
        return Some(
            "changes a mutable receiver outside the allowed immutable transform patterns"
                .to_string(),
        );
    }
    if old.non_self_param_types.len() != new.non_self_param_types.len() {
        return Some("parameter count changed".to_string());
    }
    for (a, b) in old
        .non_self_param_types
        .iter()
        .zip(new.non_self_param_types.iter())
    {
        if !is_ref_value_equivalent(a, b) {
            return Some(format!(
                "parameter type changed beyond &T<->T: `{}` -> `{}`",
                a, b
            ));
        }
    }
    match (&old.return_type, &new.return_type) {
        (None, None) => {}
        (Some(a), Some(b)) => {
            if !is_ref_value_equivalent(a, b)
                && !is_self_receiver_return_shape_equivalent(old, new, a, b)
            {
                return Some(format!(
                    "return type changed beyond &T<->T: `{}` -> `{}`",
                    a, b
                ));
            }
        }
        _ => return Some("return type presence changed".to_string()),
    }
    None
}

fn is_self_receiver_return_shape_equivalent(
    old: &FnSig,
    new: &FnSig,
    old_return: &str,
    new_return: &str,
) -> bool {
    let old_is_self_receiver = matches!(
        old.receiver,
        ReceiverKind::RefSelf
            | ReceiverKind::ValSelf
            | ReceiverKind::MutRefSelf
            | ReceiverKind::MutValSelf
    );
    let new_is_self_receiver = matches!(
        new.receiver,
        ReceiverKind::RefSelf
            | ReceiverKind::ValSelf
            | ReceiverKind::MutRefSelf
            | ReceiverKind::MutValSelf
    );
    old_is_self_receiver
        && new_is_self_receiver
        && type_shape_without_generic_args(old_return)
            == type_shape_without_generic_args(new_return)
}

fn is_allowed_immutable_receiver_transition(old: &FnSig, new: &FnSig) -> bool {
    matches!(
        old.receiver,
        ReceiverKind::MutRefSelf | ReceiverKind::MutValSelf
    ) && !matches!(
        new.receiver,
        ReceiverKind::MutRefSelf | ReceiverKind::MutValSelf
    ) && is_self_family_return(&old.return_type)
        && is_self_family_return(&new.return_type)
}

fn is_ref_value_equivalent(a: &str, b: &str) -> bool {
    // Allow &T <-> T at the top-level only; block &mut and other structural changes.
    if a.contains("&mut") || b.contains("&mut") {
        return false;
    }
    strip_top_level_ref(a) == strip_top_level_ref(b)
}

fn strip_top_level_ref(t: &str) -> String {
    let mut s = t.trim();
    if let Some(rest) = s.strip_prefix('&') {
        s = rest.trim_start();
        // Strip an optional lifetime: &'a T
        if s.starts_with('\'') {
            // Skip lifetime token up to whitespace.
            let mut it = s.chars();
            it.next();
            while let Some(c) = it.next() {
                if c.is_whitespace() {
                    break;
                }
            }
            s = it.as_str().trim_start();
        }
    }
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

fn type_shape_without_generic_args(t: &str) -> String {
    let stripped = strip_top_level_ref(t);
    let mut out = String::new();
    let mut depth = 0usize;
    for ch in stripped.chars() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 && !ch.is_whitespace() => out.push(ch),
            _ => {}
        }
    }
    out
}

fn is_constructor_name(name: &str) -> bool {
    matches!(name.strip_prefix("r#").unwrap_or(name), "new" | "try_new")
}

fn is_self_family_return(return_type: &Option<String>) -> bool {
    let Some(return_type) = return_type else {
        return false;
    };
    let normalized = return_type
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>();
    normalized == "Self"
        || normalized.starts_with("Result<Self,")
        || normalized.starts_with("core::result::Result<Self,")
        || normalized.starts_with("std::result::Result<Self,")
}

fn public_fn_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s*pub(\s*\(crate\))?\s+fn\s+").expect("valid regex"))
}

fn stub_macro_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\b(todo!\s*\(|unimplemented!\s*\(|compile_error!\s*\()").expect("valid regex")
    })
}

fn placeholder_text_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(placeholder|minimal implementation|compile-time availability|behavior details should be filled|collaborating types are finalized)\b",
        )
        .expect("valid regex")
    })
}

fn spec_method_code_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Allow `name()`, `name() -> T`, etc. (draft lines often document a return arrow before closing backtick.)
        Regex::new(r"`([A-Za-z_][A-Za-z0-9_]*)\s*(?:\([^`\n]*\))?(?:\s*->.*?)?`")
            .expect("valid regex")
    })
}

fn spec_method_bold_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\*\*([A-Za-z_][A-Za-z0-9_]*)\s*(?:\([^*\n]*\))?(?:\s*->.*?)?\*\*")
            .expect("valid regex")
    })
}

#[cfg(test)]
mod tests {
    use super::{
        check_guardrails, dedupe_paths, explicit_implementation_failure_message_from_stderr,
        map_src_to_spec, summarize_paths, trim_retry_context_to_budget,
    };
    use serde_json::json;
    use std::collections::HashMap;
    use std::collections::HashSet;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn map_src_to_spec_supports_nested_context_paths() {
        assert_eq!(
            map_src_to_spec("src/contexts/ui/terminal_renderer.rs"),
            Some("specifications/contexts/ui/terminal_renderer.md".to_string())
        );
    }

    #[test]
    fn map_src_to_spec_supports_nested_data_paths() {
        assert_eq!(
            map_src_to_spec("src/data/payments/ledger_entry.rs"),
            Some("specifications/data/payments/ledger_entry.md".to_string())
        );
    }

    #[test]
    fn detects_explicit_implementation_failure_in_stderr() {
        let stderr = format!(
            "error: {}\n  --> src/contexts/game_loop.rs:1:1\n",
            super::super::IMPLEMENTATION_FAILURE_MARKER
        );

        let detected = explicit_implementation_failure_message_from_stderr(&stderr)
            .expect("failure marker should be detected");

        assert!(detected.contains(super::super::IMPLEMENTATION_FAILURE_MARKER));
    }

    #[test]
    fn summarize_paths_deduplicates_and_caps_output() {
        let paths = vec![
            PathBuf::from("Cargo.toml"),
            PathBuf::from("src/main.rs"),
            PathBuf::from("src/main.rs"),
            PathBuf::from("src/lib.rs"),
            PathBuf::from("src/contexts/mod.rs"),
            PathBuf::from("src/data/mod.rs"),
            PathBuf::from("src/data/mod.rs"),
        ];

        assert_eq!(dedupe_paths(&paths).len(), 5);
        assert_eq!(
            summarize_paths(&paths),
            "Cargo.toml, src/main.rs, src/lib.rs, src/contexts/mod.rs, src/data/mod.rs"
        );
    }

    #[test]
    fn trim_retry_context_drops_optional_fields_to_fit_budget() {
        let mut context = HashMap::from([
            (
                "files_json".to_string(),
                json!(r#"{"src/main.rs":"fn main() {}"}"#),
            ),
            (
                "compiler_stderr".to_string(),
                json!("error[E0308]: mismatched types"),
            ),
            (
                "diagnostics_json".to_string(),
                json!(format!("[{}]", "\"x\"".repeat(4000))),
            ),
            (
                "recent_changes".to_string(),
                json!("src/main.rs\n".repeat(500)),
            ),
            (
                "specs_json".to_string(),
                json!(format!("{{{}}}", "\"x\"".repeat(4000))),
            ),
            (
                "contract_artifacts_json".to_string(),
                json!(format!("{{{}}}", "\"x\"".repeat(4000))),
            ),
            (
                "semantic_repair_plan".to_string(),
                json!(format!("{{{}}}", "\"x\"".repeat(4000))),
            ),
            (
                "previous_patch".to_string(),
                json!("diff --git ".repeat(2000)),
            ),
        ]);

        let estimated = trim_retry_context_to_budget(&mut context, 4000);

        assert!(estimated <= 4000, "estimated={estimated}");
        assert!(context.contains_key("files_json"));
        assert!(!context.contains_key("previous_patch"));
    }

    #[test]
    fn check_guardrails_allows_public_methods_declared_in_spec() {
        let root = make_temp_test_dir("compile_fix_guardrail_spec_allow");
        let src = root.join("src/data/gamestate.rs");
        let spec = root.join("specifications/data/gamestate.md");
        fs::create_dir_all(src.parent().expect("src parent")).expect("create src dir");
        fs::create_dir_all(spec.parent().expect("spec parent")).expect("create spec dir");
        fs::write(
            &src,
            "pub struct GameState;\nimpl GameState {\n    pub fn new() -> Self { Self }\n}\n",
        )
        .expect("write src");
        fs::write(
            &spec,
            "## Functionalities\n- **place_food**: takes Some(food) or None and returns a new GameState\n- **increment_score**: takes a positive whole number\n",
        )
        .expect("write spec");

        let diff = r#"diff --git a/src/data/gamestate.rs b/src/data/gamestate.rs
--- a/src/data/gamestate.rs
+++ b/src/data/gamestate.rs
@@ -1,3 +1,9 @@
 pub struct GameState;
 impl GameState {
     pub fn new() -> Self { Self }
+    pub fn place_food(&self) -> Self { Self }
+    pub fn increment_score(&self, amount: i64) -> Result<Self, ()> { let _ = amount; Ok(Self) }
 }
"#;

        let report = check_guardrails(&root, &root, diff).expect("guardrail report");
        assert!(report.ok, "issues: {:?}", report.issues);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn check_guardrails_allows_public_methods_declared_in_drafts_only() {
        let root = make_temp_test_dir("compile_fix_guardrail_drafts_spec");
        let src = root.join("src/contexts/command_input.rs");
        let draft = root.join("drafts/contexts/command_input.md");
        fs::create_dir_all(src.parent().expect("src parent")).expect("create src dir");
        fs::create_dir_all(draft.parent().expect("draft parent")).expect("create draft dir");
        fs::write(
            &src,
            "pub struct CommandInputContext;\nimpl CommandInputContext {\n}\n",
        )
        .expect("write src");
        fs::write(
            &draft,
            "## Functionalities\n- **next_key() -> Option<char>**\n- **next_action() -> Option<UserAction>**\n",
        )
        .expect("write draft");

        let diff = r#"diff --git a/src/contexts/command_input.rs b/src/contexts/command_input.rs
--- a/src/contexts/command_input.rs
+++ b/src/contexts/command_input.rs
@@ -1,3 +1,9 @@
 pub struct CommandInputContext;
 impl CommandInputContext {
+    pub fn next_key(&mut self) -> Option<char> { None }
+    pub fn next_action(&mut self) -> Option<()> { None }
 }
"#;

        let report = check_guardrails(&root, &root, diff).expect("guardrail report");
        assert!(report.ok, "issues: {:?}", report.issues);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn check_guardrails_allows_owned_mut_self_state_transition_without_spec() {
        let root = make_temp_test_dir("compile_fix_guardrail_owned_transition");
        let src = root.join("src/contexts/input.rs");
        fs::create_dir_all(src.parent().expect("src parent")).expect("create src dir");
        fs::write(&src, "pub struct Input;\nimpl Input {\n}\n").expect("write src");

        let diff = r#"diff --git a/src/contexts/input.rs b/src/contexts/input.rs
--- a/src/contexts/input.rs
+++ b/src/contexts/input.rs
@@ -1,3 +1,6 @@
 pub struct Input;
 impl Input {
+    pub fn poll(mut self) -> (Self, Option<char>) { (self, None) }
 }
"#;

        let report = check_guardrails(&root, &root, diff).expect("guardrail report");
        assert!(report.ok, "issues: {:?}", report.issues);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn check_guardrails_loads_spec_from_github_projection() {
        let root = make_temp_test_dir("compile_fix_guardrail_gh_spec");
        let src = root.join("src/contexts/command_input.rs");
        let gh_draft = root.join(".reen/github/demo__proj/drafts/contexts/command_input.md");
        fs::create_dir_all(src.parent().expect("src parent")).expect("create src dir");
        fs::create_dir_all(gh_draft.parent().expect("gh draft parent")).expect("create gh dir");
        fs::write(
            &src,
            "pub struct CommandInputContext;\nimpl CommandInputContext {\n}\n",
        )
        .expect("write src");
        fs::write(
            &gh_draft,
            "## Functionalities\n- **next_key() -> Option<char>**\n",
        )
        .expect("write gh draft");

        let artifact_root = root.join(".reen/github/demo__proj");

        let diff = r#"diff --git a/src/contexts/command_input.rs b/src/contexts/command_input.rs
--- a/src/contexts/command_input.rs
+++ b/src/contexts/command_input.rs
@@ -1,3 +1,6 @@
 pub struct CommandInputContext;
 impl CommandInputContext {
+    pub fn next_key(&mut self) -> Option<char> { None }
 }
"#;

        let report = check_guardrails(&root, &artifact_root, diff).expect("guardrail report");
        assert!(report.ok, "issues: {:?}", report.issues);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn check_guardrails_blocks_public_methods_missing_from_spec() {
        let root = make_temp_test_dir("compile_fix_guardrail_spec_block");
        let src = root.join("src/contexts/game_loop.rs");
        let spec = root.join("specifications/contexts/game_loop.md");
        fs::create_dir_all(src.parent().expect("src parent")).expect("create src dir");
        fs::create_dir_all(spec.parent().expect("spec parent")).expect("create spec dir");
        fs::write(
            &src,
            "pub struct GameLoopContext;\nimpl GameLoopContext {\n    pub fn new() -> Self { Self }\n}\n",
        )
        .expect("write src");
        fs::write(
            &spec,
            "## Functionalities\n- **current_board**: returns the current board\n",
        )
        .expect("write spec");

        let diff = r#"diff --git a/src/contexts/game_loop.rs b/src/contexts/game_loop.rs
--- a/src/contexts/game_loop.rs
+++ b/src/contexts/game_loop.rs
@@ -1,3 +1,8 @@
 pub struct GameLoopContext;
 impl GameLoopContext {
     pub fn new() -> Self { Self }
+    pub fn tick(mut self) -> Option<Self> { Some(self) }
 }
"#;

        let report = check_guardrails(&root, &root, diff).expect("guardrail report");
        assert!(!report.ok);
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.contains("patch adds new public function `tick`")),
            "issues: {:?}",
            report.issues
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn check_guardrails_allows_keyword_safe_alias_from_spec() {
        let root = make_temp_test_dir("compile_fix_guardrail_keyword_alias");
        let src = root.join("src/data/snake.rs");
        let snake_spec = root.join("specifications/data/snake.md");
        fs::create_dir_all(src.parent().expect("src parent")).expect("create src dir");
        fs::create_dir_all(snake_spec.parent().expect("spec parent")).expect("create spec dir");
        fs::write(
            &src,
            "pub struct Snake;\nimpl Snake {\n    pub fn new() -> Self { Self }\n}\n",
        )
        .expect("write src");
        fs::write(
            &snake_spec,
            "# Snake\n\n## Functionalities\n- **new**: Constructs a Snake\n- **move(grow)**: Moves to next().\n",
        )
        .expect("write snake spec");

        let diff = r#"diff --git a/src/data/snake.rs b/src/data/snake.rs
--- a/src/data/snake.rs
+++ b/src/data/snake.rs
@@ -1,3 +1,4 @@
 pub struct Snake;
 impl Snake {
     pub fn new() -> Self { Self }
+    pub fn move_snake(mut self, grow: bool) -> Self { let _ = grow; self }
 }
"#;

        let report = check_guardrails(&root, &root, diff).expect("guardrail report");
        assert!(report.ok, "issues: {:?}", report.issues);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn check_guardrails_blocks_data_methods_sourced_only_from_context_roles() {
        let root = make_temp_test_dir("compile_fix_guardrail_data_role_leak");
        let src = root.join("src/data/snake.rs");
        let snake_spec = root.join("specifications/data/snake.md");
        let game_loop_spec = root.join("specifications/contexts/game_loop.md");
        fs::create_dir_all(src.parent().expect("src parent")).expect("create src dir");
        fs::create_dir_all(snake_spec.parent().expect("spec parent")).expect("create spec dir");
        fs::create_dir_all(game_loop_spec.parent().expect("spec parent")).expect("create spec dir");
        fs::write(
            &src,
            "pub struct Snake;\nimpl Snake {\n    pub fn new() -> Self { Self }\n}\n",
        )
        .expect("write src");
        fs::write(
            &snake_spec,
            "# Snake\n\n## Functionalities\n- **new**: Constructs a Snake\n",
        )
        .expect("write snake spec");
        fs::write(
            &game_loop_spec,
            "### snake\n| Method | Description |\n|---|---|\n| **move(grow)** | Moves to next(). |\n",
        )
        .expect("write game loop spec");

        let diff = r#"diff --git a/src/data/snake.rs b/src/data/snake.rs
--- a/src/data/snake.rs
+++ b/src/data/snake.rs
@@ -1,3 +1,4 @@
 pub struct Snake;
 impl Snake {
     pub fn new() -> Self { Self }
+    pub fn move_snake(&self, grow: bool) -> Self { let _ = grow; Self }
 }
"#;

        let report = check_guardrails(&root, &root, diff).expect("guardrail report");
        assert!(!report.ok);
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.contains("patch adds new public function `move_snake`")),
            "issues: {:?}",
            report.issues
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn check_guardrails_allows_constructor_additions_even_if_not_declared() {
        let root = make_temp_test_dir("compile_fix_guardrail_constructor_add");
        let src = root.join("src/data/board.rs");
        let spec = root.join("specifications/data/board.md");
        fs::create_dir_all(src.parent().expect("src parent")).expect("create src dir");
        fs::create_dir_all(spec.parent().expect("spec parent")).expect("create spec dir");
        fs::write(&src, "pub struct Board;\n").expect("write src");
        fs::write(&spec, "# Board\n\n## Functionalities\n").expect("write spec");

        let diff = r#"diff --git a/src/data/board.rs b/src/data/board.rs
--- a/src/data/board.rs
+++ b/src/data/board.rs
@@ -1 +1,5 @@
 pub struct Board;
+impl Board {
+    pub fn new(width: u32, height: u32) -> Self { let _ = (width, height); Self }
+}
"#;

        let report = check_guardrails(&root, &root, diff).expect("guardrail report");
        assert!(report.ok, "issues: {:?}", report.issues);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn check_guardrails_allows_mut_self_to_borrowed_self_for_self_returning_api() {
        let root = make_temp_test_dir("compile_fix_guardrail_immutable_transform");
        let src = root.join("src/data/gamestate.rs");
        let spec = root.join("specifications/data/gamestate.md");
        fs::create_dir_all(src.parent().expect("src parent")).expect("create src dir");
        fs::create_dir_all(spec.parent().expect("spec parent")).expect("create spec dir");
        fs::write(
            &src,
            "pub struct GameState;\nimpl GameState {\n    pub fn increment_score(mut self, amount: i64) -> Result<Self, ()> { let _ = amount; Ok(self) }\n}\n",
        )
        .expect("write src");
        fs::write(
            &spec,
            "# GameState\n\n## Functionalities\n- **increment_score**: returns a new GameState with score increased\n",
        )
        .expect("write spec");

        let diff = r#"diff --git a/src/data/gamestate.rs b/src/data/gamestate.rs
--- a/src/data/gamestate.rs
+++ b/src/data/gamestate.rs
@@ -1,3 +1,3 @@
 pub struct GameState;
 impl GameState {
-    pub fn increment_score(mut self, amount: i64) -> Result<Self, ()> { let _ = amount; Ok(self) }
+    pub fn increment_score(&self, amount: i64) -> Result<Self, ()> { let _ = amount; Ok(Self) }
 }
"#;

        let report = check_guardrails(&root, &root, diff).expect("guardrail report");
        assert!(report.ok, "issues: {:?}", report.issues);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn check_guardrails_allows_self_receiver_return_type_with_added_generic_arity() {
        let root = make_temp_test_dir("compile_fix_guardrail_self_shape_generics");
        let src = root.join("src/contexts/game_loop.rs");
        let spec = root.join("specifications/contexts/game_loop.md");
        fs::create_dir_all(src.parent().expect("src parent")).expect("create src dir");
        fs::create_dir_all(spec.parent().expect("spec parent")).expect("create spec dir");
        fs::write(
            &src,
            "pub struct GameLoopContext<F>(F);\nimpl<F> GameLoopContext<F> {\n    pub fn tick(self) -> Option<GameLoopContext<F>> { None }\n}\n",
        )
        .expect("write src");
        fs::write(
            &spec,
            "## Functionalities\n- **tick()** returns Some(new GameLoopContext) or None\n",
        )
        .expect("write spec");

        let diff = r#"diff --git a/src/contexts/game_loop.rs b/src/contexts/game_loop.rs
--- a/src/contexts/game_loop.rs
+++ b/src/contexts/game_loop.rs
@@ -1,4 +1,4 @@
-pub struct GameLoopContext<F>(F);
-impl<F> GameLoopContext<F> {
-    pub fn tick(self) -> Option<GameLoopContext<F>> { None }
+pub struct GameLoopContext<F, S>(F, S);
+impl<F, S> GameLoopContext<F, S> {
+    pub fn tick(self) -> Option<GameLoopContext<F, S>> { None }
 }
"#;

        let report = check_guardrails(&root, &root, diff).expect("guardrail report");
        assert!(report.ok, "issues: {:?}", report.issues);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn check_guardrails_blocks_removing_explicit_implementation_failure_marker() {
        let root = make_temp_test_dir("compile_fix_guardrail_impl_refusal");
        let src = root.join("src/contexts/game_loop.rs");
        let spec = root.join("specifications/contexts/game_loop.md");
        fs::create_dir_all(src.parent().expect("src parent")).expect("create src dir");
        fs::create_dir_all(spec.parent().expect("spec parent")).expect("create spec dir");
        fs::write(
            &src,
            format!(
                "compile_error!(\"{}\");\n",
                super::super::IMPLEMENTATION_FAILURE_MARKER
            ),
        )
        .expect("write src");
        fs::write(
            &spec,
            "## Functionalities\n- **tick()** returns Some(new GameLoopContext) or None\n",
        )
        .expect("write spec");

        let diff = format!(
            "diff --git a/src/contexts/game_loop.rs b/src/contexts/game_loop.rs\n--- a/src/contexts/game_loop.rs\n+++ b/src/contexts/game_loop.rs\n@@ -1 +1,7 @@\n-compile_error!(\"{}\");\n+pub struct GameLoopContext;\n+impl GameLoopContext {{\n+    // placeholder implementation to restore compilation\n+    pub fn tick(self) -> Option<Self> {{ Some(self) }}\n+}}\n",
            super::super::IMPLEMENTATION_FAILURE_MARKER
        );

        let report = check_guardrails(&root, &root, &diff).expect("guardrail report");
        assert!(!report.ok);
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.contains("implementation-refusal marker")),
            "issues: {:?}",
            report.issues
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn check_guardrails_blocks_placeholder_style_text() {
        let root = make_temp_test_dir("compile_fix_guardrail_placeholder_text");
        let src = root.join("src/main.rs");
        fs::create_dir_all(src.parent().expect("src parent")).expect("create src dir");
        fs::write(&src, "fn main() {}\n").expect("write src");

        let diff = r#"diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -1 +1,4 @@
 fn main() {}
+// placeholder implementation until collaborators are finalized
+// minimal implementation focused on compile-time availability
"#;

        let report = check_guardrails(&root, &root, diff).expect("guardrail report");
        assert!(!report.ok);
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.contains("placeholder-style implementation text")),
            "issues: {:?}",
            report.issues
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn keyword_alias_rule_only_applies_to_keywords() {
        let allowed = HashSet::from(["move".to_string(), "type".to_string(), "score".to_string()]);

        assert!(super::is_spec_allowed_public_method_name(
            "move_snake",
            &allowed
        ));
        assert!(super::is_spec_allowed_public_method_name(
            "type_name",
            &allowed
        ));
        assert!(super::is_spec_allowed_public_method_name(
            "r#move", &allowed
        ));
        assert!(!super::is_spec_allowed_public_method_name(
            "score_value",
            &allowed
        ));
    }

    fn make_temp_test_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("{}_{}", prefix, nanos));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }
}
