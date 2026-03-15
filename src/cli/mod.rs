use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::env;
use std::future::Future;
use std::pin::Pin;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use tokio::time::sleep;

mod agent_executor;
mod compilation_fix;
mod dependency_graph;
mod progress;
mod project_structure;
mod rate_limiter;
mod token_limiter;

use agent_executor::{AgentExecutor, AgentResponse};
use rate_limiter::RateLimiter;
use token_limiter::{estimate_request_tokens, TokenLimiter};
use dependency_graph::{
    build_execution_plan, expand_with_transitive_dependencies, DependencyArtifact, ExecutionNode,
};
use progress::ProgressIndicator;
use project_structure::{
    analyze_specifications, generate_cargo_toml, generate_lib_rs, generate_mod_files, ProjectInfo,
};
use compilation_fix::apply_draft_patches;
use reen::build_tracker::{BuildTracker, Stage};
use reen::contexts::{AgentModelRegistry, AgentRegistry};
use reen::registries::{FileAgentModelRegistry, FileAgentRegistry};

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
                    "contexts" => self.include_contexts(),
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

/// Returns true if the error indicates a 429 rate limit response.
fn is_rate_limit_error(e: &anyhow::Error) -> bool {
    let s = e.to_string();
    let lower = s.to_lowercase();
    s.contains("429")
        || lower.contains("rate limit")
        || lower.contains("rate_limit")
        || lower.contains("ratelimit")
}

fn estimate_agent_request_tokens(
    executor: &AgentExecutor,
    input: &str,
    additional_context: &HashMap<String, serde_json::Value>,
) -> usize {
    executor
        .estimate_request_tokens(input, additional_context.clone())
        .unwrap_or_else(|_| estimate_request_tokens(input, additional_context))
}

async fn acquire_request_capacity(
    token_limiter: Option<&Arc<TokenLimiter>>,
    rate_limiter: Option<&Arc<RateLimiter>>,
    estimated: usize,
) -> Result<()> {
    if let Some(limiter) = token_limiter {
        if limiter.exceeds_limit(estimated).await {
            anyhow::bail!(
                "Estimated request size ({estimated} input tokens) exceeds configured --token-limit/REEN_TOKEN_LIMIT budget for a single minute. Reduce prompt size or raise the token limit."
            );
        }
        limiter.acquire_tokens(estimated).await;
    }
    if let Some(limiter) = rate_limiter {
        limiter.acquire().await;
    }
    Ok(())
}

async fn prepare_rate_limit_retry(
    item_name: &str,
    estimated: usize,
    token_limiter: Option<&Arc<TokenLimiter>>,
    rate_limiter: Option<&Arc<RateLimiter>>,
) -> bool {
    let mut waited = false;
    if let Some(limiter) = token_limiter {
        let delay = limiter.retry_delay(estimated).await;
        eprintln!(
            "Rate limit (429) exceeded for {}, waiting {}s before retrying...",
            item_name,
            delay.as_secs()
        );
        sleep(delay).await;
        waited = true;
    }
    if let Some(limiter) = rate_limiter {
        if !waited {
            eprintln!(
                "Rate limit (429) exceeded for {}, waiting and retrying with slower rate...",
                item_name
            );
            sleep(limiter.retry_delay()).await;
        }
        limiter.back_off().await;
        waited = true;
    }
    waited
}

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
        clear_tracker_stage(&mut tracker, Stage::Specification, &names_for_clear, &config)?;
    }

    let total_count: usize = execution_levels.iter().map(|level| level.len()).sum();
    println!("Creating specifications for {} draft(s)", total_count);

    let rate_limiter = rate_limit.map(RateLimiter::new).map(Arc::new);
    let token_limiter = token_limit.map(TokenLimiter::new).map(Arc::new);

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

            if can_parallel {
                if config.verbose {
                    println!("Parallel execution enabled for {}", agent_name);
                }
                let mut tasks = Vec::new();
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
                        Ok(c) => c,
                        Err(e) => {
                            progress.complete_item(&draft_name, false);
                            eprintln!("✗ Failed to create specification for {}: {}", draft_name, e);
                            continue;
                        }
                    };

                    let draft_content = fs::read_to_string(&draft_file).unwrap_or_default();
                    let estimated =
                        estimate_agent_request_tokens(&executor, &draft_content, &dependency_context);
                    let cache_hit = executor
                        .is_cache_hit(&draft_content, dependency_context.clone())
                        .unwrap_or(false);
                    if cache_hit {
                        progress.start_item_cached(&draft_name);
                    } else {
                        progress.start_item(&draft_name, Some(estimated));
                    }

                    let cfg = config;
                    let executor_clone = executor.clone();
                    let rate_limiter_clone = rate_limiter.clone();
                    let token_limiter_clone = token_limiter.clone();
                    tasks.push(tokio::task::spawn(async move {
                        if !cache_hit {
                            if let Err(e) = acquire_request_capacity(
                                token_limiter_clone.as_ref(),
                                rate_limiter_clone.as_ref(),
                                estimated,
                            )
                            .await
                            {
                                return (
                                    draft_name,
                                    draft_file,
                                    output_path,
                                    dependency_fingerprint,
                                    Err(e),
                                );
                            }
                        }
                        let mut result = process_specification(
                            &executor_clone,
                            &draft_content,
                            &draft_file,
                            &draft_name,
                            &cfg,
                            dependency_context.clone(),
                        )
                        .await;
                        if let Err(ref e) = result {
                            if is_rate_limit_error(e) {
                                if prepare_rate_limit_retry(
                                    &draft_name,
                                    estimated,
                                    token_limiter_clone.as_ref(),
                                    rate_limiter_clone.as_ref(),
                                )
                                .await
                                {
                                    if let Err(e) = acquire_request_capacity(
                                        token_limiter_clone.as_ref(),
                                        rate_limiter_clone.as_ref(),
                                        estimated,
                                    )
                                    .await
                                    {
                                        return (
                                            draft_name,
                                            draft_file,
                                            output_path,
                                            dependency_fingerprint,
                                            Err(e),
                                        );
                                    }
                                    result = process_specification(
                                        &executor_clone,
                                        &draft_content,
                                        &draft_file,
                                        &draft_name,
                                        &cfg,
                                        dependency_context,
                                    )
                                    .await;
                                }
                            }
                        }
                        (
                            draft_name,
                            draft_file,
                            output_path,
                            dependency_fingerprint,
                            result,
                        )
                    }));
                }

                for task in tasks {
                    let (draft_name, draft_file, output_path, dependency_fingerprint, result) =
                        task.await?;
                    match result {
                        Ok(ProcessSpecOutcome::Success) => {
                            tracker.record(
                                Stage::Specification,
                                &draft_name,
                                &draft_file,
                                &output_path,
                                &dependency_fingerprint,
                            )?;
                            updated_count += 1;
                            updated_in_run.insert(draft_name.clone());
                            progress.complete_item(&draft_name, true);
                            if config.verbose {
                                println!("✓ Successfully created specification for {}", draft_name);
                            }
                        }
                        Ok(ProcessSpecOutcome::BlockingAmbiguities {
                            draft_file: ba_draft_file,
                            draft_name: ba_draft_name,
                            draft_content: ba_draft_content,
                            spec_content: ba_spec_content,
                            actionable: ba_actionable,
                            additional_context: ba_context,
                        }) => {
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
            } else {
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

                    let dependency_context = build_dependency_context(&node)?;
                    let draft_content = fs::read_to_string(&draft_file).unwrap_or_default();
                    let estimated =
                        estimate_agent_request_tokens(&executor, &draft_content, &dependency_context);
                    let cache_hit = executor
                        .is_cache_hit(&draft_content, dependency_context.clone())
                        .unwrap_or(false);
                    if cache_hit {
                        progress.start_item_cached(&draft_name);
                    } else {
                        progress.start_item(&draft_name, Some(estimated));
                        acquire_request_capacity(
                            token_limiter.as_ref(),
                            rate_limiter.as_ref(),
                            estimated,
                        )
                        .await?;
                    }
                    let mut result = process_specification(
                        &executor,
                        &draft_content,
                        &draft_file,
                        &draft_name,
                        &config,
                        dependency_context.clone(),
                    )
                .await;
                if let Err(ref e) = result {
                    if is_rate_limit_error(e) {
                        if prepare_rate_limit_retry(
                            &draft_name,
                            estimated,
                            token_limiter.as_ref(),
                            rate_limiter.as_ref(),
                        )
                        .await
                        {
                            acquire_request_capacity(
                                token_limiter.as_ref(),
                                rate_limiter.as_ref(),
                                estimated,
                            )
                            .await?;
                            result = process_specification(
                                &executor,
                                &draft_content,
                                    &draft_file,
                                    &draft_name,
                                    &config,
                                    dependency_context,
                                )
                                .await;
                            }
                        }
                    }
                    match result {
                        Ok(ProcessSpecOutcome::Success) => {
                            tracker.record(
                                Stage::Specification,
                                &draft_name,
                                &draft_file,
                                &output_path,
                                &dependency_fingerprint,
                            )?;
                            updated_count += 1;
                            updated_in_run.insert(draft_name.clone());
                            progress.complete_item(&draft_name, true);
                            if config.verbose {
                                println!("✓ Successfully created specification for {}", draft_name);
                            }
                        }
                        Ok(ProcessSpecOutcome::BlockingAmbiguities {
                            draft_file: ba_draft_file,
                            draft_name: ba_draft_name,
                            draft_content: ba_draft_content,
                            spec_content: ba_spec_content,
                            actionable: ba_actionable,
                            additional_context: ba_context,
                        }) => {
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
        if let Some(blocking) = extract_blocking_ambiguities_section(&spec_content) {
            let actionable = extract_actionable_blocking_bullets(&blocking);
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
    config: &Config,
    additional_context: HashMap<String, serde_json::Value>,
) -> Result<ProcessSpecOutcome> {
    if config.dry_run {
        println!("[DRY RUN] Would create specification for: {}", draft_name);
        return Ok(ProcessSpecOutcome::Success);
    }

    // Use conversational execution to handle questions
    let spec_content = executor
        .execute_with_conversation_with_seed(&draft_content, draft_name, additional_context.clone())
        .await?;

    // Determine output path preserving folder structure
    let output_path =
        determine_specification_output_path(draft_file, DRAFTS_DIR, SPECIFICATIONS_DIR)?;

    let mut has_blocking_ambiguities = false;
    let mut actionable = Vec::new();

    // Report Blocking Ambiguities immediately if present in generated spec
    if let Some(blocking) = extract_blocking_ambiguities_section(&spec_content) {
        actionable = extract_actionable_blocking_bullets(&blocking);
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
    let closure = context
        .get("dependency_closure")
        .or_else(|| context.get("direct_dependencies"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut map = serde_json::Map::new();
    for item in closure {
        if let Some(obj) = item.as_object() {
            let path = obj.get("path").and_then(|p| p.as_str()).unwrap_or("");
            let content = obj.get("content").and_then(|c| c.as_str()).unwrap_or("");
            if !path.is_empty() && path.starts_with("drafts/") {
                map.insert(path.to_string(), serde_json::Value::String(content.to_string()));
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
        .execute_with_context("", fix_context)
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
    let patched_paths =
        apply_draft_patches(project_root, &patch_output).context("Failed to apply draft patches")?;

    let mut affected_names: HashSet<String> = patched_paths
        .iter()
        .filter_map(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .map(String::from)
        })
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
    clear_tracker_stage(&mut tracker, Stage::Specification, &affected_names_vec, config)?;

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
        context_files,
        SPECIFICATIONS_DIR,
        names_provided,
        filter,
    )?;
    let execution_levels = build_implementation_execution_plan(dependency_roots, filter)?;
    let total_count: usize = execution_levels.iter().map(|level| level.len()).sum();
    println!("Creating implementation for {} context(s)", total_count);

    // Step 1: Generate project structure (Cargo.toml, lib.rs, mod.rs files)
    if config.verbose {
        println!("Generating project structure...");
    }

    let spec_dir = PathBuf::from(SPECIFICATIONS_DIR);
    let drafts_dir = PathBuf::from(DRAFTS_DIR);
    let project_info = analyze_specifications(&spec_dir, Some(&drafts_dir))
        .context("Failed to analyze specifications")?;

    let output_dir = PathBuf::from(".");

    generate_cargo_toml(&project_info, &output_dir).context("Failed to generate Cargo.toml")?;

    generate_lib_rs(&project_info, &output_dir).context("Failed to generate lib.rs")?;

    generate_mod_files(&project_info, &output_dir).context("Failed to generate mod.rs files")?;

    if config.verbose {
        println!("✓ Project structure generated");
    }

    let mut recent_generated_files: Vec<PathBuf> = Vec::new();
    for p in [
        PathBuf::from("Cargo.toml"),
        PathBuf::from("src/lib.rs"),
        PathBuf::from("src/contexts/mod.rs"),
        PathBuf::from("src/data/mod.rs"),
    ] {
        if p.exists() {
            recent_generated_files.push(p);
        }
    }

    // Step 2: Generate individual implementation files
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

    let rate_limiter = rate_limit.map(RateLimiter::new).map(Arc::new);
    let token_limiter = token_limit.map(TokenLimiter::new).map(Arc::new);

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
            let estimated = estimate_agent_request_tokens(
                &executor,
                &context_content,
                &dependency_context,
            );
            let cache_hit = executor
                .is_cache_hit(&context_content, dependency_context.clone())
                .unwrap_or(false);
            runnable.push((
                context_file,
                context_name,
                output_path,
                dependency_fingerprint,
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
            let cfg = *config;
            let mut tasks = Vec::new();
            for (
                context_file,
                context_name,
                output_path,
                dependency_fingerprint,
                dependency_context,
                context_content,
                estimated,
                cache_hit,
            ) in runnable
            {
                if cache_hit {
                    progress.start_item_cached(&context_name);
                } else {
                    progress.start_item(&context_name, Some(estimated));
                }
                let executor_clone = executor.clone();
                let rate_limiter_clone = rate_limiter.clone();
                let token_limiter_clone = token_limiter.clone();
                tasks.push(tokio::task::spawn(async move {
                    if !cache_hit {
                        if let Err(e) = acquire_request_capacity(
                            token_limiter_clone.as_ref(),
                            rate_limiter_clone.as_ref(),
                            estimated,
                        )
                        .await
                        {
                            return (
                                context_name,
                                context_file,
                                output_path,
                                dependency_fingerprint,
                                Err(e),
                            );
                        }
                    }
                    let mut result = process_implementation(
                        &executor_clone,
                        &context_content,
                        &context_file,
                        &context_name,
                        &cfg,
                        dependency_context.clone(),
                    )
                    .await;
                    if let Err(ref e) = result {
                        if is_rate_limit_error(e) {
                            if prepare_rate_limit_retry(
                                &context_name,
                                estimated,
                                token_limiter_clone.as_ref(),
                                rate_limiter_clone.as_ref(),
                            )
                            .await
                            {
                                if let Err(e) = acquire_request_capacity(
                                    token_limiter_clone.as_ref(),
                                    rate_limiter_clone.as_ref(),
                                    estimated,
                                )
                                .await
                                {
                                    return (
                                        context_name,
                                        context_file,
                                        output_path,
                                        dependency_fingerprint,
                                        Err(e),
                                    );
                                }
                                result = process_implementation(
                                    &executor_clone,
                                    &context_content,
                                    &context_file,
                                    &context_name,
                                    &cfg,
                                    dependency_context,
                                )
                                .await;
                            }
                        }
                    }
                    (
                        context_name,
                        context_file,
                        output_path,
                        dependency_fingerprint,
                        result,
                    )
                }));
            }
            for task in tasks {
                let (context_name, context_file, output_path, dependency_fingerprint, result) =
                    task.await?;
                match result {
                    Ok(_) => {
                        tracker.record(
                            Stage::Implementation,
                            &context_name,
                            &context_file,
                            &output_path,
                            &dependency_fingerprint,
                        )?;
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
        } else {
            for (
                context_file,
                context_name,
                output_path,
                dependency_fingerprint,
                dependency_context,
                context_content,
                estimated,
                cache_hit,
            ) in runnable
            {
                if cache_hit {
                    progress.start_item_cached(&context_name);
                } else {
                    progress.start_item(&context_name, Some(estimated));
                    if config.verbose {
                        println!("Processing context: {}", context_name);
                    }
                    acquire_request_capacity(
                        token_limiter.as_ref(),
                        rate_limiter.as_ref(),
                        estimated,
                    )
                    .await?;
                }
                let mut result = process_implementation(
                    &executor,
                    &context_content,
                    &context_file,
                    &context_name,
                    config,
                    dependency_context.clone(),
                )
                .await;
                if let Err(ref e) = result {
                    if is_rate_limit_error(e) {
                        if prepare_rate_limit_retry(
                            &context_name,
                            estimated,
                            token_limiter.as_ref(),
                            rate_limiter.as_ref(),
                        )
                        .await
                        {
                            acquire_request_capacity(
                                token_limiter.as_ref(),
                                rate_limiter.as_ref(),
                                estimated,
                            )
                            .await?;
                            result = process_implementation(
                                &executor,
                                &context_content,
                                &context_file,
                                &context_name,
                                config,
                                dependency_context,
                            )
                            .await;
                        }
                    }
                }
                match result {
                    Ok(_) => {
                        tracker.record(
                            Stage::Implementation,
                            &context_name,
                            &context_file,
                            &output_path,
                            &dependency_fingerprint,
                        )?;
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

    if !config.dry_run {
        match validate_generated_rust_layout(Path::new(".")) {
            Ok(None) => {}
            Ok(Some(issues)) => {
                eprintln!("Warning: Generated implementation layout validation reported issues:");
                for issue in issues {
                    eprintln!("  - {}", issue);
                }
            }
            Err(e) => return Err(e.into()),
        }
    }

    // Automatic compile + bounded auto-fix loop to restore build validity.
    compilation_fix::ensure_compiles_with_auto_fix(
        config,
        max_compile_fix_attempts,
        Path::new("."),
        &project_info,
        &recent_generated_files,
    )
    .await?;

    // Save tracker
    tracker.save()?;

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
        .execute_with_conversation_with_seed(&context_content, context_name, additional_context)
        .await?;

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

    let dependency_roots = select_dependency_roots(
        context_files,
        SPECIFICATIONS_DIR,
        names_provided,
        filter,
    )?;
    let execution_levels =
        build_execution_plan(dependency_roots, SPECIFICATIONS_DIR, Some(DRAFTS_DIR))?;
    let total_count: usize = execution_levels.iter().map(|level| level.len()).sum();
    println!("Creating tests for {} context(s)", total_count);

    let executor = Arc::new(AgentExecutor::new("create_test", config)?);
    let can_parallel = executor.can_run_parallel().unwrap_or(false);

    let rate_limiter = rate_limit.map(RateLimiter::new).map(Arc::new);
    let token_limiter = token_limit.map(TokenLimiter::new).map(Arc::new);

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
            let dependency_context = build_dependency_context(&node)?;
            let context_content = fs::read_to_string(&context_file).unwrap_or_default();
            let estimated = estimate_agent_request_tokens(
                &executor,
                &context_content,
                &dependency_context,
            );
            let cache_hit = executor
                .is_cache_hit(&context_content, dependency_context.clone())
                .unwrap_or(false);
            runnable.push((
                context_file,
                context_name,
                dependency_context,
                context_content,
                estimated,
                cache_hit,
            ));
        }

        if can_parallel {
            if config.verbose {
                println!("Parallel execution enabled for create_test");
            }
            let cfg = *config;
            let mut tasks = Vec::new();
            for (context_file, context_name, dependency_context, context_content, estimated, cache_hit) in runnable {
                if cache_hit {
                    progress.start_item_cached(&context_name);
                } else {
                    progress.start_item(&context_name, Some(estimated));
                }
                let executor_clone = executor.clone();
                let rate_limiter_clone = rate_limiter.clone();
                let token_limiter_clone = token_limiter.clone();
                tasks.push(tokio::task::spawn(async move {
                    if !cache_hit {
                        if let Err(e) = acquire_request_capacity(
                            token_limiter_clone.as_ref(),
                            rate_limiter_clone.as_ref(),
                            estimated,
                        )
                        .await
                        {
                            return (context_name, Err(e));
                        }
                    }
                    let mut result = process_tests(
                        &executor_clone,
                        &context_content,
                        &context_file,
                        &context_name,
                        &cfg,
                        dependency_context.clone(),
                    )
                    .await;
                    if let Err(ref e) = result {
                        if is_rate_limit_error(e) {
                            if prepare_rate_limit_retry(
                                &context_name,
                                estimated,
                                token_limiter_clone.as_ref(),
                                rate_limiter_clone.as_ref(),
                            )
                            .await
                            {
                                if let Err(e) = acquire_request_capacity(
                                    token_limiter_clone.as_ref(),
                                    rate_limiter_clone.as_ref(),
                                    estimated,
                                )
                                .await
                                {
                                    return (context_name, Err(e));
                                }
                                result = process_tests(
                                    &executor_clone,
                                    &context_content,
                                    &context_file,
                                    &context_name,
                                    &cfg,
                                    dependency_context,
                                )
                                .await;
                            }
                        }
                    }
                    (context_name, result)
                }));
            }
            for task in tasks {
                let (context_name, result) = task.await?;
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
        } else {
            for (context_file, context_name, dependency_context, context_content, estimated, cache_hit) in runnable {
                if cache_hit {
                    progress.start_item_cached(&context_name);
                } else {
                    progress.start_item(&context_name, Some(estimated));
                    if config.verbose {
                        println!("Processing context: {}", context_name);
                    }
                    acquire_request_capacity(
                        token_limiter.as_ref(),
                        rate_limiter.as_ref(),
                        estimated,
                    )
                    .await?;
                }
                let mut result = process_tests(
                    &executor,
                    &context_content,
                    &context_file,
                    &context_name,
                    config,
                    dependency_context.clone(),
                )
                .await;
                if let Err(ref e) = result {
                    if is_rate_limit_error(e) {
                        if prepare_rate_limit_retry(
                            &context_name,
                            estimated,
                            token_limiter.as_ref(),
                            rate_limiter.as_ref(),
                        )
                        .await
                        {
                            acquire_request_capacity(
                                token_limiter.as_ref(),
                                rate_limiter.as_ref(),
                                estimated,
                            )
                            .await?;
                            result = process_tests(
                                &executor,
                                &context_content,
                                &context_file,
                                &context_name,
                                config,
                                dependency_context,
                            )
                            .await;
                        }
                    }
                }
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
    }

    progress.finish();
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
            let files = resolve_input_files(SPECIFICATIONS_DIR, names_vec, "md", &CategoryFilter::all())?;
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
            let files = resolve_input_files(SPECIFICATIONS_DIR, names_vec, "md", &CategoryFilter::all())?;
            let levels = build_execution_plan(files, SPECIFICATIONS_DIR, Some(DRAFTS_DIR))?;
            for node in levels.into_iter().flatten() {
                let context_content = fs::read_to_string(&node.input_path).with_context(|| {
                    format!(
                        "Failed to read specification file: {}",
                        node.input_path.display()
                    )
                })?;
                let additional = build_dependency_context(&node)?;
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
) -> Result<()> {
    if has_unfinished_specification(context_file, context_name, "tests")? {
        anyhow::bail!("unfinished specification");
    }

    if config.dry_run {
        println!("[DRY RUN] Would create tests for: {}", context_name);
        return Ok(());
    }

    // Use conversational execution to handle questions
    let test_result = executor
        .execute_with_conversation_with_seed(&context_content, context_name, additional_context)
        .await?;

    if config.verbose {
        println!("Test creation result: {}", test_result);
    }

    Ok(())
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

    // Expose full closure via direct_dependencies so existing agent prompts
    // receive transitive context without prompt/template changes.
    let value = json!(dependency_closure);
    context.insert("direct_dependencies".to_string(), value.clone());
    context.insert(
        "direct_dependencies_only".to_string(),
        json!(direct_dependencies),
    );
    context.insert("dependency_closure".to_string(), value.clone());
    // Backward compatibility with agent prompts that still reference mcp_context
    context.insert("mcp_context".to_string(), value);

    let implemented_dependencies = build_implemented_dependency_context(&dependency_closure)?;
    context.insert(
        "implemented_dependencies".to_string(),
        json!(implemented_dependencies),
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
    println!("Compiling project with cargo build...");

    if config.dry_run {
        println!("[DRY RUN] Would run: cargo build");
        return Ok(());
    }

    let output = Command::new("cargo")
        .arg("build")
        .output()
        .context("Failed to execute cargo build")?;

    if config.verbose || !output.status.success() {
        print!("{}", String::from_utf8_lossy(&output.stdout));
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }

    if output.status.success() {
        println!("✓ Build successful");
        Ok(())
    } else {
        anyhow::bail!("Build failed");
    }
}

pub async fn fix(max_compile_fix_attempts: usize, config: &Config) -> Result<()> {
    println!(
        "Attempting to restore compilation (max_attempts={})...",
        max_compile_fix_attempts
    );

    if config.dry_run {
        println!("[DRY RUN] Would run compilation-fix loop");
        return Ok(());
    }

    let project_root = Path::new(".");
    let spec_dir = PathBuf::from(SPECIFICATIONS_DIR);
    let drafts_dir = PathBuf::from(DRAFTS_DIR);

    let project_info = if spec_dir.exists() && spec_dir.is_dir() {
        analyze_specifications(&spec_dir, Some(&drafts_dir))
            .context("Failed to analyze specifications for fix loop")?
    } else {
        // If specs are missing, still allow the loop to run from compiler diagnostics alone.
        ProjectInfo::default()
    };

    let mut recent_files: Vec<PathBuf> = Vec::new();
    for p in [
        PathBuf::from("Cargo.toml"),
        PathBuf::from("src/lib.rs"),
        PathBuf::from("src/main.rs"),
        PathBuf::from("src/contexts/mod.rs"),
        PathBuf::from("src/data/mod.rs"),
    ] {
        if p.exists() {
            recent_files.push(p);
        }
    }

    compilation_fix::ensure_compiles_with_auto_fix(
        config,
        max_compile_fix_attempts,
        project_root,
        &project_info,
        &recent_files,
    )
    .await
}

pub async fn run(args: Vec<String>, config: &Config) -> Result<()> {
    println!("Building and running project with cargo run...");

    if config.dry_run {
        let args_str = if args.is_empty() {
            String::new()
        } else {
            format!(" -- {}", args.join(" "))
        };
        println!("[DRY RUN] Would run: cargo run{}", args_str);
        return Ok(());
    }

    let mut cmd = Command::new("cargo");
    cmd.arg("run");

    // Add separator and arguments if any were provided
    if !args.is_empty() {
        cmd.arg("--");
        cmd.args(&args);
    }

    let output = cmd.output().context("Failed to execute cargo run")?;

    if config.verbose || !output.status.success() {
        print!("{}", String::from_utf8_lossy(&output.stdout));
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }

    if output.status.success() {
        // Don't print success message if not verbose, as cargo run already shows output
        if config.verbose {
            println!("✓ Run successful");
        }
        Ok(())
    } else {
        anyhow::bail!("Run failed");
    }
}

pub async fn test(config: &Config) -> Result<()> {
    println!("Testing project with cargo test...");

    if config.dry_run {
        println!("[DRY RUN] Would run: cargo test");
        return Ok(());
    }

    let output = Command::new("cargo")
        .arg("test")
        .output()
        .context("Failed to execute cargo test")?;

    if config.verbose || !output.status.success() {
        print!("{}", String::from_utf8_lossy(&output.stdout));
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }

    if output.status.success() {
        println!("✓ Tests passed");
        Ok(())
    } else {
        anyhow::bail!("Tests failed");
    }
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
        remove_dir_if_empty(Path::new("src/data"))?;
        remove_dir_if_empty(Path::new("src/contexts"))?;
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
        let Some(stem) = spec_file.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        candidates.push(PathBuf::from("tests").join(format!("{}.rs", stem)));
        candidates.push(PathBuf::from("tests").join(format!("{}_test.rs", stem)));
        candidates.push(PathBuf::from("tests/generated").join(format!("{}.rs", stem)));
        candidates.push(PathBuf::from("tests/generated").join(format!("{}_test.rs", stem)));
    }

    let mut removed = 0usize;
    for file in candidates {
        if file.exists() {
            if config.dry_run {
                println!("[DRY RUN] Would remove {}", file.display());
            } else {
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
        remove_dir_if_empty(Path::new("tests/generated"))?;
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
            if data_dir.exists() && data_dir.is_dir() {
                let entries = fs::read_dir(&data_dir)
                    .context(format!("Failed to read {}/data directory", dir))?;
                for entry in entries {
                    let entry = entry?;
                    let path = entry.path();
                    if path.is_file()
                        && path.extension().and_then(|s| s.to_str()) == Some(extension)
                    {
                        files.push(path);
                    }
                }
            }
        }

        if filter.include_contexts() {
            let contexts_dir = dir_path.join("contexts");
            if contexts_dir.exists() && contexts_dir.is_dir() {
                let entries = fs::read_dir(&contexts_dir)
                    .context(format!("Failed to read {}/contexts directory", dir))?;
                for entry in entries {
                    let entry = entry?;
                    let path = entry.path();
                    if path.is_file()
                        && path.extension().and_then(|s| s.to_str()) == Some(extension)
                    {
                        files.push(path);
                    }
                }
            }
        }

        if filter.include_root() {
            let entries =
                fs::read_dir(&dir_path).context(format!("Failed to read {} directory", dir))?;
            for entry in entries {
                let entry = entry?;
                let path = entry.path();
                if path.is_file()
                    && path.extension().and_then(|s| s.to_str()) == Some(extension)
                {
                    files.push(path);
                }
            }
        }

        Ok(files)
    } else {
        let mut files = Vec::new();
        for name in names {
            let mut found = false;

            if filter.include_data() {
                let data_path = dir_path
                    .join("data")
                    .join(format!("{}.{}", name, extension));
                if data_path.exists() {
                    files.push(data_path);
                    found = true;
                }
            }

            if !found && filter.include_contexts() {
                let contexts_path = dir_path
                    .join("contexts")
                    .join(format!("{}.{}", name, extension));
                if contexts_path.exists() {
                    files.push(contexts_path);
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
                let searched = match (filter.include_data(), filter.include_contexts(), filter.include_root()) {
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

    // Build output path in specifications directory
    let output_path = PathBuf::from(specifications_dir).join(relative_path);
    Ok(output_path)
}

/// Determines the draft input path preserving folder structure
///
/// Maps:
/// - specifications/data/X.md → drafts/data/X.md
/// - specifications/contexts/X.md → drafts/contexts/X.md
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

    Ok(PathBuf::from(drafts_dir).join(relative_path))
}

/// Determines which specification agent to use based on file path
///
/// Returns:
/// - "create_specifications_data" for files in data/ folder
/// - "create_specifications_context" for files in contexts/ folder
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

/// Returns Ok(None) when validation passes, Ok(Some(issues)) when there are layout issues (warning),
/// or Err for I/O errors.
fn validate_generated_rust_layout(project_root: &Path) -> Result<Option<Vec<String>>> {
    let src_dir = project_root.join("src");
    if !src_dir.exists() {
        return Ok(None);
    }

    let mut issues = Vec::new();
    let mut needs_base64 = false;
    let mut needs_sha2 = false;

    for module_dir in [src_dir.join("data"), src_dir.join("contexts")] {
        let mod_rs = module_dir.join("mod.rs");
        if mod_rs.exists() {
            validate_mod_exports(&mod_rs, &mut issues)?;
        }
    }

    let rust_files = collect_rust_files(&src_dir)?;
    for file in rust_files {
        let content = fs::read_to_string(&file)
            .with_context(|| format!("Failed to read generated source: {}", file.display()))?;

        if content.contains("crate::types::") {
            issues.push(format!(
                "{} uses `crate::types::...`; project structure uses `crate::data`/`crate::contexts`.",
                file.display()
            ));
        }

        if content.contains("base64::") {
            needs_base64 = true;
        }
        if content.contains("sha2::") {
            needs_sha2 = true;
        }
    }

    let cargo_toml = project_root.join("Cargo.toml");
    if cargo_toml.exists() {
        let cargo_content = fs::read_to_string(&cargo_toml)
            .with_context(|| format!("Failed to read {}", cargo_toml.display()))?;
        if needs_base64 && !cargo_content.contains("\nbase64") {
            issues.push(
                "Cargo.toml is missing dependency `base64` while generated code references it."
                    .to_string(),
            );
        }
        if needs_sha2 && !cargo_content.contains("\nsha2") {
            issues.push(
                "Cargo.toml is missing dependency `sha2` while generated code references it."
                    .to_string(),
            );
        }
    }

    if issues.is_empty() {
        return Ok(None);
    }

    Ok(Some(issues))
}

fn validate_mod_exports(mod_file: &Path, issues: &mut Vec<String>) -> Result<()> {
    let content = fs::read_to_string(mod_file)
        .with_context(|| format!("Failed to read {}", mod_file.display()))?;
    let Some(parent) = mod_file.parent() else {
        return Ok(());
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if !(trimmed.starts_with("pub use ") && trimmed.ends_with(';')) {
            continue;
        }
        let path = trimmed
            .trim_start_matches("pub use ")
            .trim_end_matches(';')
            .trim();
        let Some((module_name, type_name)) = path.split_once("::") else {
            continue;
        };

        let module_file = parent.join(format!("{}.rs", module_name));
        if !module_file.exists() {
            issues.push(format!(
                "{} exports `{}` but module file {} does not exist.",
                mod_file.display(),
                path,
                module_file.display()
            ));
            continue;
        }

        let module_content = fs::read_to_string(&module_file)
            .with_context(|| format!("Failed to read {}", module_file.display()))?;
        let candidates = [
            format!("pub struct {}", type_name),
            format!("pub enum {}", type_name),
            format!("pub type {}", type_name),
            format!("pub trait {}", type_name),
        ];
        if !candidates
            .iter()
            .any(|needle| module_content.contains(needle))
        {
            issues.push(format!(
                "{} exports `{}` but {} does not declare a matching public type.",
                mod_file.display(),
                path,
                module_file.display()
            ));
        }
    }

    Ok(())
}

fn collect_rust_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !root.exists() {
        return Ok(files);
    }

    for entry in fs::read_dir(root).with_context(|| format!("Failed to read {}", root.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            files.extend(collect_rust_files(&path)?);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            files.push(path);
        }
    }

    Ok(files)
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
    let context_path = context_file.to_path_buf();
    let specifications_path = PathBuf::from(specifications_dir);

    // Get relative path from specifications directory by comparing components
    let relative_path = match context_path.strip_prefix(&specifications_path) {
        Ok(rel) => rel.to_path_buf(),
        Err(_) => {
            // If strip_prefix fails, try component-based approach
            let context_components: Vec<_> = context_path.components().collect();
            let specifications_components: Vec<_> = specifications_path.components().collect();

            // Check if context_path starts with specifications_path components
            if context_components.len() > specifications_components.len()
                && context_components
                    .iter()
                    .zip(specifications_components.iter())
                    .all(|(a, b)| a == b)
            {
                // Build path from remaining components
                PathBuf::from_iter(
                    context_components
                        .iter()
                        .skip(specifications_components.len()),
                )
            } else {
                // Use string-based fallback
                let context_str = context_file.to_str().unwrap_or("");
                let specifications_str = specifications_dir;
                if context_str.starts_with(specifications_str) {
                    let rel_str = &context_str[specifications_str.len()..].trim_start_matches('/');
                    PathBuf::from(rel_str)
                } else {
                    // Just use the filename
                    context_path
                        .file_name()
                        .map(|n| PathBuf::from(n))
                        .unwrap_or_else(|| PathBuf::from(""))
                }
            }
        }
    };

    // Determine output directory based on source folder
    // Check if relative_path starts with "data" or "contexts" by looking at first component
    let output_dir = if let Some(first_comp) = relative_path.components().next() {
        if let Some(comp_str) = first_comp.as_os_str().to_str() {
            match comp_str {
                "data" => PathBuf::from("src/data"),
                "contexts" => PathBuf::from("src/contexts"),
                _ => PathBuf::from("src"),
            }
        } else {
            PathBuf::from("src")
        }
    } else {
        PathBuf::from("src")
    };

    // Get the filename and change extension to .rs
    let file_stem = relative_path
        .file_stem()
        .and_then(|s| s.to_str())
        .context("Invalid context filename")?;

    // Special case: app.md → main.rs
    let output_filename = if file_stem.eq_ignore_ascii_case("app") {
        "main.rs"
    } else {
        &format!("{}.rs", file_stem.to_ascii_lowercase())
    };

    let output_path = output_dir.join(output_filename);
    Ok(output_path)
}

#[cfg(test)]
mod tests {
    use super::{
        determine_specification_output_path, extract_actionable_blocking_bullets,
        extract_compile_error_message,
    };
    use std::path::Path;

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
