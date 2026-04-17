//! LLM-assisted repair when `cargo build` fails after `reen build` (used with `--fix`).

use crate::agent_runner::{AgentRequest, AgentRunner, SystemBlock};
use crate::compile_repair::{
    collect_error_rs_paths, filter_paths_by_manifest, normalize_manifest_path,
};
use crate::spec_context::load_artifact_spec_context_for_generated_file;
use crate::workspace::Workspace;
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

const SYSTEM_PROMPT: &str = r#"You are a Rust compiler error repair assistant for the Reen DCI code generator.

You are given `cargo` stderr and the full contents of affected Rust source files (paths are workspace-relative).

Your task: fix the code so the project compiles. Typical issues include borrow checker errors, lifetime annotations, missing imports, and type mismatches.

## Output format

Return ONLY a JSON object with this exact shape (no markdown fences, no commentary):

{"files":[{"path":"src/example.rs","content":"...full new file contents..."}]}

Rules:
- Include every file you modify; each `content` must be the complete file text.
- Use forward slashes in `path` (e.g. `src/contexts/foo.rs`).
- Do not include files you did not change.
- Preserve project structure and public APIs unless the compiler forces a change.
- Do not add explanatory text outside the JSON."#;

#[derive(Debug, Deserialize)]
struct AgentCompileFixResponse {
    files: Vec<AgentCompileFile>,
}

#[derive(Debug, Deserialize)]
struct AgentCompileFile {
    path: String,
    content: String,
}

/// Run the compile-fix agent: writes manifest-listed files. Returns `Ok(true)` if at least one
/// file was written.
pub(crate) fn apply_agent_compile_fixes(
    workspace: &Workspace,
    allowed_files: &HashSet<String>,
    stderr: &str,
    runner: &AgentRunner,
    verbose: bool,
) -> Result<bool> {
    let raw_paths = collect_error_rs_paths(stderr);
    let paths = filter_paths_by_manifest(raw_paths, allowed_files);
    if paths.is_empty() {
        bail!(
            "No `.rs` paths from compiler output matched generated manifest files; cannot run compile-fix agent"
        );
    }

    let mut user = String::new();
    let deps_section = crate::build_agent::render_dependency_context(workspace)?;
    if !deps_section.is_empty() {
        user.push_str(&deps_section);
        user.push('\n');
    }
    user.push_str("# Cargo build errors (stderr)\n\n```\n");
    user.push_str(stderr);
    user.push_str("\n```\n\n# Source files\n\n");
    for p in &paths {
        let rel = normalize_manifest_path(p);
        let full = workspace.root.join(p);
        let content = fs::read_to_string(&full)
            .with_context(|| format!("Failed to read {}", full.display()))?;
        user.push_str(&format!("## {rel}\n\n```rust\n{content}\n```\n\n"));
    }

    user.push_str("# Matching specification context\n\n");
    for p in &paths {
        let full = workspace.root.join(p);
        if let Some(spec) = load_artifact_spec_context_for_generated_file(workspace, &full)? {
            user.push_str(&format!(
                "### Source {}\n\n{}",
                normalize_manifest_path(p),
                spec.render_prompt_block_with_heading("###")
            ));
        }
    }

    user.push_str(
        "Return ONLY the JSON object described in the system prompt. Paths must match the ## headers above.",
    );

    if verbose {
        eprintln!(
            "build --fix: compile-fix agent ({} file(s), model: {})",
            paths.len(),
            runner.model()
        );
    }

    let json = runner.run_json(&AgentRequest {
        system: vec![SystemBlock::new(SYSTEM_PROMPT)],
        user_content: &user,
        temperature: 0.2,
        max_tokens: 65_536,
    })?;

    let parsed: AgentCompileFixResponse = serde_json::from_str(&json).with_context(|| {
        format!(
            "compile-fix agent returned invalid JSON (first 400 chars): {}",
            json.chars().take(400).collect::<String>()
        )
    })?;

    if parsed.files.is_empty() {
        return Ok(false);
    }

    let mut wrote = false;
    for file in parsed.files {
        let normalized = file
            .path
            .replace('\\', "/")
            .trim_start_matches("./")
            .to_string();
        validate_relative_workspace_path(&normalized)?;
        if !allowed_files.contains(&normalized) {
            bail!(
                "compile-fix agent wrote path {:?} which is not listed in the generated manifest",
                file.path
            );
        }
        if !normalized.ends_with(".rs") {
            bail!("compile-fix agent path {:?} must be a .rs file", file.path);
        }
        let dest = safe_join_under_root(&workspace.root, &normalized)?;
        if verbose {
            eprintln!("  write {}", normalized);
        }
        fs::write(&dest, file.content)
            .with_context(|| format!("Failed to write {}", dest.display()))?;
        wrote = true;
    }

    Ok(wrote)
}

fn validate_relative_workspace_path(rel: &str) -> Result<()> {
    if rel.is_empty() || rel.starts_with('/') {
        bail!("invalid path {rel:?}");
    }
    for c in Path::new(rel).components() {
        if matches!(c, Component::ParentDir) {
            bail!("path must not contain `..`: {rel:?}");
        }
    }
    Ok(())
}

fn safe_join_under_root(root: &Path, rel: &str) -> Result<PathBuf> {
    validate_relative_workspace_path(rel)?;
    Ok(root.join(rel))
}
