use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::env;
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use tokio::task::JoinSet;

mod agent_executor;
mod artifact_backend;
mod capability_registry;
mod cargo_commands;
mod compilation_fix;
mod contract_store;
mod contracts;
mod dependency_graph;
mod dependency_tooling;
mod draft_schema;
mod external_api_expansion;
mod interface_capsules;
mod interface_resolution;
mod openapi_fetcher;
mod patch_service;
mod pipeline_context;
mod pipeline_quality;
mod planning;
mod progress;
mod project_structure;
mod rate_limiter;
mod resolved_contract;
mod run_context;
mod stage_runner;
mod types_manifest;
mod usage_report;
pub mod yaml_config;

use agent_executor::{AgentExecutor, AgentResponse};
use artifact_backend::{
    ArtifactCategory, ArtifactKind, ArtifactStore, BackendSelection, build_artifact_store,
};
use capability_registry::{
    CapabilityRegistry, add_capability_mapping_to_registry, bootstrap_registry_from_scan,
    builtin_provider_catalog_json, capability_registry_path, empty_registry, ensure_scan_coverage,
    load_capability_registry, merge_registry_proposals, parse_capability_registry_fragment,
    scan_draft_capabilities, sync_dependency_manifest_from_capability_registry,
    write_capability_registry,
};
use contract_store::{
    ContractBundle, ContractStore, LevelPolicy, UpstreamInterfaceRef, draft_relative_path,
    level_hash,
};
use contracts::{
    build_contract_artifact, contract_artifact_to_context_value,
    contract_validation_to_context_value, validate_contract_artifact,
};
use dependency_graph::{
    DependencyArtifact, ExecutionDag, ExecutionNode, ExecutionUnit, build_execution_dag,
    build_execution_plan, expand_with_transitive_dependencies,
};
use dependency_tooling::{
    ensure_tooling_artifacts_fresh, load_dependency_manifest, merge_manifest_dependencies,
};
use draft_schema::{DraftDocument, parse_repo_draft};
use external_api_expansion::{
    GeneratedDraftArtifact, parse_external_api_expansion, sanitize_generated_artifact_name,
};
use interface_capsules::InterfaceCapsule;
use interface_resolution::{InterfaceResolutionOutput, parse_interface_resolution_output};
use openapi_fetcher::is_external_api_draft_path;
use pipeline_context::{
    build_specification_context, find_cached_context_variant, fit_context_to_token_limit,
};
use pipeline_quality::{
    SpecificationKind, StaticBehaviorVerifierReport, analyze_specification,
    contract_to_context_value, verify_generated_implementation, write_json_report,
};
use planning::{ExecutionPlan, PlanKind, build_default_plan, plan_to_context_value, validate_plan};
use progress::{
    ProgressIndicator, error_tag, error_text, header_text, standard_text, success_text,
    warning_tag, warning_text,
};
use project_structure::{
    ProjectInfo, analyze_specifications, generate_cargo_toml, generate_lib_rs, generate_mod_files,
};
use reen::build_tracker::{BuildTracker, Stage, UpdateReason};
use reen::execution::{
    AgentModelRegistry, AgentRegistry, NativeExecutionControl, normalize_cache_input_value,
};
use reen::registries::{FileAgentModelRegistry, FileAgentRegistry};
use resolved_contract::synthesize_contract_resolution;
use run_context::RunContextCache;
pub use stage_runner::DEFAULT_PARALLEL_LIMIT;
use stage_runner::{
    CliExecutionControl, ExecutionResources, StageItem, estimate_agent_request_tokens,
};
use usage_report::{UsageReporter, UsageScope};

#[derive(Clone)]
pub struct Config {
    pub verbose: bool,
    pub debug: bool,
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

fn log_build_tracker_skip(verbose: bool, stage: &str, artifact_name: &str) {
    if verbose {
        println!(
            "{}",
            standard_text(format!(
                "⊚ Build tracker skip for {} '{}'; artifact is up to date",
                stage, artifact_name
            ))
        );
    }
}

fn short_hash(hash: &str) -> String {
    hash.chars().take(12).collect()
}

fn log_build_tracker_update_reason(
    verbose: bool,
    stage: &str,
    artifact_name: &str,
    reason: &UpdateReason,
) {
    if !verbose || matches!(reason, UpdateReason::UpToDate) {
        return;
    }

    let detail = match reason {
        UpdateReason::UpToDate => return,
        UpdateReason::OutputMissing => "output file is missing".to_string(),
        UpdateReason::MissingStageRecord => "no stage cache records exist yet".to_string(),
        UpdateReason::MissingFileRecord => "no cached record exists for this artifact".to_string(),
        UpdateReason::InputChanged { expected, actual } => format!(
            "input hash changed ({} -> {})",
            short_hash(expected),
            short_hash(actual)
        ),
        UpdateReason::DependencyChanged { expected, actual } => format!(
            "dependency fingerprint changed ({} -> {})",
            short_hash(expected),
            short_hash(actual)
        ),
    };

    println!(
        "{}",
        standard_text(format!(
            "⊚ Build tracker refresh for {} '{}'; {}",
            stage, artifact_name, detail
        ))
    );
}

fn log_agent_response_cache_hit(verbose: bool, stage: &str, artifact_name: &str, agent_name: &str) {
    if verbose {
        println!(
            "{}",
            standard_text(format!(
                "⊚ Agent response cache hit for {} '{}' via {}; reusing cached model output",
                stage, artifact_name, agent_name
            ))
        );
    }
}

fn extract_json_object(output: &str) -> Option<String> {
    let fenced = regex::Regex::new(r"(?s)```json\s*(\{.*\})\s*```").ok();
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

fn configured_shared_type_choices() -> Vec<String> {
    let mut rows = vec![
        "integer=i32".to_string(),
        "non_negative_integer=u32".to_string(),
        "timestamp_millis=u64".to_string(),
    ];
    if let Ok(config) = yaml_config::load_config() {
        if let Some(policy) = config.type_policy {
            if let Some(value) = policy.integer {
                rows.retain(|row| !row.starts_with("integer="));
                rows.push(format!("integer={value}"));
            }
            if let Some(value) = policy.non_negative_integer {
                rows.retain(|row| !row.starts_with("non_negative_integer="));
                rows.push(format!("non_negative_integer={value}"));
            }
            if let Some(value) = policy.timestamp_millis {
                rows.retain(|row| !row.starts_with("timestamp_millis="));
                rows.push(format!("timestamp_millis={value}"));
            }
        }
    }
    rows
}

fn default_contract_level_policy(unit: &ExecutionUnit) -> LevelPolicy {
    let artifact_paths = unit
        .nodes
        .iter()
        .map(|node| node.input_path.display().to_string())
        .collect::<Vec<_>>();
    LevelPolicy {
        stage: "contract".to_string(),
        level_hash: level_hash(
            &unit
                .nodes
                .iter()
                .map(|node| node.input_path.clone())
                .collect::<Vec<_>>(),
        ),
        artifact_paths,
        canonical_names: unit.nodes.iter().map(|node| node.name.clone()).collect(),
        import_roots: Vec::new(),
        feature_names: Vec::new(),
        shared_type_choices: configured_shared_type_choices(),
        collaborator_abstractions: Vec::new(),
        conflict_resolutions: Vec::new(),
        name_bindings: Vec::new(),
        container_shapes: Vec::new(),
    }
}

fn parse_level_policy_output(output: &str, fallback: &LevelPolicy) -> Result<LevelPolicy> {
    let candidate = extract_json_object(output)
        .ok_or_else(|| anyhow::anyhow!("coordination agent did not return a JSON object"))?;
    let mut policy: LevelPolicy = serde_json::from_str(&candidate)
        .context("coordination output was not valid policy JSON")?;
    if policy.stage.trim().is_empty() {
        policy.stage = fallback.stage.clone();
    }
    if policy.level_hash.trim().is_empty() {
        policy.level_hash = fallback.level_hash.clone();
    }
    if policy.artifact_paths.is_empty() {
        policy.artifact_paths = fallback.artifact_paths.clone();
    }
    if policy.canonical_names.is_empty() {
        policy.canonical_names = fallback.canonical_names.clone();
    }
    if policy.shared_type_choices.is_empty() {
        policy.shared_type_choices = fallback.shared_type_choices.clone();
    }
    Ok(policy)
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
    pub projections: bool,
    pub data: bool,
}

impl CategoryFilter {
    pub fn all() -> Self {
        Self {
            contexts: false,
            projections: false,
            data: false,
        }
    }

    fn is_active(&self) -> bool {
        self.contexts || self.projections || self.data
    }

    fn include_data(&self) -> bool {
        !self.is_active() || self.data
    }

    fn include_projections(&self) -> bool {
        !self.is_active() || self.projections
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
                    "projections" => self.include_projections(),
                    "contexts" | "external_apis" | "apis" => self.include_contexts(),
                    _ => self.include_root(),
                };
            }
        }
        self.include_root()
    }
}

const DRAFTS_DIR: &str = "drafts";
const SPECIFICATIONS_DIR: &str = ".reen/specifications";

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
const IMPLEMENTATION_VERIFIER_RETRY_LIMIT: usize = 1;

#[derive(Clone, Default)]
struct SerialAgentGates {
    gates: Arc<HashMap<String, Arc<AsyncMutex<()>>>>,
}

impl SerialAgentGates {
    fn new<I, S>(serial_agents: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let gates = serial_agents
            .into_iter()
            .map(|agent_name| (agent_name.into(), Arc::new(AsyncMutex::new(()))))
            .collect();
        Self {
            gates: Arc::new(gates),
        }
    }

    async fn acquire(&self, agent_name: &str) -> Option<OwnedMutexGuard<()>> {
        let gate = self.gates.get(agent_name)?.clone();
        Some(gate.lock_owned().await)
    }
}

async fn run_execution_dag_units<R, F, Fut, L, C, S>(
    dag: &ExecutionDag,
    parallel_limit: usize,
    mut on_launch: L,
    mut on_complete: C,
    classify_success: S,
    process_unit: F,
) -> Result<Vec<(usize, Result<R>)>>
where
    R: Send + 'static,
    F: Fn(ExecutionUnit) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<R>> + Send + 'static,
    L: FnMut(&ExecutionUnit),
    C: FnMut(usize, &Result<R>),
    S: Fn(&R) -> bool,
{
    let units = dag.units().to_vec();
    if units.is_empty() {
        return Ok(Vec::new());
    }

    let max_in_flight = parallel_limit.max(1);
    let mut remaining_deps = units
        .iter()
        .map(|unit| unit.dependency_units.len())
        .collect::<Vec<_>>();
    let mut dependents = vec![Vec::new(); units.len()];
    for unit in &units {
        for dep in &unit.dependency_units {
            dependents[*dep].push(unit.id);
        }
    }
    for dep_list in &mut dependents {
        dep_list.sort_unstable();
        dep_list.dedup();
    }

    let sort_keys = units
        .iter()
        .map(ExecutionUnit::sort_key)
        .collect::<Vec<_>>();
    let mut ready = BTreeSet::new();
    for unit in &units {
        if remaining_deps[unit.id] == 0 {
            ready.insert((sort_keys[unit.id].clone(), unit.id));
        }
    }

    let process_unit = Arc::new(process_unit);
    let mut tasks = JoinSet::new();
    let mut results = Vec::new();
    let mut stop_launching = false;

    loop {
        while !stop_launching && tasks.len() < max_in_flight {
            let Some((_, unit_id)) = ready.iter().next().cloned() else {
                break;
            };
            ready.remove(&(sort_keys[unit_id].clone(), unit_id));
            let unit = units[unit_id].clone();
            on_launch(&unit);
            let process_unit = process_unit.clone();
            tasks.spawn(async move {
                let result = process_unit(unit).await;
                (unit_id, result)
            });
        }

        let Some(joined) = tasks.join_next().await else {
            break;
        };
        let (unit_id, result) =
            joined.map_err(|error| anyhow::anyhow!("Execution DAG task join error: {}", error))?;

        if let Ok(ref unit_result) = result {
            if classify_success(unit_result) && !stop_launching {
                for dependent_id in &dependents[unit_id] {
                    if remaining_deps[*dependent_id] == 0 {
                        continue;
                    }
                    remaining_deps[*dependent_id] -= 1;
                    if remaining_deps[*dependent_id] == 0 {
                        ready.insert((sort_keys[*dependent_id].clone(), *dependent_id));
                    }
                }
            } else {
                stop_launching = true;
            }
        } else {
            stop_launching = true;
        }

        on_complete(unit_id, &result);
        results.push((unit_id, result));

        if tasks.is_empty() && (ready.is_empty() || stop_launching) {
            break;
        }
    }

    Ok(results)
}

#[derive(Clone)]
enum PreparedSpecAction {
    PrepFailure {
        error: String,
    },
    UpToDate,
    Run {
        agent_name: String,
        executor: Arc<AgentExecutor>,
        draft_file: PathBuf,
        output_path: PathBuf,
        dependency_fingerprint: String,
        draft_content: String,
        dependency_context: HashMap<String, serde_json::Value>,
        estimated: usize,
        cache_hit: bool,
    },
}

#[derive(Clone)]
struct PreparedSpecItem {
    name: String,
    action: PreparedSpecAction,
}

enum SpecNodeResult {
    UpToDate {
        draft_name: String,
    },
    Success {
        draft_name: String,
        draft_file: PathBuf,
        output_path: PathBuf,
        dependency_fingerprint: String,
    },
    BlockingAmbiguities {
        draft_name: String,
        draft_file: PathBuf,
        draft_content: String,
        spec_content: String,
        actionable: Vec<String>,
        additional_context: HashMap<String, serde_json::Value>,
    },
    Failure {
        draft_name: String,
        error: anyhow::Error,
    },
}

impl SpecNodeResult {
    fn draft_name(&self) -> &str {
        match self {
            Self::UpToDate { draft_name, .. }
            | Self::Success { draft_name, .. }
            | Self::BlockingAmbiguities { draft_name, .. }
            | Self::Failure { draft_name, .. } => draft_name,
        }
    }

    fn succeeded(&self) -> bool {
        matches!(self, Self::UpToDate { .. } | Self::Success { .. })
    }
}

#[derive(Clone, Debug)]
struct BlockingAmbiguitySummary {
    draft_name: String,
    draft_file: PathBuf,
    draft_content: String,
    spec_content: String,
    actionable: Vec<String>,
    additional_context: HashMap<String, serde_json::Value>,
}

enum ImplNodeResult {
    UpToDate,
    Success {
        context_name: String,
        context_file: PathBuf,
        output_path: PathBuf,
        dependency_fingerprint: String,
    },
    Failure {
        context_name: String,
        error: anyhow::Error,
        unfinished_specification: bool,
    },
}

impl ImplNodeResult {
    fn succeeded(&self) -> bool {
        matches!(self, Self::UpToDate { .. } | Self::Success { .. })
    }
}

pub async fn create_specification(
    names: Vec<String>,
    clear_cache: bool,
    filter: &CategoryFilter,
    rate_limit: Option<f64>,
    token_limit: Option<f64>,
    parallel_limit: usize,
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
        parallel_limit,
        fix,
        max_fix_attempts,
        0,
        config,
    )
    .await
}

pub async fn build(
    names: Vec<String>,
    clear_cache: bool,
    filter: &CategoryFilter,
    rate_limit: Option<f64>,
    token_limit: Option<f64>,
    parallel_limit: usize,
    _max_fix_attempts: usize,
    max_compile_fix_attempts: usize,
    config: &Config,
) -> Result<()> {
    create_specification(
        names.clone(),
        clear_cache,
        filter,
        rate_limit,
        token_limit,
        parallel_limit,
        false,
        0,
        config,
    )
    .await?;
    create_implementation(
        names,
        true,
        max_compile_fix_attempts,
        clear_cache,
        filter,
        rate_limit,
        token_limit,
        parallel_limit,
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
    parallel_limit: usize,
    fix: bool,
    max_fix_attempts: usize,
    fix_attempt: usize,
    config: &Config,
) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
    let filter = *filter;
    let config = config.clone();
    Box::pin(async move {
        let workspace = WorkspaceContext::resolve(&config)?;
        types_manifest::ensure_types_manifest_current(
            &workspace.drafts_root,
            &workspace.artifact_workspace_root(),
            config.dry_run,
            config.verbose,
        )?;
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
        let execution_dag = build_execution_dag(filtered_draft_files, &workspace.drafts_dir, None)?;

        // Load build tracker
        let mut tracker = BuildTracker::load()?;

        let total_count: usize = execution_dag
            .units()
            .iter()
            .map(|unit| unit.nodes.len())
            .sum();
        println!(
            "{}",
            header_text(format!(
                "Synthesizing contracts for {} draft(s)",
                total_count
            ))
        );

        let resources = ExecutionResources::new(
            "contract_synthesis",
            workspace.artifact_workspace_root(),
            rate_limit,
            token_limit,
            config.verbose,
        );

        let progress = Arc::new(StdMutex::new(ProgressIndicator::new(total_count)));
        let mut updated_count = 0;
        let mut executors: HashMap<String, Arc<AgentExecutor>> = HashMap::new();
        let mut serial_agents = HashSet::new();
        let mut prepared = HashMap::new();
        for unit in execution_dag.units() {
            for node in &unit.nodes {
                let agent_name =
                    determine_specification_agent(&node.input_path, &workspace.drafts_dir)
                        .to_string();
                if !executors.contains_key(&agent_name) {
                    let executor = Arc::new(AgentExecutor::new(&agent_name, &config)?);
                    if !executor.can_run_parallel().unwrap_or(false) {
                        serial_agents.insert(agent_name.clone());
                    }
                    executors.insert(agent_name.clone(), executor);
                }
                let executor = executors
                    .get(&agent_name)
                    .cloned()
                    .context("missing specification executor")?;
                let draft_file = node.input_path.clone();
                let draft_name = node.name.clone();
                let dependency_fingerprint = stage_agent_dependency_fingerprint(
                    &dependency_fingerprint_for_node(
                        node,
                        &workspace.drafts_dir,
                        None,
                        &resources.run_context_cache,
                    )?,
                    &agent_name,
                )?;
                let output_path = determine_specification_output_path(
                    &draft_file,
                    &workspace.drafts_dir,
                    &workspace.specifications_dir,
                )?;

                let needs_update = if clear_cache {
                    true
                } else {
                    let update_reason = tracker.update_reason(
                        Stage::Contract,
                        &draft_name,
                        &draft_file,
                        &output_path,
                        &dependency_fingerprint,
                    )?;
                    log_build_tracker_update_reason(
                        config.verbose,
                        "contract",
                        &draft_name,
                        &update_reason,
                    );
                    !matches!(update_reason, UpdateReason::UpToDate)
                };

                let action = if !needs_update {
                    PreparedSpecAction::UpToDate
                } else {
                    let dependency_context = match build_dependency_context(
                        node,
                        &workspace.drafts_dir,
                        None,
                        &resources.run_context_cache,
                    ) {
                        Ok(context) => context,
                        Err(e) => {
                            prepared.insert(
                                node.input_path.clone(),
                                PreparedSpecItem {
                                    name: draft_name,
                                    action: PreparedSpecAction::PrepFailure {
                                        error: e.to_string(),
                                    },
                                },
                            );
                            continue;
                        }
                    };

                    let draft_content = fs::read_to_string(&draft_file).unwrap_or_default();
                    let parsed_draft = match parse_repo_draft(
                        &draft_file,
                        &workspace.drafts_dir,
                        &draft_content,
                    ) {
                        Ok(parsed) => parsed,
                        Err(e) => {
                            prepared.insert(
                                node.input_path.clone(),
                                PreparedSpecItem {
                                    name: draft_name,
                                    action: PreparedSpecAction::PrepFailure {
                                        error: e.to_string(),
                                    },
                                },
                            );
                            continue;
                        }
                    };
                    let dependency_context = match build_specification_context(
                        &draft_file,
                        &draft_content,
                        dependency_context,
                        &workspace.drafts_dir,
                        parsed_draft.as_ref(),
                    ) {
                        Ok(context) => context,
                        Err(e) => {
                            prepared.insert(
                                node.input_path.clone(),
                                PreparedSpecItem {
                                    name: draft_name,
                                    action: PreparedSpecAction::PrepFailure {
                                        error: e.to_string(),
                                    },
                                },
                            );
                            continue;
                        }
                    };
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
                    PreparedSpecAction::Run {
                        agent_name,
                        executor,
                        draft_file,
                        output_path,
                        dependency_fingerprint,
                        draft_content,
                        dependency_context,
                        estimated,
                        cache_hit,
                    }
                };

                prepared.insert(
                    node.input_path.clone(),
                    PreparedSpecItem {
                        name: draft_name,
                        action,
                    },
                );
            }
        }

        let serial_gates = SerialAgentGates::new(serial_agents);
        let workspace_ctx = workspace.clone();
        let cfg = config.clone();
        let usage_reporter = resources.usage_reporter.clone();
        let execution_control = resources.execution_control.clone();
        let prepared_for_launch = prepared.clone();
        let prepared_for_run = prepared.clone();
        let launch_progress = progress.clone();
        let completion_progress = progress.clone();
        let results = run_execution_dag_units(
            &execution_dag,
            parallel_limit,
            |unit| {
                for node in &unit.nodes {
                    let Some(item) = prepared_for_launch.get(&node.input_path) else {
                        continue;
                    };
                    match &item.action {
                        PreparedSpecAction::PrepFailure { .. } => {
                            launch_progress
                                .lock()
                                .expect("progress mutex should not be poisoned")
                                .start_item(&item.name, None);
                        }
                        PreparedSpecAction::UpToDate { .. } => {
                            launch_progress
                                .lock()
                                .expect("progress mutex should not be poisoned")
                                .start_item_up_to_date(&item.name);
                            log_build_tracker_skip(config.verbose, "specification", &item.name);
                            log_build_tracker_skip(config.verbose, "contract", &item.name);
                        }
                        PreparedSpecAction::Run {
                            agent_name,
                            cache_hit,
                            estimated,
                            ..
                        } => {
                            if *cache_hit {
                                launch_progress
                                    .lock()
                                    .expect("progress mutex should not be poisoned")
                                    .start_item_cached(&item.name);
                                log_agent_response_cache_hit(
                                    config.verbose,
                                    "contract",
                                    &item.name,
                                    agent_name,
                                );
                            } else {
                                launch_progress
                                    .lock()
                                    .expect("progress mutex should not be poisoned")
                                    .start_item(&item.name, Some(*estimated));
                            }
                        }
                    }
                }
            },
            |_unit_id, result: &Result<Vec<SpecNodeResult>>| match result {
                Ok(entries) => {
                    for entry in entries {
                        completion_progress
                            .lock()
                            .expect("progress mutex should not be poisoned")
                            .complete_item(entry.draft_name(), entry.succeeded());
                    }
                }
                Err(_) => {}
            },
            |entries: &Vec<SpecNodeResult>| entries.iter().all(SpecNodeResult::succeeded),
            move |unit| {
                let workspace_ctx = workspace_ctx.clone();
                let cfg = cfg.clone();
                let usage_reporter = usage_reporter.clone();
                let execution_control = execution_control.clone();
                let serial_gates = serial_gates.clone();
                let prepared = prepared_for_run.clone();
                async move {
                    let fallback_level_policy = default_contract_level_policy(&unit);
                    let mut level_policy = fallback_level_policy.clone();
                    let mut coordination_context = HashMap::new();
                    coordination_context.insert(
                        "level_hash".to_string(),
                        json!(fallback_level_policy.level_hash.clone()),
                    );
                    coordination_context.insert(
                        "artifact_paths".to_string(),
                        json!(fallback_level_policy.artifact_paths.clone()),
                    );
                    let draft_summaries = unit
                        .nodes
                        .iter()
                        .filter_map(|node| prepared.get(&node.input_path))
                        .filter_map(|item| match &item.action {
                            PreparedSpecAction::Run {
                                dependency_context, ..
                            } => dependency_context.get("draft_summary").cloned(),
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    if !draft_summaries.is_empty() {
                        coordination_context
                            .insert("draft_summaries".to_string(), json!(draft_summaries));
                    }
                    if let Ok(executor) = AgentExecutor::new("coordinate_contract_level", &cfg) {
                        let coordination_scope =
                            UsageScope::new(
                                "contract_coordination",
                                &fallback_level_policy.level_hash,
                            )
                            .with_estimated_input_tokens(
                                estimate_agent_request_tokens(&executor, "", &coordination_context),
                            );
                        match execute_tracked_agent(
                            &executor,
                            "",
                            coordination_context,
                            execution_control.clone(),
                            clear_cache,
                            &usage_reporter,
                            coordination_scope,
                        )
                        .await
                        {
                            Ok(AgentResponse::Final(output)) => {
                                if let Ok(parsed) =
                                    parse_level_policy_output(&output, &fallback_level_policy)
                                {
                                    level_policy = parsed;
                                }
                            }
                            Ok(AgentResponse::Questions(_)) | Err(_) => {}
                        }
                    }
                    if !cfg.dry_run {
                        let _ = ContractStore::new(".reen").write_level_policy(&level_policy);
                    }
                    let mut unit_results = Vec::new();
                    for node in unit.nodes {
                        let item = prepared
                            .get(&node.input_path)
                            .cloned()
                            .context("missing prepared specification item")?;
                        match item.action {
                            PreparedSpecAction::PrepFailure { error } => {
                                unit_results.push(SpecNodeResult::Failure {
                                    draft_name: item.name,
                                    error: anyhow::anyhow!(error),
                                });
                            }
                            PreparedSpecAction::UpToDate => {
                                unit_results.push(SpecNodeResult::UpToDate {
                                    draft_name: item.name,
                                });
                            }
                            PreparedSpecAction::Run {
                                agent_name,
                                executor,
                                draft_file,
                                output_path,
                                dependency_fingerprint,
                                draft_content,
                                mut dependency_context,
                                estimated,
                                ..
                            } => {
                                dependency_context.insert(
                                    "level_policy".to_string(),
                                    serde_json::to_value(&level_policy)
                                        .context("serialize level policy")?,
                                );
                                let _serial_guard = serial_gates.acquire(&agent_name).await;
                                match process_specification(
                                    &executor,
                                    &draft_content,
                                    &draft_file,
                                    &item.name,
                                    &workspace_ctx,
                                    &cfg,
                                    clear_cache,
                                    dependency_context,
                                    execution_control.clone(),
                                    &usage_reporter,
                                    estimated,
                                )
                                .await
                                {
                                    Ok(ProcessSpecOutcome::Success) => {
                                        unit_results.push(SpecNodeResult::Success {
                                            draft_name: item.name,
                                            draft_file,
                                            output_path,
                                            dependency_fingerprint,
                                        });
                                    }
                                    Ok(ProcessSpecOutcome::BlockingAmbiguities {
                                        draft_file,
                                        draft_name,
                                        draft_content,
                                        spec_content,
                                        actionable,
                                        additional_context,
                                    }) => {
                                        unit_results.push(SpecNodeResult::BlockingAmbiguities {
                                            draft_name,
                                            draft_file,
                                            draft_content,
                                            spec_content,
                                            actionable,
                                            additional_context,
                                        });
                                    }
                                    Err(error) => {
                                        unit_results.push(SpecNodeResult::Failure {
                                            draft_name: item.name,
                                            error,
                                        });
                                    }
                                }
                            }
                        }
                    }
                    Ok(unit_results)
                }
            },
        )
        .await?;

        let mut blocking_entries = Vec::new();
        let mut first_error = None;
        for (_unit_id, result) in results {
            match result {
                Ok(entries) => {
                    for entry in entries {
                        match entry {
                            SpecNodeResult::UpToDate { .. } => {}
                            SpecNodeResult::Success {
                                draft_name,
                                draft_file,
                                output_path,
                                dependency_fingerprint,
                            } => {
                                if !config.dry_run {
                                    tracker.record(
                                        Stage::Contract,
                                        &draft_name,
                                        &draft_file,
                                        &output_path,
                                        &dependency_fingerprint,
                                    )?;
                                    tracker.save()?;
                                }
                                updated_count += 1;
                                if config.verbose {
                                    println!(
                                        "{}",
                                        success_text(format!(
                                            "✓ Successfully synthesized contract for {}",
                                            draft_name
                                        ))
                                    );
                                }
                            }
                            SpecNodeResult::BlockingAmbiguities {
                                draft_name: ba_draft_name,
                                draft_file: ba_draft_file,
                                draft_content: ba_draft_content,
                                spec_content: ba_spec_content,
                                actionable: ba_actionable,
                                additional_context: ba_context,
                            } => {
                                blocking_entries.push(BlockingAmbiguitySummary {
                                    draft_name: ba_draft_name,
                                    draft_file: ba_draft_file,
                                    draft_content: ba_draft_content,
                                    spec_content: ba_spec_content,
                                    actionable: ba_actionable,
                                    additional_context: ba_context,
                                });
                            }
                            SpecNodeResult::Failure { draft_name, error } => {
                                eprintln!(
                                    "{}",
                                    error_text(format!(
                                        "✗ Failed to create specification for {}: {}",
                                        draft_name, error
                                    ))
                                );
                                if first_error.is_none() {
                                    first_error = Some(error);
                                }
                            }
                        }
                    }
                }
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }

        if let Some(first_blocking) = blocking_entries.first().cloned() {
            progress
                .lock()
                .expect("progress mutex should not be poisoned")
                .finish();
            print_blocking_ambiguity_summary(&blocking_entries);

            if fix && fix_attempt < max_fix_attempts {
                return try_fix_and_retry(
                    &first_blocking.draft_file,
                    &first_blocking.draft_name,
                    &first_blocking.draft_content,
                    &first_blocking.spec_content,
                    &first_blocking.actionable,
                    first_blocking.additional_context,
                    fix_attempt,
                    max_fix_attempts,
                    &filter,
                    rate_limit,
                    token_limit,
                    parallel_limit,
                    &config,
                    resources.execution_control.clone(),
                    &resources.usage_reporter,
                )
                .await;
            }
            anyhow::bail!("generated contract contains blocking ambiguities");
        }

        if let Some(error) = first_error {
            anyhow::bail!("{}", error);
        }

        if !config.dry_run {
            tracker.save()?;
        }

        progress
            .lock()
            .expect("progress mutex should not be poisoned")
            .finish();

        if updated_count == 0 && config.verbose {
            println!("{}", standard_text("All contracts are up to date"));
        }

        Ok(())
    })
}

pub async fn check_drafts(names: Vec<String>, config: &Config) -> Result<()> {
    create_specification(
        names,
        false,
        &CategoryFilter::all(),
        None,
        None,
        DEFAULT_PARALLEL_LIMIT,
        false,
        0,
        config,
    )
    .await
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
    reporter: &UsageReporter,
    estimated_tokens: usize,
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
            reporter,
        )
        .await;
    }

    // Use conversational execution to handle questions
    let spec_content = execute_tracked_agent_conversation(
        executor,
        &draft_content,
        draft_name,
        additional_context.clone(),
        execution_control.clone(),
        ignore_cache_reads,
        reporter,
        UsageScope::new("specification", draft_name)
            .with_path(draft_file.display().to_string())
            .with_estimated_input_tokens(estimated_tokens),
    )
    .await?;

    finalize_specification_output(
        draft_content,
        draft_file,
        draft_name,
        workspace,
        config,
        spec_content,
        additional_context,
        ignore_cache_reads,
        execution_control,
        reporter,
    )
    .await
}

async fn synthesize_specification_preview(
    draft_file: &Path,
    draft_name: &str,
    workspace: &WorkspaceContext,
    base_context: HashMap<String, serde_json::Value>,
    config: &Config,
    ignore_cache_reads: bool,
    token_limit: Option<f64>,
    execution_control: Option<CliExecutionControl>,
    reporter: &UsageReporter,
    estimated_tokens: Option<usize>,
) -> Result<(String, Option<DraftDocument>)> {
    let draft_content = fs::read_to_string(draft_file)
        .with_context(|| format!("Failed to read draft file: {}", draft_file.display()))?;
    let parsed_draft = parse_repo_draft(draft_file, &workspace.drafts_dir, &draft_content)?;
    let context = build_specification_context(
        draft_file,
        &draft_content,
        base_context,
        &workspace.drafts_dir,
        parsed_draft.as_ref(),
    )?;
    let agent_name = determine_specification_agent(draft_file, &workspace.drafts_dir);
    let executor = AgentExecutor::new(agent_name, config)?;
    let (context, estimated) =
        fit_context_to_token_limit(&executor, &draft_content, context, token_limit)?;
    let scope = UsageScope::new("specification-preview", draft_name)
        .with_path(draft_file.display().to_string())
        .with_estimated_input_tokens(estimated_tokens.unwrap_or(estimated));
    let spec_content = execute_tracked_agent_conversation(
        &executor,
        &draft_content,
        draft_name,
        context,
        execution_control,
        ignore_cache_reads,
        reporter,
        scope,
    )
    .await?;
    Ok((spec_content, parsed_draft))
}

fn collect_blocking_ambiguities_for_path(content: &str, path: Option<&Path>) -> Vec<String> {
    extract_blocking_ambiguities_section(content)
        .map(|blocking| extract_actionable_blocking_bullets_for_path(&blocking, path))
        .unwrap_or_default()
}

async fn execute_tracked_agent(
    executor: &AgentExecutor,
    input: &str,
    additional_context: HashMap<String, serde_json::Value>,
    execution_control: Option<CliExecutionControl>,
    ignore_cache_reads: bool,
    reporter: &UsageReporter,
    scope: UsageScope,
) -> Result<AgentResponse> {
    executor
        .execute_with_context_options_tracked(
            input,
            additional_context,
            execution_control
                .as_ref()
                .map(|control| control as &dyn NativeExecutionControl),
            ignore_cache_reads,
            Some((reporter, &scope)),
        )
        .await
}

async fn execute_tracked_agent_conversation(
    executor: &AgentExecutor,
    input: &str,
    context_name: &str,
    additional_context: HashMap<String, serde_json::Value>,
    execution_control: Option<CliExecutionControl>,
    ignore_cache_reads: bool,
    reporter: &UsageReporter,
    scope: UsageScope,
) -> Result<String> {
    executor
        .execute_with_conversation_with_seed_options_tracked(
            input,
            context_name,
            additional_context,
            execution_control
                .as_ref()
                .map(|control| control as &dyn NativeExecutionControl),
            ignore_cache_reads,
            Some((reporter, &scope)),
        )
        .await
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
    reporter: &UsageReporter,
) -> Result<ProcessSpecOutcome> {
    let expansion = execute_external_api_expansion_with_cache_recovery(
        executor,
        draft_content,
        draft_name,
        config,
        ignore_cache_reads,
        additional_context.clone(),
        execution_control.clone(),
        reporter,
        draft_file,
    )
    .await?;

    let data_executor = AgentExecutor::new("synthesize_contract_data", config)?;
    let context_executor = AgentExecutor::new("synthesize_contract_context", config)?;
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
            config,
            generated_context.clone(),
            ignore_cache_reads,
            execution_control.clone(),
            reporter,
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
            config,
            generated_context.clone(),
            ignore_cache_reads,
            execution_control.clone(),
            reporter,
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
            config,
            generated_context.clone(),
            ignore_cache_reads,
            execution_control.clone(),
            reporter,
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
    reporter: &UsageReporter,
    draft_file: &Path,
) -> Result<external_api_expansion::ExternalApiExpansion> {
    let execute_once = |context: HashMap<String, serde_json::Value>| async {
        execute_tracked_agent_conversation(
            executor,
            draft_content,
            draft_name,
            context,
            execution_control.clone(),
            ignore_cache_reads,
            reporter,
            UsageScope::new("specification_external_api_expansion", draft_name)
                .with_path(draft_file.display().to_string()),
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
                "synthesize_contract_external_api",
                CacheAgentInput {
                    draft_content: Some(draft_content.to_string()),
                    context_content: None,
                    additional: additional_context.clone(),
                },
                config,
            )?;

            eprintln!(
                "{} cleared stale cached external API expansion for '{}'; retrying with a fresh model response",
                warning_tag("spec:cache"),
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
    config: &Config,
    additional_context: HashMap<String, serde_json::Value>,
    ignore_cache_reads: bool,
    execution_control: Option<CliExecutionControl>,
    reporter: &UsageReporter,
) -> Result<(String, WrittenSpecification)> {
    let lint_context = additional_context.clone();
    let spec_content = execute_tracked_agent_conversation(
        executor,
        &artifact.draft_markdown,
        &artifact.name,
        additional_context,
        execution_control.clone(),
        ignore_cache_reads,
        reporter,
        UsageScope::new("specification_external_generated", display_name)
            .with_path(output_path.display().to_string()),
    )
    .await?;
    let written = write_specification_output(
        workspace,
        config,
        source_draft_file,
        display_name,
        display_name,
        spec_category,
        &output_path,
        spec_content,
        Some(&lint_context),
        ignore_cache_reads,
        execution_control,
        reporter,
    )
    .await?;
    Ok((display_name.to_string(), written))
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

fn determine_interface_resolver_agent(specification_kind: &str) -> &'static str {
    match specification_kind.trim().to_ascii_lowercase().as_str() {
        "data" => "resolve_interface_contract_data",
        "projection" => "resolve_interface_contract_projection",
        "context" | "app" | "root" => "resolve_interface_contract_context",
        _ => "resolve_interface_contract_context",
    }
}

fn build_interface_resolution_context(
    workspace: &WorkspaceContext,
    dependency_context: &HashMap<String, serde_json::Value>,
    draft_relative_path: &str,
    contract_artifact: &contracts::ContractArtifact,
    contract_validation: &contracts::ContractValidationReport,
    behavior_contract: &pipeline_quality::BehaviorContract,
) -> Result<(
    HashMap<String, serde_json::Value>,
    types_manifest::TypesManifestScope,
)> {
    let Some(types_manifest_scope) = types_manifest::load_types_manifest_scope(
        &workspace.drafts_root,
        &workspace.artifact_workspace_root(),
        &contract_artifact.specification_kind,
    )?
    else {
        anyhow::bail!(
            "No supported types manifest scope for specification kind '{}'",
            contract_artifact.specification_kind
        );
    };

    let mut resolver_context = dependency_context.clone();
    resolver_context.insert(
        "contract_artifact".to_string(),
        contract_artifact_to_context_value(contract_artifact),
    );
    resolver_context.insert(
        "contract_validation".to_string(),
        contract_validation_to_context_value(contract_validation),
    );
    resolver_context.insert(
        "behavior_contract".to_string(),
        contract_to_context_value(behavior_contract),
    );
    resolver_context.insert(
        "draft_relative_path".to_string(),
        json!(draft_relative_path),
    );
    resolver_context.insert(
        "types_manifest_scope".to_string(),
        serde_json::to_value(&types_manifest_scope)
            .context("Failed to serialize types manifest scope")?,
    );

    Ok((resolver_context, types_manifest_scope))
}

async fn execute_interface_resolution_with_cache_recovery(
    agent_name: &str,
    executor: &AgentExecutor,
    spec_content: &str,
    draft_name: &str,
    draft_file: &Path,
    config: &Config,
    ignore_cache_reads: bool,
    additional_context: HashMap<String, serde_json::Value>,
    execution_control: Option<CliExecutionControl>,
    reporter: &UsageReporter,
) -> Result<InterfaceResolutionOutput> {
    let estimated_tokens =
        estimate_agent_request_tokens(executor, spec_content, &additional_context);
    let execute_once = |context: HashMap<String, serde_json::Value>| async {
        match execute_tracked_agent(
            executor,
            spec_content,
            context,
            execution_control.clone(),
            ignore_cache_reads,
            reporter,
            UsageScope::new("interface-resolution", draft_name)
                .with_path(draft_file.display().to_string())
                .with_estimated_input_tokens(estimated_tokens),
        )
        .await?
        {
            AgentResponse::Final(output) => Ok(output),
            AgentResponse::Questions(questions) => anyhow::bail!(
                "Interface resolver requested clarification for '{}': {}",
                draft_name,
                questions.trim()
            ),
        }
    };

    let resolution_output = execute_once(additional_context.clone()).await?;
    match parse_interface_resolution_output(&resolution_output) {
        Ok(parsed) => Ok(parsed),
        Err(parse_error) => {
            if ignore_cache_reads {
                return Err(parse_error);
            }
            let cache_hit = executor
                .is_cache_hit(spec_content, additional_context.clone())
                .unwrap_or(false);
            if !cache_hit {
                return Err(parse_error);
            }

            clear_specific_agent_response_cache_entry(
                agent_name,
                CacheAgentInput {
                    draft_content: None,
                    context_content: Some(spec_content.to_string()),
                    additional: additional_context.clone(),
                },
                config,
            )?;

            eprintln!(
                "{} cleared stale cached interface resolution for '{}'; retrying with a fresh model response",
                warning_tag("contract:cache"),
                draft_name
            );

            let retry_output = execute_once(additional_context).await?;
            parse_interface_resolution_output(&retry_output).with_context(|| {
                format!(
                    "interface resolution output was not valid JSON after clearing the cached response for '{}'",
                    draft_name
                )
            })
        }
    }
}

async fn finalize_specification_output(
    draft_content: &str,
    draft_file: &Path,
    draft_name: &str,
    workspace: &WorkspaceContext,
    config: &Config,
    spec_content: String,
    additional_context: HashMap<String, serde_json::Value>,
    ignore_cache_reads: bool,
    execution_control: Option<CliExecutionControl>,
    reporter: &UsageReporter,
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
        config,
        draft_file,
        draft_name,
        draft_name,
        spec_category,
        &output_path,
        spec_content,
        Some(&additional_context),
        ignore_cache_reads,
        execution_control,
        reporter,
    )
    .await?;

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

async fn write_specification_output(
    workspace: &WorkspaceContext,
    config: &Config,
    draft_file: &Path,
    draft_name: &str,
    display_name: &str,
    spec_category: ArtifactCategory,
    output_path: &Path,
    spec_content: String,
    dependency_context: Option<&HashMap<String, serde_json::Value>>,
    ignore_cache_reads: bool,
    execution_control: Option<CliExecutionControl>,
    reporter: &UsageReporter,
) -> Result<WrittenSpecification> {
    let mut has_blocking_ambiguities = false;
    let mut actionable = Vec::new();
    let empty_context = HashMap::new();
    let dependency_context = dependency_context.unwrap_or(&empty_context);

    // Report Blocking Ambiguities immediately if present in generated spec
    if let Some(blocking) = extract_blocking_ambiguities_section(&spec_content) {
        actionable = extract_actionable_blocking_bullets_for_path(&blocking, Some(output_path));
        if !actionable.is_empty() {
            has_blocking_ambiguities = true;
            eprintln!("{}", error_tag("spec:blocking"));
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
    let lint_report = analyze_specification(output_path, &spec_content, Some(dependency_context));
    let contract_output_path =
        determine_implementation_output_path(output_path, SPECIFICATIONS_DIR).ok();
    let contract_artifact = build_contract_artifact(
        output_path,
        &spec_content,
        contract_output_path.as_deref(),
        Some(dependency_context),
    );
    let contract_validation = validate_contract_artifact(
        &contract_artifact,
        output_path,
        &spec_content,
        Some(dependency_context),
    );
    let output_paths = contract_output_path.into_iter().collect::<Vec<_>>();
    let implementation_plan = build_default_plan(
        PlanKind::Implementation,
        output_path,
        &spec_content,
        &output_paths,
        dependency_context,
        None,
    );
    let plan_validation = validate_plan(&implementation_plan, &lint_report.contract, &output_paths);
    if config.debug {
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
        let _ = write_json_report(
            &artifact_root,
            "planning",
            output_path,
            "implementation_plan.json",
            &implementation_plan,
        );
        let _ = write_json_report(
            &artifact_root,
            "planning",
            output_path,
            "plan_validation_report.json",
            &plan_validation,
        );
    }
    if !lint_report.errors.is_empty() {
        has_blocking_ambiguities = true;
        for issue in &lint_report.errors {
            actionable.push(format!("- {}", issue));
        }
        eprintln!("{}", error_tag("spec:lint"));
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

    if config.debug {
        match workspace.store.backend() {
            BackendSelection::File => {
                if let Some(parent) = output_path.parent() {
                    fs::create_dir_all(parent)
                        .context("Failed to create specification output directory")?;
                }
                fs::write(output_path, &spec_content)
                    .context("Failed to write specification file")?;
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
    }

    let contract_store = ContractStore::new(".reen");
    let draft_rel = draft_relative_path(draft_file, &workspace.drafts_root)?;
    let draft_content = fs::read_to_string(draft_file)
        .with_context(|| format!("Failed to read draft file: {}", draft_file.display()))?;
    let draft_summary = parse_repo_draft(draft_file, &workspace.drafts_dir, &draft_content)?
        .map(|parsed| serde_json::to_value(parsed.summary))
        .transpose()
        .context("Failed to serialize draft summary")?;
    let draft_fingerprint = {
        let mut hasher = Sha256::new();
        hasher.update(draft_content.as_bytes());
        hex::encode(hasher.finalize())
    };
    let required_upstream_interface_references = dependency_context
        .get("implemented_direct_role_capsules")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let path = item.get("spec_path").and_then(|value| value.as_str())?;
                    Some(UpstreamInterfaceRef {
                        path: path.to_string(),
                        source: "implemented_direct_role_capsule".to_string(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let (resolver_context, types_manifest_scope) = build_interface_resolution_context(
        workspace,
        dependency_context,
        &draft_rel.display().to_string(),
        &contract_artifact,
        &contract_validation,
        &lint_report.contract,
    )?;
    let resolver_agent = determine_interface_resolver_agent(&contract_artifact.specification_kind);
    let resolver_executor = AgentExecutor::new(resolver_agent, config)?;
    let resolution = execute_interface_resolution_with_cache_recovery(
        resolver_agent,
        &resolver_executor,
        &spec_content,
        draft_name,
        draft_file,
        config,
        ignore_cache_reads,
        resolver_context,
        execution_control,
        reporter,
    )
    .await?;
    let synthesis = synthesize_contract_resolution(
        draft_name,
        &draft_rel.display().to_string(),
        &lint_report.contract,
        &contract_artifact,
        &lint_report,
        &contract_validation,
        &plan_validation,
        dependency_context,
        draft_summary.clone(),
        &types_manifest_scope,
        resolution,
        &contract_store,
    );
    if !synthesis.ambiguity_report.is_empty() {
        has_blocking_ambiguities = true;
        actionable.extend(
            synthesis
                .ambiguity_report
                .iter()
                .map(|entry| format!("- [{}] {}", entry.subject, entry.detail)),
        );
    }
    contract_store.write_interface_ir(&draft_rel, &synthesis.interface_ir)?;
    let bundle = ContractBundle {
        draft_identity: draft_name.to_string(),
        draft_relative_path: draft_rel.display().to_string(),
        draft_fingerprint,
        draft_summary,
        behavior_contract: serde_json::to_value(&lint_report.contract)
            .context("Failed to serialize behavior contract")?,
        contract_artifact: serde_json::to_value(&contract_artifact)
            .context("Failed to serialize contract artifact")?,
        implementation_plan: serde_json::to_value(&implementation_plan)
            .context("Failed to serialize implementation plan")?,
        plan_validation: serde_json::to_value(&synthesis.plan_validation)
            .context("Failed to serialize plan validation")?,
        target_output_hints: output_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect(),
        semantic_contract: synthesis.semantic_contract,
        resolved_interface: synthesis.resolved_interface,
        type_decisions: synthesis.type_decisions,
        name_bindings: synthesis.name_bindings,
        dependency_bindings: synthesis.dependency_bindings,
        ambiguity_report: synthesis.ambiguity_report,
        decision_sources: synthesis.decision_sources,
        required_upstream_interface_references,
        blocking_diagnostics: actionable.clone(),
        unresolved_assumptions: Vec::new(),
        contract_markdown: spec_content.clone(),
    };
    if config.debug {
        contract_store.write_debug_bundle(&draft_rel, &bundle)?;
    }

    Ok(WrittenSpecification {
        spec_content,
        actionable,
        has_blocking_ambiguities,
    })
}

#[cfg(test)]
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
    _draft_content: &str,
    _spec_content: &str,
    _actionable: &[String],
    _additional_context: HashMap<String, serde_json::Value>,
    _fix_attempt: usize,
    _max_fix_attempts: usize,
    _filter: &CategoryFilter,
    _rate_limit: Option<f64>,
    _token_limit: Option<f64>,
    _parallel_limit: usize,
    _config: &Config,
    _execution_control: Option<CliExecutionControl>,
    _reporter: &UsageReporter,
) -> Result<()> {
    anyhow::bail!(
        "Automatic draft repair is disabled because drafts are read-only. Update '{}' manually and retry.",
        draft_file
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(draft_name)
    );
}

pub async fn create_implementation(
    names: Vec<String>,
    fix: bool,
    max_compile_fix_attempts: usize,
    clear_cache: bool,
    filter: &CategoryFilter,
    rate_limit: Option<f64>,
    token_limit: Option<f64>,
    parallel_limit: usize,
    config: &Config,
) -> Result<()> {
    create_specification(
        names.clone(),
        clear_cache,
        filter,
        rate_limit,
        token_limit,
        parallel_limit,
        false,
        0,
        config,
    )
    .await?;

    let workspace = WorkspaceContext::resolve(config)?;
    let _ =
        sync_dependency_manifest_from_capability_registry(&workspace.drafts_root, config.verbose)?;
    let names_provided = !names.is_empty();
    let context_files = resolve_input_files(&workspace.drafts_dir, names, "md", filter)?;

    if context_files.is_empty() {
        println!("{}", standard_text("No draft files found to process"));
        return Ok(());
    }

    if config.dry_run {
        println!(
            "{}",
            standard_text(format!(
                "[DRY RUN] Would create implementation for {} draft(s)",
                context_files.len()
            ))
        );
        return Ok(());
    }

    // Load build tracker
    let mut tracker = BuildTracker::load()?;

    // Check if any contracts need to be regenerated first
    if tracker.upstream_changed(Stage::Implementation, "")? {
        println!(
            "{}",
            warning_text("⚠ Upstream contracts have changed. Run 'reen check drafts' first.")
        );
    }

    let dependency_roots =
        select_dependency_roots(context_files, &workspace.drafts_dir, names_provided, filter)?;
    let execution_dag = build_execution_dag(dependency_roots, &workspace.drafts_dir, None)?;
    let total_count: usize = execution_dag
        .units()
        .iter()
        .map(|unit| unit.nodes.len())
        .sum();
    println!(
        "{}",
        header_text(format!(
            "Creating implementation for {} draft(s)",
            total_count
        ))
    );

    // Step 1: Generate project structure (Cargo.toml, lib.rs, mod.rs files)
    if config.verbose {
        println!("{}", standard_text("Generating project structure..."));
    }

    let drafts_dir = workspace.drafts_root.clone();
    let mut project_info = analyze_specifications(&drafts_dir, Some(&drafts_dir))
        .context("Failed to analyze drafts")?;
    if let Some(manifest) = load_dependency_manifest(&drafts_dir.join("dependencies.yml"))? {
        merge_manifest_dependencies(&mut project_info.dependencies, &manifest);
    }

    let output_dir = PathBuf::from(".");

    generate_cargo_toml(&project_info, &output_dir).context("Failed to generate Cargo.toml")?;

    generate_lib_rs(&project_info, &output_dir).context("Failed to generate lib.rs")?;

    generate_mod_files(&project_info, &output_dir).context("Failed to generate mod.rs files")?;

    if config.verbose {
        println!("{}", success_text("✓ Project structure generated"));
    }

    let mut recent_generated_files: Vec<PathBuf> = Vec::new();
    for p in generated_project_structure_paths(&project_info) {
        if p.exists() {
            recent_generated_files.push(p);
        }
    }

    // Step 2: Generate individual implementation files
    let data_executor = Arc::new(AgentExecutor::new("create_implementation_data", config)?);
    let projection_executor = Arc::new(AgentExecutor::new(
        "create_implementation_projection",
        config,
    )?);
    let context_executor = Arc::new(AgentExecutor::new("create_implementation_context", config)?);
    let data_can_parallel = data_executor.can_run_parallel().unwrap_or(false);
    let projection_can_parallel = projection_executor.can_run_parallel().unwrap_or(false);
    let context_can_parallel = context_executor.can_run_parallel().unwrap_or(false);

    if config.verbose {
        let path = context_executor.model_registry().registry_path();
        println!(
            "{}",
            standard_text(format!(
                "Agent model registry: {}, implementation parallel: data={}, projection={}, context={}",
                path.display(),
                data_can_parallel,
                projection_can_parallel,
                context_can_parallel
            ))
        );
    }

    let resources = ExecutionResources::new(
        "implementation",
        workspace.artifact_workspace_root(),
        rate_limit,
        token_limit,
        config.verbose,
    );

    let progress = Arc::new(StdMutex::new(ProgressIndicator::new(total_count)));
    let mut updated_count = 0;
    let mut had_unspecified = false;
    let mut had_failures = false;
    let mut serial_agents = HashSet::new();
    if !data_can_parallel {
        serial_agents.insert("create_implementation_data".to_string());
    }
    if !projection_can_parallel {
        serial_agents.insert("create_implementation_projection".to_string());
    }
    if !context_can_parallel {
        serial_agents.insert("create_implementation_context".to_string());
    }
    let serial_gates = SerialAgentGates::new(serial_agents);

    let library_crate_name = project_info.package_name.clone();
    let artifact_store = workspace.store.clone();
    let specifications_root = workspace.specifications_root.clone();
    let drafts_root = workspace.drafts_root.clone();
    let specifications_dir = workspace.specifications_dir.clone();
    let drafts_dir = workspace.drafts_dir.clone();
    let progress_for_tasks = progress.clone();
    let tracker_snapshot = tracker.clone();
    let resources_for_tasks = resources.clone();
    let data_executor_for_tasks = data_executor.clone();
    let projection_executor_for_tasks = projection_executor.clone();
    let context_executor_for_tasks = context_executor.clone();
    let config_for_tasks = config.clone();
    let results = run_execution_dag_units(
        &execution_dag,
        parallel_limit,
        |_unit| {},
        |_unit_id, _result: &Result<Vec<ImplNodeResult>>| {},
        |entries: &Vec<ImplNodeResult>| entries.iter().all(ImplNodeResult::succeeded),
        move |unit| {
            let progress = progress_for_tasks.clone();
            let tracker = tracker_snapshot.clone();
            let resources = resources_for_tasks.clone();
            let data_executor = data_executor_for_tasks.clone();
            let projection_executor = projection_executor_for_tasks.clone();
            let context_executor = context_executor_for_tasks.clone();
            let library_crate_name = library_crate_name.clone();
            let artifact_store = artifact_store.clone();
            let specifications_root = specifications_root.clone();
            let drafts_root = drafts_root.clone();
            let specifications_dir = specifications_dir.clone();
            let drafts_dir = drafts_dir.clone();
            let serial_gates = serial_gates.clone();
            let cfg = config_for_tasks.clone();
            async move {
                let mut unit_results = Vec::new();

                for node in unit.nodes {
                    let draft_input_path = node.input_path.clone();
                    let context_name = node.name.clone();
                    let spec_identity_path = determine_specification_output_path(
                        &draft_input_path,
                        &drafts_dir,
                        &specifications_dir,
                    )?;
                    let output_path =
                        determine_implementation_output_path(&draft_input_path, &drafts_dir)?;

                    let mut dependency_context = build_dependency_context(
                        &node,
                        &drafts_dir,
                        None,
                        &resources.run_context_cache,
                    )?;
                    let contract_store = ContractStore::new(".reen");
                    let draft_rel = draft_relative_path(&draft_input_path, &drafts_root)?;
                    let interface_ir = contract_store.read_interface_ir(&draft_rel).with_context(|| {
                        format!(
                            "Missing interface_ir for {}; run `reen check drafts` first.",
                            draft_input_path.display()
                        )
                    })?;
                    let level_hash_value = level_hash(std::slice::from_ref(&draft_input_path));
                    let policy = contract_store
                        .read_level_policy("contract", &level_hash_value)
                        .unwrap_or(LevelPolicy {
                            stage: "contract".to_string(),
                            level_hash: level_hash_value,
                            artifact_paths: vec![draft_input_path.display().to_string()],
                            canonical_names: vec![context_name.clone()],
                            import_roots: Vec::new(),
                            feature_names: Vec::new(),
                            shared_type_choices: Vec::new(),
                            collaborator_abstractions: Vec::new(),
                            conflict_resolutions: Vec::new(),
                            name_bindings: Vec::new(),
                            container_shapes: Vec::new(),
                        });
                    dependency_context.insert(
                        "interface_ir".to_string(),
                        serde_json::to_value(&interface_ir).context("serialize interface_ir")?,
                    );
                    dependency_context.insert(
                        "level_policy".to_string(),
                        serde_json::to_value(&policy).context("serialize level policy")?,
                    );
                    dependency_context.insert(
                        "library_crate_name".to_string(),
                        json!(library_crate_name.clone()),
                    );
                    dependency_context.insert(
                        "public_import_guidance".to_string(),
                        json!({
                            "library_crate_name": library_crate_name.clone(),
                            "library_import_roots": [
                                "<crate>::TypeName",
                                "<crate>::data::TypeName",
                                "<crate>::projections::TypeName",
                                "<crate>::contexts::TypeName",
                            ],
                            "main_import_examples": [
                                format!("use {}::TypeName;", library_crate_name),
                                format!("use {}::data::TypeName;", library_crate_name),
                                format!("use {}::projections::TypeName;", library_crate_name),
                                format!("use {}::contexts::TypeName;", library_crate_name),
                            ],
                            "forbidden_leaf_examples": [
                                format!("use {}::data::direction::Direction;", library_crate_name),
                                format!("use {}::projections::summary::Summary;", library_crate_name),
                                format!("use {}::contexts::command_input::CommandInputContext;", library_crate_name),
                                "use crate::data::direction::Direction;".to_string(),
                            ],
                            "note": "Generated mod.rs files re-export direct public types. Leaf module paths are private and must not be imported."
                        }),
                    );

                    let (context_content, _) = synthesize_specification_preview(
                        &draft_input_path,
                        &context_name,
                        &WorkspaceContext {
                            store: artifact_store.clone(),
                            drafts_root: drafts_root.clone(),
                            specifications_root: specifications_root.clone(),
                            drafts_dir: drafts_dir.clone(),
                            specifications_dir: specifications_dir.clone(),
                        },
                        dependency_context.clone(),
                        &cfg,
                        clear_cache,
                        token_limit,
                        resources.execution_control.clone(),
                        &resources.usage_reporter,
                        None,
                    )
                    .await?;
                    let blocking = collect_blocking_ambiguities_for_path(
                        &context_content,
                        Some(&draft_input_path),
                    );
                    if !blocking.is_empty() {
                        progress
                            .lock()
                            .expect("progress mutex should not be poisoned")
                            .start_item(&context_name, None);
                        progress
                            .lock()
                            .expect("progress mutex should not be poisoned")
                            .complete_item(&context_name, false);
                        unit_results.push(ImplNodeResult::Failure {
                            context_name,
                            error: anyhow::anyhow!(
                                "unfinished specification:\n{}",
                                blocking.join("\n")
                            ),
                            unfinished_specification: true,
                        });
                        break;
                    }
                    let target_contract = build_contract_artifact(
                        &spec_identity_path,
                        &context_content,
                        Some(&output_path),
                        Some(&dependency_context),
                    );
                    let _target_contract_validation = validate_contract_artifact(
                        &target_contract,
                        &spec_identity_path,
                        &context_content,
                        Some(&dependency_context),
                    );
                    if let Some(target_type_name) =
                        infer_target_type_name(&spec_identity_path, &specifications_root, &drafts_root)?
                    {
                        dependency_context
                            .insert("target_type_name".to_string(), json!(target_type_name));
                    }

                    let behavior_contract = analyze_specification(
                        &spec_identity_path,
                        &context_content,
                        Some(&dependency_context),
                    )
                    .contract;

                    dependency_context.insert(
                        "contract_artifact".to_string(),
                        contract_artifact_to_context_value(&target_contract),
                    );
                    dependency_context.insert(
                        "behavior_contract".to_string(),
                        contract_to_context_value(&behavior_contract),
                    );
                    let dependency_context =
                        prune_implementation_prompt_context(dependency_context);
                    let impl_agent_name =
                        determine_implementation_agent(&draft_input_path, &drafts_dir)
                            .to_string();
                    let dependency_fingerprint = stage_agent_dependency_fingerprint(
                        &implementation_dependency_fingerprint_from_context(&dependency_context)?,
                        &impl_agent_name,
                    )?;
                    let needs_update = if clear_cache {
                        true
                    } else {
                        let update_reason = tracker.update_reason(
                            Stage::Implementation,
                            &context_name,
                            &draft_input_path,
                            &output_path,
                            &dependency_fingerprint,
                        )?;
                        log_build_tracker_update_reason(
                            cfg.verbose,
                            "implementation",
                            &context_name,
                            &update_reason,
                        );
                        !matches!(update_reason, UpdateReason::UpToDate)
                    };

                    if !needs_update {
                        progress
                            .lock()
                            .expect("progress mutex should not be poisoned")
                            .start_item_up_to_date(&context_name);
                        log_build_tracker_skip(cfg.verbose, "implementation", &context_name);
                        progress
                            .lock()
                            .expect("progress mutex should not be poisoned")
                            .complete_item(&context_name, true);
                        unit_results.push(ImplNodeResult::UpToDate);
                        continue;
                    }

                    let impl_executor = select_implementation_executor(
                        &draft_input_path,
                        &drafts_dir,
                        &data_executor,
                        &projection_executor,
                        &context_executor,
                    );
                    let cached_execution_context = if clear_cache {
                        None
                    } else {
                        find_cached_context_variant(
                            impl_executor,
                            &context_content,
                            dependency_context.clone(),
                        )?
                    };

                    let implementation_plan = build_default_plan(
                        PlanKind::Implementation,
                        &spec_identity_path,
                        &context_content,
                        std::slice::from_ref(&output_path),
                        &dependency_context,
                        None,
                    );
                    let plan_validation = validate_plan(
                        &implementation_plan,
                        &behavior_contract,
                        std::slice::from_ref(&output_path),
                    );

                    let require_plan_validation =
                        matches!(behavior_contract.kind, SpecificationKind::Data)
                            || cached_execution_context.is_none();
                    if require_plan_validation && !plan_validation.ok {
                        progress
                            .lock()
                            .expect("progress mutex should not be poisoned")
                            .start_item(&context_name, None);
                        progress
                            .lock()
                            .expect("progress mutex should not be poisoned")
                            .complete_item(&context_name, false);
                        eprintln!("{}", error_tag("plan:validation"));
                        eprintln!("\u{001b}[31m{}\u{001b}[0m", draft_input_path.display());
                        eprintln!("  Planning validation failed for '{}'.", context_name);
                        eprintln!();
                        for issue in &plan_validation.errors {
                            eprintln!("  - {}", issue);
                        }
                        eprintln!();
                        unit_results.push(ImplNodeResult::Failure {
                            context_name,
                            error: anyhow::anyhow!("planning validation failed"),
                            unfinished_specification: false,
                        });
                        break;
                    }

                    let mut execution_context = if let Some((cached_context, _estimated)) =
                        cached_execution_context.clone()
                    {
                        cached_context
                    } else {
                        dependency_context
                    };
                    execution_context.insert(
                        "implementation_plan".to_string(),
                        plan_to_context_value(&implementation_plan),
                    );
                    let (execution_context, estimated, cache_hit) =
                        if let Some((cached_context, cached_estimated)) = cached_execution_context {
                            let mut cached_context = cached_context;
                            cached_context.insert(
                                "implementation_plan".to_string(),
                                plan_to_context_value(&implementation_plan),
                            );
                            (cached_context, cached_estimated, true)
                        } else {
                            let (dependency_context, estimated) = fit_context_to_token_limit(
                                impl_executor,
                                &context_content,
                                execution_context,
                                token_limit,
                            )?;
                            (dependency_context, estimated, false)
                        };

                    if cache_hit {
                        progress
                            .lock()
                            .expect("progress mutex should not be poisoned")
                            .start_item_cached(&context_name);
                        log_agent_response_cache_hit(
                            cfg.verbose,
                            "implementation",
                            &context_name,
                            &impl_agent_name,
                        );
                    } else {
                        progress
                            .lock()
                            .expect("progress mutex should not be poisoned")
                            .start_item(&context_name, Some(estimated));
                    }

                    let _impl_guard = serial_gates.acquire(&impl_agent_name).await;
                    match process_implementation(
                        impl_executor,
                        &context_content,
                        &draft_input_path,
                        &context_name,
                        &drafts_dir,
                        &cfg,
                        clear_cache,
                        &implementation_plan,
                        execution_context,
                        resources.execution_control.clone(),
                        &resources.usage_reporter,
                        estimated,
                    )
                    .await
                    {
                        Ok(_) => {
                            progress
                                .lock()
                                .expect("progress mutex should not be poisoned")
                                .complete_item(&context_name, true);
                            unit_results.push(ImplNodeResult::Success {
                                context_name,
                                context_file: draft_input_path,
                                output_path,
                                dependency_fingerprint,
                            });
                        }
                        Err(error) => {
                            let unfinished =
                                error.to_string().contains("unfinished specification");
                            progress
                                .lock()
                                .expect("progress mutex should not be poisoned")
                                .complete_item(&context_name, false);
                            unit_results.push(ImplNodeResult::Failure {
                                context_name,
                                error,
                                unfinished_specification: unfinished,
                            });
                            break;
                        }
                    }
                }

                Ok(unit_results)
            }
        },
    )
    .await?;

    for (_unit_id, result) in results {
        match result {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        ImplNodeResult::UpToDate => {}
                        ImplNodeResult::Success {
                            context_name,
                            context_file,
                            output_path,
                            dependency_fingerprint,
                        } => {
                            if !config.dry_run {
                                tracker.record(
                                    Stage::Implementation,
                                    &context_name,
                                    &context_file,
                                    &output_path,
                                    &dependency_fingerprint,
                                )?;
                                tracker.save()?;
                            }
                            updated_count += 1;
                            if !config.dry_run {
                                recent_generated_files.push(output_path.clone());
                            }
                            if config.verbose {
                                println!(
                                    "{}",
                                    success_text(format!(
                                        "✓ Successfully created implementation for {}",
                                        context_name
                                    ))
                                );
                            }
                        }
                        ImplNodeResult::Failure {
                            context_name,
                            error,
                            unfinished_specification,
                        } => {
                            had_failures = true;
                            if unfinished_specification {
                                had_unspecified = true;
                            }
                            eprintln!(
                                "{}",
                                error_text(format!(
                                    "✗ Failed to create implementation for {}: {}",
                                    context_name, error
                                ))
                            );
                        }
                    }
                }
            }
            Err(error) => anyhow::bail!("{}", error),
        }
    }

    if had_unspecified {
        progress
            .lock()
            .expect("progress mutex should not be poisoned")
            .finish();
        anyhow::bail!("Unfinished specifications were detected. Aborting.");
    }

    if had_failures {
        progress
            .lock()
            .expect("progress mutex should not be poisoned")
            .finish();
        anyhow::bail!("Implementation generation failed. Skipping compilation and compile-fix.");
    }

    // Compile only after successful generation. Auto-fix is opt-in via --fix.
    if fix {
        let artifact_root = workspace.artifact_workspace_root();
        compilation_fix::ensure_compiles_with_auto_fix(
            config,
            max_compile_fix_attempts,
            Path::new("."),
            artifact_root.as_path(),
            &project_info,
            &recent_generated_files,
            token_limit,
            clear_cache,
            Some(&resources.usage_reporter),
            resources
                .execution_control
                .as_ref()
                .map(|control| control as &dyn NativeExecutionControl),
        )
        .await?;
    } else {
        cargo_commands::compile(config).await?;
    }

    progress
        .lock()
        .expect("progress mutex should not be poisoned")
        .finish();

    if updated_count == 0 && config.verbose && !had_unspecified {
        println!("{}", standard_text("All implementations are up to date"));
    }

    Ok(())
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
    reporter: &UsageReporter,
    estimated_tokens: usize,
) -> Result<()> {
    if !collect_blocking_ambiguities_for_path(context_content, Some(context_file)).is_empty() {
        anyhow::bail!("unfinished specification");
    }

    if config.dry_run {
        println!(
            "[DRY RUN] Would create implementation for: {}",
            context_name
        );
        return Ok(());
    }

    let mut additional_context = additional_context;

    for verifier_retry in 0..=IMPLEMENTATION_VERIFIER_RETRY_LIMIT {
        let impl_result = execute_tracked_agent_conversation(
            executor,
            context_content,
            context_name,
            additional_context.clone(),
            execution_control.clone(),
            ignore_cache_reads || verifier_retry > 0,
            reporter,
            UsageScope::new("implementation", context_name)
                .with_path(context_file.display().to_string())
                .with_estimated_input_tokens(estimated_tokens),
        )
        .await?;

        let previous_output = extract_code_from_output(&impl_result, context_name);
        match finalize_implementation_output(
            context_file,
            context_name,
            specifications_dir,
            config,
            implementation_plan,
            context_content,
            impl_result,
        ) {
            Ok(()) => return Ok(()),
            Err(FinalizeImplementationError::Verification { verifier_report })
                if verifier_retry < IMPLEMENTATION_VERIFIER_RETRY_LIMIT =>
            {
                eprintln!(
                    "{}",
                    warning_text(format!(
                        "Retrying implementation for {} after verifier feedback ({}/{})",
                        context_name,
                        verifier_retry + 1,
                        IMPLEMENTATION_VERIFIER_RETRY_LIMIT
                    ))
                );
                additional_context.insert("previous_output".to_string(), json!(previous_output));
                additional_context.insert(
                    "verifier_feedback".to_string(),
                    build_implementation_verifier_feedback(
                        context_name,
                        verifier_retry + 1,
                        &verifier_report,
                    ),
                );
            }
            Err(error) => return Err(error.into_anyhow()),
        }
    }

    unreachable!("implementation verifier retry loop should return on success or terminal failure")
}

#[derive(Debug)]
enum FinalizeImplementationError {
    Verification {
        verifier_report: StaticBehaviorVerifierReport,
    },
    Failure(anyhow::Error),
}

impl FinalizeImplementationError {
    fn into_anyhow(self) -> anyhow::Error {
        match self {
            Self::Verification { verifier_report } => anyhow::anyhow!(
                "Generated implementation for '{}' failed behavioral verification",
                verifier_report.contract.title
            ),
            Self::Failure(error) => error,
        }
    }
}

fn build_implementation_verifier_feedback(
    context_name: &str,
    attempt: usize,
    verifier_report: &StaticBehaviorVerifierReport,
) -> serde_json::Value {
    json!({
        "artifact": context_name,
        "attempt": attempt,
        "output_path": verifier_report.output_path,
        "errors": verifier_report.errors,
        "warnings": verifier_report.warnings,
        "high_risk_findings": verifier_report.high_risk_findings,
        "evidence": verifier_report.evidence,
        "revision_instructions": [
            "Revise the previous output instead of starting over.",
            "Fix every verifier error and high-risk finding before returning the final Rust file.",
            "Do not add external crates unless they already appear in input.resolved_dependency_plan or input.scaffold_dependencies.",
            "If you need a custom error type and no crate is authorized, implement it with std traits instead of using thiserror.",
            "For contexts and projections, keep collaborator-specific behavior in private role methods or local helper functions; do not invent new collaborator methods that are not declared on the collaborator interface."
        ]
    })
}

fn finalize_implementation_output(
    context_file: &Path,
    context_name: &str,
    specifications_dir: &str,
    config: &Config,
    implementation_plan: &ExecutionPlan,
    specification_content: &str,
    impl_result: String,
) -> std::result::Result<(), FinalizeImplementationError> {
    // Extract code from the agent output and write to file
    // The agent output may contain markdown code blocks or raw code
    let code = extract_code_from_output(&impl_result, context_name);

    // Surface explicit implementation-failure diagnostics directly in CLI output.
    let implementation_failure = extract_implementation_failure_message(&code);
    if let Some(message) = implementation_failure.as_deref() {
        eprintln!("{}", error_tag("impl:compile_error"));
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
        return Err(FinalizeImplementationError::Failure(anyhow::anyhow!(
            "Generated implementation for '{}' contains explicit failure marker",
            context_name
        )));
    }

    // Determine output path preserving folder structure
    let output_path = determine_implementation_output_path(context_file, specifications_dir)
        .map_err(FinalizeImplementationError::Failure)?;

    // Ensure the output directory exists
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .context("Failed to create implementation output directory")
            .map_err(FinalizeImplementationError::Failure)?;
    }

    // Write the implementation file
    fs::write(&output_path, code)
        .context("Failed to write implementation file")
        .map_err(FinalizeImplementationError::Failure)?;

    if config.verbose {
        println!("✓ Written implementation to: {}", output_path.display());
    }

    if config.debug {
        let _ = write_json_report(
            Path::new("."),
            "implementation",
            &output_path,
            "implementation_plan.json",
            implementation_plan,
        );
    }
    let verifier_report = verify_generated_implementation(
        Path::new("."),
        context_file,
        specification_content,
        &output_path,
    )
    .map_err(FinalizeImplementationError::Failure)?;
    if config.debug {
        let _ = write_json_report(
            Path::new("."),
            "implementation",
            &output_path,
            "static_verifier_report.json",
            &verifier_report,
        );
    }
    if !verifier_report.errors.is_empty() || !verifier_report.high_risk_findings.is_empty() {
        eprintln!("{}", error_tag("impl:verify"));
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
        return Err(FinalizeImplementationError::Verification { verifier_report });
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
        "Fields",
        "Variants",
        "Functionalities",
        "Rules",
        "Construction Rules",
        "Access Rules",
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

fn specification_path_kind(path: &Path) -> Option<&'static str> {
    let components: Vec<String> = path
        .components()
        .filter_map(|component| {
            component
                .as_os_str()
                .to_str()
                .map(|value| value.to_ascii_lowercase())
        })
        .collect();
    for window in components.windows(2) {
        if matches!(window[0].as_str(), "drafts" | "specifications") {
            return match window[1].as_str() {
                "data" => Some("data"),
                "projections" => Some("projection"),
                "contexts" => Some("context"),
                _ => None,
            };
        }
    }
    None
}

fn is_immutable_specification_path(path: &Path) -> bool {
    matches!(specification_path_kind(path), Some("data" | "projection"))
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

fn is_non_interface_or_downstream_detail(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "does not affect the exported field type or interface shape",
        "does not affect the interface shape",
        "does not block the current interface",
        "recorded here for downstream resolution",
        "recorded for downstream resolution",
        "downstream resolution but does not block",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn is_immutable_artifact_speculation_detail(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();

    let mutability_markers = [
        "mutability after construction",
        "may be mutated after",
        "pub vs accessed only through a getter",
        "accessed only through a getter",
        "field visibility",
        "&mut self setters are required",
        "setters are required",
        "no access rules defined",
        "access rules absent",
    ];
    if mutability_markers
        .iter()
        .any(|marker| lower.contains(marker))
    {
        return true;
    }

    let constructor_markers = [
        "construction rules absent",
        "no construction rules defined",
        "no constructor or smart-constructor is specified",
        "freely constructed",
    ];
    if constructor_markers
        .iter()
        .any(|marker| lower.contains(marker))
    {
        return true;
    }

    lower.contains("valid range")
        && (lower.contains("construction constraint")
            || lower.contains("board boundar")
            || lower.contains("constructor invariant")
            || lower.contains("runtime concern"))
}

fn is_interaction_role_method_binding_note_detail(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("does not export a method named")
        && lower.contains("role method")
        && lower.contains("no upstream binding")
}

fn is_projection_row_ordering_derivation_detail(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("row ordering")
        && lower.contains("coordinate origin")
        && lower.contains("top")
        && lower.contains("lower-left origin")
        && lower.contains("y = height - 1")
}

fn extract_actionable_blocking_bullets_for_path(section: &str, path: Option<&Path>) -> Vec<String> {
    let bullets = extract_bullets_with_indent(section);
    if bullets.is_empty() {
        return Vec::new();
    }

    let mut actionable = vec![false; bullets.len()];
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); bullets.len()];
    let ignore_external_source_gaps = path.is_some_and(is_external_specification_path);
    let ignore_immutable_speculation = path.is_some_and(is_immutable_specification_path);
    let ignore_interaction_role_binding_notes = matches!(
        path.and_then(specification_path_kind),
        Some("context" | "projection")
    );

    for i in 0..bullets.len() {
        actionable[i] = !is_language_or_paradigm_specific_detail(&bullets[i].1)
            && !is_no_issue_placeholder_bullet(&bullets[i].1)
            && !is_non_interface_or_downstream_detail(&bullets[i].1);
        if actionable[i]
            && ignore_external_source_gaps
            && is_external_source_gap_detail(&bullets[i].1)
        {
            actionable[i] = false;
        }
        if actionable[i]
            && ignore_immutable_speculation
            && is_immutable_artifact_speculation_detail(&bullets[i].1)
        {
            actionable[i] = false;
        }
        if actionable[i]
            && ignore_interaction_role_binding_notes
            && (is_interaction_role_method_binding_note_detail(&bullets[i].1)
                || is_projection_row_ordering_derivation_detail(&bullets[i].1))
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

fn blocking_ambiguity_summary_lines(entries: &[BlockingAmbiguitySummary]) -> Vec<String> {
    if entries.is_empty() {
        return Vec::new();
    }

    let mut lines = vec![
        error_text(
            "✗ Blocking ambiguities detected. Drafts are read-only; update these source files and retry.",
        ),
        String::new(),
    ];

    for entry in entries {
        lines.push(format!("{}:", entry.draft_name));
        lines.push(entry.draft_file.display().to_string());
        if entry.actionable.is_empty() {
            lines.push("  - No actionable ambiguity details were captured.".to_string());
        } else {
            for detail in &entry.actionable {
                lines.push(format!("  {}", detail));
            }
        }
        lines.push(String::new());
    }

    lines
}

fn print_blocking_ambiguity_summary(entries: &[BlockingAmbiguitySummary]) {
    if entries.is_empty() {
        return;
    }

    eprintln!("{}", error_tag("spec:blocking-summary"));
    for line in blocking_ambiguity_summary_lines(entries) {
        eprintln!("{line}");
    }
}

pub async fn create_tests(
    names: Vec<String>,
    clear_cache: bool,
    filter: &CategoryFilter,
    rate_limit: Option<f64>,
    token_limit: Option<f64>,
    parallel_limit: usize,
    config: &Config,
) -> Result<()> {
    create_specification(
        names.clone(),
        clear_cache,
        filter,
        rate_limit,
        token_limit,
        parallel_limit,
        false,
        0,
        config,
    )
    .await?;

    let workspace = WorkspaceContext::resolve(config)?;
    let names_provided = !names.is_empty();
    let context_files = resolve_input_files(&workspace.drafts_dir, names, "md", filter)?;

    if context_files.is_empty() {
        println!("{}", standard_text("No draft files found to process"));
        return Ok(());
    }

    let dependency_roots =
        select_dependency_roots(context_files, &workspace.drafts_dir, names_provided, filter)?;
    let execution_dag = build_execution_dag(dependency_roots, &workspace.drafts_dir, None)?;
    let total_count: usize = execution_dag
        .units()
        .iter()
        .map(|unit| unit.nodes.len())
        .sum();
    println!(
        "{}",
        header_text(format!("Creating tests for {} draft(s)", total_count))
    );

    let executor = Arc::new(AgentExecutor::new("create_test", config)?);
    let can_parallel = executor.can_run_parallel().unwrap_or(false);

    let resources = ExecutionResources::new(
        "tests",
        workspace.artifact_workspace_root(),
        rate_limit,
        token_limit,
        config.verbose,
    );

    let progress = std::cell::RefCell::new(ProgressIndicator::new(total_count));
    let mut had_unspecified = false;
    let mut prepared = HashMap::new();
    for unit in execution_dag.units() {
        for node in &unit.nodes {
            let context_file = node.input_path.clone();
            let context_name = node.name.clone();
            let mut dependency_context = build_dependency_context(
                node,
                &workspace.drafts_dir,
                None,
                &resources.run_context_cache,
            )?;
            let contract_store = ContractStore::new(".reen");
            let draft_rel = draft_relative_path(&node.input_path, &workspace.drafts_root)?;
            let interface_ir = match contract_store.read_interface_ir(&draft_rel) {
                Ok(interface_ir) => interface_ir,
                Err(error) => {
                    eprintln!("{}", warning_tag("test:missing-interface-ir"));
                    eprintln!("{} ({})", context_name, context_file.display());
                    eprintln!();
                    eprintln!("{}", error);
                    eprintln!();
                    had_unspecified = true;
                    continue;
                }
            };
            let level_hash_value = level_hash(std::slice::from_ref(&node.input_path));
            let policy = contract_store
                .read_level_policy("contract", &level_hash_value)
                .unwrap_or(LevelPolicy {
                    stage: "contract".to_string(),
                    level_hash: level_hash_value,
                    artifact_paths: vec![node.input_path.display().to_string()],
                    canonical_names: vec![context_name.clone()],
                    import_roots: Vec::new(),
                    feature_names: Vec::new(),
                    shared_type_choices: Vec::new(),
                    collaborator_abstractions: Vec::new(),
                    conflict_resolutions: Vec::new(),
                    name_bindings: Vec::new(),
                    container_shapes: Vec::new(),
                });
            dependency_context.insert(
                "interface_ir".to_string(),
                serde_json::to_value(&interface_ir).context("serialize interface_ir")?,
            );
            dependency_context.insert(
                "level_policy".to_string(),
                serde_json::to_value(&policy).context("serialize level policy")?,
            );
            augment_test_generation_context(
                &context_file,
                &workspace.drafts_root,
                &workspace.drafts_root,
                &mut dependency_context,
            )?;
            let (context_content, _) = synthesize_specification_preview(
                &context_file,
                &context_name,
                &workspace,
                dependency_context.clone(),
                config,
                clear_cache,
                token_limit,
                resources.execution_control.clone(),
                &resources.usage_reporter,
                None,
            )
            .await?;
            if !collect_blocking_ambiguities_for_path(&context_content, Some(&context_file))
                .is_empty()
            {
                had_unspecified = true;
                continue;
            }
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
            prepared.insert(
                context_file.clone(),
                StageItem {
                    name: context_name,
                    estimated,
                    cache_hit,
                    payload: (context_file, dependency_context, context_content, estimated),
                },
            );
        }
    }

    if can_parallel && config.verbose {
        println!(
            "{}",
            standard_text("Parallel execution enabled for create_test")
        );
    }

    let serial_gates = if can_parallel {
        SerialAgentGates::default()
    } else {
        SerialAgentGates::new(["create_test"])
    };
    let cfg = config.clone();
    let executor_clone = executor.clone();
    let specifications_dir = workspace.drafts_dir.clone();
    let usage_reporter = resources.usage_reporter.clone();
    let execution_control = resources.execution_control.clone();
    let dag_units = execution_dag.units().to_vec();
    let prepared_for_launch = prepared.clone();
    let prepared_for_run = prepared.clone();
    let results = run_execution_dag_units(
        &execution_dag,
        parallel_limit,
        |unit| {
            for node in &unit.nodes {
                if let Some(item) = prepared_for_launch.get(&node.input_path) {
                    if item.cache_hit {
                        progress.borrow().start_item_cached(&item.name);
                        log_agent_response_cache_hit(
                            config.verbose,
                            "test generation",
                            &item.name,
                            "create_test",
                        );
                    } else {
                        progress
                            .borrow()
                            .start_item(&item.name, Some(item.estimated));
                    }
                }
            }
        },
        |unit_id, result: &Result<Vec<(String, Result<()>)>>| match result {
            Ok(entries) => {
                for (context_name, entry) in entries {
                    progress
                        .borrow_mut()
                        .complete_item(context_name, entry.is_ok());
                }
            }
            Err(_) => {
                for node in &dag_units[unit_id].nodes {
                    if let Some(item) = prepared.get(&node.input_path) {
                        progress.borrow_mut().complete_item(&item.name, false);
                    }
                }
            }
        },
        |entries: &Vec<(String, Result<()>)>| entries.iter().all(|(_, entry)| entry.is_ok()),
        move |unit| {
            let executor = executor_clone.clone();
            let cfg = cfg.clone();
            let specifications_dir = specifications_dir.clone();
            let usage_reporter = usage_reporter.clone();
            let execution_control = execution_control.clone();
            let serial_gates = serial_gates.clone();
            let prepared = prepared_for_run.clone();
            async move {
                let _serial_guard = serial_gates.acquire("create_test").await;
                let mut unit_results = Vec::new();
                for node in unit.nodes {
                    let item = prepared
                        .get(&node.input_path)
                        .cloned()
                        .context("missing prepared test item")?;
                    let context_name = item.name.clone();
                    let (context_file, dependency_context, context_content, estimated) =
                        item.payload;
                    let result = process_tests(
                        &executor,
                        &context_content,
                        &context_file,
                        &context_name,
                        &specifications_dir,
                        &cfg,
                        clear_cache,
                        dependency_context,
                        execution_control.clone(),
                        &usage_reporter,
                        estimated,
                    )
                    .await;
                    unit_results.push((context_name, result));
                }
                Ok(unit_results)
            }
        },
    )
    .await?;

    for (_unit_id, result) in results {
        match result {
            Ok(entries) => {
                for (context_name, entry) in entries {
                    match entry {
                        Ok(_) => {
                            if config.verbose {
                                println!(
                                    "{}",
                                    success_text(format!(
                                        "✓ Successfully created tests for {}",
                                        context_name
                                    ))
                                );
                            }
                        }
                        Err(e) => {
                            if e.to_string().contains("unfinished specification") {
                                had_unspecified = true;
                            }
                            eprintln!(
                                "{}",
                                error_text(format!(
                                    "✗ Failed to create tests for {}: {}",
                                    context_name, e
                                ))
                            );
                        }
                    }
                }
            }
            Err(e) => anyhow::bail!("{}", e),
        }
    }

    let progress = progress.into_inner();
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
    let mut removed = clear_stage_auxiliary_agent_cache_dirs(stage, config)?;
    removed += if names.is_empty() {
        clear_stage_primary_agent_cache_dirs(stage, config)?
    } else {
        clear_stage_primary_agent_cache_entries_by_name(stage, names, config)?
    };
    Ok(removed)
}

fn primary_stage_agents(stage: Stage) -> &'static [&'static str] {
    match stage {
        Stage::Contract => &[
            "synthesize_contract_data",
            "resolve_interface_contract_data",
            "synthesize_contract_projection",
            "resolve_interface_contract_projection",
            "synthesize_contract_context",
            "resolve_interface_contract_context",
            "synthesize_contract_external_api",
        ],
        Stage::Implementation => &[
            "create_implementation_data",
            "create_implementation_projection",
            "create_implementation_context",
        ],
        Stage::Tests => &["create_test"],
        Stage::Compile => &[],
    }
}

fn auxiliary_stage_agents(stage: Stage) -> &'static [&'static str] {
    match stage {
        Stage::Contract => &["coordinate_contract_level", "fix_draft_blockers"],
        Stage::Implementation => &["resolve_compilation_errors"],
        Stage::Tests | Stage::Compile => &[],
    }
}

fn clear_stage_primary_agent_cache_dirs(stage: Stage, config: &Config) -> Result<usize> {
    clear_agent_cache_dirs(primary_stage_agents(stage), config)
}

fn clear_stage_auxiliary_agent_cache_dirs(stage: Stage, config: &Config) -> Result<usize> {
    clear_agent_cache_dirs(auxiliary_stage_agents(stage), config)
}

fn clear_agent_cache_dirs(agents: &[&str], config: &Config) -> Result<usize> {
    if agents.is_empty() {
        return Ok(0);
    }

    if config.dry_run {
        println!(
            "[DRY RUN] Would clear agent response cache directories: {}",
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

fn clear_stage_primary_agent_cache_entries_by_name(
    stage: Stage,
    names: &[String],
    config: &Config,
) -> Result<usize> {
    let workspace = WorkspaceContext::resolve(config)?;
    let names_vec = names.to_vec();
    let mut removed = 0usize;
    let mut candidates: Vec<(String, CacheAgentInput)> = Vec::new();
    let run_context_cache = RunContextCache::default();

    match stage {
        Stage::Contract => {
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
                let additional = build_dependency_context(
                    &node,
                    &workspace.drafts_dir,
                    None,
                    &run_context_cache,
                )?;
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
                    &run_context_cache,
                )?;
                if let Some(target_type_name) = infer_target_type_name(
                    &context_file,
                    &workspace.specifications_root,
                    &workspace.drafts_root,
                )? {
                    additional.insert("target_type_name".to_string(), json!(target_type_name));
                }
                let impl_agent =
                    determine_implementation_agent(&context_file, &workspace.specifications_dir);
                candidates.push((
                    impl_agent.to_string(),
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
                    &run_context_cache,
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
    let cache_key = agent_response_cache_key(agent_name, &instructions, input);
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

fn stage_agent_dependency_fingerprint(
    base_dependency_fingerprint: &str,
    agent_name: &str,
) -> Result<String> {
    let instructions = FileAgentRegistry::new(None)
        .get_specification(agent_name)
        .map(|template| template.canonical_for_cache())
        .map_err(|error| {
            anyhow::anyhow!(
                "Failed to load agent specification for '{}': {}",
                agent_name,
                error
            )
        })?;
    let model_name = FileAgentModelRegistry::new(None, None, None)
        .get_model(agent_name)
        .map(|model| model.name)
        .map_err(|error| {
            anyhow::anyhow!(
                "Failed to resolve model for agent '{}': {}",
                agent_name,
                error
            )
        })?;
    Ok(format!(
        "{}::agent={}",
        base_dependency_fingerprint,
        instructions_model_hash(&instructions, &model_name)
    ))
}

fn canonicalize_cache_json_value(v: serde_json::Value) -> serde_json::Value {
    match v {
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items
                .into_iter()
                .map(canonicalize_cache_json_value)
                .collect::<Vec<_>>(),
        ),
        serde_json::Value::Object(map) => {
            let mut entries: Vec<(String, serde_json::Value)> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            let mut out = serde_json::Map::new();
            for (k, val) in entries {
                out.insert(k, canonicalize_cache_json_value(val));
            }
            serde_json::Value::Object(out)
        }
        other => other,
    }
}

fn agent_response_cache_key(
    agent_name: &str,
    agent_instructions: &str,
    input: &CacheAgentInput,
) -> String {
    let input_json = serde_json::to_value(input)
        .map(|value| normalize_cache_input_value(agent_name, value))
        .and_then(|v| serde_json::to_string(&v))
        .unwrap_or_else(|_| "{}".to_string());
    let mut hasher = Sha256::new();
    hasher.update(format!("{}:{}", agent_instructions, input_json).as_bytes());
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
    reporter: &UsageReporter,
    estimated_tokens: usize,
) -> Result<()> {
    if !collect_blocking_ambiguities_for_path(context_content, Some(context_file)).is_empty() {
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

    let test_result = execute_tracked_agent_conversation(
        executor,
        &context_content,
        context_name,
        additional_context,
        execution_control,
        ignore_cache_reads,
        reporter,
        UsageScope::new("tests", context_name)
            .with_path(context_file.display().to_string())
            .with_estimated_input_tokens(estimated_tokens),
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

pub(super) fn full_implementation_dependency_context_enabled() -> bool {
    env::var("REEN_FULL_IMPLEMENTATION_DEPS")
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

fn build_dependency_context(
    node: &ExecutionNode,
    primary_root: &str,
    fallback_root: Option<&str>,
    run_context_cache: &RunContextCache,
) -> Result<HashMap<String, serde_json::Value>> {
    let mut context = HashMap::new();
    let snapshot = run_context_cache.dependency_snapshot(node, primary_root, fallback_root)?;
    let direct_dependencies = snapshot.direct_dependencies.clone();
    let dependency_closure = snapshot.dependency_closure.clone();
    let dependency_manifest = build_dependency_manifest(&dependency_closure, &direct_dependencies);
    let direct_dependency_manifest =
        build_dependency_manifest(&direct_dependencies, &direct_dependencies);
    // Direct dependency manifest only — transitive entries live in `dependency_closure`.
    context.insert(
        "direct_dependencies".to_string(),
        json!(direct_dependency_manifest.clone()),
    );
    context.insert(
        "direct_dependencies_only".to_string(),
        json!(direct_dependency_manifest),
    );
    context.insert(
        "dependency_closure".to_string(),
        json!(dependency_manifest.clone()),
    );
    if full_implementation_dependency_context_enabled() {
        context.insert("mcp_context".to_string(), json!(dependency_manifest));
    }
    context.insert(
        "dependency_fingerprint".to_string(),
        json!(snapshot.dependency_fingerprint),
    );

    let implemented_dependencies =
        build_implemented_dependency_context(&dependency_closure, run_context_cache)?;
    let implemented_direct_dependencies =
        filter_direct_implemented_dependencies(&implemented_dependencies, &direct_dependencies);
    let contract_store = ContractStore::new(".reen");
    let dependency_interfaces =
        build_dependency_interface_irs(&dependency_closure, &contract_store)?;
    let direct_dependency_interfaces =
        filter_direct_interface_irs(&dependency_interfaces, &direct_dependencies);
    let implemented_role_capsules = build_role_capsules_for_implemented_dependencies(
        &implemented_dependencies,
        run_context_cache,
    )?;
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
        "dependency_interfaces".to_string(),
        json!(dependency_interfaces),
    );
    context.insert(
        "direct_dependency_interfaces".to_string(),
        json!(direct_dependency_interfaces),
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
    if let Some(tooling_symbols) = run_context_cache.tooling_symbols(Path::new(primary_root))? {
        context.insert("tooling_symbols".to_string(), tooling_symbols);
    }
    if let Some(drafts_root) = infer_drafts_root(primary_root, fallback_root) {
        if let Some(resolved_plan) = run_context_cache.resolved_dependency_plan(&drafts_root)? {
            context.insert(
                "resolved_dependency_plan".to_string(),
                resolved_plan.clone(),
            );
            if let Some(packages) = resolved_plan.get("packages") {
                context.insert("scaffold_dependencies".to_string(), packages.clone());
            }
        }
    }
    Ok(context)
}

fn implementation_prompt_context_keys() -> &'static [&'static str] {
    &[
        "interface_ir",
        "level_policy",
        "direct_dependencies",
        "dependency_closure",
        "tooling_symbols",
        "direct_dependency_interfaces",
        "implemented_direct_role_capsules",
        "contract_artifact",
        "behavior_contract",
        "resolved_dependency_plan",
        "scaffold_dependencies",
        "library_crate_name",
        "public_import_guidance",
        "target_type_name",
        "implementation_plan",
        "previous_output",
        "verifier_feedback",
    ]
}

fn prune_implementation_prompt_context(
    mut context: HashMap<String, serde_json::Value>,
) -> HashMap<String, serde_json::Value> {
    context.retain(|key, _| implementation_prompt_context_keys().contains(&key.as_str()));
    context
}

fn implementation_dependency_fingerprint_from_context(
    context: &HashMap<String, serde_json::Value>,
) -> Result<String> {
    let canonical = canonicalize_cache_json_value(serde_json::to_value(
        prune_implementation_prompt_context(context.clone()),
    )?);
    let serialized =
        serde_json::to_string(&canonical).context("failed to serialize dependency fingerprint")?;
    let mut hasher = Sha256::new();
    hasher.update(serialized.as_bytes());
    Ok(hex::encode(hasher.finalize()))
}

fn infer_drafts_root(primary_root: &str, fallback_root: Option<&str>) -> Option<PathBuf> {
    let primary = Path::new(primary_root);
    if primary.file_name().and_then(|value| value.to_str()) == Some("drafts") {
        return Some(primary.to_path_buf());
    }
    if let Some(fallback) = fallback_root {
        let fallback = Path::new(fallback);
        if fallback.file_name().and_then(|value| value.to_str()) == Some("drafts") {
            return Some(fallback.to_path_buf());
        }
    }
    primary.parent().map(|parent| parent.join("drafts"))
}

fn build_implementation_execution_plan(
    spec_files: Vec<PathBuf>,
    filter: &CategoryFilter,
    specifications_dir: &str,
    drafts_dir: &str,
) -> Result<Vec<Vec<ExecutionNode>>> {
    Ok(
        build_implementation_execution_dag(spec_files, filter, specifications_dir, drafts_dir)?
            .levelize(),
    )
}

fn build_implementation_execution_dag(
    spec_files: Vec<PathBuf>,
    filter: &CategoryFilter,
    specifications_dir: &str,
    drafts_dir: &str,
) -> Result<ExecutionDag> {
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
    build_execution_dag(filtered_inputs, specifications_dir, Some(drafts_dir))
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
    run_context_cache: &RunContextCache,
) -> Result<String> {
    Ok(run_context_cache
        .dependency_snapshot(node, primary_root, fallback_root)?
        .dependency_fingerprint)
}

fn resolve_implementation_context_file(node_input_path: &Path) -> Result<PathBuf> {
    Ok(node_input_path.to_path_buf())
}

fn build_implemented_dependency_context(
    dependency_closure: &[DependencyArtifact],
    run_context_cache: &RunContextCache,
) -> Result<Vec<serde_json::Value>> {
    let mut artifacts = Vec::new();

    for dep in dependency_closure {
        let Some(identity_path) = resolve_dependency_identity_path(&dep.path)? else {
            continue;
        };

        let artifact_root = if identity_path.starts_with(DRAFTS_DIR) {
            DRAFTS_DIR
        } else {
            SPECIFICATIONS_DIR
        };
        let impl_path = match determine_implementation_output_path(&identity_path, artifact_root) {
            Ok(path) => path,
            Err(_) => continue,
        };

        if !impl_path.exists() {
            continue;
        }

        let content = run_context_cache.read_file(&impl_path)?;
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let sha256 = hex::encode(hasher.finalize());

        artifacts.push(json!({
            "name": dep.name,
            "spec_path": identity_path.to_string_lossy().to_string(),
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

fn resolve_dependency_identity_path(raw_path: &str) -> Result<Option<PathBuf>> {
    let path = PathBuf::from(raw_path);
    if path.starts_with(DRAFTS_DIR) && path.exists() {
        return Ok(Some(path));
    }
    if path.starts_with(SPECIFICATIONS_DIR) {
        if let Ok(draft_path) = determine_draft_input_path(&path, SPECIFICATIONS_DIR, DRAFTS_DIR) {
            if draft_path.exists() {
                return Ok(Some(draft_path));
            }
        }
        if path.exists() {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn build_role_capsules_for_implemented_dependencies(
    implemented_dependencies: &[serde_json::Value],
    run_context_cache: &RunContextCache,
) -> Result<Vec<InterfaceCapsule>> {
    let mut capsules = Vec::new();

    for item in implemented_dependencies {
        let Some(spec_path_raw) = item.get("spec_path").and_then(|value| value.as_str()) else {
            continue;
        };
        let Some(spec_path) = resolve_dependency_identity_path(spec_path_raw)? else {
            continue;
        };
        let source_path = item
            .get("path")
            .and_then(|value| value.as_str())
            .map(PathBuf::from);
        let source_content = item.get("content").and_then(|value| value.as_str());
        capsules.push(run_context_cache.interface_capsule_without_context(
            &spec_path,
            source_path.as_deref(),
            source_content,
        )?);
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
        .filter_map(|dependency| {
            resolve_dependency_identity_path(&dependency.path)
                .ok()
                .flatten()
        })
        .map(|path| path.to_string_lossy().to_string())
        .collect::<HashSet<_>>();

    capsules
        .iter()
        .filter(|capsule| direct_paths.contains(&capsule.spec_path))
        .cloned()
        .collect()
}

fn build_dependency_interface_irs(
    dependency_closure: &[DependencyArtifact],
    contract_store: &ContractStore,
) -> Result<Vec<contract_store::InterfaceIr>> {
    let mut interfaces = Vec::new();

    for dependency in dependency_closure {
        let Some(identity_path) = resolve_dependency_identity_path(&dependency.path)? else {
            continue;
        };
        let draft_rel = if identity_path.starts_with(DRAFTS_DIR) {
            identity_path
                .strip_prefix(DRAFTS_DIR)
                .map(PathBuf::from)
                .unwrap_or(identity_path.clone())
        } else {
            determine_draft_input_path(&identity_path, SPECIFICATIONS_DIR, DRAFTS_DIR)?
                .strip_prefix(DRAFTS_DIR)
                .map(PathBuf::from)
                .unwrap_or_default()
        };
        let interface_ir = match contract_store.read_interface_ir(&draft_rel) {
            Ok(interface_ir) => interface_ir,
            Err(_) => continue,
        };
        interfaces.push(interface_ir);
    }

    interfaces.sort_by(|a, b| a.draft_relative_path.cmp(&b.draft_relative_path));
    interfaces.dedup_by(|a, b| a.draft_relative_path == b.draft_relative_path);
    Ok(interfaces)
}

fn filter_direct_interface_irs(
    interfaces: &[contract_store::InterfaceIr],
    direct_dependencies: &[DependencyArtifact],
) -> Vec<contract_store::InterfaceIr> {
    let direct_paths = direct_dependencies
        .iter()
        .filter_map(|dependency| {
            resolve_dependency_identity_path(&dependency.path)
                .ok()
                .flatten()
        })
        .filter_map(|path| {
            if path.starts_with(DRAFTS_DIR) {
                path.strip_prefix(DRAFTS_DIR).ok().map(PathBuf::from)
            } else {
                determine_draft_input_path(&path, SPECIFICATIONS_DIR, DRAFTS_DIR)
                    .ok()
                    .and_then(|draft| draft.strip_prefix(DRAFTS_DIR).ok().map(PathBuf::from))
            }
        })
        .map(|path| path.to_string_lossy().to_string())
        .collect::<HashSet<_>>();

    interfaces
        .iter()
        .filter(|interface_ir| direct_paths.contains(&interface_ir.draft_relative_path))
        .cloned()
        .collect()
}

pub async fn capabilities_init(use_agent: bool, force: bool, config: &Config) -> Result<()> {
    let workspace = WorkspaceContext::resolve(config)?;
    let registry_path = capability_registry_path(&workspace.drafts_root);
    let existing = load_capability_registry(&registry_path)?;
    if existing.is_some() && !force {
        anyhow::bail!(
            "Capability registry already exists at {}. Re-run with --force to regenerate it.",
            registry_path.display()
        );
    }

    let scan = scan_draft_capabilities(&workspace.drafts_root)?;
    let mut registry = bootstrap_registry_from_scan(existing.as_ref(), &scan);
    if use_agent {
        enrich_capability_registry_with_agent(&scan, &mut registry, config).await?;
    }
    ensure_scan_coverage(&mut registry, &scan);

    if config.dry_run {
        println!(
            "[DRY RUN] Would write capability registry to {}",
            registry_path.display()
        );
        return Ok(());
    }

    write_capability_registry(&registry_path, &registry)?;
    let plan =
        sync_dependency_manifest_from_capability_registry(&workspace.drafts_root, config.verbose)?
            .unwrap_or_else(|| unreachable!("capability registry was just written"));

    println!("✓ Wrote {}", registry_path.display());
    println!(
        "✓ Regenerated {}",
        workspace.drafts_root.join("dependencies.yml").display()
    );

    if !plan.unresolved_capabilities.is_empty() {
        let unresolved = plan
            .unresolved_capabilities
            .iter()
            .map(|item| format!("{} ({})", item.capability, item.domain))
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::bail!(
            "Capability registry initialized with unresolved capabilities: {}. Resolve them with `reen capabilities add ...`.",
            unresolved
        );
    }

    Ok(())
}

pub async fn capabilities_add(
    capability: String,
    crate_name: String,
    domain: String,
    version: Option<String>,
    features: Vec<String>,
    default_features: bool,
    config: &Config,
) -> Result<()> {
    let workspace = WorkspaceContext::resolve(config)?;
    let registry_path = capability_registry_path(&workspace.drafts_root);
    let mut registry = load_capability_registry(&registry_path)?.unwrap_or_else(empty_registry);
    let resolved_version = resolve_capability_add_version(&crate_name, version).await?;
    add_capability_mapping_to_registry(
        &mut registry,
        &capability,
        &crate_name,
        &domain,
        &resolved_version,
        &features,
        default_features,
    )?;

    if config.dry_run {
        println!(
            "[DRY RUN] Would update {} with {} -> {} {} ({})",
            registry_path.display(),
            capability,
            crate_name,
            resolved_version,
            domain
        );
        return Ok(());
    }

    write_capability_registry(&registry_path, &registry)?;
    let plan =
        sync_dependency_manifest_from_capability_registry(&workspace.drafts_root, config.verbose)?
            .unwrap_or_else(|| unreachable!("capability registry was just written"));

    println!("✓ Updated {}", registry_path.display());
    println!(
        "✓ Added capability mapping: {} -> {} {} ({})",
        capability, crate_name, resolved_version, domain
    );
    println!(
        "✓ Regenerated {}",
        workspace.drafts_root.join("dependencies.yml").display()
    );
    if !plan.unresolved_capabilities.is_empty() {
        println!(
            "Unresolved capabilities remain: {}",
            plan.unresolved_capabilities
                .iter()
                .map(|item| format!("{} ({})", item.capability, item.domain))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    Ok(())
}

#[derive(Deserialize)]
struct CratesIoLookupResponse {
    #[serde(rename = "crate")]
    krate: CratesIoCrateMetadata,
}

#[derive(Deserialize)]
struct CratesIoCrateMetadata {
    #[serde(default)]
    max_stable_version: Option<String>,
    max_version: String,
}

async fn resolve_capability_add_version(
    crate_name: &str,
    requested: Option<String>,
) -> Result<String> {
    if let Some(version) = requested {
        return Ok(version);
    }

    if crate_name.trim().is_empty() {
        anyhow::bail!("crate name cannot be empty");
    }

    let crate_name = crate_name.trim().to_string();
    tokio::task::spawn_blocking(move || fetch_latest_crate_version(&crate_name))
        .await
        .context("latest crate version lookup task failed")?
}

fn fetch_latest_crate_version(crate_name: &str) -> Result<String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(format!("reen/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .context("Failed to build crates.io client")?;
    let url = format!("https://crates.io/api/v1/crates/{crate_name}");
    let response = client
        .get(&url)
        .send()
        .with_context(|| {
            format!("Failed to fetch latest version for crate '{crate_name}' from crates.io")
        })?
        .error_for_status()
        .with_context(|| {
            format!("crates.io returned an error while resolving crate '{crate_name}'")
        })?;
    let payload = response
        .text()
        .with_context(|| format!("Failed to read crates.io response for crate '{crate_name}'"))?;
    parse_latest_crate_version_response(&payload).with_context(|| {
        format!(
            "Failed to parse crates.io metadata for crate '{}'; pass --version explicitly to skip lookup",
            crate_name
        )
    })
}

fn parse_latest_crate_version_response(payload: &str) -> Result<String> {
    let parsed: CratesIoLookupResponse =
        serde_json::from_str(payload).context("crates.io response was not valid JSON")?;
    Ok(parsed
        .krate
        .max_stable_version
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(parsed.krate.max_version))
}

async fn enrich_capability_registry_with_agent(
    scan: &capability_registry::CapabilityScan,
    registry: &mut CapabilityRegistry,
    config: &Config,
) -> Result<()> {
    let unresolved = scan
        .detected
        .iter()
        .filter(|item| {
            registry
                .unmapped_capabilities
                .iter()
                .any(|candidate| candidate.capability == item.capability)
        })
        .cloned()
        .collect::<Vec<_>>();
    if unresolved.is_empty() {
        return Ok(());
    }

    let executor = AgentExecutor::new("bootstrap_capability_registry", config)?;
    let workspace = WorkspaceContext::resolve(config)?;
    let reporter = UsageReporter::new(
        "bootstrap_capability_registry",
        workspace.artifact_workspace_root(),
        config.verbose,
    );
    let mut context = HashMap::new();
    context.insert("detected_capabilities".to_string(), json!(scan));
    context.insert("unresolved_capabilities".to_string(), json!(unresolved));
    context.insert("existing_registry".to_string(), json!(registry));
    context.insert(
        "builtin_provider_catalog".to_string(),
        builtin_provider_catalog_json(),
    );
    let input = "Inspect the unresolved draft capabilities and return only a YAML capability registry fragment. Prefer the built-in provider catalog when it already covers the need. If no safe provider can be chosen, leave the capability unmapped.";
    let response = execute_tracked_agent(
        &executor,
        input,
        context,
        None,
        false,
        &reporter,
        UsageScope::new("capability_bootstrap", "capability_registry").with_path(
            capability_registry_path(&workspace.drafts_root)
                .display()
                .to_string(),
        ),
    )
    .await?;
    let output = match response {
        AgentResponse::Final(output) => output,
        AgentResponse::Questions(questions) => {
            anyhow::bail!(
                "Capability bootstrap agent requested follow-up questions unexpectedly: {}",
                questions
            );
        }
    };
    let proposal = parse_capability_registry_fragment(&output)?;
    merge_registry_proposals(registry, &proposal)?;
    Ok(())
}

pub async fn compile(config: &Config) -> Result<()> {
    cargo_commands::compile(config).await
}

pub async fn fix(
    max_compile_fix_attempts: usize,
    clear_cache: bool,
    rate_limit: Option<f64>,
    token_limit: Option<f64>,
    config: &Config,
) -> Result<()> {
    let workspace = WorkspaceContext::resolve(config)?;
    let _ =
        sync_dependency_manifest_from_capability_registry(&workspace.drafts_root, config.verbose)?;
    cargo_commands::fix(
        max_compile_fix_attempts,
        clear_cache,
        rate_limit,
        token_limit,
        config,
    )
    .await
}

pub async fn run(args: Vec<String>, config: &Config) -> Result<()> {
    cargo_commands::run(args, config).await
}

pub async fn test(config: &Config) -> Result<()> {
    cargo_commands::test(config).await
}

pub async fn clear_entire_cache(config: &Config) -> Result<()> {
    if config.dry_run {
        println!("[DRY RUN] Would clear all build-tracker entries and all agent response caches");
        return Ok(());
    }

    let mut tracker = BuildTracker::load()?;
    let removed = tracker.clear_all();
    tracker.save()?;
    println!("✓ Cleared {} build-tracker entries (all stages)", removed);

    let mut removed_agent = 0usize;
    for stage in [Stage::Contract, Stage::Implementation, Stage::Tests] {
        removed_agent += clear_agent_response_cache_for_stage(stage, &[], config)?;
    }
    println!(
        "✓ Cleared {} agent response cache entries (all stages)",
        removed_agent
    );
    Ok(())
}

pub async fn clear_implementation_src_tree(config: &Config) -> Result<()> {
    let src = PathBuf::from("src");
    if !src.exists() {
        println!("No src directory at {}", src.display());
        return Ok(());
    }
    if config.dry_run {
        println!(
            "[DRY RUN] Would remove directory {} and its contents",
            src.display()
        );
        return Ok(());
    }
    fs::remove_dir_all(&src).with_context(|| format!("Failed to remove {}", src.display()))?;
    println!("✓ Removed {}", src.display());
    Ok(())
}

pub async fn clear_all_cache_and_src(config: &Config) -> Result<()> {
    clear_entire_cache(config).await?;
    clear_implementation_src_tree(config).await?;
    Ok(())
}

/// Clear build-tracker and agent response caches, optionally scoped by category filter and/or
/// artifact names. When no filter is active and no names are given, falls back to clearing
/// everything (same as `clear_entire_cache`).
pub async fn clear_entire_cache_filtered(
    names: Vec<String>,
    filter: &CategoryFilter,
    config: &Config,
) -> Result<()> {
    if !filter.is_active() && names.is_empty() {
        return clear_entire_cache(config).await;
    }

    let resolved_names = resolve_artifact_names_for_clear(names, filter)?;
    if resolved_names.is_empty() {
        println!("No matching artifacts found");
        return Ok(());
    }

    if config.dry_run {
        println!(
            "[DRY RUN] Would clear cache entries for: {}",
            resolved_names.join(", ")
        );
        return Ok(());
    }

    let mut tracker = BuildTracker::load()?;
    let mut removed = 0usize;
    for stage in [Stage::Contract, Stage::Implementation, Stage::Tests] {
        removed += tracker.clear_stage_names(stage, &resolved_names);
    }
    tracker.save()?;

    let mut removed_agent = 0usize;
    for stage in [Stage::Contract, Stage::Implementation, Stage::Tests] {
        removed_agent += clear_agent_response_cache_for_stage(stage, &resolved_names, config)?;
    }

    println!(
        "✓ Cleared {} build-tracker entries for: {}",
        removed,
        resolved_names.join(", ")
    );
    println!(
        "✓ Cleared {} agent response cache entries for: {}",
        removed_agent,
        resolved_names.join(", ")
    );
    Ok(())
}

/// Remove generated implementation files, optionally scoped by category filter and/or artifact
/// names. When no filter is active and no names are given, falls back to removing the entire
/// `src/` tree (same as `clear_implementation_src_tree`).
pub async fn clear_implementation_filtered(
    names: Vec<String>,
    filter: &CategoryFilter,
    config: &Config,
) -> Result<()> {
    if !filter.is_active() && names.is_empty() {
        return clear_implementation_src_tree(config).await;
    }

    let spec_files = resolve_input_files(SPECIFICATIONS_DIR, names, "md", filter)?;
    if spec_files.is_empty() {
        println!("No matching implementation artifacts found");
        return Ok(());
    }

    let mut removed = 0usize;
    let mut found = 0usize;
    for spec_file in &spec_files {
        found += 1;
        let output_path = determine_implementation_output_path(spec_file, SPECIFICATIONS_DIR)?;
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

/// Resolve artifact name stems for cache clearing using the given filter and optional name list.
/// Searches the specifications directory first, then drafts as a fallback.
fn resolve_artifact_names_for_clear(
    names: Vec<String>,
    filter: &CategoryFilter,
) -> Result<Vec<String>> {
    let mut files = resolve_input_files(SPECIFICATIONS_DIR, names.clone(), "md", filter)?;
    if files.is_empty() {
        files = resolve_input_files("drafts", names, "md", filter)?;
    }
    let mut result: Vec<String> = files
        .iter()
        .filter_map(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        })
        .collect();
    result.sort();
    result.dedup();
    Ok(result)
}

#[allow(dead_code)]
pub async fn clear_cache(target: &str, names: Vec<String>, config: &Config) -> Result<()> {
    let stage = match target {
        "contract" | "contracts" | "specification" | "specifications" => Stage::Contract,
        "implementation" | "implementations" => Stage::Implementation,
        "test" | "tests" => Stage::Tests,
        other => anyhow::bail!(
            "Unsupported cache target '{}'. Expected contract(s), implementation(s), or test(s).",
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
        "✓ Cleared {} agent response cache entries for {:?}",
        removed_agent_cache_entries, stage
    );
    Ok(())
}

#[allow(dead_code)]
pub async fn clear_artifacts(target: &str, names: Vec<String>, config: &Config) -> Result<()> {
    match target {
        "contract" | "contracts" | "specification" | "specifications" => {
            clear_specification_artifacts(names, config)
        }
        "implementation" | "implementations" => clear_implementation_artifacts(names, config),
        "test" | "tests" => clear_test_artifacts(names, config),
        other => anyhow::bail!(
            "Unsupported clear target '{}'. Expected contract(s), implementation(s), or test(s).",
            other
        ),
    }
}

#[allow(dead_code)]
fn clear_specification_artifacts(names: Vec<String>, config: &Config) -> Result<()> {
    let workspace = WorkspaceContext::resolve(config)?;
    let specs_dir = PathBuf::from(SPECIFICATIONS_DIR);
    let interface_root = PathBuf::from(".reen/interfaces");
    let coordination_root = PathBuf::from(".reen/coordination");
    let debug_root = PathBuf::from(".reen/debug");
    if !interface_root.exists() && !coordination_root.exists() && !debug_root.exists() {
        println!("No contract artifacts found");
        return Ok(());
    }

    if names.is_empty() {
        if config.dry_run {
            println!("[DRY RUN] Would remove {}", interface_root.display());
            println!("[DRY RUN] Would remove {}", coordination_root.display());
            println!("[DRY RUN] Would remove {}", debug_root.display());
            return Ok(());
        }

        if interface_root.exists() {
            fs::remove_dir_all(&interface_root)
                .with_context(|| format!("Failed to remove {}", interface_root.display()))?;
        }
        if coordination_root.exists() {
            fs::remove_dir_all(&coordination_root)
                .with_context(|| format!("Failed to remove {}", coordination_root.display()))?;
        }
        if debug_root.exists() {
            fs::remove_dir_all(&debug_root)
                .with_context(|| format!("Failed to remove {}", debug_root.display()))?;
        }
        println!("✓ Removed contract artifacts at {}", specs_dir.display());
        return Ok(());
    }

    let draft_files =
        resolve_input_files(&workspace.drafts_dir, names, "md", &CategoryFilter::all())?;
    let contract_store = ContractStore::new(".reen");
    let mut removed = 0usize;
    let mut found = 0usize;
    for draft_file in draft_files {
        found += 1;
        let draft_rel = draft_relative_path(&draft_file, &workspace.drafts_root)?;
        let candidates = [
            contract_store.interface_ir_path(&draft_rel),
            contract_store.debug_bundle_path(&draft_rel),
            contract_store.debug_plan_path(&draft_rel),
            contract_store.hidden_spec_path(&draft_rel),
            contract_store.contract_bundle_path(&draft_rel),
            contract_store.implementation_plan_path(&draft_rel),
        ];
        for candidate in candidates {
            if candidate.exists() {
                if config.dry_run {
                    println!("[DRY RUN] Would remove {}", candidate.display());
                } else {
                    fs::remove_file(&candidate)
                        .with_context(|| format!("Failed to remove {}", candidate.display()))?;
                }
                removed += 1;
            }
        }
    }
    if removed == 0 {
        println!("No matching contract artifacts found");
    } else if config.dry_run {
        println!(
            "[DRY RUN] Would remove {} contract artifact file(s)",
            removed
        );
    } else {
        println!("✓ Removed {} contract artifact file(s)", removed);
    }
    if found == 0 {
        println!("No matching names were resolved in {}", specs_dir.display());
    }
    Ok(())
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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

        if filter.include_projections() {
            let projections_dir = dir_path.join("projections");
            files.extend(collect_md_files_recursive(&projections_dir, extension)?);
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

            if !found && filter.include_projections() {
                let projection_matches = resolve_named_input_in_category(
                    &dir_path.join("projections"),
                    &name,
                    extension,
                )?;
                if !projection_matches.is_empty() {
                    files.extend(projection_matches);
                    found = true;
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
                    filter.include_projections(),
                    filter.include_contexts(),
                    filter.include_root(),
                ) {
                    (true, true, true, true) => "data/, projections/, contexts/, and root",
                    (true, true, true, false) => "data/, projections/, and contexts/",
                    (true, true, false, false) => "data/ and projections/",
                    (true, false, true, false) => "data/ and contexts/",
                    (false, true, true, false) => "projections/ and contexts/",
                    (true, false, false, false) => "data/",
                    (false, true, false, false) => "projections/",
                    (false, false, true, false) => "contexts/",
                    (false, false, false, true) => "root",
                    _ => "data/, projections/, contexts/, and root",
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
/// - "synthesize_contract_data" for files in data/ folder
/// - "synthesize_contract_projection" for files in projections/ folder
/// - "synthesize_contract_context" for files in contexts/ folder
/// - "synthesize_contract_external_api" for files in external_apis/ or apis/ folder
/// - "synthesize_contract_context" for root folder drafts
fn determine_specification_agent(draft_file: &Path, drafts_dir: &str) -> &'static str {
    let draft_path = draft_file.to_path_buf();
    let drafts_path = PathBuf::from(drafts_dir);

    // Get relative path from drafts directory
    let relative_path = draft_path.strip_prefix(&drafts_path).unwrap_or(draft_file);

    // Check first component to determine folder
    if let Some(first_component) = relative_path.components().next() {
        let component_str = first_component.as_os_str().to_string_lossy();
        match component_str.as_ref() {
            "data" => "synthesize_contract_data",
            "projections" => "synthesize_contract_projection",
            "contexts" => "synthesize_contract_context",
            "external_apis" | "apis" => "synthesize_contract_external_api",
            _ => "synthesize_contract_context",
        }
    } else {
        "synthesize_contract_context"
    }
}

/// Determines which implementation agent to use based on specification file path
///
/// Returns:
/// - "create_implementation_data" for files in data/ folder
/// - "create_implementation_projection" for files in projections/ folder
/// - "create_implementation_context" for all other files (contexts, app)
fn determine_implementation_agent(context_file: &Path, artifact_root: &str) -> &'static str {
    if let Ok(rel) = relative_artifact_path(context_file, artifact_root) {
        if let Some(first_component) = rel.components().next() {
            let component_str = first_component.as_os_str().to_string_lossy();
            return match component_str.as_ref() {
                "data" => "create_implementation_data",
                "projections" => "create_implementation_projection",
                _ => "create_implementation_context",
            };
        }
    }
    "create_implementation_context"
}

/// Selects the appropriate AgentExecutor for a given specification file path.
fn select_implementation_executor<'a>(
    context_file: &Path,
    specifications_dir: &str,
    data_executor: &'a Arc<AgentExecutor>,
    projection_executor: &'a Arc<AgentExecutor>,
    context_executor: &'a Arc<AgentExecutor>,
) -> &'a Arc<AgentExecutor> {
    match determine_implementation_agent(context_file, specifications_dir) {
        "create_implementation_data" => data_executor,
        "create_implementation_projection" => projection_executor,
        _ => context_executor,
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

fn relative_artifact_path(context_file: &Path, root_dir: &str) -> Result<PathBuf> {
    let context_path = context_file.to_path_buf();
    let root_path = PathBuf::from(root_dir);

    let relative_path = match context_path.strip_prefix(&root_path) {
        Ok(rel) => rel.to_path_buf(),
        Err(_) => {
            let context_components: Vec<_> = context_path.components().collect();
            let root_components: Vec<_> = root_path.components().collect();

            if context_components.len() > root_components.len()
                && context_components
                    .iter()
                    .zip(root_components.iter())
                    .all(|(a, b)| a == b)
            {
                PathBuf::from_iter(context_components.iter().skip(root_components.len()))
            } else {
                let context_str = context_file.to_str().unwrap_or("");
                if context_str.starts_with(root_dir) {
                    let rel_str = &context_str[root_dir.len()..].trim_start_matches('/');
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
    let relative_path = relative_artifact_path(context_file, specifications_dir)?;
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
    artifact_root: &str,
) -> Result<PathBuf> {
    let relative_path = relative_artifact_path(context_file, artifact_root)?;

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
    use super::pipeline_quality::{SpecificationKind, analyze_specification};
    use super::{
        BDD_TEST_TARGETS_END, BDD_TEST_TARGETS_START, CacheAgentInput, CategoryFilter, Config,
        DEFAULT_PARALLEL_LIMIT, Stage, agent_response_cache_key, auxiliary_stage_agents,
        build_dependency_drafts_from_context, build_dependency_manifest, build_execution_plan,
        build_implementation_execution_plan, build_implemented_dependency_manifest,
        create_implementation, create_specification, determine_bdd_test_paths,
        determine_draft_input_path, determine_implementation_agent,
        determine_implementation_output_path, determine_specification_agent,
        determine_specification_output_path, ensure_dev_dependency_entry,
        external_generated_context_output_path, external_generated_data_output_path,
        extract_actionable_blocking_bullets_for_path, extract_compile_error_message,
        generated_project_structure_paths, implementation_dependency_fingerprint_from_context,
        parse_generated_files, primary_stage_agents, prune_implementation_prompt_context,
        resolve_implementation_dependency_inputs, resolve_input_files, run_execution_dag_units,
        stage_agent_dependency_fingerprint, sync_managed_block, try_fix_and_retry,
    };
    use crate::cli::dependency_graph::{
        DependencyArtifact, DependencySource, ExecutionUnit, build_execution_dag,
    };
    use crate::cli::project_structure::ProjectInfo;
    use crate::cli::usage_report::UsageReporter;
    use std::collections::HashMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::time::{Duration, timeout};

    fn temp_root(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time ok")
            .as_nanos();
        std::env::temp_dir().join(format!("reen_cli_{}_{}", prefix, nanos))
    }

    fn cwd_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct RestoreCwd(PathBuf);

    impl Drop for RestoreCwd {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }

    fn unit_label(unit: &ExecutionUnit) -> String {
        unit.nodes
            .iter()
            .map(|node| node.name.clone())
            .collect::<Vec<_>>()
            .join("+")
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
            "# AISStream API draft\n\n## Description\n\nAISStream API.\n\n## Authoritative Sources\n\n- OpenAPI Local: specs/aisstream.yaml\n",
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

        let _guard = cwd_lock().lock().expect("cwd lock");
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
        fs::create_dir_all(drafts.join("projections")).expect("mkdir");
        fs::write(
            drafts.join("contexts/ui/terminal_renderer.md"),
            "# Terminal Renderer",
        )
        .expect("write");
        fs::write(drafts.join("external_apis/stripe.md"), "# Stripe API").expect("write");
        fs::write(drafts.join("apis/aisstream.md"), "# AISStream API").expect("write");
        fs::write(
            drafts.join("projections/account_summary.md"),
            "# Account Summary",
        )
        .expect("write");
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
                .any(|p| p.ends_with("projections/account_summary.md"))
        );
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
                projections: false,
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
                projections: false,
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
                projections: false,
                data: true,
            },
        )
        .expect("nested lookup");
        assert_eq!(by_nested_name.len(), 1);
        assert!(by_nested_name[0].ends_with("data/payments/ledger_entry.md"));

        let projection_only = resolve_input_files(
            drafts.to_str().expect("drafts path"),
            vec!["account_summary".to_string()],
            "md",
            &CategoryFilter {
                contexts: false,
                projections: true,
                data: false,
            },
        )
        .expect("projection lookup");
        assert_eq!(projection_only.len(), 1);
        assert!(projection_only[0].ends_with("projections/account_summary.md"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn routes_root_specification_drafts_to_context_agent() {
        assert_eq!(
            determine_specification_agent(Path::new("drafts/app.md"), "drafts"),
            "synthesize_contract_context"
        );
    }

    #[test]
    fn routes_projection_drafts_to_projection_specification_agent() {
        assert_eq!(
            determine_specification_agent(
                Path::new("drafts/projections/account_summary.md"),
                "drafts"
            ),
            "synthesize_contract_projection"
        );
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
    fn dry_run_create_specification_does_not_write_build_tracker() {
        let root = temp_root("spec_dry_run_tracker");
        let drafts = root.join("drafts");
        fs::create_dir_all(drafts.join("data")).expect("mkdir drafts/data");
        fs::write(
            drafts.join("data/Amount.md"),
            "# Amount\n\n## Description\n\nRepresents an amount.\n\n## Fields\n\n| Field | Meaning | Notes |\n|---|---|---|\n| value | Amount value | Whole number |\n",
        )
        .expect("write draft");

        let _guard = cwd_lock().lock().expect("cwd lock");
        let original_dir = std::env::current_dir().expect("cwd");
        let _restore = RestoreCwd(original_dir);
        std::env::set_current_dir(&root).expect("set cwd");

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let config = Config {
            verbose: false,
            debug: false,
            dry_run: true,
            github_repo: None,
        };
        runtime
            .block_on(create_specification(
                Vec::new(),
                false,
                &CategoryFilter {
                    contexts: false,
                    projections: false,
                    data: true,
                },
                None,
                None,
                DEFAULT_PARALLEL_LIMIT,
                false,
                0,
                &config,
            ))
            .expect("dry-run specification creation");

        assert!(
            !root.join(".reen/build_tracker.json").exists(),
            "dry-run should not create or update the build tracker"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn dry_run_create_implementation_does_not_write_build_tracker() {
        let root = temp_root("impl_dry_run_tracker");
        let drafts = root.join("drafts");
        let specs = root.join("specifications");
        fs::create_dir_all(drafts.join("data")).expect("mkdir drafts/data");
        fs::create_dir_all(specs.join("data")).expect("mkdir specifications/data");
        fs::write(
            drafts.join("data/Amount.md"),
            "# Amount\n\n## Description\n\nRepresents an amount.\n\n## Fields\n\n| Field | Meaning | Notes |\n|---|---|---|\n| value | Amount value | Whole number |\n",
        )
        .expect("write draft");
        fs::write(
            specs.join("data/Amount.md"),
            "# Amount\n\n## Description\n\nRepresents an amount.\n\n## Fields\n\n| Field | Meaning | Notes |\n|---|---|---|\n| value | Amount value | Whole number |\n",
        )
        .expect("write specification");

        let _guard = cwd_lock().lock().expect("cwd lock");
        let original_dir = std::env::current_dir().expect("cwd");
        let _restore = RestoreCwd(original_dir);
        std::env::set_current_dir(&root).expect("set cwd");

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let config = Config {
            verbose: false,
            debug: false,
            dry_run: true,
            github_repo: None,
        };
        runtime
            .block_on(create_implementation(
                Vec::new(),
                false,
                0,
                false,
                &CategoryFilter {
                    contexts: false,
                    projections: false,
                    data: true,
                },
                None,
                None,
                DEFAULT_PARALLEL_LIMIT,
                &config,
            ))
            .expect("dry-run implementation creation");

        assert!(
            !root.join(".reen/build_tracker.json").exists(),
            "dry-run should not create or update the build tracker"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn draft_auto_fix_is_disabled() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let reporter = UsageReporter::new("test", ".", false);
        let config = Config {
            verbose: false,
            debug: false,
            dry_run: false,
            github_repo: None,
        };

        let error = runtime
            .block_on(try_fix_and_retry(
                Path::new("drafts/data/Position.md"),
                "Position",
                "# Position\n",
                "# Position\n",
                &["- duplicate sections".to_string()],
                HashMap::new(),
                0,
                3,
                &CategoryFilter::all(),
                None,
                None,
                DEFAULT_PARALLEL_LIMIT,
                &config,
                None,
                &reporter,
            ))
            .expect_err("draft auto-fix should be disabled");

        assert!(
            error
                .to_string()
                .contains("Automatic draft repair is disabled because drafts are read-only"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn parse_latest_crate_version_response_prefers_stable_release() {
        let version = super::parse_latest_crate_version_response(
            r#"{"crate":{"max_version":"2.0.0-beta.1","max_stable_version":"1.9.3"}}"#,
        )
        .expect("version");
        assert_eq!(version, "1.9.3");
    }

    #[test]
    fn parse_latest_crate_version_response_falls_back_to_max_version() {
        let version = super::parse_latest_crate_version_response(
            r#"{"crate":{"max_version":"1.4.0","max_stable_version":null}}"#,
        )
        .expect("version");
        assert_eq!(version, "1.4.0");
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

    #[test]
    fn ignores_non_interface_downstream_notes_as_actionable_blockers() {
        let section = r#"
- [position valid range / construction constraint] The contract does not specify whether `position` is validated against board boundaries on construction, enforced at the type level, or left as an unchecked runtime concern. Because no construction rules are defined and this does not affect the exported field type or interface shape, it is recorded here for downstream resolution but does not block the current interface.
"#;
        let actionable = extract_actionable_blocking_bullets_for_path(
            section,
            Some(Path::new("specifications/data/Food.md")),
        );
        assert!(actionable.is_empty(), "{actionable:?}");
    }

    #[test]
    fn ignores_immutable_data_access_and_constructor_speculation() {
        let section = r#"
- [position mutability after construction] The contract does not specify whether `position` may be mutated after a `Food` instance is created. This affects whether the field should be `pub` vs accessed only through a getter, and whether `&mut self` setters are required. Confirm access and mutation rules to finalize field visibility.
- [construction rules absent] No constructor or smart-constructor is specified. Whether Food is freely constructed or requires a validated constructor is not resolved.
- No access rules defined.
"#;
        let actionable = extract_actionable_blocking_bullets_for_path(
            section,
            Some(Path::new("drafts/data/Food.md")),
        );
        assert!(actionable.is_empty(), "{actionable:?}");
    }

    #[test]
    fn ignores_projection_role_method_binding_notes_as_actionable_blockers() {
        let section = r#"
- [symbol_at upstream Board binding] The Board capsule does not export a method named symbol_at. The contract explicitly notes this is a projection-owned role method: the projection is responsible for constructing the coordinate query using Position. No upstream binding to Board.symbol_at is possible; the projection implements this logic internally by building a Position and querying the Board through its available interface.
"#;
        let actionable = extract_actionable_blocking_bullets_for_path(
            section,
            Some(Path::new("specifications/projections/string_renderer.md")),
        );
        assert!(actionable.is_empty(), "{actionable:?}");
    }

    #[test]
    fn ignores_context_role_method_binding_notes_as_actionable_blockers() {
        let section = r#"
- [advance upstream Snake binding] The Snake capsule does not export a method named advance. The contract explicitly notes this is a context-owned role method. No upstream binding to Snake.advance is possible; the context keeps this behavior local to the interaction.
"#;
        let actionable = extract_actionable_blocking_bullets_for_path(
            section,
            Some(Path::new("specifications/contexts/game_loop.md")),
        );
        assert!(actionable.is_empty(), "{actionable:?}");
    }

    #[test]
    fn ignores_projection_row_ordering_derivation_notes_as_actionable_blockers() {
        let section = r#"
- [Row ordering coordinate origin] Contract blocking ambiguity: row ordering is not pinned to a coordinate origin. The contract assumes y = height - 1 maps to the first rendered row (visual top), consistent with Board's lower-left origin. If a different convention is intended, the ordering rule must be made explicit in the specification.
"#;
        let actionable = extract_actionable_blocking_bullets_for_path(
            section,
            Some(Path::new("specifications/projections/string_renderer.md")),
        );
        assert!(actionable.is_empty(), "{actionable:?}");
    }

    #[test]
    fn blocking_ambiguity_summary_lists_source_draft_and_details() {
        let entries = vec![super::BlockingAmbiguitySummary {
            draft_name: "Snake".to_string(),
            draft_file: PathBuf::from("tests/snake/drafts/data/Snake.md"),
            draft_content: String::new(),
            spec_content: String::new(),
            actionable: vec![
                "- Missing rule for self-collision handling.".to_string(),
                "- The draft does not specify whether growth happens before or after movement."
                    .to_string(),
            ],
            additional_context: HashMap::new(),
        }];

        let summary = super::blocking_ambiguity_summary_lines(&entries).join("\n");
        assert!(summary.contains("Snake:"));
        assert!(summary.contains("tests/snake/drafts/data/Snake.md"));
        assert!(summary.contains("Missing rule for self-collision handling"));
        assert!(summary.contains("growth happens before or after movement"));
    }

    #[test]
    fn agent_response_cache_key_is_stable_for_reordered_additional_context() {
        let mut additional_a = HashMap::new();
        additional_a.insert("zeta".to_string(), serde_json::json!(1));
        additional_a.insert("alpha".to_string(), serde_json::json!({"b": 2, "a": 1}));

        let mut additional_b = HashMap::new();
        additional_b.insert("alpha".to_string(), serde_json::json!({"a": 1, "b": 2}));
        additional_b.insert("zeta".to_string(), serde_json::json!(1));

        let input_a = CacheAgentInput {
            draft_content: Some("draft".to_string()),
            context_content: None,
            additional: additional_a,
        };
        let input_b = CacheAgentInput {
            draft_content: Some("draft".to_string()),
            context_content: None,
            additional: additional_b,
        };

        assert_eq!(
            agent_response_cache_key("synthesize_contract_context", "instructions", &input_a),
            agent_response_cache_key("synthesize_contract_context", "instructions", &input_b)
        );
    }

    #[test]
    fn stage_agent_dependency_fingerprint_tracks_agent_prompt_and_model() {
        let contract = stage_agent_dependency_fingerprint("dep", "synthesize_contract_projection")
            .expect("contract stage fingerprint");
        let implementation =
            stage_agent_dependency_fingerprint("dep", "create_implementation_projection")
                .expect("implementation stage fingerprint");

        assert_ne!(contract, implementation);
    }

    #[test]
    fn stage_agent_dependency_fingerprint_preserves_base_dependency_changes() {
        let first = stage_agent_dependency_fingerprint("first", "synthesize_contract_projection")
            .expect("first fingerprint");
        let second = stage_agent_dependency_fingerprint("second", "synthesize_contract_projection")
            .expect("second fingerprint");

        assert_ne!(first, second);
    }

    #[test]
    fn implementation_agent_cache_key_ignores_planning_fields() {
        let mut additional_a = HashMap::new();
        additional_a.insert(
            "dependency_fingerprint".to_string(),
            serde_json::json!("first"),
        );
        additional_a.insert(
            "implemented_dependencies".to_string(),
            serde_json::json!([{ "path": "src/data/amount.rs" }]),
        );
        additional_a.insert(
            "behavior_contract".to_string(),
            serde_json::json!({"kind": "Context"}),
        );
        additional_a.insert(
            "implementation_plan".to_string(),
            serde_json::json!({"tasks": ["first"]}),
        );
        additional_a.insert(
            "plan_validation".to_string(),
            serde_json::json!({"ok": true}),
        );

        let mut additional_b = HashMap::new();
        additional_b.insert(
            "dependency_fingerprint".to_string(),
            serde_json::json!("second"),
        );
        additional_b.insert(
            "implemented_dependencies".to_string(),
            serde_json::json!([{ "path": "src/data/amount.rs" }]),
        );
        additional_b.insert(
            "behavior_contract".to_string(),
            serde_json::json!({"kind": "Context"}),
        );
        additional_b.insert(
            "implementation_plan".to_string(),
            serde_json::json!({"tasks": ["second"]}),
        );
        additional_b.insert(
            "plan_validation".to_string(),
            serde_json::json!({"ok": false, "errors": ["x"]}),
        );

        let input_a = CacheAgentInput {
            draft_content: None,
            context_content: Some("spec".to_string()),
            additional: additional_a,
        };
        let input_b = CacheAgentInput {
            draft_content: None,
            context_content: Some("spec".to_string()),
            additional: additional_b,
        };

        assert_eq!(
            agent_response_cache_key("create_implementation_context", "instructions", &input_a),
            agent_response_cache_key("create_implementation_context", "instructions", &input_b)
        );
    }

    #[test]
    fn implementation_prompt_context_prunes_generated_dependency_inputs() {
        let pruned = prune_implementation_prompt_context(HashMap::from([
            (
                "interface_ir".to_string(),
                serde_json::json!({"draft_identity": "Board"}),
            ),
            (
                "level_policy".to_string(),
                serde_json::json!({"stage": "implementation"}),
            ),
            (
                "behavior_contract".to_string(),
                serde_json::json!({"kind": "Context"}),
            ),
            (
                "direct_dependency_interfaces".to_string(),
                serde_json::json!([{ "draft_identity": "Position" }]),
            ),
            (
                "implemented_dependencies".to_string(),
                serde_json::json!([{ "path": "src/data/amount.rs" }]),
            ),
            (
                "implemented_direct_role_capsules".to_string(),
                serde_json::json!([{
                    "name": "Amount",
                    "public_methods": ["value"],
                }]),
            ),
            (
                "dependency_fingerprint".to_string(),
                serde_json::json!("abc"),
            ),
            ("direct_dependencies".to_string(), serde_json::json!([])),
        ]));

        assert!(pruned.contains_key("interface_ir"));
        assert!(pruned.contains_key("level_policy"));
        assert!(pruned.contains_key("behavior_contract"));
        assert!(pruned.contains_key("direct_dependencies"));
        assert!(pruned.contains_key("direct_dependency_interfaces"));
        assert!(pruned.contains_key("implemented_direct_role_capsules"));
        assert!(!pruned.contains_key("implemented_dependencies"));
        assert!(!pruned.contains_key("dependency_fingerprint"));
    }

    #[test]
    fn implementation_dependency_fingerprint_ignores_generated_dependency_inputs() {
        let first = HashMap::from([
            (
                "behavior_contract".to_string(),
                serde_json::json!({"kind": "Context"}),
            ),
            ("library_crate_name".to_string(), serde_json::json!("snake")),
            (
                "implemented_dependencies".to_string(),
                serde_json::json!([{ "path": "src/data/amount.rs" }]),
            ),
        ]);
        let second = HashMap::from([
            (
                "behavior_contract".to_string(),
                serde_json::json!({"kind": "Context"}),
            ),
            ("library_crate_name".to_string(), serde_json::json!("snake")),
            (
                "implemented_dependencies".to_string(),
                serde_json::json!([{ "path": "src/data/other.rs" }]),
            ),
        ]);

        assert_eq!(
            implementation_dependency_fingerprint_from_context(&first).expect("first fingerprint"),
            implementation_dependency_fingerprint_from_context(&second)
                .expect("second fingerprint")
        );
    }

    #[test]
    fn implementation_dependency_fingerprint_tracks_direct_interface_capsules() {
        let first = HashMap::from([
            (
                "behavior_contract".to_string(),
                serde_json::json!({"kind": "Context"}),
            ),
            (
                "implemented_direct_role_capsules".to_string(),
                serde_json::json!([{
                    "name": "Board",
                    "public_methods": ["width"],
                    "selected_snippets": [{"label": "width", "content": "pub fn width(&self) -> i32"}],
                }]),
            ),
        ]);
        let second = HashMap::from([
            (
                "behavior_contract".to_string(),
                serde_json::json!({"kind": "Context"}),
            ),
            (
                "implemented_direct_role_capsules".to_string(),
                serde_json::json!([{
                    "name": "Board",
                    "public_methods": ["width"],
                    "selected_snippets": [{"label": "width", "content": "pub fn width(&self) -> usize"}],
                }]),
            ),
        ]);

        assert_ne!(
            implementation_dependency_fingerprint_from_context(&first).expect("first fingerprint"),
            implementation_dependency_fingerprint_from_context(&second)
                .expect("second fingerprint")
        );
    }

    #[test]
    fn implementation_dependency_fingerprint_tracks_interface_ir_and_level_policy() {
        let first = HashMap::from([
            (
                "interface_ir".to_string(),
                serde_json::json!({"draft_identity": "Board", "interface_fingerprint": "a"}),
            ),
            (
                "level_policy".to_string(),
                serde_json::json!({"stage": "implementation", "level_hash": "x"}),
            ),
        ]);
        let second = HashMap::from([
            (
                "interface_ir".to_string(),
                serde_json::json!({"draft_identity": "Board", "interface_fingerprint": "b"}),
            ),
            (
                "level_policy".to_string(),
                serde_json::json!({"stage": "implementation", "level_hash": "y"}),
            ),
        ]);

        assert_ne!(
            implementation_dependency_fingerprint_from_context(&first).expect("first fingerprint"),
            implementation_dependency_fingerprint_from_context(&second)
                .expect("second fingerprint")
        );
    }

    #[test]
    fn stage_cache_cleanup_includes_auxiliary_agents() {
        assert_eq!(
            auxiliary_stage_agents(Stage::Contract),
            &["coordinate_contract_level", "fix_draft_blockers"]
        );
        assert_eq!(
            auxiliary_stage_agents(Stage::Implementation),
            &["resolve_compilation_errors"]
        );
        assert_eq!(
            primary_stage_agents(Stage::Contract),
            &[
                "synthesize_contract_data",
                "resolve_interface_contract_data",
                "synthesize_contract_projection",
                "resolve_interface_contract_projection",
                "synthesize_contract_context",
                "resolve_interface_contract_context",
                "synthesize_contract_external_api",
            ]
        );
        let impl_agents = primary_stage_agents(Stage::Implementation);
        assert!(impl_agents.contains(&"create_implementation_data"));
        assert!(impl_agents.contains(&"create_implementation_projection"));
        assert!(impl_agents.contains(&"create_implementation_context"));
    }

    #[test]
    fn routes_data_spec_to_data_implementation_agent() {
        assert_eq!(
            determine_implementation_agent(
                Path::new("specifications/data/amount.md"),
                "specifications"
            ),
            "create_implementation_data"
        );
    }

    #[test]
    fn routes_projection_spec_to_projection_implementation_agent() {
        assert_eq!(
            determine_implementation_agent(
                Path::new("specifications/projections/account_summary.md"),
                "specifications"
            ),
            "create_implementation_projection"
        );
    }

    #[test]
    fn routes_context_spec_to_context_implementation_agent() {
        assert_eq!(
            determine_implementation_agent(
                Path::new("specifications/contexts/money_transfer.md"),
                "specifications"
            ),
            "create_implementation_context"
        );
    }

    #[test]
    fn data_specifications_infer_data_kind_for_default_implementation_plan() {
        let data_spec = r#"# Amount
## Description
Payment amount.
## Fields
- value: numeric
"#;
        let report =
            analyze_specification(Path::new("specifications/data/amount.md"), data_spec, None);
        assert!(matches!(report.contract.kind, SpecificationKind::Data));

        let projection_spec = r#"# Summary
## Purpose
Read model.
## Role Players
- ledger: reads balances
## Props
- id: Id
## Functionalities
- total: returns money
"#;
        let report = analyze_specification(
            Path::new("specifications/projections/account_summary.md"),
            projection_spec,
            None,
        );
        assert!(!matches!(report.contract.kind, SpecificationKind::Data));
    }

    #[tokio::test]
    async fn eager_scheduler_releases_dependent_unit_before_unrelated_slow_unit_finishes() {
        let root = temp_root("eager_scheduler");
        let drafts = root.join("drafts");
        fs::create_dir_all(drafts.join("contexts")).expect("mkdir contexts");
        fs::create_dir_all(drafts.join("data")).expect("mkdir data");

        let amount = drafts.join("data/amount.md");
        let account = drafts.join("contexts/account.md");
        let slow_branch = drafts.join("contexts/slow_branch.md");
        fs::write(&amount, "# Amount").expect("write amount");
        fs::write(&account, "Depends on: amount").expect("write account");
        fs::write(&slow_branch, "# Slow Branch").expect("write slow branch");

        let selected = super::expand_with_transitive_dependencies(
            vec![account.clone(), slow_branch.clone()],
            drafts.to_str().expect("drafts path"),
            None,
        )
        .expect("expanded inputs");
        let dag = build_execution_dag(selected, drafts.to_str().expect("drafts path"), None)
            .expect("dag");

        let launched = Arc::new(Mutex::new(Vec::<String>::new()));
        let account_started = Arc::new(AtomicBool::new(false));

        let launched_log = launched.clone();
        let results = timeout(
            Duration::from_secs(1),
            run_execution_dag_units(
                &dag,
                2,
                move |unit| {
                    launched_log
                        .lock()
                        .expect("launch log mutex")
                        .push(unit_label(unit));
                },
                |_unit_id, _result| {},
                |_result: &String| true,
                {
                    let account_started = account_started.clone();
                    move |unit| {
                        let account_started = account_started.clone();
                        async move {
                            let label = unit_label(&unit);
                            if label == "amount" {
                                tokio::time::sleep(Duration::from_millis(20)).await;
                            } else if label == "slow_branch" {
                                while !account_started.load(Ordering::SeqCst) {
                                    tokio::time::sleep(Duration::from_millis(5)).await;
                                }
                            } else if label == "account" {
                                account_started.store(true, Ordering::SeqCst);
                            }
                            Ok(label)
                        }
                    }
                },
            ),
        )
        .await
        .expect("scheduler should not deadlock")
        .expect("scheduler run");

        let launched = launched.lock().expect("launch log mutex");
        assert!(launched.iter().any(|label| label == "account"));
        assert_eq!(results.len(), 3);
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn eager_scheduler_does_not_release_dependents_after_failure() {
        let root = temp_root("scheduler_failure");
        let drafts = root.join("drafts");
        fs::create_dir_all(drafts.join("contexts")).expect("mkdir contexts");
        fs::create_dir_all(drafts.join("data")).expect("mkdir data");

        let amount = drafts.join("data/amount.md");
        let account = drafts.join("contexts/account.md");
        fs::write(&amount, "# Amount").expect("write amount");
        fs::write(&account, "Depends on: amount").expect("write account");

        let selected = super::expand_with_transitive_dependencies(
            vec![account.clone()],
            drafts.to_str().expect("drafts path"),
            None,
        )
        .expect("expanded inputs");
        let dag = build_execution_dag(selected, drafts.to_str().expect("drafts path"), None)
            .expect("dag");

        let launched = Arc::new(Mutex::new(Vec::<String>::new()));
        let launched_log = launched.clone();
        let results = run_execution_dag_units(
            &dag,
            2,
            move |unit| {
                launched_log
                    .lock()
                    .expect("launch log mutex")
                    .push(unit_label(unit));
            },
            |_unit_id, _result| {},
            |_result: &String| true,
            move |unit| async move {
                let label = unit_label(&unit);
                if label == "amount" {
                    Err(anyhow::anyhow!("simulated failure"))
                } else {
                    Ok(label)
                }
            },
        )
        .await
        .expect("scheduler run");

        let launched = launched.lock().expect("launch log mutex");
        assert_eq!(launched.as_slice(), ["amount"]);
        assert_eq!(results.len(), 1);
        assert!(results[0].1.is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn eager_scheduler_keeps_scc_units_atomic() {
        let root = temp_root("scheduler_scc");
        let drafts = root.join("drafts");
        fs::create_dir_all(drafts.join("contexts")).expect("mkdir contexts");

        let left = drafts.join("contexts/left.md");
        let right = drafts.join("contexts/right.md");
        let app = drafts.join("app.md");
        fs::write(&left, "Depends on: right").expect("write left");
        fs::write(&right, "Depends on: left").expect("write right");
        fs::write(&app, "Depends on: left").expect("write app");

        let selected = super::expand_with_transitive_dependencies(
            vec![app.clone()],
            drafts.to_str().expect("drafts path"),
            None,
        )
        .expect("expanded inputs");
        let dag = build_execution_dag(selected, drafts.to_str().expect("drafts path"), None)
            .expect("dag");
        assert_eq!(dag.units().len(), 2);
        assert!(dag.units().iter().any(|unit| unit.nodes.len() == 2));

        let cycle_completed = Arc::new(AtomicBool::new(false));
        let results = run_execution_dag_units(
            &dag,
            2,
            |_unit| {},
            |_unit_id, _result| {},
            |_result: &String| true,
            {
                let cycle_completed = cycle_completed.clone();
                move |unit| {
                    let cycle_completed = cycle_completed.clone();
                    async move {
                        if unit.nodes.len() == 2 {
                            tokio::time::sleep(Duration::from_millis(30)).await;
                            cycle_completed.store(true, Ordering::SeqCst);
                            Ok(unit_label(&unit))
                        } else {
                            if !cycle_completed.load(Ordering::SeqCst) {
                                anyhow::bail!("dependent launched before SCC completed");
                            }
                            Ok(unit_label(&unit))
                        }
                    }
                }
            },
        )
        .await
        .expect("scheduler run");

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|(_, result)| result.is_ok()));
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn eager_scheduler_respects_parallel_limit() {
        let root = temp_root("scheduler_parallel_limit");
        let drafts = root.join("drafts");
        fs::create_dir_all(drafts.join("data")).expect("mkdir data");

        let selected = ["a", "b", "c", "d"]
            .into_iter()
            .map(|name| {
                let path = drafts.join("data").join(format!("{name}.md"));
                fs::write(&path, format!("# {name}")).expect("write data draft");
                path
            })
            .collect::<Vec<_>>();
        let dag = build_execution_dag(selected, drafts.to_str().expect("drafts path"), None)
            .expect("dag");

        let current = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let results = run_execution_dag_units(
            &dag,
            2,
            |_unit| {},
            |_unit_id, _result| {},
            |_result: &String| true,
            {
                let current = current.clone();
                let peak = peak.clone();
                move |unit| {
                    let current = current.clone();
                    let peak = peak.clone();
                    async move {
                        let in_flight = current.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(in_flight, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(25)).await;
                        current.fetch_sub(1, Ordering::SeqCst);
                        Ok(unit_label(&unit))
                    }
                }
            },
        )
        .await
        .expect("scheduler run");

        assert_eq!(results.len(), 4);
        assert!(peak.load(Ordering::SeqCst) <= 2);
        let _ = fs::remove_dir_all(root);
    }
}
