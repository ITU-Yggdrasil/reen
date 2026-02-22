use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::fs;
use std::process::Command;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

mod agent_executor;
mod dependency_graph;
mod progress;
mod project_structure;

use agent_executor::AgentExecutor;
use dependency_graph::{build_execution_plan, ExecutionNode};
use progress::ProgressIndicator;
use project_structure::{analyze_specifications, generate_cargo_toml, generate_lib_rs, generate_mod_files};
use reen::build_tracker::{BuildTracker, Stage};

#[derive(Clone, Copy)]
pub struct Config {
    pub verbose: bool,
    pub dry_run: bool,
}

const DRAFTS_DIR: &str = "drafts";
const SPECIFICATIONS_DIR: &str = "specifications";

pub async fn create_specification(names: Vec<String>, config: &Config) -> Result<()> {
    let draft_files = resolve_input_files(DRAFTS_DIR, names, "md")?;

    if draft_files.is_empty() {
        println!("No draft files found to process");
        return Ok(());
    }

    let execution_levels = build_execution_plan(draft_files, DRAFTS_DIR, None)?;

    // Load build tracker
    let mut tracker = BuildTracker::load()?;

    let total_count: usize = execution_levels.iter().map(|level| level.len()).sum();
    println!("Creating specifications for {} draft(s)", total_count);

    let mut progress = ProgressIndicator::new(total_count);
    let mut updated_count = 0;
    let mut updated_in_run: HashSet<String> = HashSet::new();
    let mut executors: HashMap<String, Arc<AgentExecutor>> = HashMap::new();

    for (level_idx, level_nodes) in execution_levels.into_iter().enumerate() {
        if config.verbose {
            println!("Processing dependency level {} ({} item(s))", level_idx, level_nodes.len());
        }

        let mut nodes_by_agent: HashMap<String, Vec<ExecutionNode>> = HashMap::new();
        for node in level_nodes {
            let agent = determine_specification_agent(&node.input_path, DRAFTS_DIR).to_string();
            nodes_by_agent.entry(agent).or_default().push(node);
        }

        for (agent_name, nodes) in nodes_by_agent {
            if !executors.contains_key(&agent_name) {
                executors.insert(agent_name.clone(), Arc::new(AgentExecutor::new(&agent_name, config)?));
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
                    let output_path =
                        determine_specification_output_path(&draft_file, DRAFTS_DIR, SPECIFICATIONS_DIR)?;
                    progress.start_item(&draft_name);

                    let needs_update = if dependency_invalidated {
                        true
                    } else {
                        tracker.needs_update(
                            Stage::Specification,
                            &draft_name,
                            &draft_file,
                            &output_path,
                        )?
                    };
                    if !needs_update {
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

                    let cfg = *config;
                    let executor_clone = executor.clone();
                    tasks.push(tokio::task::spawn(async move {
                        let result = process_specification(
                            &executor_clone,
                            &draft_file,
                            &draft_name,
                            &cfg,
                            dependency_context,
                        )
                        .await;
                        (draft_name, draft_file, output_path, result)
                    }));
                }

                for task in tasks {
                    let (draft_name, draft_file, output_path, result) = task.await?;
                    match result {
                        Ok(_) => {
                            tracker.record(Stage::Specification, &draft_name, &draft_file, &output_path)?;
                            updated_count += 1;
                            updated_in_run.insert(draft_name.clone());
                            progress.complete_item(&draft_name, true);
                            if config.verbose {
                                println!("✓ Successfully created specification for {}", draft_name);
                            }
                        }
                        Err(e) => {
                            progress.complete_item(&draft_name, false);
                            eprintln!("✗ Failed to create specification for {}: {}", draft_name, e);
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
                    let output_path =
                        determine_specification_output_path(&draft_file, DRAFTS_DIR, SPECIFICATIONS_DIR)?;

                    progress.start_item(&draft_name);
                    let needs_update = if dependency_invalidated {
                        true
                    } else {
                        tracker.needs_update(
                            Stage::Specification,
                            &draft_name,
                            &draft_file,
                            &output_path,
                        )?
                    };
                    if !needs_update {
                        if config.verbose {
                            println!("⊚ Skipping {} (up to date)", draft_name);
                        }
                        progress.complete_item(&draft_name, true);
                        continue;
                    }

                    let dependency_context = build_dependency_context(&node)?;
                    match process_specification(
                        &executor,
                        &draft_file,
                        &draft_name,
                        config,
                        dependency_context,
                    )
                    .await
                    {
                        Ok(_) => {
                            tracker.record(Stage::Specification, &draft_name, &draft_file, &output_path)?;
                            updated_count += 1;
                            updated_in_run.insert(draft_name.clone());
                            progress.complete_item(&draft_name, true);
                            if config.verbose {
                                println!("✓ Successfully created specification for {}", draft_name);
                            }
                        }
                        Err(e) => {
                            progress.complete_item(&draft_name, false);
                            eprintln!("✗ Failed to create specification for {}: {}", draft_name, e);
                        }
                    }
                }
            }
        }
    }

    if !config.dry_run {
        validate_generated_rust_layout(Path::new("."))?;
    }

    // Save tracker
    tracker.save()?;

    progress.finish();

    if updated_count == 0 && config.verbose {
        println!("All specifications are up to date");
    }

    Ok(())
}

pub async fn check_specification(names: Vec<String>, _config: &Config) -> Result<()> {
    let draft_files = resolve_input_files(DRAFTS_DIR, names, "md")?;
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

        let spec_content = fs::read_to_string(&spec_path)
            .with_context(|| format!("Failed to read specification file: {}", spec_path.display()))?;
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
    draft_file: &Path,
    draft_name: &str,
    config: &Config,
    additional_context: HashMap<String, serde_json::Value>,
) -> Result<()> {
    let draft_content = fs::read_to_string(draft_file)
        .context("Failed to read draft file")?;

    if config.dry_run {
        println!("[DRY RUN] Would create specification for: {}", draft_name);
        return Ok(());
    }

    // Use conversational execution to handle questions
    let spec_content = executor
        .execute_with_conversation_with_seed(&draft_content, draft_name, additional_context)
        .await?;

    // Determine output path preserving folder structure
    let output_path = determine_specification_output_path(draft_file, DRAFTS_DIR, SPECIFICATIONS_DIR)?;

    let mut has_blocking_ambiguities = false;

    // Report Blocking Ambiguities immediately if present in generated spec
    if let Some(blocking) = extract_blocking_ambiguities_section(&spec_content) {
        let actionable = extract_actionable_blocking_bullets(&blocking);
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
            for bullet in actionable {
                eprintln!("  {}", bullet);
            }
            eprintln!();
        }
    }
    
    // Ensure the output directory exists
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .context("Failed to create specification output directory")?;
    }
    
    fs::write(&output_path, spec_content)
        .context("Failed to write specification file")?;

    if has_blocking_ambiguities {
        anyhow::bail!("generated specification contains blocking ambiguities");
    }

    Ok(())
}

pub async fn create_implementation(names: Vec<String>, config: &Config) -> Result<()> {
    let context_files = resolve_input_files(SPECIFICATIONS_DIR, names, "md")?;

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

    let execution_levels = build_execution_plan(
        context_files,
        SPECIFICATIONS_DIR,
        Some(DRAFTS_DIR),
    )?;
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

    generate_cargo_toml(&project_info, &output_dir)
        .context("Failed to generate Cargo.toml")?;

    generate_lib_rs(&project_info, &output_dir)
        .context("Failed to generate lib.rs")?;

    generate_mod_files(&project_info, &output_dir)
        .context("Failed to generate mod.rs files")?;

    if config.verbose {
        println!("✓ Project structure generated");
    }

    // Step 2: Generate individual implementation files
    let executor = Arc::new(AgentExecutor::new("create_implementation", config)?);
    let can_parallel = executor.can_run_parallel().unwrap_or(false);

    let mut progress = ProgressIndicator::new(total_count);
    let mut updated_count = 0;
    let mut updated_in_run: HashSet<String> = HashSet::new();
    let mut had_unspecified = false;
    for (level_idx, level_nodes) in execution_levels.into_iter().enumerate() {
        if config.verbose {
            println!("Processing dependency level {} ({} item(s))", level_idx, level_nodes.len());
        }

        let mut runnable = Vec::new();
        for node in level_nodes {
            let context_file = node.input_path.clone();
            let context_name = node.name.clone();
            let dependency_invalidated = node
                .direct_dependency_names()
                .iter()
                .any(|dep_name| updated_in_run.contains(dep_name));
            let output_path = determine_implementation_output_path(&context_file, SPECIFICATIONS_DIR)?;
            progress.start_item(&context_name);

            if has_unfinished_specification(&context_file, &context_name, "implementation")? {
                had_unspecified = true;
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
                )?
            };

            if !needs_update {
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
            runnable.push((context_file, context_name, output_path, dependency_context));
        }

        if can_parallel {
            if config.verbose {
                println!("Parallel execution enabled for create_implementation");
            }
            let cfg = *config;
            let mut tasks = Vec::new();
            for (context_file, context_name, output_path, dependency_context) in runnable {
                let executor_clone = executor.clone();
                tasks.push(tokio::task::spawn(async move {
                    let result = process_implementation(
                        &executor_clone,
                        &context_file,
                        &context_name,
                        &cfg,
                        dependency_context,
                    )
                    .await;
                    (context_name, context_file, output_path, result)
                }));
            }
            for task in tasks {
                let (context_name, context_file, output_path, result) = task.await?;
                match result {
                    Ok(_) => {
                        tracker.record(Stage::Implementation, &context_name, &context_file, &output_path)?;
                        updated_count += 1;
                        updated_in_run.insert(context_name.clone());
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
                        eprintln!("✗ Failed to create implementation for {}: {}", context_name, e);
                    }
                }
            }
        } else {
            for (context_file, context_name, output_path, dependency_context) in runnable {
                if config.verbose {
                    println!("Processing context: {}", context_name);
                }
                match process_implementation(
                    &executor,
                    &context_file,
                    &context_name,
                    config,
                    dependency_context,
                )
                .await
                {
                    Ok(_) => {
                        tracker.record(Stage::Implementation, &context_name, &context_file, &output_path)?;
                        updated_count += 1;
                        updated_in_run.insert(context_name.clone());
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
                        eprintln!("✗ Failed to create implementation for {}: {}", context_name, e);
                    }
                }
            }
        }
    }

    if !config.dry_run {
        validate_generated_rust_layout(Path::new("."))?;
    }

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
    context_file: &Path,
    context_name: &str,
    config: &Config,
    additional_context: HashMap<String, serde_json::Value>,
) -> Result<()> {
    if has_unfinished_specification(context_file, context_name, "implementation")? {
        anyhow::bail!("unfinished specification");
    }

    let context_content = fs::read_to_string(context_file)
        .context("Failed to read context file")?;

    if config.dry_run {
        println!("[DRY RUN] Would create implementation for: {}", context_name);
        return Ok(());
    }

    // Use conversational execution to handle questions
    let impl_result = executor
        .execute_with_conversation_with_seed(&context_content, context_name, additional_context)
        .await?;

    // Extract code from the agent output and write to file
    // The agent output may contain markdown code blocks or raw code
    let code = extract_code_from_output(&impl_result, context_name);
    
    // Determine output path preserving folder structure
    let output_path = determine_implementation_output_path(context_file, SPECIFICATIONS_DIR)?;
    
    // Ensure the output directory exists
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .context("Failed to create implementation output directory")?;
    }
    
    // Write the implementation file
    fs::write(&output_path, code)
        .context("Failed to write implementation file")?;

    if config.verbose {
        println!("✓ Written implementation to: {}", output_path.display());
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
            if line.contains("pub struct") || line.contains("impl ") || line.contains("fn ") || line.contains("mod ") {
                return lines[i..].join("\n").trim().to_string();
            }
        }
    }
    
    // Fallback: return the entire output trimmed
    trimmed.to_string()
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
    if section.is_empty() { None } else { Some(section) }
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

fn extract_actionable_blocking_bullets(section: &str) -> Vec<String> {
    let bullets = extract_bullets_with_indent(section);
    if bullets.is_empty() {
        return Vec::new();
    }

    let mut actionable = vec![false; bullets.len()];
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); bullets.len()];

    for i in 0..bullets.len() {
        actionable[i] = !is_language_or_paradigm_specific_detail(&bullets[i].1);
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

pub async fn create_tests(names: Vec<String>, config: &Config) -> Result<()> {
    let context_files = resolve_input_files(SPECIFICATIONS_DIR, names, "md")?;

    if context_files.is_empty() {
        println!("No context files found to process");
        return Ok(());
    }

    let execution_levels = build_execution_plan(
        context_files,
        SPECIFICATIONS_DIR,
        Some(DRAFTS_DIR),
    )?;
    let total_count: usize = execution_levels.iter().map(|level| level.len()).sum();
    println!("Creating tests for {} context(s)", total_count);

    let executor = Arc::new(AgentExecutor::new("create_test", config)?);
    let can_parallel = executor.can_run_parallel().unwrap_or(false);

    let mut progress = ProgressIndicator::new(total_count);
    let mut had_unspecified = false;
    for (level_idx, level_nodes) in execution_levels.into_iter().enumerate() {
        if config.verbose {
            println!("Processing dependency level {} ({} item(s))", level_idx, level_nodes.len());
        }

        let mut runnable = Vec::new();
        for node in level_nodes {
            let context_file = node.input_path.clone();
            let context_name = node.name.clone();
            let dependency_context = build_dependency_context(&node)?;
            progress.start_item(&context_name);
            runnable.push((context_file, context_name, dependency_context));
        }

        if can_parallel {
            if config.verbose {
                println!("Parallel execution enabled for create_test");
            }
            let cfg = *config;
            let mut tasks = Vec::new();
            for (context_file, context_name, dependency_context) in runnable {
                let executor_clone = executor.clone();
                tasks.push(tokio::task::spawn(async move {
                    let result = process_tests(
                        &executor_clone,
                        &context_file,
                        &context_name,
                        &cfg,
                        dependency_context,
                    )
                    .await;
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
            for (context_file, context_name, dependency_context) in runnable {
                if config.verbose {
                    println!("Processing context: {}", context_name);
                }
                match process_tests(
                    &executor,
                    &context_file,
                    &context_name,
                    config,
                    dependency_context,
                )
                .await
                {
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

async fn process_tests(
    executor: &AgentExecutor,
    context_file: &Path,
    context_name: &str,
    config: &Config,
    additional_context: HashMap<String, serde_json::Value>,
) -> Result<()> {
    if has_unfinished_specification(context_file, context_name, "tests")? {
        anyhow::bail!("unfinished specification");
    }

    let context_content = fs::read_to_string(context_file)
        .context("Failed to read context file")?;

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
    let dependencies = node.resolve_direct_dependencies()?;
    let value = json!(dependencies);
    context.insert("direct_dependencies".to_string(), value.clone());
    // Backward compatibility with agent prompts that still reference mcp_context
    context.insert("mcp_context".to_string(), value);
    Ok(context)
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

    let output = cmd.output()
        .context("Failed to execute cargo run")?;

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
        println!("✓ Removed specification artifacts at {}", specs_dir.display());
        return Ok(());
    }

    let spec_files = resolve_input_files(SPECIFICATIONS_DIR, names, "md")?;
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
        println!("[DRY RUN] Would remove {} specification artifact file(s)", removed);
    } else {
        println!("✓ Removed {} specification artifact file(s)", removed);
    }
    if found == 0 {
        println!("No matching names were resolved in {}", specs_dir.display());
    }
    Ok(())
}

fn clear_implementation_artifacts(names: Vec<String>, config: &Config) -> Result<()> {
    let spec_files = resolve_input_files(SPECIFICATIONS_DIR, names, "md")?;
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
            println!("[DRY RUN] Would remove {} implementation artifact file(s)", removed);
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
    let spec_files = resolve_input_files(SPECIFICATIONS_DIR, names, "md")?;
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
    let mut entries = fs::read_dir(path)
        .with_context(|| format!("Failed to inspect {}", path.display()))?;
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
fn resolve_input_files(dir: &str, names: Vec<String>, extension: &str) -> Result<Vec<PathBuf>> {
    let dir_path = PathBuf::from(dir);

    if !dir_path.exists() {
        return Ok(Vec::new());
    }

    if names.is_empty() {
        // Process files in order: data/, contexts/, then root
        let mut files = Vec::new();

        // 1. Process data/ folder first
        let data_dir = dir_path.join("data");
        if data_dir.exists() && data_dir.is_dir() {
            let entries = fs::read_dir(&data_dir)
                .context(format!("Failed to read {}/data directory", dir))?;
            for entry in entries {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some(extension) {
                    files.push(path);
                }
            }
        }

        // 2. Process contexts/ folder second
        let contexts_dir = dir_path.join("contexts");
        if contexts_dir.exists() && contexts_dir.is_dir() {
            let entries = fs::read_dir(&contexts_dir)
                .context(format!("Failed to read {}/contexts directory", dir))?;
            for entry in entries {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some(extension) {
                    files.push(path);
                }
            }
        }

        // 3. Process root files last
        let entries = fs::read_dir(&dir_path)
            .context(format!("Failed to read {} directory", dir))?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            // Only include files (not directories) with the correct extension
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some(extension) {
                files.push(path);
            }
        }

        Ok(files)
    } else {
        // When specific names are provided, search in order: data/, contexts/, then root
        let mut files = Vec::new();
        for name in names {
            // Try data/ folder first
            let data_path = dir_path.join("data").join(format!("{}.{}", name, extension));
            if data_path.exists() {
                files.push(data_path);
                continue;
            }

            // Try contexts/ folder second
            let contexts_path = dir_path.join("contexts").join(format!("{}.{}", name, extension));
            if contexts_path.exists() {
                files.push(contexts_path);
                continue;
            }

            // Try root folder last
            let root_path = dir_path.join(format!("{}.{}", name, extension));
            if root_path.exists() {
                files.push(root_path);
            } else {
                eprintln!("Warning: File not found: {}.{} (searched in data/, contexts/, and root)", name, extension);
            }
        }
        Ok(files)
    }
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
                && draft_components.iter().zip(drafts_components.iter()).all(|(a, b)| a == b) {
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
                    draft_path.file_name()
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
    let relative_path = draft_path.strip_prefix(&drafts_path)
        .unwrap_or(draft_file);

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
        Err(_) => return Ok(spec_file
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(to_pascal_case_title)),
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
    if out.is_empty() { None } else { Some(out) }
}

fn validate_generated_rust_layout(project_root: &Path) -> Result<()> {
    let src_dir = project_root.join("src");
    if !src_dir.exists() {
        return Ok(());
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
            issues.push("Cargo.toml is missing dependency `base64` while generated code references it.".to_string());
        }
        if needs_sha2 && !cargo_content.contains("\nsha2") {
            issues.push("Cargo.toml is missing dependency `sha2` while generated code references it.".to_string());
        }
    }

    if issues.is_empty() {
        return Ok(());
    }

    let mut msg = String::from("Generated implementation layout validation failed:\n");
    for issue in issues {
        msg.push_str(&format!("  - {}\n", issue));
    }
    anyhow::bail!(msg.trim_end().to_string())
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
        if !candidates.iter().any(|needle| module_content.contains(needle)) {
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

    for entry in fs::read_dir(root)
        .with_context(|| format!("Failed to read {}", root.display()))?
    {
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
                && context_components.iter().zip(specifications_components.iter()).all(|(a, b)| a == b) {
                // Build path from remaining components
                PathBuf::from_iter(context_components.iter().skip(specifications_components.len()))
            } else {
                // Use string-based fallback
                let context_str = context_file.to_str().unwrap_or("");
                let specifications_str = specifications_dir;
                if context_str.starts_with(specifications_str) {
                    let rel_str = &context_str[specifications_str.len()..].trim_start_matches('/');
                    PathBuf::from(rel_str)
                } else {
                    // Just use the filename
                    context_path.file_name()
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
    let file_stem = relative_path.file_stem()
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
