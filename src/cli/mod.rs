use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::fs;
use std::process::Command;
use regex;

mod agent_executor;
mod progress;
mod project_structure;

use agent_executor::AgentExecutor;
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

    // Load build tracker
    let mut tracker = BuildTracker::load()?;

    println!("Creating specifications for {} draft(s)", draft_files.len());

    let mut progress = ProgressIndicator::new(draft_files.len());
    let mut updated_count = 0;

    // Group files by agent type and collect files that need processing
    use std::collections::HashMap;
    let mut files_by_agent: HashMap<&str, Vec<(PathBuf, String, PathBuf)>> = HashMap::new();

    for draft_file in draft_files {
        let draft_name = draft_file.file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .context("Invalid draft filename")?;

        // Determine output path preserving folder structure
        let output_path = determine_specification_output_path(&draft_file, DRAFTS_DIR, SPECIFICATIONS_DIR)?;

        // Check if update is needed
        let needs_update = tracker.needs_update(Stage::Specification, &draft_name, &draft_file, &output_path)?;

        if !needs_update {
            if config.verbose {
                println!("⊚ Skipping {} (up to date)", draft_name);
            }
            continue;
        }

        // Determine which agent to use based on file path
        let agent_name = determine_specification_agent(&draft_file, DRAFTS_DIR);
        files_by_agent.entry(agent_name)
            .or_insert_with(Vec::new)
            .push((draft_file, draft_name, output_path));
    }

    // Process files grouped by agent type
    for (agent_name, files_to_process) in files_by_agent {
        if files_to_process.is_empty() {
            continue;
        }

        if config.verbose {
            println!("Using agent '{}' for {} file(s)", agent_name, files_to_process.len());
        }

        let executor = AgentExecutor::new(agent_name, config)?;
        let can_parallel = executor.model_registry()
            .can_run_parallel(agent_name)
            .unwrap_or(false);

        if can_parallel && config.verbose {
            println!("Parallel execution enabled for {}", agent_name);
        }

    if can_parallel && !files_to_process.is_empty() {
        // Process files in parallel
        use tokio::task;
        use std::sync::Arc;
        
        let executor_arc = Arc::new(executor);
        let config_clone = *config;
        
        let tasks: Vec<_> = files_to_process.into_iter().map(|(draft_file, draft_name, output_path)| {
            let executor = executor_arc.clone();
            let config = config_clone;
            let draft_file_clone = draft_file.clone();
            let draft_name_clone = draft_name.clone();
            let output_path_clone = output_path.clone();
            
            task::spawn(async move {
                if config.verbose {
                    println!("Processing draft: {}", draft_name_clone);
                }

                let result = process_specification(&*executor, &draft_file_clone, &draft_name_clone, &config).await;
                
                match result {
                    Ok(_) => {
                        if config.verbose {
                            println!("✓ Successfully created specification for {}", draft_name_clone);
                        }
                        Ok::<(String, PathBuf, PathBuf, bool), anyhow::Error>((draft_name_clone, draft_file_clone, output_path_clone, true))
                    }
                    Err(e) => {
                        eprintln!("✗ Failed to create specification for {}: {}", draft_name_clone, e);
                        Ok::<(String, PathBuf, PathBuf, bool), anyhow::Error>((draft_name_clone, draft_file_clone, output_path_clone, false))
                    }
                }
            })
        }).collect();
        
        // Wait for all tasks to complete and update progress/tracker
        for task in tasks {
            match task.await {
                Ok(Ok((draft_name, draft_file, output_path, success))) => {
                    progress.start_item(&draft_name);
                    if success {
                        tracker.record(Stage::Specification, &draft_name, &draft_file, &output_path)?;
                        progress.complete_item(&draft_name, true);
                        updated_count += 1;
                    } else {
                        progress.complete_item(&draft_name, false);
                    }
                }
                Ok(Err(e)) => {
                    eprintln!("Error processing file: {}", e);
                }
                Err(e) => {
                    eprintln!("Task join error: {}", e);
                }
            }
        }
    } else {
        // Process files sequentially
        for (draft_file, draft_name, output_path) in files_to_process {
            progress.start_item(&draft_name);

            if config.verbose {
                println!("Processing draft: {}", draft_name);
            }

            match process_specification(&executor, &draft_file, &draft_name, config).await {
                Ok(_) => {
                    // Record successful generation
                    tracker.record(Stage::Specification, &draft_name, &draft_file, &output_path)?;
                    updated_count += 1;

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
    } // End of for loop processing each agent's files

    // Save tracker
    tracker.save()?;

    progress.finish();

    if updated_count == 0 && config.verbose {
        println!("All specifications are up to date");
    }

    Ok(())
}

async fn process_specification(
    executor: &AgentExecutor,
    draft_file: &Path,
    draft_name: &str,
    config: &Config,
) -> Result<()> {
    let draft_content = fs::read_to_string(draft_file)
        .context("Failed to read draft file")?;

    if config.dry_run {
        println!("[DRY RUN] Would create specification for: {}", draft_name);
        return Ok(());
    }

    // Use conversational execution to handle questions
    let spec_content = executor
        .execute_with_conversation(&draft_content, draft_name)
        .await?;

    // Determine output path preserving folder structure
    let output_path = determine_specification_output_path(draft_file, DRAFTS_DIR, SPECIFICATIONS_DIR)?;

    // Report Unspecified or Ambiguous Aspects immediately if present in generated spec
    if let Some(unspecified) = extract_unspecified_or_ambiguous_section(&spec_content) {
        eprintln!("error[spec:unspecified]:");
        // Print file path in red on its own line
        eprintln!("\u{001b}[31m{}\u{001b}[0m", output_path.display());
        eprintln!("  Unspecified or Ambiguous Aspects detected in generated specification for '{}'.", draft_name);
        eprintln!();
        for line in unspecified.lines() {
            eprintln!("  {}", line);
        }
        eprintln!();
    }
    
    // Ensure the output directory exists
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .context("Failed to create specification output directory")?;
    }
    
    fs::write(&output_path, spec_content)
        .context("Failed to write specification file")?;

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

    println!("Creating implementation for {} context(s)", context_files.len());

    // Step 1: Generate project structure (Cargo.toml, lib.rs, mod.rs files)
    if config.verbose {
        println!("Generating project structure...");
    }

    let spec_dir = PathBuf::from(SPECIFICATIONS_DIR);
    let project_info = analyze_specifications(&spec_dir)
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
    let executor = AgentExecutor::new("create_implementation", config)?;
    let can_parallel = executor.model_registry()
        .can_run_parallel("create_implementation")
        .unwrap_or(false);

    if can_parallel && config.verbose {
        println!("Parallel execution enabled for create_implementation");
    }

    let mut progress = ProgressIndicator::new(context_files.len());
    let mut updated_count = 0;
    let mut had_unspecified = false;

    // Collect files that need processing
    let mut files_to_process = Vec::new();
    for context_file in context_files {
        let context_name = context_file.file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .context("Invalid context filename")?;

        // Determine output path preserving folder structure
        let output_path = determine_implementation_output_path(&context_file, SPECIFICATIONS_DIR)?;

        // Check if update is needed
        let needs_update = tracker.needs_update(Stage::Implementation, &context_name, &context_file, &output_path)?;

        if !needs_update {
            if config.verbose {
                println!("⊚ Skipping {} (up to date)", context_name);
            }
            continue;
        }

        files_to_process.push((context_file, context_name, output_path));
    }

    if can_parallel && !files_to_process.is_empty() {
        // Process files in parallel
        use tokio::task;
        use std::sync::Arc;
        
        let executor_arc = Arc::new(executor);
        let config_clone = *config;
        
        let tasks: Vec<_> = files_to_process.into_iter().map(|(context_file, context_name, output_path)| {
            let executor = executor_arc.clone();
            let config = config_clone;
            let context_file_clone = context_file.clone();
            let context_name_clone = context_name.clone();
            let output_path_clone = output_path.clone();
            
            task::spawn(async move {
                if config.verbose {
                    println!("Processing context: {}", context_name_clone);
                }

                let result = process_implementation(&*executor, &context_file_clone, &context_name_clone, &config).await;
                
                match result {
                    Ok(_) => {
                        if config.verbose {
                            println!("✓ Successfully created implementation for {}", context_name_clone);
                        }
                        Ok::<(String, PathBuf, PathBuf, bool, bool), anyhow::Error>((context_name_clone, context_file_clone, output_path_clone, true, false))
                    }
                    Err(e) => {
                        eprintln!("✗ Failed to create implementation for {}: {}", context_name_clone, e);
                        let is_unspecified = e.to_string().contains("unfinished specification");
                        Ok::<(String, PathBuf, PathBuf, bool, bool), anyhow::Error>((context_name_clone, context_file_clone, output_path_clone, false, is_unspecified))
                    }
                }
            })
        }).collect();
        
        // Wait for all tasks to complete and update progress/tracker
        for task in tasks {
            match task.await {
                Ok(Ok((context_name, context_file, output_path, success, unspecified))) => {
                    progress.start_item(&context_name);
                    if unspecified {
                        had_unspecified = true;
                    }
                    if success {
                        tracker.record(Stage::Implementation, &context_name, &context_file, &output_path)?;
                        progress.complete_item(&context_name, true);
                        updated_count += 1;
                    } else {
                        progress.complete_item(&context_name, false);
                    }
                }
                Ok(Err(e)) => {
                    eprintln!("Error processing file: {}", e);
                }
                Err(e) => {
                    eprintln!("Task join error: {}", e);
                }
            }
        }
    } else {
        // Process files sequentially
        for (context_file, context_name, output_path) in files_to_process {
            progress.start_item(&context_name);

            if config.verbose {
                println!("Processing context: {}", context_name);
            }

            match process_implementation(&executor, &context_file, &context_name, config).await {
                Ok(_) => {
                    // Record successful generation
                    tracker.record(Stage::Implementation, &context_name, &context_file, &output_path)?;
                    updated_count += 1;

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

    // Save tracker
    tracker.save()?;

    progress.finish();

    if updated_count == 0 && config.verbose {
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
) -> Result<()> {
    let context_content = fs::read_to_string(context_file)
        .context("Failed to read context file")?;

    // If the specification contains an "Unspecified or Ambiguous Aspects" section,
    // mark as unfinished and skip detailed reporting (reported during specification step).
    if let Some(unspecified) = extract_unspecified_or_ambiguous_section(&context_content) {
        let _ = unspecified; // avoid unused variable warning in case of cfgs
        eprintln!("error[spec:unfinished]:");
        // Print file path in red on its own line
        eprintln!("\u{001b}[31m{}\u{001b}[0m", context_file.display());
        eprintln!("  Specification has Unspecified or Ambiguous Aspects; skipping implementation for '{}'.", context_name);
        anyhow::bail!("unfinished specification");
    }

    if config.dry_run {
        println!("[DRY RUN] Would create implementation for: {}", context_name);
        return Ok(());
    }

    // Use conversational execution to handle questions
    let impl_result = executor
        .execute_with_conversation(&context_content, context_name)
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

/// Extracts the content of the "Unspecified or Ambiguous Aspects" section from markdown content.
/// Returns None if the section is not present.
fn extract_unspecified_or_ambiguous_section(content: &str) -> Option<String> {
    use regex::Regex;

    // Match a markdown header for the section, level 1-6, allowing up to 3 leading spaces
    let header_re = Regex::new(r"(?m)^\s{0,3}#{1,6}\s+Unspecified or Ambiguous Aspects\s*$").ok()?;
    let start = header_re.find(content)?.end();

    // Find the next header to delimit the section
    let rest = &content[start..];
    let next_header_re = Regex::new(r"(?m)^\s{0,3}#{1,6}\s+").ok()?;
    let section = if let Some(m) = next_header_re.find(rest) {
        &rest[..m.start()]
    } else {
        rest
    };

    let trimmed = section.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub async fn create_tests(names: Vec<String>, config: &Config) -> Result<()> {
    let context_files = resolve_input_files(SPECIFICATIONS_DIR, names, "md")?;

    if context_files.is_empty() {
        println!("No context files found to process");
        return Ok(());
    }

    println!("Creating tests for {} context(s)", context_files.len());

    let executor = AgentExecutor::new("create_test", config)?;
    let can_parallel = executor.model_registry()
        .can_run_parallel("create_test")
        .unwrap_or(false);

    if can_parallel && config.verbose {
        println!("Parallel execution enabled for create_test");
    }

    let mut progress = ProgressIndicator::new(context_files.len());

    // Collect files that need processing
    let mut files_to_process = Vec::new();
    for context_file in context_files {
        let context_name = context_file.file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .context("Invalid context filename")?;

        files_to_process.push((context_file, context_name));
    }

    if can_parallel && !files_to_process.is_empty() {
        // Process files in parallel
        use tokio::task;
        use std::sync::Arc;
        
        let executor_arc = Arc::new(executor);
        let config_clone = *config;
        
        let tasks: Vec<_> = files_to_process.into_iter().map(|(context_file, context_name)| {
            let executor = executor_arc.clone();
            let config = config_clone;
            let context_file_clone = context_file.clone();
            let context_name_clone = context_name.clone();
            
            task::spawn(async move {
                if config.verbose {
                    println!("Processing context: {}", context_name_clone);
                }

                let result = process_tests(&*executor, &context_file_clone, &context_name_clone, &config).await;
                
                match result {
                    Ok(_) => {
                        if config.verbose {
                            println!("✓ Successfully created tests for {}", context_name_clone);
                        }
                        Ok::<(String, bool), anyhow::Error>((context_name_clone, true))
                    }
                    Err(e) => {
                        eprintln!("✗ Failed to create tests for {}: {}", context_name_clone, e);
                        Ok::<(String, bool), anyhow::Error>((context_name_clone, false))
                    }
                }
            })
        }).collect();
        
        // Wait for all tasks to complete and update progress
        for task in tasks {
            match task.await {
                Ok(Ok((context_name, success))) => {
                    progress.start_item(&context_name);
                    progress.complete_item(&context_name, success);
                }
                Ok(Err(e)) => {
                    eprintln!("Error processing file: {}", e);
                }
                Err(e) => {
                    eprintln!("Task join error: {}", e);
                }
            }
        }
    } else {
        // Process files sequentially
        for (context_file, context_name) in files_to_process {
            progress.start_item(&context_name);

            if config.verbose {
                println!("Processing context: {}", context_name);
            }

            match process_tests(&executor, &context_file, &context_name, config).await {
                Ok(_) => {
                    progress.complete_item(&context_name, true);
                    if config.verbose {
                        println!("✓ Successfully created tests for {}", context_name);
                    }
                }
                Err(e) => {
                    progress.complete_item(&context_name, false);
                    eprintln!("✗ Failed to create tests for {}: {}", context_name, e);
                }
            }
        }
    }

    progress.finish();
    Ok(())
}

async fn process_tests(
    executor: &AgentExecutor,
    context_file: &Path,
    context_name: &str,
    config: &Config,
) -> Result<()> {
    let context_content = fs::read_to_string(context_file)
        .context("Failed to read context file")?;

    if config.dry_run {
        println!("[DRY RUN] Would create tests for: {}", context_name);
        return Ok(());
    }

    // Use conversational execution to handle questions
    let test_result = executor
        .execute_with_conversation(&context_content, context_name)
        .await?;

    if config.verbose {
        println!("Test creation result: {}", test_result);
    }

    Ok(())
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
    let output_filename = if file_stem == "app" {
        "main.rs"
    } else {
        &format!("{}.rs", file_stem)
    };
    
    let output_path = output_dir.join(output_filename);
    Ok(output_path)
}
