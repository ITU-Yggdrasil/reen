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
mod brand_specs;
mod cargo_commands;
mod compilation_fix;
mod dependency_graph;
mod openapi_fetcher;
mod patch_service;
mod pipeline_context;
mod progress;
mod project_structure;
mod rate_limiter;
mod stage_runner;

use agent_executor::{AgentExecutor, AgentResponse};
use brand_specs::{
    collect_brand_token_references, is_brand_draft_path, is_brand_spec_path,
    unresolved_brand_token_references, validate_brand_spec_content,
};
use dependency_graph::{
    build_execution_plan, expand_with_transitive_dependencies, DependencyArtifact, ExecutionNode,
};
use patch_service::apply_draft_patches;
use pipeline_context::{build_specification_context, fit_context_to_token_limit};
use progress::ProgressIndicator;
use project_structure::{
    analyze_specifications, generate_cargo_toml, generate_lib_rs, generate_mod_files, ProjectInfo,
};
use reen::build_tracker::{BuildTracker, Stage};
use reen::execution::{AgentModelRegistry, AgentRegistry, NativeExecutionControl};
use reen::registries::{FileAgentModelRegistry, FileAgentRegistry};
use stage_runner::{run_stage_items, CliExecutionControl, ExecutionResources, StageItem};

#[derive(Clone, Copy)]
pub struct Config {
    pub verbose: bool,
    pub dry_run: bool,
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
    pub brands: bool,
    pub visuals: bool,
}

impl CategoryFilter {
    pub fn all() -> Self {
        Self {
            contexts: false,
            data: false,
            brands: false,
            visuals: false,
        }
    }

    fn is_active(&self) -> bool {
        self.contexts || self.data || self.brands || self.visuals
    }

    fn include_data(&self) -> bool {
        !self.is_active() || self.data
    }

    fn include_contexts(&self) -> bool {
        !self.is_active() || self.contexts
    }

    fn include_brands(&self) -> bool {
        !self.is_active() || self.brands
    }

    fn include_visuals(&self) -> bool {
        !self.is_active() || self.visuals
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
                    "contexts" | "external_apis" => self.include_contexts(),
                    "brands" => self.include_brands(),
                    "visuals" => self.include_visuals(),
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

fn print_blocking_items(path: &Path, label: &str, headline: &str, items: &[String]) {
    eprintln!("error[{}]:", label);
    eprintln!("\u{001b}[31m{}\u{001b}[0m", path.display());
    eprintln!("  {}", headline);
    eprintln!();
    for item in items {
        eprintln!("  {}", item);
    }
    eprintln!();
}

fn check_brand_references_in_spec_content(path: &Path, content: &str) -> Result<Vec<String>> {
    unresolved_brand_token_references(content, SPECIFICATIONS_DIR).with_context(|| {
        format!(
            "failed to validate brand token references in {}",
            path.display()
        )
    })
}

struct PreparedImplementationBatchItem {
    context_name: String,
    context_file: PathBuf,
    output_path: PathBuf,
    dependency_fingerprint: String,
    dependency_context: HashMap<String, serde_json::Value>,
    context_content: String,
    prepared: reen::execution::PreparedExecution,
}

#[derive(Clone)]
struct ImplementationRunnable {
    agent_name: &'static str,
    context_file: PathBuf,
    context_name: String,
    output_path: PathBuf,
    dependency_fingerprint: String,
    dependency_context: HashMap<String, serde_json::Value>,
    context_content: String,
    estimated: usize,
    cache_hit: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GeneratedOutputFile {
    path: PathBuf,
    content: String,
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
    let config = *config;
    Box::pin(async move {
        let names_provided = !names.is_empty();
        let names_for_clear = names.clone();
        let draft_files = resolve_input_files(DRAFTS_DIR, names, "md", &filter)?;

        if draft_files.is_empty() {
            println!("No draft files found to process");
            return Ok(());
        }

        let dependency_roots =
            select_dependency_roots(draft_files, DRAFTS_DIR, names_provided, &filter)?;
        let expanded_draft_files =
            expand_with_transitive_dependencies(dependency_roots, DRAFTS_DIR, None)?;
        let filtered_draft_files = if filter.is_active() {
            expanded_draft_files
                .into_iter()
                .filter(|f| filter.matches_path(f, DRAFTS_DIR))
                .collect()
        } else {
            expanded_draft_files
        };
        let execution_levels = build_execution_plan(filtered_draft_files, DRAFTS_DIR, None)?;

        // Load build tracker
        let mut tracker = BuildTracker::load()?;
        if clear_cache {
            clear_tracker_stage(
                &mut tracker,
                Stage::Specification,
                &names_for_clear,
                &config,
            )?;
        }

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
                let agent = determine_specification_agent(&node.input_path, DRAFTS_DIR).to_string();
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
                        dependency_fingerprint_for_node(&node, DRAFTS_DIR, None)?;
                    let output_path = determine_specification_output_path(
                        &draft_file,
                        DRAFTS_DIR,
                        SPECIFICATIONS_DIR,
                    )?;

                    let needs_update = if dependency_invalidated {
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

                    let dependency_context = match build_dependency_context(&node) {
                        Ok(context) => context,
                        Err(e) => {
                            progress.complete_item(&draft_name, false);
                            eprintln!("✗ Failed to create specification for {}: {}", draft_name, e);
                            continue;
                        }
                    };

                    let draft_content = fs::read_to_string(&draft_file).unwrap_or_default();
                    let dependency_context = build_specification_context(
                        &draft_file,
                        &draft_content,
                        dependency_context,
                    )?;
                    let (dependency_context, estimated) = fit_context_to_token_limit(
                        &executor,
                        &draft_content,
                        dependency_context,
                        token_limit,
                    )?;
                    let cache_hit = executor
                        .is_cache_hit(&draft_content, dependency_context.clone())
                        .unwrap_or(false);
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

                let cfg = config;
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
                        async move {
                            let result = process_specification(
                                &executor,
                                &draft_content,
                                &draft_file,
                                &draft_name,
                                &cfg,
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

pub async fn check_specification(names: Vec<String>, _config: &Config) -> Result<()> {
    let draft_files = resolve_input_files(DRAFTS_DIR, names, "md", &CategoryFilter::all())?;
    if draft_files.is_empty() {
        println!("No draft files found to process");
        return Ok(());
    }

    let tracker = BuildTracker::load()?;
    let mut issues = 0usize;
    println!("Checking specifications for {} draft(s)", draft_files.len());

    for draft_file in draft_files {
        let draft_name = draft_file
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .context("Invalid draft filename")?;
        let spec_path =
            determine_specification_output_path(&draft_file, DRAFTS_DIR, SPECIFICATIONS_DIR)?;

        if !spec_path.exists() {
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
        }

        let spec_content = fs::read_to_string(&spec_path).with_context(|| {
            format!("Failed to read specification file: {}", spec_path.display())
        })?;
        if is_brand_spec_path(&spec_path, SPECIFICATIONS_DIR)
            || is_brand_draft_path(&draft_file, DRAFTS_DIR)
        {
            match validate_brand_spec_content(&spec_content) {
                Ok(validation) => {
                    if !validation.blocking_ambiguities.is_empty() {
                        issues += 1;
                        print_blocking_items(
                            &draft_file,
                            "spec:blocking",
                            &format!(
                                "Blocking ambiguities detected in specification for '{}'.",
                                draft_name
                            ),
                            &validation.blocking_ambiguities,
                        );
                    }
                }
                Err(err) => {
                    issues += 1;
                    eprintln!("error[spec:invalid-brand]:");
                    eprintln!("\u{001b}[31m{}\u{001b}[0m", draft_file.display());
                    eprintln!(
                        "  Invalid brand specification for '{}': {}",
                        draft_name, err
                    );
                    eprintln!();
                }
            }
            continue;
        }

        if let Some(blocking) = extract_blocking_ambiguities_section(&spec_content) {
            let actionable = extract_actionable_blocking_bullets(&blocking);
            if !actionable.is_empty() {
                issues += 1;
                print_blocking_items(
                    &draft_file,
                    "spec:blocking",
                    &format!(
                        "Blocking ambiguities detected in specification for '{}'.",
                        draft_name
                    ),
                    &actionable,
                );
            }
        }

        let unresolved = check_brand_references_in_spec_content(&spec_path, &spec_content)?;
        if !unresolved.is_empty() {
            issues += 1;
            print_blocking_items(
                &draft_file,
                "spec:brand-token",
                &format!(
                    "Undefined brand token references detected in specification for '{}'.",
                    draft_name
                ),
                &unresolved,
            );
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
    config: &Config,
    additional_context: HashMap<String, serde_json::Value>,
    execution_control: Option<CliExecutionControl>,
) -> Result<ProcessSpecOutcome> {
    if config.dry_run {
        println!("[DRY RUN] Would create specification for: {}", draft_name);
        return Ok(ProcessSpecOutcome::Success);
    }

    // Use conversational execution to handle questions
    let spec_content = executor
        .execute_with_conversation_with_seed(
            &draft_content,
            draft_name,
            additional_context.clone(),
            execution_control
                .as_ref()
                .map(|control| control as &dyn NativeExecutionControl),
        )
        .await?;

    finalize_specification_output(
        draft_content,
        draft_file,
        draft_name,
        spec_content,
        additional_context,
    )
}

fn finalize_specification_output(
    draft_content: &str,
    draft_file: &Path,
    draft_name: &str,
    spec_content: String,
    additional_context: HashMap<String, serde_json::Value>,
) -> Result<ProcessSpecOutcome> {
    // Determine output path preserving folder structure
    let output_path =
        determine_specification_output_path(draft_file, DRAFTS_DIR, SPECIFICATIONS_DIR)?;

    let mut has_blocking_ambiguities = false;
    let mut actionable = Vec::new();

    if is_brand_draft_path(draft_file, DRAFTS_DIR) {
        let validation = validate_brand_spec_content(&spec_content).with_context(|| {
            format!(
                "generated brand specification for '{}' is invalid",
                draft_name
            )
        })?;
        actionable = validation.blocking_ambiguities;
        has_blocking_ambiguities = !actionable.is_empty();

        if has_blocking_ambiguities {
            print_blocking_items(
                draft_file,
                "spec:blocking",
                &format!(
                    "Blocking ambiguities detected in generated specification for '{}'.",
                    draft_name
                ),
                &actionable,
            );
        }
    } else {
        // Report Blocking Ambiguities immediately if present in generated spec
        if let Some(blocking) = extract_blocking_ambiguities_section(&spec_content) {
            actionable = extract_actionable_blocking_bullets(&blocking);
            if !actionable.is_empty() {
                has_blocking_ambiguities = true;
                print_blocking_items(
                    draft_file,
                    "spec:blocking",
                    &format!(
                        "Blocking ambiguities detected in generated specification for '{}'.",
                        draft_name
                    ),
                    &actionable,
                );
            }
        }

        let unresolved = check_brand_references_in_spec_content(&output_path, &spec_content)?;
        if !unresolved.is_empty() {
            anyhow::bail!(
                "generated specification for '{}' references undefined brand token(s): {}",
                draft_name,
                unresolved.join(", ")
            );
        }
    }

    // Ensure the output directory exists
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).context("Failed to create specification output directory")?;
    }

    fs::write(&output_path, &spec_content).context("Failed to write specification file")?;

    if has_blocking_ambiguities {
        return Ok(ProcessSpecOutcome::BlockingAmbiguities {
            draft_file: draft_file.to_path_buf(),
            draft_name: draft_name.to_string(),
            draft_content: draft_content.to_string(),
            spec_content,
            actionable,
            additional_context,
        });
    }

    Ok(ProcessSpecOutcome::Success)
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

    let mut tracker = BuildTracker::load()?;
    clear_tracker_stage(
        &mut tracker,
        Stage::Specification,
        &affected_names_vec,
        config,
    )?;

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
    let names_provided = !names.is_empty();
    let names_for_clear = names.clone();
    let context_files = resolve_input_files(SPECIFICATIONS_DIR, names, "md", filter)?;

    if context_files.is_empty() {
        println!("No context files found to process");
        return Ok(());
    }

    // Load build tracker
    let mut tracker = BuildTracker::load()?;
    if clear_cache {
        clear_tracker_stage(
            &mut tracker,
            Stage::Implementation,
            &names_for_clear,
            config,
        )?;
    }

    // Check if any specifications need to be regenerated first
    if tracker.upstream_changed(Stage::Implementation, "")? {
        println!("⚠ Upstream specifications have changed. Run 'reen create specification' first.");
    }

    let dependency_roots = select_dependency_roots(
        context_files.clone(),
        SPECIFICATIONS_DIR,
        names_provided,
        filter,
    )?;
    let execution_levels = build_implementation_execution_plan(dependency_roots, filter)?;
    let total_count: usize = execution_levels.iter().map(|level| level.len()).sum();
    println!(
        "Creating implementation for {} specification(s)",
        total_count
    );

    // Step 1: Generate project structure (Cargo.toml, lib.rs, mod.rs files)
    if config.verbose {
        println!("Generating project structure...");
    }

    let spec_dir = PathBuf::from(SPECIFICATIONS_DIR);
    let drafts_dir = PathBuf::from(DRAFTS_DIR);
    let mut project_info = analyze_specifications(&spec_dir, Some(&drafts_dir))
        .context("Failed to analyze specifications")?;
    project_info
        .modules
        .retain(|folder, _| folder != "brands" && !folder.starts_with("brands/"));
    project_info
        .type_names
        .retain(|key, _| key != "brands" && !key.starts_with("brands/"));
    let has_non_brand_targets = context_files
        .iter()
        .any(|path| !is_brand_spec_path(path, SPECIFICATIONS_DIR));

    if has_non_brand_targets {
        let output_dir = PathBuf::from(".");

        generate_cargo_toml(&project_info, &output_dir).context("Failed to generate Cargo.toml")?;

        generate_lib_rs(&project_info, &output_dir).context("Failed to generate lib.rs")?;

        generate_mod_files(&project_info, &output_dir)
            .context("Failed to generate mod.rs files")?;

        if config.verbose {
            println!("✓ Project structure generated");
        }
    } else if config.verbose {
        println!("Skipping generic project structure generation for brand-only implementation run");
    }

    let mut recent_generated_files: Vec<PathBuf> = Vec::new();
    if has_non_brand_targets {
        for p in generated_project_structure_paths(&project_info) {
            if p.exists() {
                recent_generated_files.push(p);
            }
        }
    }

    // Step 2: Generate individual implementation files
    let implementation_executor = Arc::new(AgentExecutor::new("create_implementation", config)?);
    let brand_executor = Arc::new(AgentExecutor::new("create_implementation_brand", config)?);

    if config.verbose {
        let path = implementation_executor.model_registry().registry_path();
        println!("Agent model registry: {}", path.display());
        println!(
            "create_implementation parallel: {}",
            implementation_executor.can_run_parallel().unwrap_or(false)
        );
        println!(
            "create_implementation_brand parallel: {}",
            brand_executor.can_run_parallel().unwrap_or(false)
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

        let mut runnable: Vec<ImplementationRunnable> = Vec::new();
        for node in level_nodes {
            let context_file = resolve_implementation_context_file(&node.input_path)?;
            let agent_name = implementation_agent_name(&context_file);
            let context_name = node.name.clone();
            let dependency_invalidated = node
                .direct_dependency_names()
                .iter()
                .any(|dep_name| updated_in_run.contains(dep_name));
            let dependency_fingerprint = dependency_fingerprint_for_node(&node, DRAFTS_DIR, None)?;
            let output_path =
                determine_implementation_output_path(&context_file, SPECIFICATIONS_DIR)?;

            if has_unfinished_specification(&context_file, &context_name, "implementation")? {
                had_unspecified = true;
                progress.start_item(&context_name, None);
                progress.complete_item(&context_name, false);
                continue;
            }

            let needs_update = if dependency_invalidated {
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

            let mut dependency_context = build_dependency_context(&node)?;
            if let Some(target_type_name) = infer_target_type_name(&context_file)? {
                dependency_context.insert("target_type_name".to_string(), json!(target_type_name));
            }
            let context_content = fs::read_to_string(&context_file).unwrap_or_default();
            let executor = if agent_name == "create_implementation_brand" {
                &brand_executor
            } else {
                &implementation_executor
            };
            let (dependency_context, estimated) = fit_context_to_token_limit(
                executor,
                &context_content,
                dependency_context,
                token_limit,
            )?;
            let cache_hit = executor
                .is_cache_hit(&context_content, dependency_context.clone())
                .unwrap_or(false);
            runnable.push(ImplementationRunnable {
                agent_name,
                context_file,
                context_name,
                output_path,
                dependency_fingerprint,
                dependency_context,
                context_content,
                estimated,
                cache_hit,
            });
        }

        let level_agent_name = runnable.first().map(|item| item.agent_name);
        let single_agent_level = level_agent_name.is_some()
            && runnable
                .iter()
                .all(|item| Some(item.agent_name) == level_agent_name);
        let level_executor = level_agent_name.map(|agent_name| {
            if agent_name == "create_implementation_brand" {
                brand_executor.clone()
            } else {
                implementation_executor.clone()
            }
        });
        let can_parallel = single_agent_level
            && level_executor
                .as_ref()
                .and_then(|executor| executor.can_run_parallel().ok())
                .unwrap_or(false);

        if can_parallel {
            if config.verbose {
                println!(
                    "Parallel execution enabled for {}",
                    level_agent_name.unwrap_or("create_implementation")
                );
            }
            let cfg = *config;
            let executor = level_executor.expect("parallel execution requires a level executor");
            let use_batch = executor.can_use_batch().unwrap_or(false);
            if use_batch {
                let mut batch_items: Vec<PreparedImplementationBatchItem> = Vec::new();
                let mut batch_results: Vec<(String, PathBuf, PathBuf, String, Result<()>)> =
                    Vec::new();

                for item in runnable {
                    if item.cache_hit {
                        progress.start_item_cached(&item.context_name);
                        let result = process_implementation(
                            &executor,
                            &item.context_content,
                            &item.context_file,
                            &item.context_name,
                            &cfg,
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
                        continue;
                    }

                    progress.start_item(&item.context_name, Some(item.estimated));
                    match executor
                        .prepare_execution(&item.context_content, item.dependency_context.clone())?
                    {
                        reen::execution::PreparedExecutionState::Cached(output) => {
                            let result = finalize_implementation_output(
                                &item.context_file,
                                &item.context_name,
                                &cfg,
                                output,
                            );
                            batch_results.push((
                                item.context_name,
                                item.context_file,
                                item.output_path,
                                item.dependency_fingerprint,
                                result,
                            ));
                        }
                        reen::execution::PreparedExecutionState::Ready(prepared) => {
                            batch_items.push(PreparedImplementationBatchItem {
                                context_name: item.context_name,
                                context_file: item.context_file,
                                output_path: item.output_path,
                                dependency_fingerprint: item.dependency_fingerprint,
                                dependency_context: item.dependency_context,
                                context_content: item.context_content,
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
                                        &cfg,
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
                                        &cfg,
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
                                "Batch execution failed for {}; falling back to sequential execution: {}",
                                level_agent_name.unwrap_or("create_implementation"),
                                batch_error
                            );
                            for item in batch_items {
                                let result = process_implementation(
                                    &executor,
                                    &item.context_content,
                                    &item.context_file,
                                    &item.context_name,
                                    &cfg,
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
                    .map(|item| StageItem {
                        name: item.context_name.clone(),
                        estimated: item.estimated,
                        cache_hit: item.cache_hit,
                        payload: item,
                    })
                    .collect::<Vec<_>>();

                let implementation_executor_clone = implementation_executor.clone();
                let brand_executor_clone = brand_executor.clone();
                let results = run_stage_items(
                    stage_items,
                    true,
                    &mut progress,
                    &resources,
                    config,
                    move |item, execution_control| {
                        let implementation_executor = implementation_executor_clone.clone();
                        let brand_executor = brand_executor_clone.clone();
                        async move {
                            let executor = if item.agent_name == "create_implementation_brand" {
                                brand_executor
                            } else {
                                implementation_executor
                            };
                            process_implementation(
                                &executor,
                                &item.context_content,
                                &item.context_file,
                                &item.context_name,
                                &cfg,
                                item.dependency_context,
                                execution_control,
                            )
                            .await?;
                            Ok((
                                item.context_file,
                                item.output_path,
                                item.dependency_fingerprint,
                            ))
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
                .map(|item| StageItem {
                    name: item.context_name.clone(),
                    estimated: item.estimated,
                    cache_hit: item.cache_hit,
                    payload: item,
                })
                .collect::<Vec<_>>();

            let implementation_executor_clone = implementation_executor.clone();
            let brand_executor_clone = brand_executor.clone();
            let cfg = *config;
            let results = run_stage_items(
                stage_items,
                false,
                &mut progress,
                &resources,
                config,
                move |item, execution_control| {
                    let implementation_executor = implementation_executor_clone.clone();
                    let brand_executor = brand_executor_clone.clone();
                    async move {
                        let executor = if item.agent_name == "create_implementation_brand" {
                            brand_executor
                        } else {
                            implementation_executor
                        };
                        process_implementation(
                            &executor,
                            &item.context_content,
                            &item.context_file,
                            &item.context_name,
                            &cfg,
                            item.dependency_context,
                            execution_control,
                        )
                        .await?;
                        Ok((
                            item.context_file,
                            item.output_path,
                            item.dependency_fingerprint,
                        ))
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
        compilation_fix::ensure_compiles_with_auto_fix(
            config,
            max_compile_fix_attempts,
            Path::new("."),
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
    config: &Config,
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
        .execute_with_conversation_with_seed(
            &context_content,
            context_name,
            additional_context,
            execution_control
                .as_ref()
                .map(|control| control as &dyn NativeExecutionControl),
        )
        .await?;

    finalize_implementation_output(context_file, context_name, config, impl_result)
}

fn implementation_agent_name(context_file: &Path) -> &'static str {
    if is_brand_spec_path(context_file, SPECIFICATIONS_DIR) {
        "create_implementation_brand"
    } else {
        "create_implementation"
    }
}

fn finalize_implementation_output(
    context_file: &Path,
    context_name: &str,
    config: &Config,
    impl_result: String,
) -> Result<()> {
    if is_brand_spec_path(context_file, SPECIFICATIONS_DIR) {
        return finalize_brand_implementation_output(
            context_file,
            context_name,
            config,
            impl_result,
        );
    }

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
    let output_path = determine_implementation_output_path(context_file, SPECIFICATIONS_DIR)?;

    // Ensure the output directory exists
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).context("Failed to create implementation output directory")?;
    }

    // Write the implementation file
    fs::write(&output_path, code).context("Failed to write implementation file")?;

    if config.verbose {
        println!("✓ Written implementation to: {}", output_path.display());
    }

    if implementation_failure.is_some() {
        return Err(anyhow::anyhow!(
            "Generated implementation for '{}' contains explicit failure marker",
            context_name
        ));
    }

    Ok(())
}

fn finalize_brand_implementation_output(
    context_file: &Path,
    context_name: &str,
    config: &Config,
    impl_result: String,
) -> Result<()> {
    let generated_files = parse_generated_output_files(&impl_result)?;
    validate_brand_generated_output(context_file, context_name, &generated_files)?;

    for file in &generated_files {
        if let Some(parent) = file.path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "Failed to create brand implementation directory {}",
                    parent.display()
                )
            })?;
        }
        fs::write(&file.path, &file.content).with_context(|| {
            format!(
                "Failed to write brand implementation file {}",
                file.path.display()
            )
        })?;
        if config.verbose {
            println!("Written brand implementation file: {}", file.path.display());
        }
    }

    if let Some((failed_file, message)) = generated_files
        .iter()
        .filter(|file| file.path.extension().and_then(|ext| ext.to_str()) == Some("rs"))
        .find_map(|file| {
            extract_implementation_failure_message(&file.content)
                .map(|message| (file.path.clone(), message))
        })
    {
        eprintln!("error[impl:compile_error]:");
        eprintln!("\u{001b}[31m{}\u{001b}[0m", context_file.display());
        eprintln!(
            "  Generated brand implementation for '{}' contains an explicit failure marker in {}:",
            context_name,
            failed_file.display()
        );
        eprintln!();
        for line in message.lines() {
            eprintln!("  {}", line);
        }
        eprintln!();
        anyhow::bail!(
            "Generated brand implementation for '{}' contains explicit failure marker",
            context_name
        );
    }

    Ok(())
}

fn parse_generated_output_files(output: &str) -> Result<Vec<GeneratedOutputFile>> {
    const FILE_PREFIX: &str = "===FILE:";
    const FILE_SUFFIX: &str = "===";
    const END_MARKER: &str = "===END_FILE===";

    let mut files = Vec::new();
    let mut current_path: Option<String> = None;
    let mut current_lines: Vec<String> = Vec::new();
    let mut seen_paths = HashSet::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(FILE_PREFIX) && trimmed.ends_with(FILE_SUFFIX) {
            if current_path.is_some() {
                anyhow::bail!(
                    "generated output started a new file block before closing the previous one"
                );
            }
            let raw_path = trimmed
                .trim_start_matches(FILE_PREFIX)
                .trim_end_matches(FILE_SUFFIX)
                .trim();
            if raw_path.is_empty() {
                anyhow::bail!("generated output declared an empty file path");
            }
            current_path = Some(raw_path.to_string());
            current_lines.clear();
            continue;
        }

        if trimmed == END_MARKER {
            let raw_path = current_path.take().ok_or_else(|| {
                anyhow::anyhow!("generated output ended a file block before starting one")
            })?;
            let path = validate_generated_output_path(&raw_path)?;
            let key = path.to_string_lossy().to_string();
            if !seen_paths.insert(key.clone()) {
                anyhow::bail!("generated output contains duplicate file entry '{}'", key);
            }
            files.push(GeneratedOutputFile {
                path,
                content: current_lines.join("\n"),
            });
            current_lines.clear();
            continue;
        }

        if current_path.is_some() {
            current_lines.push(line.to_string());
        } else if !trimmed.is_empty() {
            anyhow::bail!("generated output contains non-file content outside file blocks");
        }
    }

    if current_path.is_some() {
        anyhow::bail!("generated output ended before closing the last file block");
    }
    if files.is_empty() {
        anyhow::bail!("generated output did not contain any file blocks");
    }

    Ok(files)
}

fn validate_generated_output_path(raw_path: &str) -> Result<PathBuf> {
    let path = Path::new(raw_path);
    if path.is_absolute() {
        anyhow::bail!("generated output path '{}' must be relative", raw_path);
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(part) => normalized.push(part),
            _ => anyhow::bail!(
                "generated output path '{}' contains disallowed path traversal or prefix components",
                raw_path
            ),
        }
    }

    if normalized.as_os_str().is_empty() {
        anyhow::bail!(
            "generated output path '{}' is empty after normalization",
            raw_path
        );
    }

    Ok(normalized)
}

fn validate_brand_generated_output(
    context_file: &Path,
    context_name: &str,
    generated_files: &[GeneratedOutputFile],
) -> Result<()> {
    let required_paths = [
        Path::new("Cargo.toml"),
        Path::new("Leptos.toml"),
        Path::new("src/main.rs"),
        Path::new("src/lib.rs"),
    ];
    for required in required_paths {
        if !generated_files.iter().any(|file| file.path == required) {
            anyhow::bail!(
                "Generated brand implementation for '{}' is missing required scaffold file '{}'",
                context_name,
                required.display()
            );
        }
    }

    if !generated_files
        .iter()
        .any(|file| file.path.starts_with("style") && file.path.file_name().is_some())
    {
        anyhow::bail!(
            "Generated brand implementation for '{}' must include at least one file under style/",
            context_name
        );
    }

    let lib_rs = generated_files
        .iter()
        .find(|file| file.path == Path::new("src/lib.rs"))
        .ok_or_else(|| anyhow::anyhow!("Generated brand implementation is missing src/lib.rs"))?;
    if !contains_root_route(&lib_rs.content) {
        anyhow::bail!(
            "Generated brand implementation for '{}' does not define a detectable root route in src/lib.rs",
            context_name
        );
    }

    let combined = generated_files
        .iter()
        .map(|file| file.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let referenced_tokens = collect_brand_token_references(&combined);
    if !referenced_tokens.is_empty() {
        let unresolved = unresolved_brand_token_references(&combined, SPECIFICATIONS_DIR)
            .with_context(|| {
                format!(
                    "failed to validate generated brand token references for {}",
                    context_file.display()
                )
            })?;
        if !unresolved.is_empty() {
            anyhow::bail!(
                "Generated brand implementation for '{}' references undefined brand token(s): {}",
                context_name,
                unresolved.join(", ")
            );
        }
    }

    Ok(())
}

fn contains_root_route(content: &str) -> bool {
    let markers = [
        "path=\"/\"",
        "path = \"/\"",
        "path=path!(\"/\")",
        "path = path!(\"/\")",
        "StaticSegment(\"\")",
    ];
    content.contains("Route") && markers.iter().any(|marker| content.contains(marker))
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

    const MARKER: &str = "ERROR: Cannot implement specification as written.";
    if code.contains(MARKER) {
        return Some(MARKER.to_string());
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

fn extract_actionable_blocking_bullets(section: &str) -> Vec<String> {
    let bullets = extract_bullets_with_indent(section);
    if bullets.is_empty() {
        return Vec::new();
    }

    let mut actionable = vec![false; bullets.len()];
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); bullets.len()];

    for i in 0..bullets.len() {
        actionable[i] = !is_language_or_paradigm_specific_detail(&bullets[i].1)
            && !is_no_issue_placeholder_bullet(&bullets[i].1);
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
        let actionable = extract_actionable_blocking_bullets(&blocking);
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

    let unresolved = check_brand_references_in_spec_content(path, &content)?;
    if !unresolved.is_empty() {
        print_blocking_items(
            path,
            "spec:brand-token",
            &format!(
                "Specification references undefined brand token(s); skipping {} for '{}'.",
                stage_name, context_name
            ),
            &unresolved,
        );
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
    let names_provided = !names.is_empty();
    let names_for_clear = names.clone();
    let context_files = resolve_input_files(SPECIFICATIONS_DIR, names, "md", filter)?;

    if context_files.is_empty() {
        println!("No context files found to process");
        return Ok(());
    }

    // Clear build-tracker entries for tests stage if requested.
    // Note: test generation does not currently use build-tracker caching,
    // but we support the flag for consistency.
    if clear_cache {
        let mut tracker = BuildTracker::load()?;
        clear_tracker_stage(&mut tracker, Stage::Tests, &names_for_clear, config)?;
        if !config.dry_run {
            tracker.save()?;
        }
    }

    let dependency_roots =
        select_dependency_roots(context_files, SPECIFICATIONS_DIR, names_provided, filter)?;
    let execution_levels =
        build_execution_plan(dependency_roots, SPECIFICATIONS_DIR, Some(DRAFTS_DIR))?;
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
            let mut dependency_context = build_dependency_context(&node)?;
            augment_test_generation_context(&context_file, &mut dependency_context)?;
            let context_content = fs::read_to_string(&context_file).unwrap_or_default();
            let (dependency_context, estimated) = fit_context_to_token_limit(
                &executor,
                &context_content,
                dependency_context,
                token_limit,
            )?;
            let cache_hit = executor
                .is_cache_hit(&context_content, dependency_context.clone())
                .unwrap_or(false);
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
        let cfg = *config;
        let executor_clone = executor.clone();
        let results = run_stage_items(
            runnable,
            can_parallel,
            &mut progress,
            &resources,
            config,
            move |(context_file, context_name, dependency_context, context_content),
                  execution_control| {
                let executor = executor_clone.clone();
                async move {
                    process_tests(
                        &executor,
                        &context_content,
                        &context_file,
                        &context_name,
                        &cfg,
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

fn clear_tracker_stage(
    tracker: &mut BuildTracker,
    stage: Stage,
    names: &[String],
    config: &Config,
) -> Result<()> {
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

    let removed = if names.is_empty() {
        tracker.clear_stage(stage)
    } else {
        tracker.clear_stage_names(stage, names)
    };
    let removed_agent_cache_entries = clear_agent_response_cache_for_stage(stage, names, config)?;
    if config.verbose {
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
    }
    Ok(())
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
            "create_specifications_main",
        ],
        Stage::Implementation => &["create_implementation", "create_implementation_brand"],
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
    let names_vec = names.to_vec();
    let mut removed = 0usize;
    let mut candidates: Vec<(String, CacheAgentInput)> = Vec::new();

    match stage {
        Stage::Specification => {
            let files = resolve_input_files(DRAFTS_DIR, names_vec, "md", &CategoryFilter::all())?;
            let levels = build_execution_plan(files, DRAFTS_DIR, None)?;
            for node in levels.into_iter().flatten() {
                let draft_content = fs::read_to_string(&node.input_path).with_context(|| {
                    format!("Failed to read draft file: {}", node.input_path.display())
                })?;
                let additional = build_dependency_context(&node)?;
                let agent_name =
                    determine_specification_agent(&node.input_path, DRAFTS_DIR).to_string();
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
            let files =
                resolve_input_files(SPECIFICATIONS_DIR, names_vec, "md", &CategoryFilter::all())?;
            let levels = build_implementation_execution_plan(files, &CategoryFilter::all())?;
            for node in levels.into_iter().flatten() {
                let context_file = resolve_implementation_context_file(&node.input_path)?;
                let context_content = fs::read_to_string(&context_file).with_context(|| {
                    format!(
                        "Failed to read specification file: {}",
                        context_file.display()
                    )
                })?;
                let mut additional = build_dependency_context(&node)?;
                if let Some(target_type_name) = infer_target_type_name(&context_file)? {
                    additional.insert("target_type_name".to_string(), json!(target_type_name));
                }
                candidates.push((
                    implementation_agent_name(&context_file).to_string(),
                    CacheAgentInput {
                        draft_content: None,
                        context_content: Some(context_content),
                        additional,
                    },
                ));
            }
        }
        Stage::Tests => {
            let files =
                resolve_input_files(SPECIFICATIONS_DIR, names_vec, "md", &CategoryFilter::all())?;
            let levels = build_execution_plan(files, SPECIFICATIONS_DIR, Some(DRAFTS_DIR))?;
            for node in levels.into_iter().flatten() {
                let context_content = fs::read_to_string(&node.input_path).with_context(|| {
                    format!(
                        "Failed to read specification file: {}",
                        node.input_path.display()
                    )
                })?;
                let mut additional = build_dependency_context(&node)?;
                augment_test_generation_context(&node.input_path, &mut additional)?;
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
    config: &Config,
    additional_context: HashMap<String, serde_json::Value>,
    execution_control: Option<CliExecutionControl>,
) -> Result<()> {
    if has_unfinished_specification(context_file, context_name, "tests")? {
        anyhow::bail!("unfinished specification");
    }

    let test_paths = determine_bdd_test_paths(context_file, SPECIFICATIONS_DIR)?;

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
        .execute_with_conversation_with_seed(
            &context_content,
            context_name,
            additional_context,
            execution_control
                .as_ref()
                .map(|control| control as &dyn NativeExecutionControl),
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
    additional_context: &mut HashMap<String, serde_json::Value>,
) -> Result<()> {
    let test_paths = determine_bdd_test_paths(context_file, SPECIFICATIONS_DIR)?;
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
    if let Some(target_type_name) = infer_target_type_name(context_file)? {
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

fn build_dependency_context(node: &ExecutionNode) -> Result<HashMap<String, serde_json::Value>> {
    let mut context = HashMap::new();
    let direct_dependencies = node.resolve_direct_dependencies()?;
    let (primary_root, fallback_root) = if node.input_path.starts_with(SPECIFICATIONS_DIR) {
        (SPECIFICATIONS_DIR, Some(DRAFTS_DIR))
    } else {
        (DRAFTS_DIR, None)
    };
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
    let implemented_dependency_manifest =
        build_implemented_dependency_manifest(&implemented_dependencies, &direct_dependencies);
    context.insert(
        "implemented_dependencies".to_string(),
        json!(implemented_dependency_manifest),
    );
    context.insert(
        "dependency_tool_context".to_string(),
        json!({
            "dependency_artifacts": dependency_closure,
            "implemented_dependency_artifacts": implemented_dependencies,
        }),
    );
    Ok(context)
}

fn build_implementation_execution_plan(
    spec_files: Vec<PathBuf>,
    filter: &CategoryFilter,
) -> Result<Vec<Vec<ExecutionNode>>> {
    let mut draft_inputs = Vec::new();
    for spec_file in spec_files {
        let draft_path = determine_draft_input_path(&spec_file, SPECIFICATIONS_DIR, DRAFTS_DIR)?;
        if draft_path.exists() {
            draft_inputs.push(draft_path);
        } else {
            draft_inputs.push(spec_file);
        }
    }

    let expanded_inputs = expand_with_transitive_dependencies(draft_inputs, DRAFTS_DIR, None)?;
    let filtered_inputs = if filter.is_active() {
        expanded_inputs
            .into_iter()
            .filter(|f| filter.matches_path(f, DRAFTS_DIR))
            .collect()
    } else {
        expanded_inputs
    };
    build_execution_plan(filtered_inputs, DRAFTS_DIR, None)
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
        if is_brand_spec_path(&spec_path, SPECIFICATIONS_DIR) {
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

    let draft_files = resolve_input_files(DRAFTS_DIR, names, "md", &CategoryFilter::all())?;
    let mut removed = 0usize;
    let mut found = 0usize;
    for draft_file in draft_files {
        found += 1;
        let spec_file =
            determine_specification_output_path(&draft_file, DRAFTS_DIR, SPECIFICATIONS_DIR)?;
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
/// 3. visuals/ folder (UI component drafts)
/// 4. Root files (like app.md)
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
        }

        if filter.include_brands() {
            let brands_dir = dir_path.join("brands");
            files.extend(collect_md_files_recursive(&brands_dir, extension)?);
        }

        if filter.include_visuals() {
            let visuals_dir = dir_path.join("visuals");
            files.extend(collect_md_files_recursive(&visuals_dir, extension)?);
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
                    }
                }
            }

            if !found && filter.include_brands() {
                let brand_matches =
                    resolve_named_input_in_category(&dir_path.join("brands"), &name, extension)?;
                if !brand_matches.is_empty() {
                    files.extend(brand_matches);
                    found = true;
                }
            }

            if !found && filter.include_visuals() {
                let visuals_dir = dir_path.join("visuals");
                let mut candidates = Vec::new();
                collect_files_recursive(&visuals_dir, extension, &mut candidates)
                    .context(format!("Failed to scan {}/visuals directory", dir))?;
                for candidate in candidates {
                    if candidate
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .is_some_and(|stem| stem == name)
                    {
                        files.push(candidate);
                        found = true;
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
                let searched = {
                    let mut parts: Vec<&str> = Vec::new();
                    if filter.include_data() {
                        parts.push("data/");
                    }
                    if filter.include_contexts() {
                        parts.push("contexts/");
                    }
                    if filter.include_brands() {
                        parts.push("brands/");
                    }
                    if filter.include_visuals() {
                        parts.push("visuals/");
                    }
                    if filter.include_root() {
                        parts.push("root");
                    }
                    match parts.len() {
                        0 => "no categories".to_string(),
                        1 => parts[0].to_string(),
                        2 => format!("{} and {}", parts[0], parts[1]),
                        _ => {
                            let mut s = String::new();
                            for (i, p) in parts.iter().enumerate() {
                                if i > 0 {
                                    if i == parts.len() - 1 {
                                        s.push_str(" and ");
                                    } else {
                                        s.push_str(", ");
                                    }
                                }
                                s.push_str(p);
                            }
                            s
                        }
                    }
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
        == Some("external_apis")
    {
        let remainder = PathBuf::from_iter(relative_path.components().skip(1));
        return Ok(PathBuf::from(specifications_dir)
            .join("contexts")
            .join("external")
            .join(remainder));
    }

    if relative_path
        .components()
        .next()
        .and_then(|component| component.as_os_str().to_str())
        == Some("brands")
    {
        return Ok(PathBuf::from(specifications_dir).join(relative_path));
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
/// - specifications/contexts/external/X.md → drafts/external_apis/X.md
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
        return Ok(PathBuf::from(drafts_dir)
            .join("external_apis")
            .join(remainder));
    }

    if first == Some("brands") {
        return Ok(PathBuf::from(drafts_dir)
            .join(relative_path)
            .with_extension("md"));
    }

    Ok(PathBuf::from(drafts_dir).join(relative_path))
}

/// Determines which specification agent to use based on file path
///
/// Returns:
/// - "create_specifications_data" for files in data/ folder
/// - "create_specifications_context" for files in contexts/ folder
/// - "create_specifications_external_api" for files in external_apis/ folder
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
            "external_apis" => "create_specifications_external_api",
            "brands" => "create_specifications_brand",
            _ => "create_specifications_main",
        }
    } else {
        // Default to main for root files
        "create_specifications_main"
    }
}

fn infer_target_type_name(spec_file: &Path) -> Result<Option<String>> {
    let rel = match spec_file.strip_prefix(Path::new(SPECIFICATIONS_DIR)) {
        Ok(r) => r.to_path_buf(),
        Err(_) => {
            return Ok(spec_file
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(to_pascal_case_title))
        }
    };

    let draft_path = PathBuf::from(DRAFTS_DIR).join(&rel);
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
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
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

    if relative_path
        .components()
        .next()
        .and_then(|component| component.as_os_str().to_str())
        == Some("brands")
    {
        return Ok(PathBuf::from("Cargo.toml"));
    }

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
        build_dependency_drafts_from_context, build_dependency_manifest,
        build_implemented_dependency_manifest, determine_bdd_test_paths,
        determine_draft_input_path, determine_implementation_output_path,
        determine_specification_output_path, ensure_dev_dependency_entry,
        extract_actionable_blocking_bullets, extract_compile_error_message,
        generated_project_structure_paths, implementation_agent_name, parse_generated_files,
        parse_generated_output_files, resolve_input_files, sync_managed_block,
        validate_brand_generated_output, CategoryFilter, BDD_TEST_TARGETS_END,
        BDD_TEST_TARGETS_START,
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
    fn maps_brand_draft_to_brand_specification_path() {
        let path = determine_specification_output_path(
            Path::new("drafts/brands/acme.md"),
            "drafts",
            "specifications",
        )
        .expect("path mapping");
        assert_eq!(path, Path::new("specifications/brands/acme.md"));
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
    fn maps_brand_specification_back_to_draft_path() {
        let path = determine_draft_input_path(
            Path::new("specifications/brands/acme.md"),
            "specifications",
            "drafts",
        )
        .expect("path mapping");
        assert_eq!(path, Path::new("drafts/brands/acme.md"));
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
    fn determine_implementation_output_path_maps_brand_specs_to_scaffold_tracking_file() {
        let path = determine_implementation_output_path(
            Path::new("specifications/brands/acme.md"),
            "specifications",
        )
        .expect("implementation path");
        assert_eq!(path, Path::new("Cargo.toml"));
    }

    #[test]
    fn implementation_agent_name_routes_brand_specs_to_brand_agent() {
        assert_eq!(
            implementation_agent_name(Path::new("specifications/brands/acme.md")),
            "create_implementation_brand"
        );
        assert_eq!(
            implementation_agent_name(Path::new("specifications/contexts/account.md")),
            "create_implementation"
        );
    }

    #[test]
    fn resolve_input_files_discovers_nested_paths_and_stems() {
        let root = temp_root("nested_inputs");
        let drafts = root.join("drafts");
        fs::create_dir_all(drafts.join("contexts/ui")).expect("mkdir");
        fs::create_dir_all(drafts.join("external_apis")).expect("mkdir");
        fs::create_dir_all(drafts.join("brands")).expect("mkdir");
        fs::create_dir_all(drafts.join("data/payments")).expect("mkdir");
        fs::write(
            drafts.join("contexts/ui/terminal_renderer.md"),
            "# Terminal Renderer",
        )
        .expect("write");
        fs::write(drafts.join("external_apis/stripe.md"), "# Stripe API").expect("write");
        fs::write(drafts.join("brands/acme.md"), "# Acme Brand").expect("write");
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
        assert!(all
            .iter()
            .any(|p| p.ends_with("contexts/ui/terminal_renderer.md")));
        assert!(all.iter().any(|p| p.ends_with("external_apis/stripe.md")));
        assert!(all.iter().any(|p| p.ends_with("brands/acme.md")));
        assert!(all
            .iter()
            .any(|p| p.ends_with("data/payments/ledger_entry.md")));

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
                brands: false,
                visuals: false,
            },
        )
        .expect("external lookup");
        assert_eq!(by_external_name.len(), 1);
        assert!(by_external_name[0].ends_with("external_apis/stripe.md"));

        let by_nested_name = resolve_input_files(
            drafts.to_str().expect("drafts path"),
            vec!["payments/ledger_entry".to_string()],
            "md",
            &CategoryFilter {
                contexts: false,
                data: true,
                brands: false,
                visuals: false,
            },
        )
        .expect("nested lookup");
        assert_eq!(by_nested_name.len(), 1);
        assert!(by_nested_name[0].ends_with("data/payments/ledger_entry.md"));

        let by_brand_name = resolve_input_files(
            drafts.to_str().expect("drafts path"),
            vec!["acme".to_string()],
            "md",
            &CategoryFilter {
                contexts: false,
                data: false,
                brands: true,
                visuals: false,
            },
        )
        .expect("brand lookup");
        assert_eq!(by_brand_name.len(), 1);
        assert!(by_brand_name[0].ends_with("brands/acme.md"));

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
    fn parse_generated_output_files_reads_brand_scaffold_envelope() {
        let output = r#"===FILE: Cargo.toml===
[package]
name = "acme"
===END_FILE===
===FILE: src/lib.rs===
use leptos_router::components::Route;
===END_FILE==="#;

        let files = parse_generated_output_files(output).expect("parse generated output files");
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, PathBuf::from("Cargo.toml"));
        assert!(files[0].content.contains("[package]"));
        assert_eq!(files[1].path, PathBuf::from("src/lib.rs"));
    }

    #[test]
    fn parse_generated_output_files_rejects_path_traversal() {
        let output = r#"===FILE: ../Cargo.toml===
oops
===END_FILE==="#;

        let err =
            parse_generated_output_files(output).expect_err("expected path traversal failure");
        assert!(err.to_string().contains("disallowed path traversal"));
    }

    #[test]
    fn validate_brand_generated_output_requires_scaffold_files_and_root_route() {
        let files = parse_generated_output_files(
            r#"===FILE: Cargo.toml===
[package]
name = "acme"
===END_FILE===
===FILE: Leptos.toml===
output-name = "acme"
===END_FILE===
===FILE: src/main.rs===
fn main() {}
===END_FILE===
===FILE: src/lib.rs===
use leptos::*;
use leptos_router::components::{Route, Router, Routes};

#[component]
pub fn App() -> impl IntoView {
    view! {
        <Router>
            <Routes fallback=|| view! { <main></main> }>
                <Route path="/" view=|| view! { <main class="shell"></main> }/>
            </Routes>
        </Router>
    }
}
===END_FILE===
===FILE: style/app.css===
:root { --brand-colors-primary-default: #112233; }
===END_FILE==="#,
        )
        .expect("parse");

        validate_brand_generated_output(Path::new("specifications/brands/acme.md"), "acme", &files)
            .expect("valid brand generated output");
    }

    #[test]
    fn validate_brand_generated_output_rejects_missing_root_route() {
        let files = parse_generated_output_files(
            r#"===FILE: Cargo.toml===
[package]
name = "acme"
===END_FILE===
===FILE: Leptos.toml===
output-name = "acme"
===END_FILE===
===FILE: src/main.rs===
fn main() {}
===END_FILE===
===FILE: src/lib.rs===
pub fn app() {}
===END_FILE===
===FILE: style/app.css===
:root {}
===END_FILE==="#,
        )
        .expect("parse");

        let err = validate_brand_generated_output(
            Path::new("specifications/brands/acme.md"),
            "acme",
            &files,
        )
        .expect_err("expected missing root route failure");
        assert!(err.to_string().contains("root route"));
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
        let actionable = extract_actionable_blocking_bullets(section);
        assert!(actionable.is_empty());
    }

    #[test]
    fn preserves_real_blockers_while_ignoring_placeholders() {
        let section = r#"
- none
- Missing required role method for game loop construction
"#;
        let actionable = extract_actionable_blocking_bullets(section);
        assert_eq!(actionable.len(), 1);
        assert!(actionable[0].contains("Missing required role method"));
    }
}
