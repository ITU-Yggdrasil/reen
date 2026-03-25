use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

mod agent_executor;
mod artifact_backend;
mod cargo_commands;
mod compilation_fix;
mod contracts;
mod dependency_graph;
mod dependency_tooling;
mod external_api_expansion;
mod interface_capsules;
mod openapi_fetcher;
mod patch_service;
mod planning;
mod pipeline_context;
mod pipeline_quality;
mod progress;
mod project_structure;
mod rate_limiter;
mod stage_runner;
pub mod yaml_config;

use agent_executor::{AgentExecutor, AgentResponse};
use artifact_backend::{
    ArtifactCategory, ArtifactKind, ArtifactStore, BackendSelection, build_artifact_store,
};
use contracts::{
    ContractArtifact, build_contract_artifact, contract_artifact_to_context_value,
    contract_validation_to_context_value, validate_contract_artifact,
};
use dependency_graph::{
    DependencyArtifact, ExecutionNode, build_execution_plan, expand_with_transitive_dependencies,
};
use dependency_tooling::{
    ensure_tooling_artifacts_fresh, load_dependency_manifest, load_symbols_context,
    merge_manifest_dependencies,
};
use external_api_expansion::{
    GeneratedDraftArtifact, parse_external_api_expansion, sanitize_generated_artifact_name,
};
use interface_capsules::{InterfaceCapsule, build_interface_capsule};
use openapi_fetcher::is_external_api_draft_path;
use patch_service::apply_draft_patches;
use planning::{
    ExecutionPlan, PlanKind, build_default_plan, parse_plan_output, plan_to_context_value,
    validate_plan, validation_to_context_value,
};
use pipeline_context::{build_specification_context, fit_context_to_token_limit};
use pipeline_quality::{
    analyze_specification, contract_to_context_value, verify_generated_implementation,
    write_json_report,
};
use progress::{ProgressIndicator, print_timed_status};
use project_structure::{
    ProjectInfo, analyze_specifications, generate_cargo_toml, generate_lib_rs, generate_mod_files,
};
use reen::build_tracker::{BuildTracker, Stage};
use reen::execution::{AgentModelRegistry, AgentRegistry, NativeExecutionControl};
use reen::registries::{FileAgentModelRegistry, FileAgentRegistry};
use stage_runner::{
    CliExecutionControl, ExecutionResources, StageItem, is_rate_limit_error,
    prepare_rate_limit_retry, run_stage_items,
};

#[derive(Clone)]
pub struct Config {
    pub verbose: bool,
    pub dry_run: bool,
    pub github_repo: Option<String>,
}

#[derive(Clone)]
pub(crate) struct WorkspaceContext {
    store: Arc<dyn ArtifactStore>,
    drafts_root: PathBuf,
    specifications_root: PathBuf,
    drafts_dir: String,
    specifications_dir: String,
}

impl WorkspaceContext {
    /// Single root for the active backend’s `drafts/` + `specifications/` trees (cwd for files,
    /// `.reen/github/<owner>__<repo>` for GitHub). Downstream code should not merge this with a second root.
    pub(crate) fn artifact_workspace_root(&self) -> PathBuf {
        self.store.artifact_workspace_root()
    }

    pub(crate) fn resolve(config: &Config) -> Result<Self> {
        let backend = BackendSelection::from_repo_spec(config.github_repo.as_deref())?;
        let store = build_artifact_store(&backend)?;
        let drafts_root = store.drafts_root().to_path_buf();
        let specifications_root = store.specifications_root().to_path_buf();
        Ok(Self {
            store,
            drafts_dir: drafts_root.to_string_lossy().into_owned(),
            specifications_dir: specifications_root.to_string_lossy().into_owned(),
            drafts_root,
            specifications_root,
        })
    }
}

pub fn resolve_github_repo(cli_github: Option<&str>) -> Result<Option<String>> {
    if let Some(repo) = cli_github {
        return Ok(Some(repo.trim().to_string()));
    }
    Ok(yaml_config::load_config()?.github)
}

pub fn ensure_create_preconditions(config: &Config) -> Result<()> {
    if config.dry_run {
        return Ok(());
    }
    let workspace = WorkspaceContext::resolve(config)?;
    ensure_tooling_artifacts_fresh(
        &workspace.drafts_root,
        &workspace.artifact_workspace_root(),
        config.verbose,
    )
}

/// Resolves rate limit (requests per second) from CLI, env, or registry.
/// Precedence: cli_arg > REEN_RATE_LIMIT env > agent_model_registry.yml rate_limit.
pub fn resolve_rate_limit(cli_arg: Option<f64>) -> Option<f64> {
    if let Some(r) = cli_arg {
        return Some(r);
    }
    if let Ok(s) = env::var("REEN_RATE_LIMIT") {
        if let Ok(r) = s.parse::<f64>() {
            return Some(r);
        }
    }
    FileAgentModelRegistry::new(None, None, None).get_rate_limit()
}

/// Resolves token limit (tokens per minute) from CLI, env, or registry.
/// Precedence: cli_arg > REEN_TOKEN_LIMIT env > agent_model_registry.yml token_limit.
pub fn resolve_token_limit(cli_arg: Option<f64>) -> Option<f64> {
    if let Some(t) = cli_arg {
        return Some(t);
    }
    if let Ok(s) = env::var("REEN_TOKEN_LIMIT") {
        if let Ok(t) = s.parse::<f64>() {
            return Some(t);
        }
    }
    FileAgentModelRegistry::new(None, None, None).get_token_limit()
}

/// Controls which draft categories are included when resolving input files.
/// When both fields are false, all categories are included (no filter).
/// When one or both are true, only the selected categories are included.
#[derive(Clone, Copy)]
pub struct CategoryFilter {
    pub contexts: bool,
    pub data: bool,
}

impl CategoryFilter {
    pub fn all() -> Self {
        Self {
            contexts: false,
            data: false,
        }
    }

    fn is_active(&self) -> bool {
        self.contexts || self.data
    }

    fn include_data(&self) -> bool {
        !self.is_active() || self.data
    }

    fn include_contexts(&self) -> bool {
        !self.is_active() || self.contexts
    }

    fn include_root(&self) -> bool {
        !self.is_active()
    }

    /// Returns true if the given path (relative to a base dir like "drafts" or
    /// "specifications") belongs to a category this filter includes.
    fn matches_path(&self, path: &Path, base_dir: &str) -> bool {
        if !self.is_active() {
            return true;
        }
        let base = PathBuf::from(base_dir);
        if let Ok(relative) = path.strip_prefix(&base) {
            if let Some(first) = relative.components().next() {
                let component = first.as_os_str().to_string_lossy();
                return match component.as_ref() {
                    "data" => self.include_data(),
                    "contexts" | "external_apis" | "apis" => self.include_contexts(),
                    _ => self.include_root(),
                };
            }
        }
        self.include_root()
    }
}

const DRAFTS_DIR: &str = "drafts";
const SPECIFICATIONS_DIR: &str = "specifications";

/// Outcome of processing a single draft for specification creation.
#[derive(Debug)]
pub enum ProcessSpecOutcome {
    Success,
    BlockingAmbiguities {
        draft_file: PathBuf,
        draft_name: String,
        draft_content: String,
        spec_content: String,
        actionable: Vec<String>,
        additional_context: HashMap<String, serde_json::Value>,
    },
}

struct PreparedImplementationBatchItem {
    context_name: String,
    context_file: PathBuf,
    output_path: PathBuf,
    dependency_fingerprint: String,
    implementation_plan: ExecutionPlan,
    dependency_context: HashMap<String, serde_json::Value>,
    context_content: String,
    prepared: reen::execution::PreparedExecution,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BddTestPaths {
    feature_path: PathBuf,
    steps_path: PathBuf,
    runner_path: PathBuf,
    runner_test_name: String,
}

const BDD_CUCUMBER_VERSION: &str = "0.22.1";
const BDD_TOKIO_SPEC: &str = r#"{ version = "1.40", features = ["macros", "rt-multi-thread"] }"#;
const BDD_TEST_TARGETS_START: &str = "# reen:bdd-tests:start";
const BDD_TEST_TARGETS_END: &str = "# reen:bdd-tests:end";
pub(crate) const IMPLEMENTATION_FAILURE_MARKER: &str =
    "ERROR: Cannot implement specification as written.";

pub async fn create_specification(
    names: Vec<String>,
    clear_cache: bool,
    filter: &CategoryFilter,
    rate_limit: Option<f64>,
    token_limit: Option<f64>,
    fix: bool,
    max_fix_attempts: usize,
    config: &Config,
) -> Result<()> {
    create_specification_inner(
        names,
        clear_cache,
        filter,
        rate_limit,
        token_limit,
        fix,
        max_fix_attempts,
        0,
        config,
    )
    .await
}

fn create_specification_inner(
    names: Vec<String>,
    clear_cache: bool,
    filter: &CategoryFilter,
    rate_limit: Option<f64>,
    token_limit: Option<f64>,
    fix: bool,
    max_fix_attempts: usize,
    fix_attempt: usize,
    config: &Config,
) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
    let filter = *filter;
    let config = config.clone();
    Box::pin(async move {
        let workspace = WorkspaceContext::resolve(&config)?;
        let names_provided = !names.is_empty();
        let draft_artifacts =
            workspace
                .store
                .resolve_inputs(ArtifactKind::Draft, names, &filter)?;

        if draft_artifacts.is_empty() {
            println!("No draft files found to process");
            return Ok(());
        }

        let dependency_roots =
            workspace
                .store
                .select_dependency_roots(draft_artifacts, names_provided, &filter)?;
        let expanded_draft_files = expand_with_transitive_dependencies(
            dependency_roots
                .iter()
                .map(|artifact| artifact.path.clone())
                .collect(),
            &workspace.drafts_dir,
            None,
        )?;
        let filtered_draft_files = if filter.is_active() {
            expanded_draft_files
                .into_iter()
                .filter(|f| filter.matches_path(f, &workspace.drafts_dir))
                .collect()
        } else {
            expanded_draft_files
        };
        let execution_levels =
            build_execution_plan(filtered_draft_files, &workspace.drafts_dir, None)?;

        // Load build tracker
        let mut tracker = BuildTracker::load()?;

        let total_count: usize = execution_levels.iter().map(|level| level.len()).sum();
        println!("Creating specifications for {} draft(s)", total_count);

        let resources = ExecutionResources::new(rate_limit, token_limit);

        let mut progress = ProgressIndicator::new(total_count);
        let mut updated_count = 0;
        let mut updated_in_run: HashSet<String> = HashSet::new();
        let mut executors: HashMap<String, Arc<AgentExecutor>> = HashMap::new();

        for (level_idx, level_nodes) in execution_levels.into_iter().enumerate() {
            if config.verbose {
                println!(
                    "Processing dependency level {} ({} item(s))",
                    level_idx,
                    level_nodes.len()
                );
            }

            let mut nodes_by_agent: HashMap<String, Vec<ExecutionNode>> = HashMap::new();
            for node in level_nodes {
                let agent = determine_specification_agent(&node.input_path, &workspace.drafts_dir)
                    .to_string();
                nodes_by_agent.entry(agent).or_default().push(node);
            }

            for (agent_name, nodes) in nodes_by_agent {
                if !executors.contains_key(&agent_name) {
                    executors.insert(
                        agent_name.clone(),
                        Arc::new(AgentExecutor::new(&agent_name, &config)?),
                    );
                }
                let executor = executors
                    .get(&agent_name)
                    .cloned()
                    .context("missing specification executor")?;
                let can_parallel = executor.can_run_parallel().unwrap_or(false);
                if can_parallel && config.verbose {
                    println!("Parallel execution enabled for {}", agent_name);
                }

                let mut stage_items = Vec::new();
                for node in nodes {
                    let draft_file = node.input_path.clone();
                    let draft_name = node.name.clone();
                    let dependency_invalidated = node
                        .direct_dependency_names()
                        .iter()
                        .any(|dep_name| updated_in_run.contains(dep_name));
                    let dependency_fingerprint =
                        dependency_fingerprint_for_node(&node, &workspace.drafts_dir, None)?;
                    let output_path = determine_specification_output_path(
                        &draft_file,
                        &workspace.drafts_dir,
                        &workspace.specifications_dir,
                    )?;

                    let needs_update = if clear_cache || dependency_invalidated {
                        true
                    } else {
                        tracker.needs_update(
                            Stage::Specification,
                            &draft_name,
                            &draft_file,
                            &output_path,
                            &dependency_fingerprint,
                        )?
                    };
                    if !needs_update {
                        progress.start_item_up_to_date(&draft_name);
                        if config.verbose {
                            println!("⊚ Skipping {} (up to date)", draft_name);
                        }
                        progress.complete_item(&draft_name, true);
                        continue;
                    }

                    let dependency_context =
                        match build_dependency_context(&node, &workspace.drafts_dir, None) {
                            Ok(context) => context,
                            Err(e) => {
                                progress.complete_item(&draft_name, false);
                                eprintln!(
                                    "✗ Failed to create specification for {}: {}",
                                    draft_name, e
                                );
                                continue;
                            }
                        };

                    let draft_content = fs::read_to_string(&draft_file).unwrap_or_default();
                    let dependency_context = build_specification_context(
                        &draft_file,
                        &draft_content,
                        dependency_context,
                        &workspace.drafts_dir,
                    )?;
                    let (dependency_context, estimated) = fit_context_to_token_limit(
                        &executor,
                        &draft_content,
                        dependency_context,
                        token_limit,
                    )?;
                    let cache_hit = if clear_cache {
                        false
                    } else {
                        executor
                            .is_cache_hit(&draft_content, dependency_context.clone())
                            .unwrap_or(false)
                    };
                    stage_items.push(StageItem {
                        name: draft_name.clone(),
                        estimated,
                        cache_hit,
                        payload: (
                            draft_file,
                            draft_name,
                            output_path,
                            dependency_fingerprint,
                            draft_content,
                            dependency_context,
                        ),
                    });
                }

                let cfg = config.clone();
                let workspace_ctx = workspace.clone();
                let executor_clone = executor.clone();
                let results = run_stage_items(
                    stage_items,
                    can_parallel,
                    &mut progress,
                    &resources,
                    &config,
                    move |(
                        draft_file,
                        draft_name,
                        output_path,
                        dependency_fingerprint,
                        draft_content,
                        dependency_context,
                    ),
                          execution_control| {
                        let executor = executor_clone.clone();
                        let cfg = cfg.clone();
                        let workspace_ctx = workspace_ctx.clone();
                        async move {
                            let result = process_specification(
                                &executor,
                                &draft_content,
                                &draft_file,
                                &draft_name,
                                &workspace_ctx,
                                &cfg,
                                clear_cache,
                                dependency_context,
                                execution_control,
                            )
                            .await?;
                            Ok((draft_file, output_path, dependency_fingerprint, result))
                        }
                    },
                )
                .await?;

                for (draft_name, result) in results {
                    match result {
                        Ok((
                            draft_file,
                            output_path,
                            dependency_fingerprint,
                            ProcessSpecOutcome::Success,
                        )) => {
                            tracker.record(
                                Stage::Specification,
                                &draft_name,
                                &draft_file,
                                &output_path,
                                &dependency_fingerprint,
                            )?;
                            tracker.save()?;
                            updated_count += 1;
                            updated_in_run.insert(draft_name.clone());
                            progress.complete_item(&draft_name, true);
                            if config.verbose {
                                println!("✓ Successfully created specification for {}", draft_name);
                            }
                        }
                        Ok((
                            _draft_file,
                            _output_path,
                            _dependency_fingerprint,
                            ProcessSpecOutcome::BlockingAmbiguities {
                                draft_file: ba_draft_file,
                                draft_name: ba_draft_name,
                                draft_content: ba_draft_content,
                                spec_content: ba_spec_content,
                                actionable: ba_actionable,
                                additional_context: ba_context,
                            },
                        )) => {
                            progress.complete_item(&draft_name, false);
                            if fix && fix_attempt < max_fix_attempts {
                                return try_fix_and_retry(
                                    &ba_draft_file,
                                    &ba_draft_name,
                                    &ba_draft_content,
                                    &ba_spec_content,
                                    &ba_actionable,
                                    ba_context,
                                    fix_attempt,
                                    max_fix_attempts,
                                    &filter,
                                    rate_limit,
                                    token_limit,
                                    &config,
                                    resources.execution_control.clone(),
                                )
                                .await;
                            }
                            eprintln!("✗ Blocking ambiguities (use --fix to auto-fix drafts)");
                            anyhow::bail!("generated specification contains blocking ambiguities");
                        }
                        Err(e) => {
                            progress.complete_item(&draft_name, false);
                            eprintln!("✗ Failed to create specification for {}: {}", draft_name, e);
                            anyhow::bail!("{}", e);
                        }
                    }
                }
            }
        }

        // Save tracker
        tracker.save()?;

        progress.finish();

        if updated_count == 0 && config.verbose {
            println!("All specifications are up to date");
        }

        Ok(())
    })
}

pub async fn check_specification(names: Vec<String>, config: &Config) -> Result<()> {
    let workspace = WorkspaceContext::resolve(config)?;
    let draft_artifacts =
        workspace
            .store
            .resolve_inputs(ArtifactKind::Draft, names, &CategoryFilter::all())?;
    if draft_artifacts.is_empty() {
        println!("No draft files found to process");
        return Ok(());
    }

    let tracker = BuildTracker::load()?;
    let mut issues = 0usize;
    println!(
        "Checking specifications for {} draft(s)",
        draft_artifacts.len()
    );

    for draft_artifact in draft_artifacts {
        let draft_file = draft_artifact.path.clone();
        let draft_name = draft_artifact.name.clone();
        let Some(spec_artifact) = workspace
            .store
            .find_specification_for_draft(&draft_artifact)?
        else {
            let spec_path = determine_specification_output_path(
                &draft_file,
                &workspace.drafts_dir,
                &workspace.specifications_dir,
            )?;
            issues += 1;
            eprintln!("error[spec:missing]:");
            eprintln!("\u{001b}[31m{}\u{001b}[0m", draft_file.display());
            eprintln!("  Missing specification artifact for '{}'.", draft_name);
            eprintln!("  Expected at: {}", spec_path.display());
            if tracker.has_track(Stage::Specification, &draft_name) {
                eprintln!(
                    "  note: Build tracker contains a cache entry for '{}'; artifact may have been removed after generation.",
                    draft_name
                );
            }
            eprintln!();
            continue;
        };
        let spec_path = spec_artifact.path.clone();
        let spec_content = workspace.store.read_content(&spec_artifact).or_else(|_| {
            fs::read_to_string(&spec_path).with_context(|| {
                format!("Failed to read specification file: {}", spec_path.display())
            })
        })?;
        if let Some(blocking) = extract_blocking_ambiguities_section(&spec_content) {
            let actionable =
                extract_actionable_blocking_bullets_for_path(&blocking, Some(&spec_path));
            if !actionable.is_empty() {
                issues += 1;
                eprintln!("error[spec:blocking]:");
                eprintln!("\u{001b}[31m{}\u{001b}[0m", draft_file.display());
                eprintln!(
                    "  Blocking Ambiguities detected in specification for '{}'.",
                    draft_name
                );
                eprintln!();
                for bullet in actionable {
                    eprintln!("  {}", bullet);
                }
                eprintln!();
            }
        }
    }

    if issues > 0 {
        anyhow::bail!("Specification check failed with {} issue(s).", issues);
    }

    println!("✓ Specifications check passed");
    Ok(())
}

async fn process_specification(
    executor: &AgentExecutor,
    draft_content: &str,
    draft_file: &Path,
    draft_name: &str,
    workspace: &WorkspaceContext,
    config: &Config,
    ignore_cache_reads: bool,
    additional_context: HashMap<String, serde_json::Value>,
    execution_control: Option<CliExecutionControl>,
) -> Result<ProcessSpecOutcome> {
    if config.dry_run {
        println!("[DRY RUN] Would create specification for: {}", draft_name);
        return Ok(ProcessSpecOutcome::Success);
    }

    if is_external_api_draft_path(draft_file, &workspace.drafts_dir) {
        return process_external_api_specification(
            executor,
            draft_content,
            draft_file,
            draft_name,
            workspace,
            config,
            ignore_cache_reads,
            additional_context,
            execution_control,
        )
        .await;
    }

    // Use conversational execution to handle questions
    let spec_content = executor
        .execute_with_conversation_with_seed_options(
            &draft_content,
            draft_name,
            additional_context.clone(),
            execution_control
                .as_ref()
                .map(|control| control as &dyn NativeExecutionControl),
            ignore_cache_reads,
        )
        .await?;

    finalize_specification_output(
        draft_content,
        draft_file,
        draft_name,
        workspace,
        spec_content,
        additional_context,
    )
}

#[derive(Debug)]
struct WrittenSpecification {
    spec_content: String,
    actionable: Vec<String>,
    has_blocking_ambiguities: bool,
}

async fn process_external_api_specification(
    executor: &AgentExecutor,
    draft_content: &str,
    draft_file: &Path,
    draft_name: &str,
    workspace: &WorkspaceContext,
    config: &Config,
    ignore_cache_reads: bool,
    additional_context: HashMap<String, serde_json::Value>,
    execution_control: Option<CliExecutionControl>,
) -> Result<ProcessSpecOutcome> {
    let expansion = execute_external_api_expansion_with_cache_recovery(
        executor,
        draft_content,
        draft_name,
        config,
        ignore_cache_reads,
        additional_context.clone(),
        execution_control.clone(),
    )
    .await?;

    let data_executor = AgentExecutor::new("create_specifications_data", config)?;
    let context_executor = AgentExecutor::new("create_specifications_context", config)?;
    let namespace = sanitize_generated_artifact_name(&expansion.api_name);
    let generated_context = prune_generated_spec_context(additional_context.clone());

    clear_external_generated_output_dirs(&workspace.specifications_dir, &namespace)?;

    let primary_context_path = determine_specification_output_path(
        draft_file,
        &workspace.drafts_dir,
        &workspace.specifications_dir,
    )?;
    let mut used_data_names = HashMap::new();
    let mut used_context_names = HashMap::new();
    let mut generated = Vec::new();

    for artifact in &expansion.data_drafts {
        let file_stem = unique_generated_name(
            &sanitize_generated_artifact_name(&artifact.name),
            &mut used_data_names,
        );
        let output_path = external_generated_data_output_path(
            &workspace.specifications_dir,
            &namespace,
            &file_stem,
        );
        let result = generate_external_spec_artifact(
            &data_executor,
            artifact,
            draft_file,
            &artifact.name,
            output_path,
            ArtifactCategory::Data,
            workspace,
            generated_context.clone(),
            ignore_cache_reads,
            execution_control.clone(),
        )
        .await?;
        generated.push(result);
    }

    let mut context_iter = expansion.context_drafts.iter();
    if let Some(primary_context) = context_iter.next() {
        let result = generate_external_spec_artifact(
            &context_executor,
            primary_context,
            draft_file,
            &primary_context.name,
            primary_context_path,
            ArtifactCategory::Api,
            workspace,
            generated_context.clone(),
            ignore_cache_reads,
            execution_control.clone(),
        )
        .await?;
        generated.push(result);
    }
    for artifact in context_iter {
        let file_stem = unique_generated_name(
            &sanitize_generated_artifact_name(&artifact.name),
            &mut used_context_names,
        );
        let output_path = external_generated_context_output_path(
            &workspace.specifications_dir,
            &namespace,
            &file_stem,
        );
        let result = generate_external_spec_artifact(
            &context_executor,
            artifact,
            draft_file,
            &artifact.name,
            output_path,
            ArtifactCategory::Context,
            workspace,
            generated_context.clone(),
            ignore_cache_reads,
            execution_control.clone(),
        )
        .await?;
        generated.push(result);
    }

    let mut actionable = Vec::new();
    let mut combined_spec_content = String::new();
    for (artifact_name, written) in generated {
        if !combined_spec_content.is_empty() {
            combined_spec_content.push_str("\n\n");
        }
        combined_spec_content.push_str(&format!("<!-- {} -->\n", artifact_name));
        combined_spec_content.push_str(&written.spec_content);
        if written.has_blocking_ambiguities {
            actionable.extend(
                written
                    .actionable
                    .into_iter()
                    .map(|item| format!("[{}] {}", artifact_name, item)),
            );
        }
    }

    if !actionable.is_empty() {
        return Ok(ProcessSpecOutcome::BlockingAmbiguities {
            draft_file: draft_file.to_path_buf(),
            draft_name: draft_name.to_string(),
            draft_content: draft_content.to_string(),
            spec_content: combined_spec_content,
            actionable,
            additional_context,
        });
    }

    Ok(ProcessSpecOutcome::Success)
}

async fn execute_external_api_expansion_with_cache_recovery(
    executor: &AgentExecutor,
    draft_content: &str,
    draft_name: &str,
    config: &Config,
    ignore_cache_reads: bool,
    additional_context: HashMap<String, serde_json::Value>,
    execution_control: Option<CliExecutionControl>,
) -> Result<external_api_expansion::ExternalApiExpansion> {
    let execute_once = |context: HashMap<String, serde_json::Value>| async {
        executor
            .execute_with_conversation_with_seed_options(
                draft_content,
                draft_name,
                context,
                execution_control
                    .as_ref()
                    .map(|control| control as &dyn NativeExecutionControl),
                ignore_cache_reads,
            )
            .await
    };

    let expansion_output = execute_once(additional_context.clone()).await?;
    match parse_external_api_expansion(&expansion_output, draft_name) {
        Ok(expansion) => Ok(expansion),
        Err(parse_error) => {
            if ignore_cache_reads {
                return Err(parse_error);
            }
            let cache_hit = executor
                .is_cache_hit(draft_content, additional_context.clone())
                .unwrap_or(false);
            if !cache_hit {
                return Err(parse_error);
            }

            clear_specific_agent_response_cache_entry(
                "create_specifications_external_api",
                CacheAgentInput {
                    draft_content: Some(draft_content.to_string()),
                    context_content: None,
                    additional: additional_context.clone(),
                },
                config,
            )?;

            eprintln!(
                "warning[spec:cache]: cleared stale cached external API expansion for '{}'; retrying with a fresh model response",
                draft_name
            );

            let retry_output = execute_once(additional_context).await?;
            parse_external_api_expansion(&retry_output, draft_name).with_context(|| {
                format!(
                    "external API expansion output was not valid JSON after clearing the cached response for '{}'",
                    draft_name
                )
            })
        }
    }
}

async fn generate_external_spec_artifact(
    executor: &AgentExecutor,
    artifact: &GeneratedDraftArtifact,
    source_draft_file: &Path,
    display_name: &str,
    output_path: PathBuf,
    spec_category: ArtifactCategory,
    workspace: &WorkspaceContext,
    additional_context: HashMap<String, serde_json::Value>,
    ignore_cache_reads: bool,
    execution_control: Option<CliExecutionControl>,
) -> Result<(String, WrittenSpecification)> {
    let lint_context = additional_context.clone();
    let spec_content = executor
        .execute_with_conversation_with_seed_options(
            &artifact.draft_markdown,
            &artifact.name,
            additional_context,
            execution_control
                .as_ref()
                .map(|control| control as &dyn NativeExecutionControl),
            ignore_cache_reads,
        )
        .await?;
    let written = write_specification_output(
        workspace,
        source_draft_file,
        display_name,
        display_name,
        spec_category,
        &output_path,
        spec_content,
        Some(&lint_context),
    )?;
    Ok((display_name.to_string(), written))
}

async fn generate_execution_plan_with_agent(
    planner: &AgentExecutor,
    plan_kind: PlanKind,
    spec_path: &Path,
    display_name: &str,
    spec_content: &str,
    output_paths: &[PathBuf],
    dependency_context: &HashMap<String, serde_json::Value>,
    diagnostic_text: Option<&str>,
    token_limit: Option<f64>,
    ignore_cache_reads: bool,
    execution_control: Option<CliExecutionControl>,
) -> Result<(ExecutionPlan, planning::PlanValidationReport)> {
    let status_label = match plan_kind {
        PlanKind::Implementation => "Planning implementation",
        PlanKind::SemanticRepair => "Planning repair",
    };
    print_timed_status(status_label, display_name);

    let behavior_contract = analyze_specification(spec_path, spec_content, Some(dependency_context)).contract;
    let contract_artifact = build_contract_artifact(
        spec_path,
        spec_content,
        output_paths.first().map(PathBuf::as_path),
        Some(dependency_context),
    );
    let contract_validation =
        validate_contract_artifact(&contract_artifact, spec_path, spec_content, Some(dependency_context));
    let fallback_plan = build_default_plan(
        plan_kind,
        spec_path,
        spec_content,
        output_paths,
        dependency_context,
        diagnostic_text,
    );

    let mut planning_context = compact_agent_dependency_context(dependency_context);
    planning_context.insert("plan_kind".to_string(), json!(plan_kind.as_str()));
    planning_context.insert("context_content".to_string(), json!(spec_content));
    planning_context.insert(
        "contract_artifact".to_string(),
        contract_artifact_to_context_value(&contract_artifact),
    );
    planning_context.insert(
        "contract_validation".to_string(),
        contract_validation_to_context_value(&contract_validation),
    );
    planning_context.insert(
        "behavior_contract".to_string(),
        contract_to_context_value(&behavior_contract),
    );
    planning_context.insert(
        "default_plan".to_string(),
        plan_to_context_value(&fallback_plan),
    );
    planning_context.insert(
        "target_output_paths".to_string(),
        json!(
            output_paths
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
        ),
    );
    if let Some(text) = diagnostic_text {
        if !text.trim().is_empty() {
            planning_context.insert("diagnostics_text".to_string(), json!(text));
        }
    }

    let (planning_context, estimated) = fit_context_to_token_limit(
        planner,
        spec_content,
        planning_context,
        token_limit,
    )?;

    let execution_seed = format!(
        "{}::{}",
        spec_path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("plan"),
        plan_kind.as_str()
    );
    let execution_control_ref = execution_control
        .as_ref()
        .map(|control| control as &dyn NativeExecutionControl);
    let mut response = planner
        .execute_with_conversation_with_seed_options(
            spec_content,
            &execution_seed,
            planning_context.clone(),
            execution_control_ref,
            ignore_cache_reads,
        )
        .await;
    if let Err(ref error) = response {
        if is_rate_limit_error(error)
            && prepare_rate_limit_retry(
                error,
                display_name,
                estimated,
                execution_control
                    .as_ref()
                    .and_then(CliExecutionControl::token_limiter),
                execution_control
                    .as_ref()
                    .and_then(CliExecutionControl::rate_limiter),
            )
            .await
        {
            response = planner
                .execute_with_conversation_with_seed_options(
                    spec_content,
                    &execution_seed,
                    planning_context,
                    execution_control_ref,
                    ignore_cache_reads,
                )
                .await;
        }
    }
    let response = response?;

    let plan = parse_plan_output(&response).unwrap_or(fallback_plan);
    let validation = validate_plan(&plan, &behavior_contract, output_paths);
    Ok((plan, validation))
}

fn prune_generated_spec_context(
    mut context: HashMap<String, serde_json::Value>,
) -> HashMap<String, serde_json::Value> {
    for key in [
        "openapi_content",
        "documentation_urls",
        "openapi_scope",
        "external_symbol_inventory",
        "previous_questions",
        "user_answers",
        "conversation_round",
    ] {
        context.remove(key);
    }
    context
}

fn clear_external_generated_output_dirs(
    specifications_dir: &str,
    api_namespace: &str,
) -> Result<()> {
    for dir in [
        PathBuf::from(specifications_dir)
            .join("data")
            .join("external")
            .join(api_namespace),
        PathBuf::from(specifications_dir)
            .join("contexts")
            .join("external")
            .join(api_namespace),
    ] {
        if dir.exists() {
            fs::remove_dir_all(&dir).with_context(|| {
                format!("Failed to remove generated directory: {}", dir.display())
            })?;
        }
    }
    Ok(())
}

fn unique_generated_name(base: &str, used: &mut HashMap<String, usize>) -> String {
    let count = used.entry(base.to_string()).or_insert(0usize);
    *count += 1;
    if *count == 1 {
        base.to_string()
    } else {
        format!("{}_{}", base, count)
    }
}

fn external_generated_data_output_path(
    specifications_dir: &str,
    api_namespace: &str,
    artifact_file_stem: &str,
) -> PathBuf {
    PathBuf::from(specifications_dir)
        .join("data")
        .join("external")
        .join(api_namespace)
        .join(format!("{artifact_file_stem}.md"))
}

fn external_generated_context_output_path(
    specifications_dir: &str,
    api_namespace: &str,
    artifact_file_stem: &str,
) -> PathBuf {
    PathBuf::from(specifications_dir)
        .join("contexts")
        .join("external")
        .join(api_namespace)
        .join(format!("{artifact_file_stem}.md"))
}

fn finalize_specification_output(
    draft_content: &str,
    draft_file: &Path,
    draft_name: &str,
    workspace: &WorkspaceContext,
    spec_content: String,
    additional_context: HashMap<String, serde_json::Value>,
) -> Result<ProcessSpecOutcome> {
    // Determine output path preserving folder structure
    let output_path = determine_specification_output_path(
        draft_file,
        &workspace.drafts_dir,
        &workspace.specifications_dir,
    )?;

    let spec_category = workspace
        .store
        .artifact_for_path(draft_file)
        .map(|artifact| artifact.category)
        .unwrap_or(ArtifactCategory::Root);

    let written = write_specification_output(
        workspace,
        draft_file,
        draft_name,
        draft_name,
        spec_category,
        &output_path,
        spec_content,
        Some(&additional_context),
    )?;

    if written.has_blocking_ambiguities {
        return Ok(ProcessSpecOutcome::BlockingAmbiguities {
            draft_file: draft_file.to_path_buf(),
            draft_name: draft_name.to_string(),
            draft_content: draft_content.to_string(),
            spec_content: written.spec_content,
            actionable: written.actionable,
            additional_context,
        });
    }

    Ok(ProcessSpecOutcome::Success)
}

fn write_specification_output(
    workspace: &WorkspaceContext,
    draft_file: &Path,
    draft_name: &str,
    display_name: &str,
    spec_category: ArtifactCategory,
    output_path: &Path,
    spec_content: String,
    dependency_context: Option<&HashMap<String, serde_json::Value>>,
) -> Result<WrittenSpecification> {
    let mut has_blocking_ambiguities = false;
    let mut actionable = Vec::new();

    // Report Blocking Ambiguities immediately if present in generated spec
    if let Some(blocking) = extract_blocking_ambiguities_section(&spec_content) {
        actionable = extract_actionable_blocking_bullets_for_path(&blocking, Some(output_path));
        if !actionable.is_empty() {
            has_blocking_ambiguities = true;
            eprintln!("error[spec:blocking]:");
            // Print source input path (draft) so IDEs can navigate to where fixes are needed.
            eprintln!("\u{001b}[31m{}\u{001b}[0m", draft_file.display());
            eprintln!(
                "  Blocking Ambiguities detected in generated specification for '{}'.",
                draft_name
            );
            eprintln!();
            for bullet in &actionable {
                eprintln!("  {}", bullet);
            }
            eprintln!();
        }
    }

    let artifact_root = workspace.artifact_workspace_root();
    let lint_report = analyze_specification(output_path, &spec_content, dependency_context);
    let contract_output_path = determine_implementation_output_path(output_path, SPECIFICATIONS_DIR).ok();
    let contract_artifact = build_contract_artifact(
        output_path,
        &spec_content,
        contract_output_path.as_deref(),
        dependency_context,
    );
    let contract_validation = validate_contract_artifact(
        &contract_artifact,
        output_path,
        &spec_content,
        dependency_context,
    );
    let _ = write_json_report(
        &artifact_root,
        "specification",
        output_path,
        "spec_lint_report.json",
        &lint_report,
    );
    let _ = write_json_report(
        &artifact_root,
        "specification",
        output_path,
        "behavior_contract.json",
        &lint_report.contract,
    );
    let _ = write_json_report(
        &artifact_root,
        "contracts",
        output_path,
        "contract_artifact.json",
        &contract_artifact,
    );
    let _ = write_json_report(
        &artifact_root,
        "contracts",
        output_path,
        "contract_validation_report.json",
        &contract_validation,
    );
    if !lint_report.errors.is_empty() {
        has_blocking_ambiguities = true;
        for issue in &lint_report.errors {
            actionable.push(format!("- {}", issue));
        }
        eprintln!("error[spec:lint]:");
        eprintln!("\u{001b}[31m{}\u{001b}[0m", draft_file.display());
        eprintln!(
            "  Specification lint failed for generated specification '{}'.",
            draft_name
        );
        eprintln!();
        for issue in &lint_report.errors {
            eprintln!("  - {}", issue);
        }
        eprintln!();
    }

    match workspace.store.backend() {
        BackendSelection::File => {
            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent)
                    .context("Failed to create specification output directory")?;
            }
            fs::write(output_path, &spec_content).context("Failed to write specification file")?;
        }
        BackendSelection::GitHub { .. } => {
            let draft_artifact =
                workspace
                    .store
                    .artifact_for_path(draft_file)
                    .with_context(|| {
                        format!("missing projected draft artifact {}", draft_file.display())
                    })?;
            workspace.store.write_specification(
                &draft_artifact,
                display_name,
                spec_category,
                spec_content.clone(),
            )?;
        }
    }

    Ok(WrittenSpecification {
        spec_content,
        actionable,
        has_blocking_ambiguities,
    })
}

fn build_dependency_drafts_from_context(
    context: &HashMap<String, serde_json::Value>,
) -> serde_json::Value {
    let closure: Vec<serde_json::Value> = if let Some(values) = context
        .get("dependency_tool_context")
        .and_then(|v| v.get("dependency_artifacts"))
        .and_then(|v| v.as_array())
    {
        values.clone()
    } else if let Some(values) = context.get("dependency_closure").and_then(|v| v.as_array()) {
        values.clone()
    } else if let Some(values) = context
        .get("direct_dependencies")
        .and_then(|v| v.as_array())
    {
        values.clone()
    } else {
        Vec::new()
    };

    let mut map = serde_json::Map::new();
    for item in closure {
        if let Some(obj) = item.as_object() {
            let path = obj.get("path").and_then(|p| p.as_str()).unwrap_or("");
            let content = obj.get("content").and_then(|c| c.as_str()).unwrap_or("");
            if !path.is_empty() && path.starts_with("drafts/") {
                map.insert(
                    path.to_string(),
                    serde_json::Value::String(content.to_string()),
                );
            }
        }
    }
    serde_json::Value::Object(map)
}

async fn try_fix_and_retry(
    draft_file: &Path,
    draft_name: &str,
    draft_content: &str,
    spec_content: &str,
    actionable: &[String],
    additional_context: HashMap<String, serde_json::Value>,
    fix_attempt: usize,
    max_fix_attempts: usize,
    filter: &CategoryFilter,
    rate_limit: Option<f64>,
    token_limit: Option<f64>,
    config: &Config,
    execution_control: Option<CliExecutionControl>,
) -> Result<()> {
    let dependency_drafts = build_dependency_drafts_from_context(&additional_context);

    let mut fix_context = HashMap::new();
    fix_context.insert(
        "blocking_ambiguities".to_string(),
        serde_json::Value::String(actionable.join("\n")),
    );
    fix_context.insert(
        "failed_draft_path".to_string(),
        serde_json::Value::String(draft_file.display().to_string()),
    );
    fix_context.insert(
        "failed_draft_content".to_string(),
        serde_json::Value::String(draft_content.to_string()),
    );
    fix_context.insert(
        "failed_spec_content".to_string(),
        serde_json::Value::String(spec_content.to_string()),
    );
    fix_context.insert(
        "dependency_drafts".to_string(),
        serde_json::Value::String(
            serde_json::to_string_pretty(&dependency_drafts).unwrap_or_else(|_| "{}".to_string()),
        ),
    );

    let fix_executor = Arc::new(AgentExecutor::new("fix_draft_blockers", config)?);
    let agent_response = fix_executor
        .execute_with_context(
            "",
            fix_context,
            execution_control
                .as_ref()
                .map(|control| control as &dyn NativeExecutionControl),
        )
        .await
        .context("Failed to execute fix_draft_blockers agent")?;

    let patch_output = match agent_response {
        AgentResponse::Final(s) => s,
        AgentResponse::Questions(q) => {
            anyhow::bail!(
                "Fix agent requested clarification; cannot auto-fix. Output: {}",
                q
            );
        }
    };

    let project_root = Path::new(".");
    let patched_paths = apply_draft_patches(project_root, &patch_output)
        .context("Failed to apply draft patches")?;

    let mut affected_names: HashSet<String> = patched_paths
        .iter()
        .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(String::from))
        .collect();
    affected_names.insert(draft_name.to_string());

    if config.verbose {
        println!(
            "✓ Applied fix patches to {} draft(s); retrying (attempt {}/{})",
            patched_paths.len(),
            fix_attempt + 1,
            max_fix_attempts
        );
    }

    let affected_names_vec: Vec<String> = affected_names.into_iter().collect();

    create_specification_inner(
        affected_names_vec,
        true,
        filter,
        rate_limit,
        token_limit,
        true,
        max_fix_attempts,
        fix_attempt + 1,
        config,
    )
    .await
}

pub async fn create_implementation(
    names: Vec<String>,
    fix: bool,
    max_compile_fix_attempts: usize,
    clear_cache: bool,
    filter: &CategoryFilter,
    rate_limit: Option<f64>,
    token_limit: Option<f64>,
    config: &Config,
) -> Result<()> {
    let workspace = WorkspaceContext::resolve(config)?;
    let names_provided = !names.is_empty();
    let context_files = resolve_input_files(&workspace.specifications_dir, names, "md", filter)?;

    if context_files.is_empty() {
        println!("No context files found to process");
        return Ok(());
    }

    // Load build tracker
    let mut tracker = BuildTracker::load()?;

    // Check if any specifications need to be regenerated first
    if tracker.upstream_changed(Stage::Implementation, "")? {
        println!("⚠ Upstream specifications have changed. Run 'reen create specification' first.");
    }

    let dependency_roots = select_dependency_roots(
        context_files,
        &workspace.specifications_dir,
        names_provided,
        filter,
    )?;
    let execution_levels = build_implementation_execution_plan(
        dependency_roots,
        filter,
        &workspace.specifications_dir,
        &workspace.drafts_dir,
    )?;
    let total_count: usize = execution_levels.iter().map(|level| level.len()).sum();
    println!(
        "Creating implementation for {} specification(s)",
        total_count
    );

    // Step 1: Generate project structure (Cargo.toml, lib.rs, mod.rs files)
    if config.verbose {
        println!("Generating project structure...");
    }

    let spec_dir = workspace.specifications_root.clone();
    let drafts_dir = workspace.drafts_root.clone();
    let mut project_info = analyze_specifications(&spec_dir, Some(&drafts_dir))
        .context("Failed to analyze specifications")?;
    if let Some(manifest) = load_dependency_manifest(&drafts_dir.join("dependencies.yml"))? {
        merge_manifest_dependencies(&mut project_info.dependencies, &manifest);
    }

    let output_dir = PathBuf::from(".");

    generate_cargo_toml(&project_info, &output_dir).context("Failed to generate Cargo.toml")?;

    generate_lib_rs(&project_info, &output_dir).context("Failed to generate lib.rs")?;

    generate_mod_files(&project_info, &output_dir).context("Failed to generate mod.rs files")?;

    if config.verbose {
        println!("✓ Project structure generated");
    }

    let mut recent_generated_files: Vec<PathBuf> = Vec::new();
    for p in generated_project_structure_paths(&project_info) {
        if p.exists() {
            recent_generated_files.push(p);
        }
    }

    // Step 2: Generate individual implementation files
    let planner = Arc::new(AgentExecutor::new("create_plan", config)?);
    let executor = Arc::new(AgentExecutor::new("create_implementation", config)?);
    let can_parallel = executor.can_run_parallel().unwrap_or(false);

    if config.verbose {
        let path = executor.model_registry().registry_path();
        println!(
            "Agent model registry: {}, create_implementation parallel: {}",
            path.display(),
            can_parallel
        );
    }

    let resources = ExecutionResources::new(rate_limit, token_limit);

    let mut progress = ProgressIndicator::new(total_count);
    let mut updated_count = 0;
    let mut updated_in_run: HashSet<String> = HashSet::new();
    let mut had_unspecified = false;
    for (level_idx, level_nodes) in execution_levels.into_iter().enumerate() {
        if config.verbose {
            println!(
                "Processing dependency level {} ({} item(s))",
                level_idx,
                level_nodes.len()
            );
        }

        let mut runnable = Vec::new();
        for node in level_nodes {
            let context_file = resolve_implementation_context_file(&node.input_path)?;
            let context_name = node.name.clone();
            let dependency_invalidated = node
                .direct_dependency_names()
                .iter()
                .any(|dep_name| updated_in_run.contains(dep_name));
            let (fingerprint_primary_root, fingerprint_fallback_root) =
                if node.input_path.starts_with(&workspace.specifications_root) {
                    (
                        &workspace.specifications_dir,
                        Some(workspace.drafts_dir.as_str()),
                    )
                } else {
                    (&workspace.drafts_dir, None)
                };
            let dependency_fingerprint = dependency_fingerprint_for_node(
                &node,
                fingerprint_primary_root,
                fingerprint_fallback_root,
            )?;
            let output_path =
                determine_implementation_output_path(&context_file, &workspace.specifications_dir)?;

            if has_unfinished_specification(&context_file, &context_name, "implementation")? {
                had_unspecified = true;
                progress.start_item(&context_name, None);
                progress.complete_item(&context_name, false);
                continue;
            }

            let needs_update = if clear_cache || dependency_invalidated {
                true
            } else {
                tracker.needs_update(
                    Stage::Implementation,
                    &context_name,
                    &context_file,
                    &output_path,
                    &dependency_fingerprint,
                )?
            };

            if !needs_update {
                progress.start_item_up_to_date(&context_name);
                if config.verbose {
                    println!("⊚ Skipping {} (up to date)", context_name);
                }
                progress.complete_item(&context_name, true);
                continue;
            }

            let mut dependency_context = build_dependency_context(
                &node,
                &workspace.specifications_dir,
                Some(&workspace.drafts_dir),
            )?;
            let context_content = fs::read_to_string(&context_file).unwrap_or_default();
            let target_contract = build_contract_artifact(
                &context_file,
                &context_content,
                Some(&output_path),
                Some(&dependency_context),
            );
            let target_contract_validation = validate_contract_artifact(
                &target_contract,
                &context_file,
                &context_content,
                Some(&dependency_context),
            );
            let _ = write_json_report(
                Path::new("."),
                "contracts",
                &output_path,
                "contract_artifact.json",
                &target_contract,
            );
            let _ = write_json_report(
                Path::new("."),
                "contracts",
                &output_path,
                "contract_validation_report.json",
                &target_contract_validation,
            );
            if let Some(target_type_name) = infer_target_type_name(
                &context_file,
                &workspace.specifications_root,
                &workspace.drafts_root,
            )? {
                dependency_context.insert("target_type_name".to_string(), json!(target_type_name));
            }
            let (implementation_plan, plan_validation) = generate_execution_plan_with_agent(
                &planner,
                PlanKind::Implementation,
                &context_file,
                &context_name,
                &context_content,
                std::slice::from_ref(&output_path),
                &dependency_context,
                None,
                token_limit,
                clear_cache,
                resources.execution_control.clone(),
            )
            .await?;
            let _ = write_json_report(
                Path::new("."),
                "planning",
                &output_path,
                "implementation_plan.json",
                &implementation_plan,
            );
            let _ = write_json_report(
                Path::new("."),
                "planning",
                &output_path,
                "plan_validation_report.json",
                &plan_validation,
            );
            if !plan_validation.ok {
                progress.complete_item(&context_name, false);
                eprintln!("error[plan:validation]:");
                eprintln!("\u{001b}[31m{}\u{001b}[0m", context_file.display());
                eprintln!("  Planning validation failed for '{}'.", context_name);
                eprintln!();
                for issue in &plan_validation.errors {
                    eprintln!("  - {}", issue);
                }
                eprintln!();
                continue;
            }
            let contract = analyze_specification(&context_file, &context_content, Some(&dependency_context)).contract;
            dependency_context.insert(
                "contract_artifact".to_string(),
                contract_artifact_to_context_value(&target_contract),
            );
            dependency_context.insert(
                "contract_validation".to_string(),
                contract_validation_to_context_value(&target_contract_validation),
            );
            dependency_context.insert(
                "behavior_contract".to_string(),
                contract_to_context_value(&contract),
            );
            dependency_context.insert(
                "implementation_plan".to_string(),
                plan_to_context_value(&implementation_plan),
            );
            dependency_context.insert(
                "plan_validation".to_string(),
                validation_to_context_value(&plan_validation),
            );
            let compact_dependency_context = compact_agent_dependency_context(&dependency_context);
            let (dependency_context, estimated) = fit_context_to_token_limit(
                &executor,
                &context_content,
                compact_dependency_context,
                token_limit,
            )?;
            let cache_hit = if clear_cache {
                false
            } else {
                executor
                    .is_cache_hit(&context_content, dependency_context.clone())
                    .unwrap_or(false)
            };
            runnable.push((
                context_file,
                context_name,
                output_path,
                dependency_fingerprint,
                implementation_plan,
                dependency_context,
                context_content,
                estimated,
                cache_hit,
            ));
        }

        if can_parallel {
            if config.verbose {
                println!("Parallel execution enabled for create_implementation");
            }
            let cfg = config.clone();
            let use_batch = executor.can_use_batch().unwrap_or(false);
            if use_batch {
                let mut batch_items: Vec<PreparedImplementationBatchItem> = Vec::new();
                let mut batch_results: Vec<(String, PathBuf, PathBuf, String, Result<()>)> =
                    Vec::new();

                for (
                    context_file,
                    context_name,
                    output_path,
                    dependency_fingerprint,
                    implementation_plan,
                    dependency_context,
                    context_content,
                    estimated,
                    cache_hit,
                ) in runnable
                {
                    if cache_hit {
                        progress.start_item_cached(&context_name);
                        let result = process_implementation(
                            &executor,
                            &context_content,
                            &context_file,
                            &context_name,
                            &workspace.specifications_dir,
                            &cfg,
                            clear_cache,
                            &implementation_plan,
                            dependency_context,
                            resources.execution_control.clone(),
                        )
                        .await;
                        batch_results.push((
                            context_name,
                            context_file,
                            output_path,
                            dependency_fingerprint,
                            result,
                        ));
                        continue;
                    }

                    progress.start_item(&context_name, Some(estimated));
                    match executor.prepare_execution_options(
                        &context_content,
                        dependency_context.clone(),
                        clear_cache,
                    )? {
                        reen::execution::PreparedExecutionState::Cached(output) => {
                            let result = finalize_implementation_output(
                                &context_file,
                                &context_name,
                                &workspace.specifications_dir,
                                &cfg,
                                &implementation_plan,
                                output,
                            );
                            batch_results.push((
                                context_name,
                                context_file,
                                output_path,
                                dependency_fingerprint,
                                result,
                            ));
                        }
                        reen::execution::PreparedExecutionState::Ready(prepared) => {
                            batch_items.push(PreparedImplementationBatchItem {
                                context_name,
                                context_file,
                                output_path,
                                dependency_fingerprint,
                                implementation_plan,
                                dependency_context,
                                context_content,
                                prepared,
                            });
                        }
                    }
                }

                if !batch_items.is_empty() {
                    let batch_request = batch_items
                        .iter()
                        .map(|item| (item.context_name.clone(), item.prepared.clone()))
                        .collect::<Vec<_>>();
                    match executor.execute_batch(
                        batch_request,
                        resources
                            .execution_control
                            .as_ref()
                            .map(|control| control as &dyn NativeExecutionControl),
                    ) {
                        Ok(outputs) => {
                            for item in batch_items {
                                let result = if let Some(output) =
                                    outputs.get(&item.context_name).cloned()
                                {
                                    finalize_implementation_output(
                                        &item.context_file,
                                        &item.context_name,
                                        &workspace.specifications_dir,
                                        &cfg,
                                        &item.implementation_plan,
                                        output,
                                    )
                                } else {
                                    eprintln!(
                                        "Batch execution returned no output for {}; falling back to sequential execution for that item.",
                                        item.context_name
                                    );
                                    process_implementation(
                                        &executor,
                                        &item.context_content,
                                        &item.context_file,
                                        &item.context_name,
                                        &workspace.specifications_dir,
                                        &cfg,
                                        clear_cache,
                                        &item.implementation_plan,
                                        item.dependency_context.clone(),
                                        resources.execution_control.clone(),
                                    )
                                    .await
                                };
                                batch_results.push((
                                    item.context_name,
                                    item.context_file,
                                    item.output_path,
                                    item.dependency_fingerprint,
                                    result,
                                ));
                            }
                        }
                        Err(batch_error) => {
                            eprintln!(
                                "Batch execution failed for create_implementation; falling back to sequential execution: {}",
                                batch_error
                            );
                            for item in batch_items {
                                let result = process_implementation(
                                    &executor,
                                    &item.context_content,
                                    &item.context_file,
                                    &item.context_name,
                                    &workspace.specifications_dir,
                                    &cfg,
                                    clear_cache,
                                    &item.implementation_plan,
                                    item.dependency_context,
                                    resources.execution_control.clone(),
                                )
                                .await;
                                batch_results.push((
                                    item.context_name,
                                    item.context_file,
                                    item.output_path,
                                    item.dependency_fingerprint,
                                    result,
                                ));
                            }
                        }
                    }
                }

                for (context_name, context_file, output_path, dependency_fingerprint, result) in
                    batch_results
                {
                    match result {
                        Ok(_) => {
                            tracker.record(
                                Stage::Implementation,
                                &context_name,
                                &context_file,
                                &output_path,
                                &dependency_fingerprint,
                            )?;
                            tracker.save()?;
                            updated_count += 1;
                            updated_in_run.insert(context_name.clone());
                            recent_generated_files.push(output_path.clone());
                            progress.complete_item(&context_name, true);
                            if config.verbose {
                                println!(
                                    "✓ Successfully created implementation for {}",
                                    context_name
                                );
                            }
                        }
                        Err(e) => {
                            if e.to_string().contains("unfinished specification") {
                                had_unspecified = true;
                            }
                            progress.complete_item(&context_name, false);
                            eprintln!(
                                "✗ Failed to create implementation for {}: {}",
                                context_name, e
                            );
                        }
                    }
                }
            } else {
                let stage_items = runnable
                    .into_iter()
                    .map(
                        |(
                            context_file,
                            context_name,
                            output_path,
                            dependency_fingerprint,
                            implementation_plan,
                            dependency_context,
                            context_content,
                            estimated,
                            cache_hit,
                        )| StageItem {
                            name: context_name.clone(),
                            estimated,
                            cache_hit,
                            payload: (
                                context_file,
                                context_name,
                                output_path,
                                dependency_fingerprint,
                                implementation_plan,
                                dependency_context,
                                context_content,
                            ),
                        },
                    )
                    .collect::<Vec<_>>();

                let executor_clone = executor.clone();
                let specifications_dir = workspace.specifications_dir.clone();
                let results = run_stage_items(
                    stage_items,
                    true,
                    &mut progress,
                    &resources,
                    config,
                    move |(
                        context_file,
                        context_name,
                        output_path,
                        dependency_fingerprint,
                        implementation_plan,
                        dependency_context,
                        context_content,
                    ),
                          execution_control| {
                        let executor = executor_clone.clone();
                        let cfg = cfg.clone();
                        let specifications_dir = specifications_dir.clone();
                        async move {
                            process_implementation(
                                &executor,
                                &context_content,
                                &context_file,
                                &context_name,
                                &specifications_dir,
                                &cfg,
                                clear_cache,
                                &implementation_plan,
                                dependency_context,
                                execution_control,
                            )
                            .await?;
                            Ok((context_file, output_path, dependency_fingerprint))
                        }
                    },
                )
                .await?;

                for (context_name, result) in results {
                    match result {
                        Ok((context_file, output_path, dependency_fingerprint)) => {
                            tracker.record(
                                Stage::Implementation,
                                &context_name,
                                &context_file,
                                &output_path,
                                &dependency_fingerprint,
                            )?;
                            tracker.save()?;
                            updated_count += 1;
                            updated_in_run.insert(context_name.clone());
                            recent_generated_files.push(output_path.clone());
                            progress.complete_item(&context_name, true);
                            if config.verbose {
                                println!(
                                    "✓ Successfully created implementation for {}",
                                    context_name
                                );
                            }
                        }
                        Err(e) => {
                            if e.to_string().contains("unfinished specification") {
                                had_unspecified = true;
                            }
                            progress.complete_item(&context_name, false);
                            eprintln!(
                                "✗ Failed to create implementation for {}: {}",
                                context_name, e
                            );
                        }
                    }
                }
            }
        } else {
            let stage_items = runnable
                .into_iter()
                .map(
                    |(
                        context_file,
                        context_name,
                        output_path,
                        dependency_fingerprint,
                        implementation_plan,
                        dependency_context,
                        context_content,
                        estimated,
                        cache_hit,
                    )| StageItem {
                        name: context_name.clone(),
                        estimated,
                        cache_hit,
                        payload: (
                            context_file,
                            context_name,
                            output_path,
                            dependency_fingerprint,
                            implementation_plan,
                            dependency_context,
                            context_content,
                        ),
                    },
                )
                .collect::<Vec<_>>();

            let executor_clone = executor.clone();
            let cfg = config.clone();
            let specifications_dir = workspace.specifications_dir.clone();
            let results = run_stage_items(
                stage_items,
                false,
                &mut progress,
                &resources,
                config,
                move |(
                    context_file,
                    context_name,
                    output_path,
                    dependency_fingerprint,
                    implementation_plan,
                    dependency_context,
                    context_content,
                ),
                      execution_control| {
                    let executor = executor_clone.clone();
                    let cfg = cfg.clone();
                    let specifications_dir = specifications_dir.clone();
                    async move {
                        process_implementation(
                            &executor,
                            &context_content,
                            &context_file,
                            &context_name,
                            &specifications_dir,
                            &cfg,
                            clear_cache,
                            &implementation_plan,
                            dependency_context,
                            execution_control,
                        )
                        .await?;
                        Ok((context_file, output_path, dependency_fingerprint))
                    }
                },
            )
            .await?;

            for (context_name, result) in results {
                match result {
                    Ok((context_file, output_path, dependency_fingerprint)) => {
                        tracker.record(
                            Stage::Implementation,
                            &context_name,
                            &context_file,
                            &output_path,
                            &dependency_fingerprint,
                        )?;
                        tracker.save()?;
                        updated_count += 1;
                        updated_in_run.insert(context_name.clone());
                        recent_generated_files.push(output_path.clone());
                        progress.complete_item(&context_name, true);
                        if config.verbose {
                            println!("✓ Successfully created implementation for {}", context_name);
                        }
                    }
                    Err(e) => {
                        if e.to_string().contains("unfinished specification") {
                            had_unspecified = true;
                        }
                        progress.complete_item(&context_name, false);
                        eprintln!(
                            "✗ Failed to create implementation for {}: {}",
                            context_name, e
                        );
                    }
                }
            }
        }
    }

    // Always compile after generation. Auto-fix is opt-in via --fix.
    if fix {
        let artifact_root = workspace.artifact_workspace_root();
        compilation_fix::ensure_compiles_with_auto_fix(
            config,
            max_compile_fix_attempts,
            Path::new("."),
            artifact_root.as_path(),
            &project_info,
            &recent_generated_files,
            resources
                .execution_control
                .as_ref()
                .map(|control| control as &dyn NativeExecutionControl),
        )
        .await?;
    } else {
        cargo_commands::compile(config).await?;
    }

    progress.finish();

    if updated_count == 0 && config.verbose && !had_unspecified {
        println!("All implementations are up to date");
    }

    if had_unspecified {
        anyhow::bail!("Unfinished specifications were detected. Aborting.");
    } else {
        Ok(())
    }
}

async fn process_implementation(
    executor: &AgentExecutor,
    context_content: &str,
    context_file: &Path,
    context_name: &str,
    specifications_dir: &str,
    config: &Config,
    ignore_cache_reads: bool,
    implementation_plan: &ExecutionPlan,
    additional_context: HashMap<String, serde_json::Value>,
    execution_control: Option<CliExecutionControl>,
) -> Result<()> {
    if has_unfinished_specification(context_file, context_name, "implementation")? {
        anyhow::bail!("unfinished specification");
    }

    if config.dry_run {
        println!(
            "[DRY RUN] Would create implementation for: {}",
            context_name
        );
        return Ok(());
    }

    // Use conversational execution to handle questions
    let impl_result = executor
        .execute_with_conversation_with_seed_options(
            &context_content,
            context_name,
            additional_context,
            execution_control
                .as_ref()
                .map(|control| control as &dyn NativeExecutionControl),
            ignore_cache_reads,
        )
        .await?;

    finalize_implementation_output(
        context_file,
        context_name,
        specifications_dir,
        config,
        implementation_plan,
        impl_result,
    )
}

fn finalize_implementation_output(
    context_file: &Path,
    context_name: &str,
    specifications_dir: &str,
    config: &Config,
    implementation_plan: &ExecutionPlan,
    impl_result: String,
) -> Result<()> {
    // Extract code from the agent output and write to file
    // The agent output may contain markdown code blocks or raw code
    let code = extract_code_from_output(&impl_result, context_name);

    // Surface explicit implementation-failure diagnostics directly in CLI output.
    let implementation_failure = extract_implementation_failure_message(&code);
    if let Some(message) = implementation_failure.as_deref() {
        eprintln!("error[impl:compile_error]:");
        eprintln!("\u{001b}[31m{}\u{001b}[0m", context_file.display());
        eprintln!(
            "  Generated implementation for '{}' contains an explicit failure marker:",
            context_name
        );
        eprintln!();
        for line in message.lines() {
            eprintln!("  {}", line);
        }
        eprintln!();
    }

    // Determine output path preserving folder structure
    let output_path = determine_implementation_output_path(context_file, specifications_dir)?;

    // Ensure the output directory exists
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).context("Failed to create implementation output directory")?;
    }

    // Write the implementation file
    fs::write(&output_path, code).context("Failed to write implementation file")?;

    if config.verbose {
        println!("✓ Written implementation to: {}", output_path.display());
    }

    let _ = write_json_report(
        Path::new("."),
        "implementation",
        &output_path,
        "implementation_plan.json",
        implementation_plan,
    );
    let verifier_report = verify_generated_implementation(
        Path::new("."),
        context_file,
        &fs::read_to_string(context_file).unwrap_or_default(),
        &output_path,
    )?;
    let _ = write_json_report(
        Path::new("."),
        "implementation",
        &output_path,
        "static_verifier_report.json",
        &verifier_report,
    );
    if !verifier_report.errors.is_empty() || !verifier_report.high_risk_findings.is_empty() {
        eprintln!("error[impl:verify]:");
        eprintln!("\u{001b}[31m{}\u{001b}[0m", context_file.display());
        eprintln!(
            "  Generated implementation for '{}' failed behavioral verification.",
            context_name
        );
        eprintln!();
        for issue in &verifier_report.errors {
            eprintln!("  - {}", issue);
        }
        for issue in &verifier_report.high_risk_findings {
            eprintln!("  - {}", issue);
        }
        eprintln!();
        return Err(anyhow::anyhow!(
            "Generated implementation for '{}' failed behavioral verification",
            context_name
        ));
    }

    if implementation_failure.is_some() {
        return Err(anyhow::anyhow!(
            "Generated implementation for '{}' contains explicit failure marker",
            context_name
        ));
    }

    Ok(())
}

/// Extract Rust code from agent output
/// Handles both raw code and markdown code blocks
fn extract_code_from_output(output: &str, _context_name: &str) -> String {
    use regex::Regex;

    // Try to find Rust code blocks in markdown (```rust ... ```)
    if let Ok(re) = Regex::new(r"(?s)```rust\s*\n(.*?)```") {
        if let Some(captures) = re.captures(output) {
            if let Some(code) = captures.get(1) {
                return code.as_str().trim().to_string();
            }
        }
    }

    // Try generic code blocks (``` ... ```)
    if let Ok(re) = Regex::new(r"(?s)```\s*\n(.*?)```") {
        if let Some(captures) = re.captures(output) {
            if let Some(code) = captures.get(1) {
                return code.as_str().trim().to_string();
            }
        }
    }

    // If no code blocks found, try to find code after markdown headers
    let trimmed = output.trim();
    if trimmed.starts_with("#") {
        // Looks like markdown, try to find the first code-like section
        let lines: Vec<&str> = trimmed.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if line.contains("pub struct")
                || line.contains("impl ")
                || line.contains("fn ")
                || line.contains("mod ")
            {
                return lines[i..].join("\n").trim().to_string();
            }
        }
    }

    // Fallback: return the entire output trimmed
    trimmed.to_string()
}

fn extract_compile_error_message(code: &str) -> Option<String> {
    use regex::Regex;

    let re = Regex::new(r#"(?s)compile_error!\s*\(\s*"((?:\\.|[^"\\])*)"\s*\)\s*;"#).ok()?;
    let captures = re.captures(code)?;
    let raw = captures.get(1)?.as_str();
    Some(unescape_common_rust_string_escapes(raw))
}

fn extract_implementation_failure_message(code: &str) -> Option<String> {
    if let Some(msg) = extract_compile_error_message(code) {
        return Some(msg);
    }

    if code.contains(IMPLEMENTATION_FAILURE_MARKER) {
        return Some(IMPLEMENTATION_FAILURE_MARKER.to_string());
    }

    None
}

fn unescape_common_rust_string_escapes(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('0') => out.push('\0'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Extracts the content of a markdown/plain section by exact title.
fn extract_section(content: &str, section_title: &str) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    let mut start_idx: Option<usize> = None;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let is_markdown_header = trimmed
            .strip_prefix('#')
            .map(|s| s.trim())
            .map(|s| s.eq_ignore_ascii_case(section_title))
            .unwrap_or(false);
        let is_plain_header = trimmed.eq_ignore_ascii_case(section_title);
        if is_markdown_header || is_plain_header {
            start_idx = Some(i + 1);
            break;
        }
    }

    let start = start_idx?;
    let mut section_lines = Vec::new();
    for line in lines.iter().skip(start) {
        let trimmed = line.trim();
        if trimmed.starts_with('#')
            || is_numbered_section_heading(trimmed)
            || (is_known_section_title(trimmed) && !trimmed.eq_ignore_ascii_case(section_title))
        {
            break;
        }
        section_lines.push(*line);
    }

    let section = section_lines.join("\n").trim().to_string();
    if section.is_empty() {
        None
    } else {
        Some(section)
    }
}

fn is_known_section_title(line: &str) -> bool {
    const TITLES: &[&str] = &[
        "Description",
        "Type Kind (Struct / Enum / NewType / Unspecified)",
        "Type Kind (Struct / Enum / NewType / **Unspecified**)",
        "Mutability (Immutable / Mutable)",
        "Properties",
        "Functionalities",
        "Constraints & Rules",
        "Inferred Types or Structures",
        "Inferred Types or Structures (Non-Blocking)",
        "Blocking Ambiguities",
        "Implementation Choices Left Open",
        "Implementation Choices Left Open (Non-Blocking)",
        "Resolved From Dependencies",
        "Worth to Consider",
        "Unspecified or Ambiguous Aspects",
    ];
    TITLES.iter().any(|t| line.eq_ignore_ascii_case(t))
}

/// Extracts the content of the "Blocking Ambiguities" section from markdown content.
/// Returns None if the section is not present.
fn extract_blocking_ambiguities_section(content: &str) -> Option<String> {
    extract_section(content, "Blocking Ambiguities")
}

fn is_numbered_section_heading(line: &str) -> bool {
    let mut parts = line.splitn(2, '.');
    matches!(
        (parts.next(), parts.next()),
        (Some(n), Some(rest)) if n.chars().all(|c| c.is_ascii_digit()) && !rest.trim().is_empty()
    )
}

fn extract_bullets_with_indent(section: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for line in section.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let indent = line.chars().take_while(|c| c.is_whitespace()).count();
        let trimmed = line.trim_start();
        let is_bullet = trimmed.starts_with("- ")
            || trimmed.starts_with("* ")
            || (trimmed
                .chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false)
                && trimmed.contains('.'));
        if !is_bullet {
            continue;
        }
        out.push((indent, trimmed.to_string()));
    }
    out
}

fn is_language_or_paradigm_specific_detail(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    let markers = [
        "&str",
        "string vs",
        "result<",
        "anyhow",
        "serde",
        "chrono",
        "derive",
        "macro",
        "trait",
        "ownership",
        "borrowing",
        "parameter names",
        "parameter passing",
        "signature",
        "placeholder",
        "debug-format",
        "debug format",
        "crate",
        "library",
        "vec",
        "hashmap",
        "btreemap",
        "u64",
        "u32",
        "u16",
        "u8",
        "i64",
        "i32",
        "i16",
        "i8",
        "usize",
        "isize",
        "operator overloading",
    ];
    markers.iter().any(|m| t.contains(m))
}

fn is_no_issue_placeholder_bullet(text: &str) -> bool {
    let normalized = text
        .trim()
        .trim_start_matches('-')
        .trim_start_matches('*')
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "none" | "n/a" | "na" | "no blockers" | "no ambiguities" | "no blocking ambiguities"
    )
}

fn is_external_specification_path(path: &Path) -> bool {
    let components: Vec<String> = path
        .components()
        .filter_map(|component| {
            component
                .as_os_str()
                .to_str()
                .map(|value| value.to_string())
        })
        .collect();
    components.windows(3).any(|window| {
        window[0] == "specifications"
            && ((window[1] == "data" && window[2] == "external")
                || (window[1] == "contexts" && window[2] == "external"))
    })
}

fn is_external_source_gap_detail(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let contradiction_markers = [
        "conflict",
        "conflicting",
        "contradiction",
        "contradictory",
        "contradicts",
        "incompatible",
        "mismatch",
        "disagree",
        "disagrees",
        "diverge",
        "diverges",
    ];
    if contradiction_markers
        .iter()
        .any(|marker| lower.contains(marker))
    {
        return false;
    }

    let source_gap_markers = [
        "undefined",
        "undocumented",
        "unspecified",
        "not specified",
        "not documented",
        "documentation does not specify",
        "openapi does not specify",
        "provider does not specify",
        "api does not specify",
        "lacks a defined structure",
        "lacks defined structure",
        "structure is undefined",
        "unknown structure",
        "missing from the documentation",
        "not described by the provider",
    ];
    source_gap_markers
        .iter()
        .any(|marker| lower.contains(marker))
}

fn extract_actionable_blocking_bullets_for_path(section: &str, path: Option<&Path>) -> Vec<String> {
    let bullets = extract_bullets_with_indent(section);
    if bullets.is_empty() {
        return Vec::new();
    }

    let mut actionable = vec![false; bullets.len()];
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); bullets.len()];
    let ignore_external_source_gaps = path.is_some_and(is_external_specification_path);

    for i in 0..bullets.len() {
        actionable[i] = !is_language_or_paradigm_specific_detail(&bullets[i].1)
            && !is_no_issue_placeholder_bullet(&bullets[i].1);
        if actionable[i]
            && ignore_external_source_gaps
            && is_external_source_gap_detail(&bullets[i].1)
        {
            actionable[i] = false;
        }
        let parent_indent = bullets[i].0;
        let mut j = i + 1;
        while j < bullets.len() && bullets[j].0 > parent_indent {
            children[i].push(j);
            j += 1;
        }
    }

    // A heading line ending with ":" should only be actionable if at least one child is actionable.
    for i in 0..bullets.len() {
        let text = bullets[i].1.as_str();
        if text.ends_with(':') && !children[i].is_empty() {
            actionable[i] = children[i].iter().any(|idx| actionable[*idx]);
        }
    }

    bullets
        .into_iter()
        .enumerate()
        .filter(|(i, _)| actionable[*i])
        .map(|(_, (_, text))| text)
        .collect()
}

fn has_unfinished_specification(path: &Path, context_name: &str, stage_name: &str) -> Result<bool> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read context file: {}", path.display()))?;
    if let Some(blocking) = extract_blocking_ambiguities_section(&content) {
        let actionable = extract_actionable_blocking_bullets_for_path(&blocking, Some(path));
        if actionable.is_empty() {
            return Ok(false);
        }

        eprintln!("error[spec:blocking]:");
        eprintln!("\u{001b}[31m{}\u{001b}[0m", path.display());
        eprintln!(
            "  Specification has Blocking Ambiguities; skipping {} for '{}'.",
            stage_name, context_name
        );
        eprintln!();
        for bullet in actionable {
            eprintln!("  {}", bullet);
        }
        return Ok(true);
    }
    Ok(false)
}

pub async fn create_tests(
    names: Vec<String>,
    clear_cache: bool,
    filter: &CategoryFilter,
    rate_limit: Option<f64>,
    token_limit: Option<f64>,
    config: &Config,
) -> Result<()> {
    let workspace = WorkspaceContext::resolve(config)?;
    let names_provided = !names.is_empty();
    let context_files = resolve_input_files(&workspace.specifications_dir, names, "md", filter)?;

    if context_files.is_empty() {
        println!("No context files found to process");
        return Ok(());
    }

    let dependency_roots = select_dependency_roots(
        context_files,
        &workspace.specifications_dir,
        names_provided,
        filter,
    )?;
    let execution_levels = build_execution_plan(
        dependency_roots,
        &workspace.specifications_dir,
        Some(&workspace.drafts_dir),
    )?;
    let total_count: usize = execution_levels.iter().map(|level| level.len()).sum();
    println!("Creating tests for {} context(s)", total_count);

    let executor = Arc::new(AgentExecutor::new("create_test", config)?);
    let can_parallel = executor.can_run_parallel().unwrap_or(false);

    let resources = ExecutionResources::new(rate_limit, token_limit);

    let mut progress = ProgressIndicator::new(total_count);
    let mut had_unspecified = false;
    for (level_idx, level_nodes) in execution_levels.into_iter().enumerate() {
        if config.verbose {
            println!(
                "Processing dependency level {} ({} item(s))",
                level_idx,
                level_nodes.len()
            );
        }

        let mut runnable = Vec::new();
        for node in level_nodes {
            let context_file = node.input_path.clone();
            let context_name = node.name.clone();
            let mut dependency_context = build_dependency_context(
                &node,
                &workspace.specifications_dir,
                Some(&workspace.drafts_dir),
            )?;
            augment_test_generation_context(
                &context_file,
                &workspace.specifications_root,
                &workspace.drafts_root,
                &mut dependency_context,
            )?;
            let context_content = fs::read_to_string(&context_file).unwrap_or_default();
            let (dependency_context, estimated) = fit_context_to_token_limit(
                &executor,
                &context_content,
                dependency_context,
                token_limit,
            )?;
            let cache_hit = if clear_cache {
                false
            } else {
                executor
                    .is_cache_hit(&context_content, dependency_context.clone())
                    .unwrap_or(false)
            };
            runnable.push(StageItem {
                name: context_name.clone(),
                estimated,
                cache_hit,
                payload: (
                    context_file,
                    context_name.clone(),
                    dependency_context,
                    context_content,
                ),
            });
        }

        if can_parallel && config.verbose {
            println!("Parallel execution enabled for create_test");
        }
        let cfg = config.clone();
        let executor_clone = executor.clone();
        let specifications_dir = workspace.specifications_dir.clone();
        let results = run_stage_items(
            runnable,
            can_parallel,
            &mut progress,
            &resources,
            config,
            move |(context_file, context_name, dependency_context, context_content),
                  execution_control| {
                let executor = executor_clone.clone();
                let cfg = cfg.clone();
                let specifications_dir = specifications_dir.clone();
                async move {
                    process_tests(
                        &executor,
                        &context_content,
                        &context_file,
                        &context_name,
                        &specifications_dir,
                        &cfg,
                        clear_cache,
                        dependency_context,
                        execution_control,
                    )
                    .await
                }
            },
        )
        .await?;

        for (context_name, result) in results {
            match result {
                Ok(_) => {
                    progress.complete_item(&context_name, true);
                    if config.verbose {
                        println!("✓ Successfully created tests for {}", context_name);
                    }
                }
                Err(e) => {
                    if e.to_string().contains("unfinished specification") {
                        had_unspecified = true;
                    }
                    progress.complete_item(&context_name, false);
                    eprintln!("✗ Failed to create tests for {}: {}", context_name, e);
                }
            }
        }
    }

    progress.finish();
    if !config.dry_run {
        sync_bdd_cargo_support(config)?;
    }
    if had_unspecified {
        anyhow::bail!("Unfinished specifications were detected. Aborting.");
    } else {
        Ok(())
    }
}

#[derive(Serialize)]
struct CacheAgentInput {
    draft_content: Option<String>,
    context_content: Option<String>,
    #[serde(flatten)]
    additional: HashMap<String, serde_json::Value>,
}

fn clear_agent_response_cache_for_stage(
    stage: Stage,
    names: &[String],
    config: &Config,
) -> Result<usize> {
    if names.is_empty() {
        return clear_stage_agent_cache_dirs(stage, config);
    }
    clear_stage_agent_cache_entries_by_name(stage, names, config)
}

fn clear_stage_agent_cache_dirs(stage: Stage, config: &Config) -> Result<usize> {
    let agents: &[&str] = match stage {
        Stage::Specification => &[
            "create_specifications",
            "create_specifications_data",
            "create_specifications_context",
            "create_specifications_external_api",
            "create_specifications_main",
        ],
        Stage::Implementation => &["create_implementation"],
        Stage::Tests => &["create_test"],
        Stage::Compile => &[],
    };

    if config.dry_run {
        println!(
            "[DRY RUN] Would clear agent response cache directories for {:?}: {}",
            stage,
            agents.join(", ")
        );
        return Ok(0);
    }

    let agent_registry = FileAgentRegistry::new(None);
    let model_registry = FileAgentModelRegistry::new(None, None, None);
    let mut removed = 0usize;

    for agent_name in agents {
        let instructions = match agent_registry.get_specification(agent_name) {
            Ok(template) => template.canonical_for_cache(),
            Err(e) => {
                if config.verbose {
                    eprintln!(
                        "Skipping agent cache clear for '{}': failed to load agent spec ({})",
                        agent_name, e
                    );
                }
                continue;
            }
        };
        let model = match model_registry.get_model(agent_name) {
            Ok(m) => m,
            Err(e) => {
                if config.verbose {
                    eprintln!(
                        "Skipping agent cache clear for '{}': failed to resolve model ({})",
                        agent_name, e
                    );
                }
                continue;
            }
        };

        let instructions_model_hash = instructions_model_hash(&instructions, &model.name);
        let cache_dir = PathBuf::from(".reen").join(instructions_model_hash);
        if cache_dir.exists() {
            fs::remove_dir_all(&cache_dir).with_context(|| {
                format!(
                    "Failed to remove agent response cache directory: {}",
                    cache_dir.display()
                )
            })?;
            removed += 1;
        }
    }

    Ok(removed)
}

fn clear_stage_agent_cache_entries_by_name(
    stage: Stage,
    names: &[String],
    config: &Config,
) -> Result<usize> {
    let workspace = WorkspaceContext::resolve(config)?;
    let names_vec = names.to_vec();
    let mut removed = 0usize;
    let mut candidates: Vec<(String, CacheAgentInput)> = Vec::new();

    match stage {
        Stage::Specification => {
            let files = resolve_input_files(
                &workspace.drafts_dir,
                names_vec,
                "md",
                &CategoryFilter::all(),
            )?;
            let levels = build_execution_plan(files, &workspace.drafts_dir, None)?;
            for node in levels.into_iter().flatten() {
                let draft_content = fs::read_to_string(&node.input_path).with_context(|| {
                    format!("Failed to read draft file: {}", node.input_path.display())
                })?;
                let additional = build_dependency_context(&node, &workspace.drafts_dir, None)?;
                let agent_name =
                    determine_specification_agent(&node.input_path, &workspace.drafts_dir)
                        .to_string();
                candidates.push((
                    agent_name,
                    CacheAgentInput {
                        draft_content: Some(draft_content),
                        context_content: None,
                        additional,
                    },
                ));
            }
        }
        Stage::Implementation => {
            let files = resolve_input_files(
                &workspace.specifications_dir,
                names_vec,
                "md",
                &CategoryFilter::all(),
            )?;
            let levels = build_implementation_execution_plan(
                files,
                &CategoryFilter::all(),
                &workspace.specifications_dir,
                &workspace.drafts_dir,
            )?;
            for node in levels.into_iter().flatten() {
                let context_file = resolve_implementation_context_file(&node.input_path)?;
                let context_content = fs::read_to_string(&context_file).with_context(|| {
                    format!(
                        "Failed to read specification file: {}",
                        context_file.display()
                    )
                })?;
                let mut additional = build_dependency_context(
                    &node,
                    &workspace.specifications_dir,
                    Some(&workspace.drafts_dir),
                )?;
                if let Some(target_type_name) = infer_target_type_name(
                    &context_file,
                    &workspace.specifications_root,
                    &workspace.drafts_root,
                )? {
                    additional.insert("target_type_name".to_string(), json!(target_type_name));
                }
                candidates.push((
                    "create_implementation".to_string(),
                    CacheAgentInput {
                        draft_content: None,
                        context_content: Some(context_content),
                        additional,
                    },
                ));
            }
        }
        Stage::Tests => {
            let files = resolve_input_files(
                &workspace.specifications_dir,
                names_vec,
                "md",
                &CategoryFilter::all(),
            )?;
            let levels = build_execution_plan(
                files,
                &workspace.specifications_dir,
                Some(&workspace.drafts_dir),
            )?;
            for node in levels.into_iter().flatten() {
                let context_content = fs::read_to_string(&node.input_path).with_context(|| {
                    format!(
                        "Failed to read specification file: {}",
                        node.input_path.display()
                    )
                })?;
                let mut additional = build_dependency_context(
                    &node,
                    &workspace.specifications_dir,
                    Some(&workspace.drafts_dir),
                )?;
                augment_test_generation_context(
                    &node.input_path,
                    &workspace.specifications_root,
                    &workspace.drafts_root,
                    &mut additional,
                )?;
                candidates.push((
                    "create_test".to_string(),
                    CacheAgentInput {
                        draft_content: None,
                        context_content: Some(context_content),
                        additional,
                    },
                ));
            }
        }
        Stage::Compile => {}
    }

    if config.dry_run {
        println!(
            "[DRY RUN] Would clear {} agent response cache entrie(s) for {:?}: {}",
            candidates.len(),
            stage,
            names.join(", ")
        );
        return Ok(0);
    }

    let agent_registry = FileAgentRegistry::new(None);
    let model_registry = FileAgentModelRegistry::new(None, None, None);
    for (agent_name, input) in candidates {
        if clear_single_agent_cache_entry(
            &agent_registry,
            &model_registry,
            &agent_name,
            &input,
            config,
        )? {
            removed += 1;
        }
    }

    Ok(removed)
}

fn clear_single_agent_cache_entry(
    agent_registry: &FileAgentRegistry,
    model_registry: &FileAgentModelRegistry,
    agent_name: &str,
    input: &CacheAgentInput,
    config: &Config,
) -> Result<bool> {
    let instructions = match agent_registry.get_specification(agent_name) {
        Ok(template) => template.canonical_for_cache(),
        Err(e) => {
            if config.verbose {
                eprintln!(
                    "Skipping targeted agent cache clear for '{}': failed to load agent spec ({})",
                    agent_name, e
                );
            }
            return Ok(false);
        }
    };

    let model = match model_registry.get_model(agent_name) {
        Ok(m) => m,
        Err(e) => {
            if config.verbose {
                eprintln!(
                    "Skipping targeted agent cache clear for '{}': failed to resolve model ({})",
                    agent_name, e
                );
            }
            return Ok(false);
        }
    };

    let folder_hash = instructions_model_hash(&instructions, &model.name);
    let input_json = serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string());
    let mut hasher = Sha256::new();
    hasher.update(format!("{}:{}", instructions, input_json).as_bytes());
    let cache_key = hex::encode(hasher.finalize());
    let cache_path = PathBuf::from(".reen")
        .join(folder_hash)
        .join(format!("{}.cache", cache_key));
    if cache_path.exists() {
        fs::remove_file(&cache_path).with_context(|| {
            format!(
                "Failed to remove targeted agent response cache file: {}",
                cache_path.display()
            )
        })?;
        return Ok(true);
    }
    Ok(false)
}

fn clear_specific_agent_response_cache_entry(
    agent_name: &str,
    input: CacheAgentInput,
    config: &Config,
) -> Result<bool> {
    let agent_registry = FileAgentRegistry::new(None);
    let model_registry = FileAgentModelRegistry::new(None, None, None);
    clear_single_agent_cache_entry(&agent_registry, &model_registry, agent_name, &input, config)
}

fn instructions_model_hash(agent_instructions: &str, model_name: &str) -> String {
    let composite = format!("{}:{}", agent_instructions, model_name);
    let mut hasher = Sha256::new();
    hasher.update(composite.as_bytes());
    hex::encode(hasher.finalize())
}

async fn process_tests(
    executor: &AgentExecutor,
    context_content: &str,
    context_file: &Path,
    context_name: &str,
    specifications_dir: &str,
    config: &Config,
    ignore_cache_reads: bool,
    additional_context: HashMap<String, serde_json::Value>,
    execution_control: Option<CliExecutionControl>,
) -> Result<()> {
    if has_unfinished_specification(context_file, context_name, "tests")? {
        anyhow::bail!("unfinished specification");
    }

    let test_paths = determine_bdd_test_paths(context_file, specifications_dir)?;

    if config.dry_run {
        println!(
            "[DRY RUN] Would create BDD tests for {}: {}, {}, {}",
            context_name,
            test_paths.feature_path.display(),
            test_paths.steps_path.display(),
            test_paths.runner_path.display()
        );
        return Ok(());
    }

    let test_result = executor
        .execute_with_conversation_with_seed_options(
            &context_content,
            context_name,
            additional_context,
            execution_control
                .as_ref()
                .map(|control| control as &dyn NativeExecutionControl),
            ignore_cache_reads,
        )
        .await?;

    finalize_test_output(context_name, &test_paths, config, &test_result)
}

fn finalize_test_output(
    context_name: &str,
    test_paths: &BddTestPaths,
    config: &Config,
    test_result: &str,
) -> Result<()> {
    let artifacts = parse_generated_files(test_result)?;
    let expected_paths = HashSet::from([
        test_paths.feature_path.clone(),
        test_paths.steps_path.clone(),
        test_paths.runner_path.clone(),
    ]);
    let actual_paths: HashSet<PathBuf> = artifacts
        .iter()
        .map(|artifact| artifact.0.clone())
        .collect();

    if actual_paths != expected_paths {
        let expected = expected_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let actual = actual_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::bail!(
            "Generated BDD artifacts for '{}' did not match expected paths. Expected [{}], got [{}].",
            context_name,
            expected,
            actual
        );
    }

    for (path, content) in artifacts {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create BDD output directory {}", parent.display())
            })?;
        }
        fs::write(&path, content).with_context(|| {
            format!("Failed to write generated BDD artifact {}", path.display())
        })?;
        if config.verbose {
            println!("✓ Written BDD artifact: {}", path.display());
        }
    }

    Ok(())
}

fn parse_generated_files(output: &str) -> Result<Vec<(PathBuf, String)>> {
    let re = regex::Regex::new(r#"(?s)<file path="([^"]+)">\n?(.*?)\n?</file>"#)
        .context("Failed to compile BDD artifact parser regex")?;
    let mut files = Vec::new();

    for captures in re.captures_iter(output) {
        let path = captures
            .get(1)
            .map(|m| PathBuf::from(m.as_str()))
            .context("Generated BDD artifact missing path")?;
        let raw = captures
            .get(2)
            .map(|m| m.as_str())
            .context("Generated BDD artifact missing content")?;
        let trimmed_start = raw.strip_prefix('\n').unwrap_or(raw);
        let content = trimmed_start
            .strip_suffix('\n')
            .unwrap_or(trimmed_start)
            .to_string();
        files.push((path, content));
    }

    if files.is_empty() {
        anyhow::bail!("Generated BDD test output did not contain any <file path=\"...\"> blocks");
    }

    let mut unique_paths = HashSet::new();
    for (path, _) in &files {
        if !unique_paths.insert(path.clone()) {
            anyhow::bail!(
                "Generated BDD output contained duplicate file path {}",
                path.display()
            );
        }
    }

    Ok(files)
}

fn augment_test_generation_context(
    context_file: &Path,
    specifications_root: &Path,
    drafts_root: &Path,
    additional_context: &mut HashMap<String, serde_json::Value>,
) -> Result<()> {
    let test_paths =
        determine_bdd_test_paths(context_file, specifications_root.to_string_lossy().as_ref())?;
    additional_context.insert(
        "feature_output_path".to_string(),
        json!(test_paths.feature_path.to_string_lossy().to_string()),
    );
    additional_context.insert(
        "steps_output_path".to_string(),
        json!(test_paths.steps_path.to_string_lossy().to_string()),
    );
    additional_context.insert(
        "runner_output_path".to_string(),
        json!(test_paths.runner_path.to_string_lossy().to_string()),
    );
    additional_context.insert(
        "runner_test_name".to_string(),
        json!(test_paths.runner_test_name),
    );
    if let Some(target_type_name) =
        infer_target_type_name(context_file, specifications_root, drafts_root)?
    {
        additional_context.insert("target_type_name".to_string(), json!(target_type_name));
    }
    Ok(())
}

fn build_dependency_manifest(
    dependency_closure: &[DependencyArtifact],
    direct_dependencies: &[DependencyArtifact],
) -> Vec<serde_json::Value> {
    let direct_paths: HashSet<&str> = direct_dependencies
        .iter()
        .map(|dep| dep.path.as_str())
        .collect();

    dependency_closure
        .iter()
        .map(|dep| {
            json!({
                "name": dep.name,
                "path": dep.path,
                "source": dep.source,
                "sha256": dep.sha256,
                "artifact_type": "draft_or_spec",
                "dependency_kind": if direct_paths.contains(dep.path.as_str()) {
                    "direct"
                } else {
                    "transitive"
                }
            })
        })
        .collect()
}

fn build_implemented_dependency_manifest(
    implemented_dependencies: &[serde_json::Value],
    direct_dependencies: &[DependencyArtifact],
) -> Vec<serde_json::Value> {
    let direct_paths: HashSet<&str> = direct_dependencies
        .iter()
        .map(|dep| dep.path.as_str())
        .collect();

    implemented_dependencies
        .iter()
        .filter_map(|item| {
            let spec_path = item.get("spec_path")?.as_str()?;
            let path = item.get("path")?.as_str()?;
            Some(json!({
                "name": item.get("name").cloned().unwrap_or(serde_json::Value::Null),
                "spec_path": spec_path,
                "path": path,
                "sha256": item.get("sha256").cloned().unwrap_or(serde_json::Value::Null),
                "artifact_type": "implementation_source",
                "dependency_kind": if direct_paths.contains(spec_path) {
                    "direct"
                } else {
                    "transitive"
                }
            }))
        })
        .collect()
}

fn filter_direct_implemented_dependencies(
    implemented_dependencies: &[serde_json::Value],
    direct_dependencies: &[DependencyArtifact],
) -> Vec<serde_json::Value> {
    let direct_paths: HashSet<&str> = direct_dependencies
        .iter()
        .map(|dep| dep.path.as_str())
        .collect();

    implemented_dependencies
        .iter()
        .filter(|item| {
            item.get("spec_path")
                .and_then(|v| v.as_str())
                .map(|spec_path| direct_paths.contains(spec_path))
                .unwrap_or(false)
        })
        .cloned()
        .collect()
}

fn build_dependency_context(
    node: &ExecutionNode,
    primary_root: &str,
    fallback_root: Option<&str>,
) -> Result<HashMap<String, serde_json::Value>> {
    let mut context = HashMap::new();
    let direct_dependencies = node.resolve_direct_dependencies()?;
    let dependency_closure = node.resolve_dependency_closure(primary_root, fallback_root)?;
    let dependency_manifest = build_dependency_manifest(&dependency_closure, &direct_dependencies);
    let direct_dependency_manifest =
        build_dependency_manifest(&direct_dependencies, &direct_dependencies);
    context.insert(
        "direct_dependencies".to_string(),
        json!(dependency_manifest.clone()),
    );
    context.insert(
        "direct_dependencies_only".to_string(),
        json!(direct_dependency_manifest),
    );
    context.insert(
        "dependency_closure".to_string(),
        json!(dependency_manifest.clone()),
    );
    // Backward compatibility with agent prompts that still reference mcp_context
    context.insert("mcp_context".to_string(), json!(dependency_manifest));

    let implemented_dependencies = build_implemented_dependency_context(&dependency_closure)?;
    let implemented_direct_dependencies =
        filter_direct_implemented_dependencies(&implemented_dependencies, &direct_dependencies);
    let dependency_contracts = build_dependency_contract_artifacts(&dependency_closure)?;
    let direct_dependency_contracts =
        filter_direct_contract_artifacts(&dependency_contracts, &direct_dependencies);
    let implemented_role_capsules =
        build_role_capsules_for_implemented_dependencies(&implemented_dependencies)?;
    let implemented_direct_role_capsules =
        filter_direct_role_capsules(&implemented_role_capsules, &direct_dependencies);
    let implemented_dependency_manifest =
        build_implemented_dependency_manifest(&implemented_dependencies, &direct_dependencies);
    let implemented_direct_dependency_manifest = build_implemented_dependency_manifest(
        &implemented_direct_dependencies,
        &direct_dependencies,
    );
    context.insert(
        "implemented_dependencies".to_string(),
        json!(implemented_dependency_manifest),
    );
    context.insert(
        "implemented_direct_dependencies".to_string(),
        json!(implemented_direct_dependency_manifest),
    );
    context.insert(
        "dependency_contracts".to_string(),
        json!(dependency_contracts),
    );
    context.insert(
        "direct_dependency_contracts".to_string(),
        json!(direct_dependency_contracts),
    );
    context.insert(
        "implemented_role_capsules".to_string(),
        json!(implemented_role_capsules),
    );
    context.insert(
        "implemented_direct_role_capsules".to_string(),
        json!(implemented_direct_role_capsules),
    );
    context.insert(
        "dependency_tool_context".to_string(),
        json!({
            "dependency_artifacts": dependency_closure,
            "implemented_dependency_artifacts": implemented_dependencies,
            "implemented_direct_dependency_artifacts": implemented_direct_dependencies,
        }),
    );
    if let Some(tooling_symbols) = load_symbols_context(Path::new(primary_root))? {
        context.insert("tooling_symbols".to_string(), tooling_symbols);
    }
    Ok(context)
}

fn build_implementation_execution_plan(
    spec_files: Vec<PathBuf>,
    filter: &CategoryFilter,
    specifications_dir: &str,
    drafts_dir: &str,
) -> Result<Vec<Vec<ExecutionNode>>> {
    let implementation_inputs =
        resolve_implementation_dependency_inputs(spec_files, specifications_dir, drafts_dir)?;

    let expanded_inputs = expand_with_transitive_dependencies(
        implementation_inputs,
        specifications_dir,
        Some(drafts_dir),
    )?;
    let filtered_inputs = if filter.is_active() {
        expanded_inputs
            .into_iter()
            .filter(|f| {
                let base_dir = if f.starts_with(specifications_dir) {
                    specifications_dir
                } else {
                    drafts_dir
                };
                filter.matches_path(f, base_dir)
            })
            .collect()
    } else {
        expanded_inputs
    };
    build_execution_plan(filtered_inputs, specifications_dir, Some(drafts_dir))
}

fn resolve_implementation_dependency_inputs(
    spec_files: Vec<PathBuf>,
    specifications_dir: &str,
    drafts_dir: &str,
) -> Result<Vec<PathBuf>> {
    let mut inputs = Vec::new();
    for spec_file in spec_files {
        if is_external_specification_path(&spec_file) {
            inputs.push(spec_file);
            continue;
        }

        let draft_path = determine_draft_input_path(&spec_file, specifications_dir, drafts_dir)?;
        if draft_path.exists() {
            inputs.push(draft_path);
        } else {
            inputs.push(spec_file);
        }
    }
    Ok(inputs)
}

fn dependency_fingerprint_for_node(
    node: &ExecutionNode,
    primary_root: &str,
    fallback_root: Option<&str>,
) -> Result<String> {
    let closure = node.resolve_dependency_closure(primary_root, fallback_root)?;
    if closure.is_empty() {
        return Ok(String::new());
    }

    let mut deps: Vec<String> = closure
        .into_iter()
        .map(|dep| format!("{}:{}", dep.path, dep.sha256))
        .collect();
    deps.sort();
    let joined = deps.join("|");
    let mut hasher = Sha256::new();
    hasher.update(joined.as_bytes());
    Ok(hex::encode(hasher.finalize()))
}

fn resolve_implementation_context_file(node_input_path: &Path) -> Result<PathBuf> {
    if node_input_path.starts_with(DRAFTS_DIR) {
        determine_specification_output_path(node_input_path, DRAFTS_DIR, SPECIFICATIONS_DIR)
    } else {
        Ok(node_input_path.to_path_buf())
    }
}

fn build_implemented_dependency_context(
    dependency_closure: &[DependencyArtifact],
) -> Result<Vec<serde_json::Value>> {
    let mut artifacts = Vec::new();

    for dep in dependency_closure {
        let mut spec_path = PathBuf::from(&dep.path);
        if spec_path.starts_with(DRAFTS_DIR) {
            let mapped =
                determine_specification_output_path(&spec_path, DRAFTS_DIR, SPECIFICATIONS_DIR)?;
            if mapped.exists() {
                spec_path = mapped;
            }
        }
        if !spec_path.starts_with(SPECIFICATIONS_DIR) {
            continue;
        }

        let impl_path = match determine_implementation_output_path(&spec_path, SPECIFICATIONS_DIR) {
            Ok(path) => path,
            Err(_) => continue,
        };

        if !impl_path.exists() {
            continue;
        }

        let content = fs::read_to_string(&impl_path).with_context(|| {
            format!(
                "failed reading implemented dependency artifact: {}",
                impl_path.display()
            )
        })?;

        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let sha256 = hex::encode(hasher.finalize());

        artifacts.push(json!({
            "name": dep.name,
            "spec_path": dep.path,
            "path": impl_path.to_string_lossy().to_string(),
            "content": content,
            "sha256": sha256
        }));
    }

    artifacts.sort_by(|a, b| {
        let ap = a.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let bp = b.get("path").and_then(|v| v.as_str()).unwrap_or("");
        ap.cmp(bp)
    });

    Ok(artifacts)
}

fn resolve_dependency_spec_path(raw_path: &str) -> Result<Option<PathBuf>> {
    let mut spec_path = PathBuf::from(raw_path);
    if spec_path.starts_with(DRAFTS_DIR) {
        let mapped = determine_specification_output_path(&spec_path, DRAFTS_DIR, SPECIFICATIONS_DIR)?;
        if mapped.exists() {
            spec_path = mapped;
        }
    }
    if spec_path.starts_with(SPECIFICATIONS_DIR) && spec_path.exists() {
        Ok(Some(spec_path))
    } else {
        Ok(None)
    }
}

fn build_dependency_contract_artifacts(
    dependency_closure: &[DependencyArtifact],
) -> Result<Vec<ContractArtifact>> {
    let mut contracts = Vec::new();

    for dependency in dependency_closure {
        let Some(spec_path) = resolve_dependency_spec_path(&dependency.path)? else {
            continue;
        };
        let spec_content = fs::read_to_string(&spec_path)
            .with_context(|| format!("failed reading dependency specification: {}", spec_path.display()))?;
        let output_hint = determine_implementation_output_path(&spec_path, SPECIFICATIONS_DIR).ok();
        contracts.push(build_contract_artifact(
            &spec_path,
            &spec_content,
            output_hint.as_deref(),
            None,
        ));
    }

    contracts.sort_by(|a, b| a.source_spec_path.cmp(&b.source_spec_path));
    Ok(contracts)
}

fn filter_direct_contract_artifacts(
    contracts: &[ContractArtifact],
    direct_dependencies: &[DependencyArtifact],
) -> Vec<ContractArtifact> {
    let direct_paths = direct_dependencies
        .iter()
        .filter_map(|dependency| resolve_dependency_spec_path(&dependency.path).ok().flatten())
        .map(|path| path.to_string_lossy().to_string())
        .collect::<HashSet<_>>();

    contracts
        .iter()
        .filter(|contract| direct_paths.contains(&contract.source_spec_path))
        .cloned()
        .collect()
}

fn build_role_capsules_for_implemented_dependencies(
    implemented_dependencies: &[serde_json::Value],
) -> Result<Vec<InterfaceCapsule>> {
    let mut capsules = Vec::new();

    for item in implemented_dependencies {
        let Some(spec_path_raw) = item.get("spec_path").and_then(|value| value.as_str()) else {
            continue;
        };
        let Some(spec_path) = resolve_dependency_spec_path(spec_path_raw)? else {
            continue;
        };
        let spec_content = fs::read_to_string(&spec_path)
            .with_context(|| format!("failed reading dependency specification: {}", spec_path.display()))?;
        let source_path = item
            .get("path")
            .and_then(|value| value.as_str())
            .map(PathBuf::from);
        let source_content = item.get("content").and_then(|value| value.as_str());
        let contract = build_contract_artifact(
            &spec_path,
            &spec_content,
            source_path.as_deref(),
            None,
        );
        capsules.push(build_interface_capsule(
            &contract,
            source_path.as_deref(),
            source_content,
        ));
    }

    capsules.sort_by(|a, b| a.spec_path.cmp(&b.spec_path));
    Ok(capsules)
}

fn filter_direct_role_capsules(
    capsules: &[InterfaceCapsule],
    direct_dependencies: &[DependencyArtifact],
) -> Vec<InterfaceCapsule> {
    let direct_paths = direct_dependencies
        .iter()
        .filter_map(|dependency| resolve_dependency_spec_path(&dependency.path).ok().flatten())
        .map(|path| path.to_string_lossy().to_string())
        .collect::<HashSet<_>>();

    capsules
        .iter()
        .filter(|capsule| direct_paths.contains(&capsule.spec_path))
        .cloned()
        .collect()
}

fn compact_agent_dependency_context(
    context: &HashMap<String, serde_json::Value>,
) -> HashMap<String, serde_json::Value> {
    let mut compact = context.clone();
    compact.remove("dependency_tool_context");
    compact
}

pub async fn compile(config: &Config) -> Result<()> {
    cargo_commands::compile(config).await
}

pub async fn fix(max_compile_fix_attempts: usize, config: &Config) -> Result<()> {
    cargo_commands::fix(max_compile_fix_attempts, config).await
}

pub async fn run(args: Vec<String>, config: &Config) -> Result<()> {
    cargo_commands::run(args, config).await
}

pub async fn test(config: &Config) -> Result<()> {
    cargo_commands::test(config).await
}

pub async fn clear_cache(target: &str, names: Vec<String>, config: &Config) -> Result<()> {
    let stage = match target {
        "specification" | "specifications" => Stage::Specification,
        "implementation" | "implementations" => Stage::Implementation,
        "test" | "tests" => Stage::Tests,
        other => anyhow::bail!(
            "Unsupported cache target '{}'. Expected specification(s), implementation(s), or test(s).",
            other
        ),
    };

    if config.dry_run {
        if names.is_empty() {
            println!("[DRY RUN] Would clear cache entries for {:?}", stage);
        } else {
            println!(
                "[DRY RUN] Would clear cache entries for {:?}: {}",
                stage,
                names.join(", ")
            );
        }
        return Ok(());
    }

    let mut tracker = BuildTracker::load()?;
    let removed = if names.is_empty() {
        tracker.clear_stage(stage)
    } else {
        tracker.clear_stage_names(stage, &names)
    };
    let removed_agent_cache_entries = clear_agent_response_cache_for_stage(stage, &names, config)?;
    tracker.save()?;
    if names.is_empty() {
        println!("✓ Cleared {} cache entries for {:?}", removed, stage);
    } else {
        println!(
            "✓ Cleared {} cache entries for {:?}: {}",
            removed,
            stage,
            names.join(", ")
        );
    }
    println!(
        "✓ Cleared {} agent response cache entrie(s) for {:?}",
        removed_agent_cache_entries, stage
    );
    Ok(())
}

pub async fn clear_artifacts(target: &str, names: Vec<String>, config: &Config) -> Result<()> {
    match target {
        "specification" | "specifications" => clear_specification_artifacts(names, config),
        "implementation" | "implementations" => clear_implementation_artifacts(names, config),
        "test" | "tests" => clear_test_artifacts(names, config),
        other => anyhow::bail!(
            "Unsupported clear target '{}'. Expected specification(s), implementation(s), or test(s).",
            other
        ),
    }
}

fn clear_specification_artifacts(names: Vec<String>, config: &Config) -> Result<()> {
    let specs_dir = PathBuf::from(SPECIFICATIONS_DIR);
    if !specs_dir.exists() {
        println!("No specification artifacts found");
        return Ok(());
    }

    if names.is_empty() {
        if config.dry_run {
            println!("[DRY RUN] Would remove {}", specs_dir.display());
            return Ok(());
        }

        fs::remove_dir_all(&specs_dir)
            .with_context(|| format!("Failed to remove {}", specs_dir.display()))?;
        println!(
            "✓ Removed specification artifacts at {}",
            specs_dir.display()
        );
        return Ok(());
    }

    let spec_files = resolve_input_files(SPECIFICATIONS_DIR, names, "md", &CategoryFilter::all())?;
    let mut removed = 0usize;
    let mut found = 0usize;
    for spec_file in spec_files {
        found += 1;
        if spec_file.exists() {
            if config.dry_run {
                println!("[DRY RUN] Would remove {}", spec_file.display());
            } else {
                fs::remove_file(&spec_file)
                    .with_context(|| format!("Failed to remove {}", spec_file.display()))?;
            }
            removed += 1;
        }
    }
    if removed == 0 {
        println!("No matching specification artifacts found");
    } else if config.dry_run {
        println!(
            "[DRY RUN] Would remove {} specification artifact file(s)",
            removed
        );
    } else {
        println!("✓ Removed {} specification artifact file(s)", removed);
    }
    if found == 0 {
        println!("No matching names were resolved in {}", specs_dir.display());
    }
    Ok(())
}

fn clear_implementation_artifacts(names: Vec<String>, config: &Config) -> Result<()> {
    let spec_files = resolve_input_files(SPECIFICATIONS_DIR, names, "md", &CategoryFilter::all())?;
    if spec_files.is_empty() {
        println!("No implementation artifacts found");
        return Ok(());
    }
    let mut removed = 0usize;
    let mut found = 0usize;

    for spec_file in spec_files {
        found += 1;
        let output_path = determine_implementation_output_path(&spec_file, SPECIFICATIONS_DIR)?;
        if output_path.exists() {
            if config.dry_run {
                println!("[DRY RUN] Would remove {}", output_path.display());
            } else {
                fs::remove_file(&output_path)
                    .with_context(|| format!("Failed to remove {}", output_path.display()))?;
            }
            removed += 1;
        }
    }

    if config.dry_run {
        if removed == 0 {
            println!("[DRY RUN] No implementation artifacts would be removed");
        } else {
            println!(
                "[DRY RUN] Would remove {} implementation artifact file(s)",
                removed
            );
        }
    } else {
        remove_empty_dirs_upward(Path::new("src/data"), Path::new("src"))?;
        remove_empty_dirs_upward(Path::new("src/contexts"), Path::new("src"))?;
        if removed == 0 {
            println!("No matching implementation artifacts found");
        } else {
            println!("✓ Removed {} implementation artifact file(s)", removed);
        }
    }
    if found == 0 {
        println!("No matching names were resolved in {}", SPECIFICATIONS_DIR);
    }
    Ok(())
}

fn clear_test_artifacts(names: Vec<String>, config: &Config) -> Result<()> {
    let spec_files = resolve_input_files(SPECIFICATIONS_DIR, names, "md", &CategoryFilter::all())?;
    if spec_files.is_empty() {
        println!("No test artifacts found");
        return Ok(());
    }
    let mut candidates = Vec::new();
    let mut found = 0usize;

    for spec_file in spec_files {
        found += 1;
        let test_paths = determine_bdd_test_paths(&spec_file, SPECIFICATIONS_DIR)?;
        candidates.push(test_paths.feature_path);
        candidates.push(test_paths.steps_path);
        candidates.push(test_paths.runner_path);
    }

    let mut removed = 0usize;
    let mut parent_dirs = Vec::new();
    for file in candidates {
        if file.exists() {
            if config.dry_run {
                println!("[DRY RUN] Would remove {}", file.display());
            } else {
                if let Some(parent) = file.parent() {
                    parent_dirs.push(parent.to_path_buf());
                }
                fs::remove_file(&file)
                    .with_context(|| format!("Failed to remove {}", file.display()))?;
                removed += 1;
            }
        }
    }

    if config.dry_run {
        if removed == 0 {
            println!("[DRY RUN] No test artifacts would be removed");
        } else {
            println!("[DRY RUN] Would remove {} test artifact file(s)", removed);
        }
    } else {
        sync_bdd_cargo_support(config)?;
        parent_dirs.sort();
        parent_dirs.dedup();
        for dir in parent_dirs {
            if dir.starts_with(Path::new("tests/features"))
                || dir.starts_with(Path::new("tests/steps"))
            {
                remove_empty_dirs_upward(&dir, Path::new("tests"))?;
            }
        }
        if removed == 0 {
            println!("No matching test artifacts found");
        } else {
            println!("✓ Removed {} test artifact file(s)", removed);
        }
    }
    if found == 0 {
        println!("No matching names were resolved in {}", SPECIFICATIONS_DIR);
    }
    Ok(())
}

fn remove_dir_if_empty(path: &Path) -> Result<()> {
    if !path.exists() || !path.is_dir() {
        return Ok(());
    }
    let mut entries =
        fs::read_dir(path).with_context(|| format!("Failed to inspect {}", path.display()))?;
    if entries.next().is_none() {
        fs::remove_dir(path)
            .with_context(|| format!("Failed to remove empty directory {}", path.display()))?;
    }
    Ok(())
}

fn remove_empty_dirs_upward(path: &Path, stop_at: &Path) -> Result<()> {
    let mut current = Some(path);
    while let Some(dir) = current {
        if dir == stop_at {
            remove_dir_if_empty(dir)?;
            break;
        }
        remove_dir_if_empty(dir)?;
        current = dir.parent();
    }
    Ok(())
}

fn collect_files_recursive(dir: &Path, extension: &str, files: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.exists() || !dir.is_dir() {
        return Ok(());
    }

    let entries = fs::read_dir(dir).with_context(|| format!("Failed to read {}", dir.display()))?;
    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            collect_files_recursive(&path, extension, files)?;
        } else if path
            .extension()
            .and_then(|s| s.to_str())
            .map(|ext| ext == extension)
            .unwrap_or(false)
        {
            files.push(path);
        }
    }

    Ok(())
}

fn collect_md_files_recursive(dir: &Path, extension: &str) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_files_recursive(dir, extension, &mut files)?;
    files.sort();
    Ok(files)
}

fn resolve_named_input_in_category(
    category_dir: &Path,
    name: &str,
    extension: &str,
) -> Result<Vec<PathBuf>> {
    let mut matches = Vec::new();
    if !category_dir.exists() || !category_dir.is_dir() {
        return Ok(matches);
    }

    let direct_path = category_dir.join(format!("{}.{}", name, extension));
    if direct_path.exists() {
        matches.push(direct_path);
        return Ok(matches);
    }

    let stem = Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(name);

    for path in collect_md_files_recursive(category_dir, extension)? {
        if path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|candidate| candidate == stem)
            .unwrap_or(false)
        {
            matches.push(path);
        }
    }

    matches.sort();
    matches.dedup();
    Ok(matches)
}

/// Resolves input files in a structured order:
/// 1. data/ folder (simple data types)
/// 2. contexts/ folder (use cases with role players)
/// 3. Root files (like app.md)
///
/// The `filter` controls which categories are included. When no filter is
/// active (both flags false), all three categories are scanned.
fn resolve_input_files(
    dir: &str,
    names: Vec<String>,
    extension: &str,
    filter: &CategoryFilter,
) -> Result<Vec<PathBuf>> {
    let dir_path = PathBuf::from(dir);

    if !dir_path.exists() {
        return Ok(Vec::new());
    }

    if names.is_empty() {
        let mut files = Vec::new();

        if filter.include_data() {
            let data_dir = dir_path.join("data");
            files.extend(collect_md_files_recursive(&data_dir, extension)?);
        }

        if filter.include_contexts() {
            let contexts_dir = dir_path.join("contexts");
            files.extend(collect_md_files_recursive(&contexts_dir, extension)?);
            let external_dir = dir_path.join("external_apis");
            files.extend(collect_md_files_recursive(&external_dir, extension)?);
            let apis_dir = dir_path.join("apis");
            files.extend(collect_md_files_recursive(&apis_dir, extension)?);
        }

        if filter.include_root() {
            let entries =
                fs::read_dir(&dir_path).context(format!("Failed to read {} directory", dir))?;
            for entry in entries {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some(extension) {
                    files.push(path);
                }
            }
        }

        files.sort();
        files.dedup();
        Ok(files)
    } else {
        let mut files = Vec::new();
        for name in names {
            let mut found = false;

            if filter.include_data() {
                let data_matches =
                    resolve_named_input_in_category(&dir_path.join("data"), &name, extension)?;
                if !data_matches.is_empty() {
                    files.extend(data_matches);
                    found = true;
                }
            }

            if !found && filter.include_contexts() {
                let context_matches =
                    resolve_named_input_in_category(&dir_path.join("contexts"), &name, extension)?;
                if !context_matches.is_empty() {
                    files.extend(context_matches);
                    found = true;
                } else {
                    let external_matches = resolve_named_input_in_category(
                        &dir_path.join("external_apis"),
                        &name,
                        extension,
                    )?;
                    if !external_matches.is_empty() {
                        files.extend(external_matches);
                        found = true;
                    } else {
                        let api_matches = resolve_named_input_in_category(
                            &dir_path.join("apis"),
                            &name,
                            extension,
                        )?;
                        if !api_matches.is_empty() {
                            files.extend(api_matches);
                            found = true;
                        }
                    }
                }
            }

            if !found && filter.include_root() {
                let root_path = dir_path.join(format!("{}.{}", name, extension));
                if root_path.exists() {
                    files.push(root_path);
                    found = true;
                }
            }

            if !found {
                let searched = match (
                    filter.include_data(),
                    filter.include_contexts(),
                    filter.include_root(),
                ) {
                    (true, true, true) => "data/, contexts/, and root",
                    (true, true, false) => "data/ and contexts/",
                    (true, false, false) => "data/",
                    (false, true, false) => "contexts/",
                    (false, false, true) => "root",
                    _ => "data/, contexts/, and root",
                };
                eprintln!(
                    "Warning: File not found: {}.{} (searched in {})",
                    name, extension, searched
                );
            }
        }
        files.sort();
        files.dedup();
        Ok(files)
    }
}

fn select_dependency_roots(
    selected_inputs: Vec<PathBuf>,
    base_dir: &str,
    names_provided: bool,
    filter: &CategoryFilter,
) -> Result<Vec<PathBuf>> {
    if names_provided {
        return Ok(selected_inputs);
    }

    let app_path = PathBuf::from(base_dir).join("app.md");
    if app_path.exists() && filter.matches_path(&app_path, base_dir) {
        return Ok(vec![app_path]);
    }

    Ok(selected_inputs)
}

/// Determines the specification output path preserving folder structure
///
/// Maps:
/// - drafts/data/X.md → specifications/data/X.md
/// - drafts/contexts/X.md → specifications/contexts/X.md
/// - drafts/external_apis/X.md → specifications/contexts/external/X.md
/// - drafts/apis/X.md → specifications/contexts/external/X.md
/// - drafts/X.md → specifications/X.md
fn determine_specification_output_path(
    draft_file: &Path,
    drafts_dir: &str,
    specifications_dir: &str,
) -> Result<PathBuf> {
    let draft_path = draft_file.to_path_buf();
    let drafts_path = PathBuf::from(drafts_dir);

    // Get relative path from drafts directory
    let relative_path = match draft_path.strip_prefix(&drafts_path) {
        Ok(rel) => rel.to_path_buf(),
        Err(_) => {
            // If strip_prefix fails, try component-based approach
            let draft_components: Vec<_> = draft_path.components().collect();
            let drafts_components: Vec<_> = drafts_path.components().collect();

            // Check if draft_path starts with drafts_path components
            if draft_components.len() > drafts_components.len()
                && draft_components
                    .iter()
                    .zip(drafts_components.iter())
                    .all(|(a, b)| a == b)
            {
                // Build path from remaining components
                PathBuf::from_iter(draft_components.iter().skip(drafts_components.len()))
            } else {
                // Use string-based fallback
                let draft_str = draft_file.to_str().unwrap_or("");
                let drafts_str = drafts_dir;
                if draft_str.starts_with(drafts_str) {
                    let rel_str = &draft_str[drafts_str.len()..].trim_start_matches('/');
                    PathBuf::from(rel_str)
                } else {
                    // Just use the filename
                    draft_path
                        .file_name()
                        .map(|n| PathBuf::from(n))
                        .unwrap_or_else(|| PathBuf::from(""))
                }
            }
        }
    };

    if relative_path
        .components()
        .next()
        .and_then(|component| component.as_os_str().to_str())
        .is_some_and(|component| matches!(component, "external_apis" | "apis"))
    {
        let remainder = PathBuf::from_iter(relative_path.components().skip(1));
        return Ok(PathBuf::from(specifications_dir)
            .join("contexts")
            .join("external")
            .join(remainder));
    }

    // Build output path in specifications directory
    let output_path = PathBuf::from(specifications_dir).join(relative_path);
    Ok(output_path)
}

/// Determines the draft input path preserving folder structure
///
/// Maps:
/// - specifications/data/X.md → drafts/data/X.md
/// - specifications/contexts/X.md → drafts/contexts/X.md
/// - specifications/contexts/external/X.md → drafts/external_apis/X.md (or drafts/apis/X.md when present)
/// - specifications/contexts/external/X/Y.md → drafts/external_apis/X.md (or drafts/apis/X.md when present)
/// - specifications/X.md → drafts/X.md
fn determine_draft_input_path(
    specification_file: &Path,
    specifications_dir: &str,
    drafts_dir: &str,
) -> Result<PathBuf> {
    let spec_path = specification_file.to_path_buf();
    let specs_root = PathBuf::from(specifications_dir);

    let relative_path = match spec_path.strip_prefix(&specs_root) {
        Ok(rel) => rel.to_path_buf(),
        Err(_) => {
            let spec_components: Vec<_> = spec_path.components().collect();
            let specs_components: Vec<_> = specs_root.components().collect();

            if spec_components.len() > specs_components.len()
                && spec_components
                    .iter()
                    .zip(specs_components.iter())
                    .all(|(a, b)| a == b)
            {
                PathBuf::from_iter(spec_components.iter().skip(specs_components.len()))
            } else {
                let spec_str = specification_file.to_str().unwrap_or("");
                if spec_str.starts_with(specifications_dir) {
                    let rel_str = &spec_str[specifications_dir.len()..].trim_start_matches('/');
                    PathBuf::from(rel_str)
                } else {
                    spec_path
                        .file_name()
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from(""))
                }
            }
        }
    };

    let mut components = relative_path.components();
    let first = components.next().and_then(|c| c.as_os_str().to_str());
    let second = components.next().and_then(|c| c.as_os_str().to_str());
    if first == Some("contexts") && second == Some("external") {
        let remainder = PathBuf::from_iter(relative_path.components().skip(2));
        if remainder.components().count() <= 1 {
            let external_apis_path = PathBuf::from(drafts_dir)
                .join("external_apis")
                .join(&remainder);
            if external_apis_path.exists() {
                return Ok(external_apis_path);
            }
            let apis_path = PathBuf::from(drafts_dir).join("apis").join(&remainder);
            if apis_path.exists() {
                return Ok(apis_path);
            }
            return Ok(external_apis_path);
        }
        let api_name = remainder
            .components()
            .next()
            .and_then(|component| component.as_os_str().to_str())
            .unwrap_or_default();
        let external_apis_path = PathBuf::from(drafts_dir)
            .join("external_apis")
            .join(format!("{api_name}.md"));
        if external_apis_path.exists() {
            return Ok(external_apis_path);
        }
        let apis_path = PathBuf::from(drafts_dir)
            .join("apis")
            .join(format!("{api_name}.md"));
        if apis_path.exists() {
            return Ok(apis_path);
        }
        return Ok(external_apis_path);
    }

    Ok(PathBuf::from(drafts_dir).join(relative_path))
}

/// Determines which specification agent to use based on file path
///
/// Returns:
/// - "create_specifications_data" for files in data/ folder
/// - "create_specifications_context" for files in contexts/ folder
/// - "create_specifications_external_api" for files in external_apis/ or apis/ folder
/// - "create_specifications_main" for files in root folder
fn determine_specification_agent(draft_file: &Path, drafts_dir: &str) -> &'static str {
    let draft_path = draft_file.to_path_buf();
    let drafts_path = PathBuf::from(drafts_dir);

    // Get relative path from drafts directory
    let relative_path = draft_path.strip_prefix(&drafts_path).unwrap_or(draft_file);

    // Check first component to determine folder
    if let Some(first_component) = relative_path.components().next() {
        let component_str = first_component.as_os_str().to_string_lossy();
        match component_str.as_ref() {
            "data" => "create_specifications_data",
            "contexts" => "create_specifications_context",
            "external_apis" | "apis" => "create_specifications_external_api",
            _ => "create_specifications_main",
        }
    } else {
        // Default to main for root files
        "create_specifications_main"
    }
}

fn infer_target_type_name(
    spec_file: &Path,
    specifications_root: &Path,
    drafts_root: &Path,
) -> Result<Option<String>> {
    let rel = match spec_file.strip_prefix(specifications_root) {
        Ok(r) => r.to_path_buf(),
        Err(_) => {
            return Ok(spec_file
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(to_pascal_case_title));
        }
    };

    let draft_path = drafts_root.join(&rel);
    if draft_path.exists() {
        let content = fs::read_to_string(&draft_path)
            .with_context(|| format!("Failed to read draft file: {}", draft_path.display()))?;
        if let Some(name) = extract_markdown_title_type(&content) {
            return Ok(Some(name));
        }
    }

    let spec_content = fs::read_to_string(spec_file)
        .with_context(|| format!("Failed to read specification file: {}", spec_file.display()))?;
    if let Some(name) = extract_markdown_title_type(&spec_content) {
        return Ok(Some(name));
    }

    Ok(spec_file
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(to_pascal_case_title))
}

fn extract_markdown_title_type(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") {
            let title = trimmed.trim_start_matches('#').trim();
            if !title.is_empty() {
                return to_pascal_case_title(title);
            }
        }
    }
    None
}

fn to_pascal_case_title(s: &str) -> Option<String> {
    let mut out = String::new();
    for raw in s.split(|c: char| !c.is_ascii_alphanumeric()) {
        if raw.is_empty() {
            continue;
        }
        let has_lower = raw.chars().any(|c| c.is_ascii_lowercase());
        let has_upper = raw.chars().any(|c| c.is_ascii_uppercase());
        let token = if has_lower && has_upper {
            let mut ch = raw.chars();
            match ch.next() {
                Some(first) => first.to_uppercase().collect::<String>() + ch.as_str(),
                None => String::new(),
            }
        } else {
            let lower = raw.to_ascii_lowercase();
            let mut ch = lower.chars();
            match ch.next() {
                Some(first) => first.to_uppercase().collect::<String>() + ch.as_str(),
                None => String::new(),
            }
        };
        out.push_str(&token);
    }
    if out.is_empty() { None } else { Some(out) }
}

fn relative_specification_path(context_file: &Path, specifications_dir: &str) -> Result<PathBuf> {
    let context_path = context_file.to_path_buf();
    let specifications_path = PathBuf::from(specifications_dir);

    let relative_path = match context_path.strip_prefix(&specifications_path) {
        Ok(rel) => rel.to_path_buf(),
        Err(_) => {
            let context_components: Vec<_> = context_path.components().collect();
            let specifications_components: Vec<_> = specifications_path.components().collect();

            if context_components.len() > specifications_components.len()
                && context_components
                    .iter()
                    .zip(specifications_components.iter())
                    .all(|(a, b)| a == b)
            {
                PathBuf::from_iter(
                    context_components
                        .iter()
                        .skip(specifications_components.len()),
                )
            } else {
                let context_str = context_file.to_str().unwrap_or("");
                let specifications_str = specifications_dir;
                if context_str.starts_with(specifications_str) {
                    let rel_str = &context_str[specifications_str.len()..].trim_start_matches('/');
                    PathBuf::from(rel_str)
                } else {
                    context_path
                        .file_name()
                        .map(PathBuf::from)
                        .unwrap_or_default()
                }
            }
        }
    };

    Ok(relative_path)
}

fn determine_bdd_test_paths(context_file: &Path, specifications_dir: &str) -> Result<BddTestPaths> {
    let relative_path = relative_specification_path(context_file, specifications_dir)?;
    let file_stem = relative_path
        .file_stem()
        .and_then(|s| s.to_str())
        .context("Invalid specification filename")?;

    let mut feature_relative = relative_path.clone();
    feature_relative.set_extension("feature");

    let mut steps_relative = relative_path.clone();
    steps_relative.set_file_name(format!("{}_steps.rs", file_stem.to_ascii_lowercase()));

    let runner_slug = relative_path
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .map(|part| {
            part.chars()
                .map(|ch| {
                    if ch.is_ascii_alphanumeric() {
                        ch.to_ascii_lowercase()
                    } else {
                        '_'
                    }
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("_")
        .trim_end_matches("_md")
        .to_string();
    let runner_test_name = format!("bdd_{}", runner_slug);

    Ok(BddTestPaths {
        feature_path: PathBuf::from("tests/features").join(feature_relative),
        steps_path: PathBuf::from("tests/steps").join(steps_relative),
        runner_path: PathBuf::from("tests").join(format!("{}.rs", runner_test_name)),
        runner_test_name,
    })
}

fn sync_bdd_cargo_support(config: &Config) -> Result<()> {
    let cargo_toml = PathBuf::from("Cargo.toml");
    if !cargo_toml.exists() {
        return Ok(());
    }

    let mut runner_paths = Vec::new();
    collect_bdd_runner_paths(Path::new("tests"), &mut runner_paths)?;
    runner_paths.sort();
    runner_paths.dedup();

    let content = fs::read_to_string(&cargo_toml)
        .with_context(|| format!("Failed to read {}", cargo_toml.display()))?;
    let content = ensure_dev_dependency_entry(
        &content,
        "cucumber",
        &format!("\"{}\"", BDD_CUCUMBER_VERSION),
    );
    let content = ensure_dev_dependency_entry(&content, "tokio", BDD_TOKIO_SPEC);
    let content = sync_managed_block(
        &content,
        BDD_TEST_TARGETS_START,
        BDD_TEST_TARGETS_END,
        &render_bdd_test_targets(&runner_paths),
    );
    fs::write(&cargo_toml, content)
        .with_context(|| format!("Failed to update {}", cargo_toml.display()))?;

    if config.verbose {
        println!(
            "✓ Synchronized Cargo.toml for {} BDD runner(s)",
            runner_paths.len()
        );
    }

    Ok(())
}

fn collect_bdd_runner_paths(dir: &Path, runner_paths: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.exists() || !dir.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(dir).with_context(|| format!("Failed to read {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            collect_bdd_runner_paths(&path, runner_paths)?;
            continue;
        }
        let is_runner = path
            .file_name()
            .and_then(|s| s.to_str())
            .map(|name| name.starts_with("bdd_") && name.ends_with(".rs"))
            .unwrap_or(false);
        if is_runner {
            runner_paths.push(path);
        }
    }

    Ok(())
}

fn ensure_dev_dependency_entry(content: &str, name: &str, spec: &str) -> String {
    let mut lines = content.lines().map(ToString::to_string).collect::<Vec<_>>();
    let section_idx = lines
        .iter()
        .position(|line| line.trim() == "[dev-dependencies]");

    match section_idx {
        Some(idx) => {
            let section_end = lines
                .iter()
                .enumerate()
                .skip(idx + 1)
                .find(|(_, line)| {
                    let trimmed = line.trim();
                    trimmed.starts_with('[') && trimmed.ends_with(']')
                })
                .map(|(index, _)| index)
                .unwrap_or(lines.len());
            let new_line = format!("{name} = {spec}");
            if let Some(existing_idx) = (idx + 1..section_end).find(|line_idx| {
                lines[*line_idx]
                    .split('=')
                    .next()
                    .map(|candidate| candidate.trim() == name)
                    .unwrap_or(false)
            }) {
                lines[existing_idx] = new_line;
            } else {
                lines.insert(section_end, new_line);
            }
        }
        None => {
            if !lines.is_empty() && !lines.last().is_some_and(|line| line.is_empty()) {
                lines.push(String::new());
            }
            lines.push("[dev-dependencies]".to_string());
            lines.push(format!("{name} = {spec}"));
        }
    }

    format!("{}\n", lines.join("\n"))
}

fn render_bdd_test_targets(runner_paths: &[PathBuf]) -> String {
    runner_paths
        .iter()
        .filter_map(|path| {
            let test_name = path.file_stem()?.to_str()?;
            Some(format!(
                "[[test]]\nname = \"{}\"\npath = \"{}\"\nharness = false",
                test_name,
                path.to_string_lossy()
            ))
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn sync_managed_block(content: &str, start_marker: &str, end_marker: &str, body: &str) -> String {
    let replacement = if body.trim().is_empty() {
        format!("{start_marker}\n{end_marker}")
    } else {
        format!("{start_marker}\n{body}\n{end_marker}")
    };

    match (content.find(start_marker), content.find(end_marker)) {
        (Some(start), Some(end)) if start <= end => {
            let end_idx = end + end_marker.len();
            let mut updated = String::new();
            updated.push_str(&content[..start]);
            if !updated.ends_with('\n') && !updated.is_empty() {
                updated.push('\n');
            }
            updated.push_str(&replacement);
            updated.push_str(&content[end_idx..]);
            if !updated.ends_with('\n') {
                updated.push('\n');
            }
            updated
        }
        _ => {
            let mut updated = content.trim_end().to_string();
            if !updated.is_empty() {
                updated.push_str("\n\n");
            }
            updated.push_str(&replacement);
            updated.push('\n');
            updated
        }
    }
}

/// Determines the implementation output path preserving folder structure
///
/// Maps:
/// - specifications/data/X.md → src/data/X.rs
/// - specifications/contexts/X.md → src/contexts/X.rs
/// - specifications/X.md → src/X.rs (or src/main.rs for app.md)
fn determine_implementation_output_path(
    context_file: &Path,
    specifications_dir: &str,
) -> Result<PathBuf> {
    let relative_path = relative_specification_path(context_file, specifications_dir)?;

    let file_stem = relative_path
        .file_stem()
        .and_then(|s| s.to_str())
        .context("Invalid context filename")?;

    // Special case: app.md → main.rs
    let output_filename = if file_stem.eq_ignore_ascii_case("app") {
        "main.rs"
    } else {
        let mut output_rel = relative_path.to_path_buf();
        output_rel.set_file_name(format!("{}.rs", file_stem.to_ascii_lowercase()));
        return Ok(PathBuf::from("src").join(output_rel));
    };

    let output_path = PathBuf::from("src").join(output_filename);
    Ok(output_path)
}

fn generated_project_structure_paths(project_info: &ProjectInfo) -> Vec<PathBuf> {
    let mut paths = vec![PathBuf::from("Cargo.toml"), PathBuf::from("src/lib.rs")];

    let mut folders: Vec<_> = project_info.modules.keys().cloned().collect();
    folders.sort();
    folders.dedup();

    for folder in folders {
        if folder.is_empty() {
            continue;
        }
        paths.push(PathBuf::from("src").join(&folder).join("mod.rs"));
        if let Some(modules) = project_info.modules.get(&folder) {
            for module in modules {
                paths.push(
                    PathBuf::from("src")
                        .join(&folder)
                        .join(format!("{}.rs", module)),
                );
            }
        }
    }

    paths.sort();
    paths.dedup();
    paths
}

#[cfg(test)]
mod tests {
    use super::{
        BDD_TEST_TARGETS_END, BDD_TEST_TARGETS_START, CategoryFilter,
        build_dependency_drafts_from_context, build_dependency_manifest, build_execution_plan,
        build_implementation_execution_plan, build_implemented_dependency_manifest,
        determine_bdd_test_paths, determine_draft_input_path, determine_implementation_output_path,
        determine_specification_output_path, ensure_dev_dependency_entry,
        external_generated_context_output_path, external_generated_data_output_path,
        extract_actionable_blocking_bullets_for_path, extract_compile_error_message,
        generated_project_structure_paths, parse_generated_files,
        resolve_implementation_dependency_inputs, resolve_input_files, sync_managed_block,
    };
    use crate::cli::dependency_graph::{DependencyArtifact, DependencySource};
    use crate::cli::project_structure::ProjectInfo;
    use std::collections::HashMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time ok")
            .as_nanos();
        std::env::temp_dir().join(format!("reen_cli_{}_{}", prefix, nanos))
    }

    #[test]
    fn extracts_compile_error_message_from_generated_code() {
        let code = r#"#![cfg(feature = "account")]
compile_error!(
    "ERROR: Cannot implement specification as written.

Problem:
- Missing required role method.
"
);"#;

        let msg = extract_compile_error_message(code).expect("expected compile_error message");
        assert!(msg.contains("ERROR: Cannot implement specification as written."));
        assert!(msg.contains("Missing required role method."));
    }

    #[test]
    fn returns_none_when_compile_error_macro_is_absent() {
        let code = "pub struct Account {}";
        assert!(extract_compile_error_message(code).is_none());
    }

    #[test]
    fn maps_specification_path_back_to_draft_path() {
        let path = determine_specification_output_path(
            Path::new("specifications/contexts/game_loop.md"),
            "specifications",
            "drafts",
        )
        .expect("path mapping");
        assert_eq!(path, Path::new("drafts/contexts/game_loop.md"));
    }

    #[test]
    fn maps_nested_specification_path_back_to_draft_path() {
        let path = determine_specification_output_path(
            Path::new("specifications/contexts/ui/terminal_renderer.md"),
            "specifications",
            "drafts",
        )
        .expect("path mapping");
        assert_eq!(path, Path::new("drafts/contexts/ui/terminal_renderer.md"));
    }

    #[test]
    fn maps_external_api_draft_to_contexts_external_specification_path() {
        let path = determine_specification_output_path(
            Path::new("drafts/external_apis/stripe.md"),
            "drafts",
            "specifications",
        )
        .expect("path mapping");
        assert_eq!(
            path,
            Path::new("specifications/contexts/external/stripe.md")
        );
    }

    #[test]
    fn maps_apis_draft_to_contexts_external_specification_path() {
        let path = determine_specification_output_path(
            Path::new("drafts/apis/aisstream.md"),
            "drafts",
            "specifications",
        )
        .expect("path mapping");
        assert_eq!(
            path,
            Path::new("specifications/contexts/external/aisstream.md")
        );
    }

    #[test]
    fn maps_contexts_external_specification_back_to_external_api_draft_path() {
        let path = determine_draft_input_path(
            Path::new("specifications/contexts/external/stripe.md"),
            "specifications",
            "drafts",
        )
        .expect("path mapping");
        assert_eq!(path, Path::new("drafts/external_apis/stripe.md"));
    }

    #[test]
    fn maps_nested_external_context_specification_back_to_root_external_api_draft() {
        let path = determine_draft_input_path(
            Path::new("specifications/contexts/external/stripe/auth_flow.md"),
            "specifications",
            "drafts",
        )
        .expect("path mapping");
        assert_eq!(path, Path::new("drafts/external_apis/stripe.md"));
    }

    #[test]
    fn builds_generated_external_output_paths_under_external_namespaces() {
        assert_eq!(
            external_generated_data_output_path("specifications", "stripe", "PaymentIntent"),
            Path::new("specifications/data/external/stripe/PaymentIntent.md")
        );
        assert_eq!(
            external_generated_context_output_path("specifications", "stripe", "auth_flow"),
            Path::new("specifications/contexts/external/stripe/auth_flow.md")
        );
    }

    #[test]
    fn implementation_plan_keeps_generated_external_specs_and_their_dependencies() {
        let root = temp_root("external_impl_plan");
        let drafts = root.join("drafts");
        let specs = root.join("specifications");
        fs::create_dir_all(drafts.join("apis")).expect("mkdir drafts apis");
        fs::create_dir_all(specs.join("contexts/external")).expect("mkdir contexts external");
        fs::create_dir_all(specs.join("data/external/aisstream")).expect("mkdir data external");

        let external_draft = drafts.join("apis/aisstream.md");
        let context_spec = specs.join("contexts/external/aisstream.md");
        let message_spec = specs.join("data/external/aisstream/AisStreamMessage.md");
        let subscription_spec = specs.join("data/external/aisstream/SubscriptionMessage.md");
        let position_spec = specs.join("data/external/aisstream/PositionReport.md");

        fs::write(
            &external_draft,
            "# AISStream API draft\n\n## OpenAPI\n- Local: specs/aisstream.yaml\n",
        )
        .expect("write draft");
        fs::create_dir_all(drafts.join("apis/specs")).expect("mkdir api specs");
        fs::write(
            drafts.join("apis/specs/aisstream.yaml"),
            "openapi: 3.1.0\npaths: {}\ncomponents: {}\n",
        )
        .expect("write openapi spec");
        fs::write(
            &context_spec,
            "# AISStream\n\nUses `SubscriptionMessage` and `AisStreamMessage`.\n",
        )
        .expect("write context spec");
        fs::write(
            &message_spec,
            "# AisStreamMessage\n\nCarries `PositionReport` payloads.\n",
        )
        .expect("write message spec");
        fs::write(&subscription_spec, "# SubscriptionMessage\n").expect("write subscription spec");
        fs::write(&position_spec, "# PositionReport\n").expect("write position spec");

        let implementation_inputs = resolve_implementation_dependency_inputs(
            vec![
                context_spec.clone(),
                message_spec.clone(),
                subscription_spec.clone(),
                position_spec.clone(),
            ],
            specs.to_str().unwrap_or("specifications"),
            drafts.to_str().unwrap_or("drafts"),
        )
        .expect("implementation inputs");

        assert!(implementation_inputs.contains(&context_spec));
        assert!(!implementation_inputs.contains(&external_draft));

        let levels = build_execution_plan(
            implementation_inputs.clone(),
            specs.to_str().unwrap_or("specifications"),
            Some(drafts.to_str().unwrap_or("drafts")),
        )
        .expect("plan");

        let context_node = levels
            .iter()
            .flatten()
            .find(|node| node.input_path == context_spec)
            .expect("context node");
        let message_node = levels
            .iter()
            .flatten()
            .find(|node| node.input_path == message_spec)
            .expect("message node");
        assert_eq!(
            context_node.direct_dependency_names(),
            vec![
                "AisStreamMessage".to_string(),
                "SubscriptionMessage".to_string()
            ]
        );
        assert_eq!(
            message_node.direct_dependency_names(),
            vec!["PositionReport".to_string()]
        );

        let original_dir = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(&root).expect("set cwd");
        let implementation_levels = build_implementation_execution_plan(
            vec![context_spec, message_spec, subscription_spec, position_spec],
            &CategoryFilter::all(),
            specs.to_str().unwrap_or("specifications"),
            drafts.to_str().unwrap_or("drafts"),
        )
        .expect("implementation plan");
        std::env::set_current_dir(original_dir).expect("restore cwd");

        let context_level = implementation_levels
            .iter()
            .position(|level| level.iter().any(|node| node.name == "aisstream"))
            .expect("context level");
        let message_level = implementation_levels
            .iter()
            .position(|level| level.iter().any(|node| node.name == "AisStreamMessage"))
            .expect("message level");
        let position_level = implementation_levels
            .iter()
            .position(|level| level.iter().any(|node| node.name == "PositionReport"))
            .expect("position level");

        assert!(position_level < message_level);
        assert!(message_level < context_level);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn determine_implementation_output_path_preserves_nested_hierarchy() {
        let path = determine_implementation_output_path(
            Path::new("specifications/contexts/ui/terminal_renderer.md"),
            "specifications",
        )
        .expect("implementation path");
        assert_eq!(path, Path::new("src/contexts/ui/terminal_renderer.rs"));
    }

    #[test]
    fn resolve_input_files_discovers_nested_paths_and_stems() {
        let root = temp_root("nested_inputs");
        let drafts = root.join("drafts");
        fs::create_dir_all(drafts.join("contexts/ui")).expect("mkdir");
        fs::create_dir_all(drafts.join("external_apis")).expect("mkdir");
        fs::create_dir_all(drafts.join("apis")).expect("mkdir");
        fs::create_dir_all(drafts.join("data/payments")).expect("mkdir");
        fs::write(
            drafts.join("contexts/ui/terminal_renderer.md"),
            "# Terminal Renderer",
        )
        .expect("write");
        fs::write(drafts.join("external_apis/stripe.md"), "# Stripe API").expect("write");
        fs::write(drafts.join("apis/aisstream.md"), "# AISStream API").expect("write");
        fs::write(
            drafts.join("data/payments/ledger_entry.md"),
            "# Ledger Entry",
        )
        .expect("write");

        let all = resolve_input_files(
            drafts.to_str().expect("drafts path"),
            Vec::new(),
            "md",
            &CategoryFilter::all(),
        )
        .expect("all files");
        assert!(
            all.iter()
                .any(|p| p.ends_with("contexts/ui/terminal_renderer.md"))
        );
        assert!(all.iter().any(|p| p.ends_with("external_apis/stripe.md")));
        assert!(all.iter().any(|p| p.ends_with("apis/aisstream.md")));
        assert!(
            all.iter()
                .any(|p| p.ends_with("data/payments/ledger_entry.md"))
        );

        let by_stem = resolve_input_files(
            drafts.to_str().expect("drafts path"),
            vec!["terminal_renderer".to_string()],
            "md",
            &CategoryFilter::all(),
        )
        .expect("stem lookup");
        assert_eq!(by_stem.len(), 1);
        assert!(by_stem[0].ends_with("contexts/ui/terminal_renderer.md"));

        let by_external_name = resolve_input_files(
            drafts.to_str().expect("drafts path"),
            vec!["stripe".to_string()],
            "md",
            &CategoryFilter {
                contexts: true,
                data: false,
            },
        )
        .expect("external lookup");
        assert_eq!(by_external_name.len(), 1);
        assert!(by_external_name[0].ends_with("external_apis/stripe.md"));

        let by_api_name = resolve_input_files(
            drafts.to_str().expect("drafts path"),
            vec!["aisstream".to_string()],
            "md",
            &CategoryFilter {
                contexts: true,
                data: false,
            },
        )
        .expect("api lookup");
        assert_eq!(by_api_name.len(), 1);
        assert!(by_api_name[0].ends_with("apis/aisstream.md"));

        let by_nested_name = resolve_input_files(
            drafts.to_str().expect("drafts path"),
            vec!["payments/ledger_entry".to_string()],
            "md",
            &CategoryFilter {
                contexts: false,
                data: true,
            },
        )
        .expect("nested lookup");
        assert_eq!(by_nested_name.len(), 1);
        assert!(by_nested_name[0].ends_with("data/payments/ledger_entry.md"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn generated_project_structure_paths_include_nested_mod_files() {
        let mut modules = HashMap::new();
        modules.insert("contexts".to_string(), vec!["account".to_string()]);
        modules.insert(
            "contexts/ui".to_string(),
            vec!["terminal_renderer".to_string()],
        );

        let project_info = ProjectInfo {
            modules,
            ..Default::default()
        };

        let paths = generated_project_structure_paths(&project_info);
        assert!(paths.contains(&PathBuf::from("Cargo.toml")));
        assert!(paths.contains(&PathBuf::from("src/lib.rs")));
        assert!(paths.contains(&PathBuf::from("src/contexts/mod.rs")));
        assert!(paths.contains(&PathBuf::from("src/contexts/account.rs")));
        assert!(paths.contains(&PathBuf::from("src/contexts/ui/mod.rs")));
        assert!(paths.contains(&PathBuf::from("src/contexts/ui/terminal_renderer.rs")));
    }

    #[test]
    fn determine_bdd_test_paths_preserves_feature_hierarchy() {
        let paths = determine_bdd_test_paths(
            Path::new("specifications/contexts/ui/terminal_renderer.md"),
            "specifications",
        )
        .expect("bdd test paths");
        assert_eq!(
            paths.feature_path,
            PathBuf::from("tests/features/contexts/ui/terminal_renderer.feature")
        );
        assert_eq!(
            paths.steps_path,
            PathBuf::from("tests/steps/contexts/ui/terminal_renderer_steps.rs")
        );
        assert_eq!(
            paths.runner_path,
            PathBuf::from("tests/bdd_contexts_ui_terminal_renderer.rs")
        );
        assert_eq!(paths.runner_test_name, "bdd_contexts_ui_terminal_renderer");
    }

    #[test]
    fn parse_generated_files_reads_xml_style_blocks() {
        let output = r#"<file path="tests/features/account.feature">
Feature: Account

  Scenario: Open an account
    Given an account exists
</file>
<file path="tests/steps/account_steps.rs">
use super::GeneratedWorld;
</file>
<file path="tests/bdd_account.rs">
fn main() {}
</file>"#;

        let files = parse_generated_files(output).expect("parse generated files");
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].0, PathBuf::from("tests/features/account.feature"));
        assert!(files[0].1.contains("Feature: Account"));
        assert_eq!(files[1].0, PathBuf::from("tests/steps/account_steps.rs"));
        assert_eq!(files[2].0, PathBuf::from("tests/bdd_account.rs"));
    }

    #[test]
    fn cargo_support_helpers_insert_dev_deps_and_managed_targets() {
        let content = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n";
        let updated = ensure_dev_dependency_entry(content, "cucumber", "\"0.22.1\"");
        let updated = ensure_dev_dependency_entry(
            &updated,
            "tokio",
            r#"{ version = "1.40", features = ["macros", "rt-multi-thread"] }"#,
        );
        let updated = sync_managed_block(
            &updated,
            BDD_TEST_TARGETS_START,
            BDD_TEST_TARGETS_END,
            "[[test]]\nname = \"bdd_account\"\npath = \"tests/bdd_account.rs\"\nharness = false",
        );

        assert!(updated.contains("[dev-dependencies]"));
        assert!(updated.contains("cucumber = \"0.22.1\""));
        assert!(updated.contains(
            "tokio = { version = \"1.40\", features = [\"macros\", \"rt-multi-thread\"] }"
        ));
        assert!(updated.contains(BDD_TEST_TARGETS_START));
        assert!(updated.contains("name = \"bdd_account\""));
        assert!(updated.contains(BDD_TEST_TARGETS_END));
    }

    #[test]
    fn dependency_manifest_uses_metadata_only_and_marks_directness() {
        let direct = DependencyArtifact {
            name: "amount".to_string(),
            path: "drafts/data/amount.md".to_string(),
            source: DependencySource::Primary,
            content: "full amount content".to_string(),
            sha256: "abc123".to_string(),
        };
        let transitive = DependencyArtifact {
            name: "currency".to_string(),
            path: "drafts/data/currency.md".to_string(),
            source: DependencySource::Primary,
            content: "full currency content".to_string(),
            sha256: "def456".to_string(),
        };

        let manifest = build_dependency_manifest(&[direct.clone(), transitive], &[direct]);
        assert_eq!(manifest.len(), 2);
        assert!(manifest[0].get("content").is_none());
        assert_eq!(manifest[0]["dependency_kind"], "direct");
        assert_eq!(manifest[1]["dependency_kind"], "transitive");
    }

    #[test]
    fn implemented_dependency_manifest_uses_metadata_only() {
        let direct = DependencyArtifact {
            name: "amount".to_string(),
            path: "specifications/data/amount.md".to_string(),
            source: DependencySource::Primary,
            content: "spec".to_string(),
            sha256: "abc123".to_string(),
        };
        let implemented = serde_json::json!([
            {
                "name": "amount",
                "spec_path": "specifications/data/amount.md",
                "path": "src/data/amount.rs",
                "content": "pub struct Amount;",
                "sha256": "impl123"
            }
        ]);

        let manifest = build_implemented_dependency_manifest(
            implemented.as_array().expect("array"),
            &[direct],
        );
        assert_eq!(manifest.len(), 1);
        assert!(manifest[0].get("content").is_none());
        assert_eq!(manifest[0]["dependency_kind"], "direct");
    }

    #[test]
    fn build_dependency_drafts_uses_hidden_tool_context_when_available() {
        let context = HashMap::from([(
            "dependency_tool_context".to_string(),
            serde_json::json!({
                "dependency_artifacts": [
                    {
                        "path": "drafts/data/amount.md",
                        "content": "Amount draft"
                    }
                ]
            }),
        )]);

        let drafts = build_dependency_drafts_from_context(&context);
        assert_eq!(drafts["drafts/data/amount.md"], "Amount draft");
    }

    #[test]
    fn ignores_placeholder_no_issue_bullets() {
        let section = r#"
- None
- no blocking ambiguities
- N/A
"#;
        let actionable = extract_actionable_blocking_bullets_for_path(section, None);
        assert!(actionable.is_empty());
    }

    #[test]
    fn preserves_real_blockers_while_ignoring_placeholders() {
        let section = r#"
- none
- Missing required role method for game loop construction
"#;
        let actionable = extract_actionable_blocking_bullets_for_path(section, None);
        assert_eq!(actionable.len(), 1);
        assert!(actionable[0].contains("Missing required role method"));
    }

    #[test]
    fn ignores_external_upstream_source_gaps_as_actionable_blockers() {
        let section = r#"
1. **Undefined `MetaData` Structure**:
   - The `MetaData` field lacks a defined structure.
   - Its purpose and contents are unspecified.

2. **Undocumented Limits on Subscription Parameters**:
   - The documentation does not specify the maximum number of filters.
"#;
        let actionable = extract_actionable_blocking_bullets_for_path(
            section,
            Some(Path::new("specifications/contexts/external/aisstream.md")),
        );
        assert!(actionable.is_empty());
    }

    #[test]
    fn preserves_external_conflicts_as_actionable_blockers() {
        let section = r#"
- Documentation conflicts with the OpenAPI authentication requirement for the same request.
"#;
        let actionable = extract_actionable_blocking_bullets_for_path(
            section,
            Some(Path::new("specifications/contexts/external/world_bank.md")),
        );
        assert_eq!(actionable.len(), 1);
        assert!(actionable[0].contains("conflicts"));
    }
}
