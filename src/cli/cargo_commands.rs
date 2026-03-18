use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::compilation_fix;
use super::project_structure::{analyze_specifications, ProjectInfo};
use super::stage_runner::ExecutionResources;
use super::{Config, DRAFTS_DIR, SPECIFICATIONS_DIR};
use reen::execution::NativeExecutionControl;

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
        ProjectInfo::default()
    };

    let mut recent_files: Vec<PathBuf> = Vec::new();
    for path in [
        PathBuf::from("Cargo.toml"),
        PathBuf::from("src/lib.rs"),
        PathBuf::from("src/main.rs"),
        PathBuf::from("src/execution/mod.rs"),
    ] {
        if path.exists() {
            recent_files.push(path);
        }
    }

    let resources = ExecutionResources::new(
        super::resolve_rate_limit(None),
        super::resolve_token_limit(None),
    );

    compilation_fix::ensure_compiles_with_auto_fix(
        config,
        max_compile_fix_attempts,
        project_root,
        &project_info,
        &recent_files,
        resources
            .execution_control
            .as_ref()
            .map(|control| control as &dyn NativeExecutionControl),
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
