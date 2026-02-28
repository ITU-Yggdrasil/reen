use anyhow::{Context, Result};
use chrono::Utc;
use regex::Regex;
use serde::Serialize;
use serde_json::json;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use super::agent_executor::{AgentExecutor, AgentResponse};
use super::project_structure::ProjectInfo;
use super::Config;

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
    project_info: &ProjectInfo,
    recent_generated_files: &[PathBuf],
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

    let session_dir = create_session_dir(project_root)?;
    let session_dir_display = session_dir.display().to_string();
    eprintln!(
        "error[compile]: build failed; attempting automatic compilation fixes (max_attempts={}). Logs: {}",
        max_attempts, session_dir_display
    );

    for attempt in 1..=max_attempts {
        let attempt_dir = session_dir.join(format!("attempt_{}", attempt));
        fs::create_dir_all(&attempt_dir)
            .with_context(|| format!("Failed to create {}", attempt_dir.display()))?;

        write_attempt_compile_output(&attempt_dir, &output)?;

        let diagnostics = parse_rustc_diagnostics(&output.stderr);
        let relevant_paths = collect_relevant_paths(
            project_root,
            &diagnostics,
            &output.stderr,
            recent_generated_files,
        )?;

        let files_json = snapshot_files_json(project_root, &relevant_paths)?;
        let specs_json = snapshot_specs_json(project_root, &relevant_paths)?;

        let additional_context = build_agent_context(
            &output,
            &diagnostics,
            &files_json,
            &specs_json,
            recent_generated_files,
            project_info,
        )?;

        fs::write(
            attempt_dir.join("diagnostics.json"),
            serde_json::to_string_pretty(&diagnostics).unwrap_or_else(|_| "[]".to_string()),
        )
        .ok();
        fs::write(
            attempt_dir.join("context_files.json"),
            serde_json::to_string_pretty(&files_json).unwrap_or_else(|_| "{}".to_string()),
        )
        .ok();
        if !specs_json.is_empty() {
            fs::write(
                attempt_dir.join("context_specs.json"),
                serde_json::to_string_pretty(&specs_json).unwrap_or_else(|_| "{}".to_string()),
            )
            .ok();
        }

        let executor = AgentExecutor::new("resolve_compilation_errors", config)
            .context("Failed to create compilation error resolver agent")?;

        let agent_response = executor
            .execute_with_context("Compilation failed; propose minimal fix patch.", additional_context)
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

        let extracted = extract_unified_diff(&patch_text)
            .context("Resolver output did not contain a unified diff starting with 'diff --git'")?;

        let guardrail = check_guardrails(project_root, &extracted)?;
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

        let applied_patch = apply_unified_diff(project_root, &extracted)
            .context("Failed to apply proposed patch")?;
        fs::write(attempt_dir.join("applied.patch"), &applied_patch).ok();

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
    fs::create_dir_all(&base)
        .with_context(|| format!("Failed to create {}", base.display()))?;
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

fn collect_relevant_paths(
    project_root: &Path,
    diagnostics: &[DiagnosticSpan],
    stderr: &str,
    recent_generated_files: &[PathBuf],
) -> Result<Vec<PathBuf>> {
    let mut paths: HashSet<PathBuf> = HashSet::new();

    // Always include Cargo.toml and src/lib.rs if present.
    for always in ["Cargo.toml", "src/lib.rs", "src/contexts/mod.rs", "src/data/mod.rs"] {
        let p = project_root.join(always);
        if p.exists() {
            paths.insert(p);
        }
    }

    for p in recent_generated_files {
        let full = if p.is_absolute() { p.clone() } else { project_root.join(p) };
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

    let mut out: Vec<PathBuf> = paths.into_iter().collect();
    out.sort();
    Ok(out)
}

fn snapshot_files_json(project_root: &Path, paths: &[PathBuf]) -> Result<BTreeMap<String, String>> {
    let mut map = BTreeMap::new();
    for p in paths {
        if !p.exists() || p.is_dir() {
            continue;
        }
        let rel = p.strip_prefix(project_root).unwrap_or(p).to_string_lossy().to_string();
        let content = fs::read_to_string(p)
            .with_context(|| format!("Failed to read {}", p.display()))?;
        map.insert(rel, content);
    }
    Ok(map)
}

fn snapshot_specs_json(project_root: &Path, src_paths: &[PathBuf]) -> Result<BTreeMap<String, String>> {
    let mut spec_paths: HashSet<PathBuf> = HashSet::new();
    for p in src_paths {
        let rel = p.strip_prefix(project_root).unwrap_or(p);
        let rel_s = rel.to_string_lossy();
        let spec_rel = map_src_to_spec(&rel_s);
        if let Some(spec_rel) = spec_rel {
            let spec_full = project_root.join(&spec_rel);
            if spec_full.exists() {
                spec_paths.insert(spec_full);
            }
        }
    }

    let mut map = BTreeMap::new();
    let mut list: Vec<PathBuf> = spec_paths.into_iter().collect();
    list.sort();
    for p in list {
        let rel = p.strip_prefix(project_root).unwrap_or(&p).to_string_lossy().to_string();
        let content = fs::read_to_string(&p).with_context(|| format!("Failed to read {}", p.display()))?;
        map.insert(rel, content);
    }
    Ok(map)
}

fn map_src_to_spec(src_rel: &str) -> Option<String> {
    // Best-effort mapping. This is intentionally conservative.
    // src/contexts/x.rs -> specifications/contexts/x.md
    // src/data/x.rs -> specifications/data/x.md
    // src/main.rs -> specifications/app.md
    if let Some(stem) = src_rel.strip_prefix("src/contexts/").and_then(|s| s.strip_suffix(".rs")) {
        return Some(format!("specifications/contexts/{}.md", stem));
    }
    if let Some(stem) = src_rel.strip_prefix("src/data/").and_then(|s| s.strip_suffix(".rs")) {
        return Some(format!("specifications/data/{}.md", stem));
    }
    if src_rel == "src/main.rs" {
        return Some("specifications/app.md".to_string());
    }
    None
}

fn build_agent_context(
    output: &CompilationOutput,
    diagnostics: &[DiagnosticSpan],
    files_json: &BTreeMap<String, String>,
    specs_json: &BTreeMap<String, String>,
    recent_generated_files: &[PathBuf],
    project_info: &ProjectInfo,
) -> Result<HashMap<String, serde_json::Value>> {
    let mut ctx = HashMap::new();
    ctx.insert("compiler_stdout".to_string(), json!(output.stdout));
    ctx.insert("compiler_stderr".to_string(), json!(output.stderr));
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
    }
    ctx.insert(
        "recent_changes".to_string(),
        json!(recent_generated_files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("\n")),
    );
    ctx.insert("project_info".to_string(), json!(project_info.package_name));
    Ok(ctx)
}

fn extract_unified_diff(text: &str) -> Option<String> {
    if let Some(idx) = text.find("diff --git ") {
        return Some(text[idx..].trim().to_string());
    }

    // Also allow raw patches that start with ---/+++ (less preferred)
    if let Some(idx) = text.find("\n--- ") {
        return Some(text[idx + 1..].trim().to_string());
    }
    if text.trim_start().starts_with("--- ") {
        return Some(text.trim().to_string());
    }
    None
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

fn check_guardrails(project_root: &Path, diff: &str) -> Result<GuardrailReport> {
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

        let target = if !path.is_empty() { path.clone() } else { old.clone() };
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
        for hl in &fp.hunk_lines {
            match hl.kind {
                HunkLineKind::Remove => {
                    total_deleted_lines += 1;
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
                }
                _ => {}
            }
        }

        issues.extend(evaluate_public_api_changes(&target, &removed_pub_fns, &added_pub_fns));

        // Also ensure target resolves within root when joined.
        let full = project_root.join(&target);
        if let Ok(canon) = full.canonicalize() {
            if let Ok(root) = project_root.canonicalize() {
                if !canon.starts_with(root) {
                    issues.push(format!("Blocked path (outside repo after resolve): {}", target));
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
        params_raw.split(',').map(|p| p.trim().to_string()).collect()
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
            ReceiverKind::RefSelf | ReceiverKind::ValSelf | ReceiverKind::MutRefSelf | ReceiverKind::MutValSelf
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

fn evaluate_public_api_changes(target: &str, removed: &[FnSig], added: &[FnSig]) -> Vec<String> {
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
                // Allow adding getters; block setters and other public additions.
                if !is_allowed_new_public_method(a) {
                    issues.push(format!(
                        "{}: patch adds new public function `{}` outside allowed patterns (getter-only additions are allowed; setters are not).",
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

fn is_allowed_new_public_method(sig: &FnSig) -> bool {
    // Allowed:
    // - adding getters (no args except receiver, no mut receiver), and not a setter
    // - adding constructors: `new` / `try_new` (associated functions; no `self` receiver)
    if sig.name.starts_with("set_") {
        return false;
    }
    if matches!(sig.receiver, ReceiverKind::MutRefSelf | ReceiverKind::MutValSelf) {
        return false;
    }
    if matches!(sig.name.as_str(), "new" | "try_new") {
        // Constructors should be associated functions; allow args.
        return matches!(sig.receiver, ReceiverKind::Other);
    }

    // Otherwise, allow only getter-shaped additions.
    sig.non_self_param_types.is_empty()
}

fn disallowed_pub_fn_modification(old: &FnSig, new: &FnSig) -> Option<String> {
    if old.name != new.name {
        return Some("function name changed".to_string());
    }
    if matches!(old.receiver, ReceiverKind::MutRefSelf | ReceiverKind::MutValSelf)
        || matches!(new.receiver, ReceiverKind::MutRefSelf | ReceiverKind::MutValSelf)
    {
        return Some("introduces mutable receiver (`&mut self`/`mut self`)".to_string());
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
            return Some(format!("parameter type changed beyond &T<->T: `{}` -> `{}`", a, b));
        }
    }
    match (&old.return_type, &new.return_type) {
        (None, None) => {}
        (Some(a), Some(b)) => {
            if !is_ref_value_equivalent(a, b) {
                return Some(format!("return type changed beyond &T<->T: `{}` -> `{}`", a, b));
            }
        }
        _ => return Some("return type presence changed".to_string()),
    }
    None
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

#[derive(Debug, Clone)]
struct FilePatch {
    old_path: Option<String>,
    new_path: Option<String>,
    hunks: Vec<Hunk>,
    hunk_lines: Vec<HunkLine>,
    is_new_file: bool,
    is_deletion: bool,
}

#[derive(Debug, Clone)]
struct Hunk {
    old_start: usize,
    lines: Vec<HunkLine>,
}

#[derive(Debug, Clone)]
struct HunkLine {
    kind: HunkLineKind,
    text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HunkLineKind {
    Context,
    Add,
    Remove,
}

fn parse_unified_diff(diff: &str) -> Result<Vec<FilePatch>> {
    let mut lines = diff.lines().peekable();
    let mut patches = Vec::new();

    while let Some(line) = lines.next() {
        if !line.starts_with("diff --git ") {
            continue;
        }

        // Next lines include --- and +++.
        let mut old_path: Option<String> = None;
        let mut new_path: Option<String> = None;
        let mut is_new_file = false;
        let mut is_deletion = false;
        let mut hunks = Vec::new();
        let mut hunk_lines_flat = Vec::new();

        while let Some(peek) = lines.peek() {
            let p = *peek;
            if p.starts_with("diff --git ") {
                break;
            }
            let l = lines.next().unwrap();
            if l.starts_with("new file mode") {
                is_new_file = true;
            } else if l.starts_with("deleted file mode") {
                is_deletion = true;
            } else if l.starts_with("--- ") {
                old_path = Some(extract_patch_path(l, "--- ")?);
                if old_path.as_deref() == Some("/dev/null") {
                    old_path = None;
                    is_new_file = true;
                }
            } else if l.starts_with("+++ ") {
                new_path = Some(extract_patch_path(l, "+++ ")?);
                if new_path.as_deref() == Some("/dev/null") {
                    new_path = None;
                    is_deletion = true;
                }
            } else if l.starts_with("@@ ") {
                let (old_start, _new_start) = parse_hunk_header(l)?;
                let mut h_lines = Vec::new();
                while let Some(next) = lines.peek() {
                    let nl = *next;
                    if nl.starts_with("diff --git ") || nl.starts_with("@@ ") {
                        break;
                    }
                    let hl = lines.next().unwrap();
                    if hl.starts_with("\\ No newline") {
                        continue;
                    }
                    if hl.is_empty() {
                        // Empty context line is valid; treat as context.
                        h_lines.push(HunkLine {
                            kind: HunkLineKind::Context,
                            text: String::new(),
                        });
                        continue;
                    }
                    let (kind, text) = match hl.chars().next().unwrap() {
                        ' ' => (HunkLineKind::Context, hl[1..].to_string()),
                        '+' => (HunkLineKind::Add, hl[1..].to_string()),
                        '-' => (HunkLineKind::Remove, hl[1..].to_string()),
                        _ => continue,
                    };
                    let h = HunkLine { kind, text };
                    hunk_lines_flat.push(h.clone());
                    h_lines.push(h);
                }
                hunks.push(Hunk {
                    old_start,
                    lines: h_lines,
                });
            }
        }

        let old_path_norm = old_path.and_then(normalize_patch_path);
        let new_path_norm = new_path.and_then(normalize_patch_path);

        patches.push(FilePatch {
            old_path: old_path_norm,
            new_path: new_path_norm,
            hunks,
            hunk_lines: hunk_lines_flat,
            is_new_file,
            is_deletion,
        });
    }

    if patches.is_empty() {
        anyhow::bail!("No file patches found");
    }
    Ok(patches)
}

fn extract_patch_path(line: &str, prefix: &str) -> Result<String> {
    let raw = line.strip_prefix(prefix).unwrap_or("").trim();
    // Paths may include trailing metadata (timestamps) separated by whitespace.
    // Keep only the first token.
    Ok(raw.split_whitespace().next().unwrap_or("").to_string())
}

fn normalize_patch_path(p: String) -> Option<String> {
    // Strip "a/" and "b/" prefixes used in git diffs.
    if p == "/dev/null" {
        return None;
    }
    Some(
        p.strip_prefix("a/")
            .or_else(|| p.strip_prefix("b/"))
            .unwrap_or(&p)
            .to_string(),
    )
}

fn parse_hunk_header(line: &str) -> Result<(usize, usize)> {
    // @@ -oldStart,oldCount +newStart,newCount @@
    let re = Regex::new(r"^@@\s+-(\d+)(?:,\d+)?\s+\+(\d+)(?:,\d+)?\s+@@").unwrap();
    let cap = re
        .captures(line)
        .ok_or_else(|| anyhow::anyhow!("Invalid hunk header: {}", line))?;
    let old_start = cap.get(1).unwrap().as_str().parse::<usize>()?;
    let new_start = cap.get(2).unwrap().as_str().parse::<usize>()?;
    Ok((old_start, new_start))
}

fn apply_unified_diff(project_root: &Path, diff: &str) -> Result<String> {
    let patches = parse_unified_diff(diff)?;
    for fp in patches {
        if fp.is_deletion {
            anyhow::bail!("Refusing to apply deletion patch");
        }
        let target_rel = fp
            .new_path
            .clone()
            .or(fp.old_path.clone())
            .ok_or_else(|| anyhow::anyhow!("Patch missing file path"))?;

        let target_full = project_root.join(&target_rel);
        if let Some(parent) = target_full.parent() {
            fs::create_dir_all(parent).ok();
        }

        let original = if target_full.exists() {
            fs::read_to_string(&target_full)
                .with_context(|| format!("Failed to read {}", target_full.display()))?
        } else {
            String::new()
        };
        let orig_lines = split_lines_preserve_empty(&original);
        let new_lines = apply_hunks(&orig_lines, &fp.hunks)
            .with_context(|| format!("Failed applying hunks to {}", target_rel))?;

        let new_content = join_lines(&new_lines);
        fs::write(&target_full, &new_content)
            .with_context(|| format!("Failed to write {}", target_full.display()))?;

    }
    Ok(diff.trim().to_string())
}

fn split_lines_preserve_empty(s: &str) -> Vec<String> {
    if s.is_empty() {
        return Vec::new();
    }
    // `split_terminator` matches patch semantics better than `split` because it
    // doesn't create a trailing empty line when the file ends with '\n'.
    s.split_terminator('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
        .collect()
}

fn join_lines(lines: &[String]) -> String {
    // Preserve trailing newline if file had one is not tracked; write with \n join.
    lines.join("\n")
}

fn apply_hunks(orig: &[String], hunks: &[Hunk]) -> Result<Vec<String>> {
    // Apply hunks using a fuzzy context search (similar to `patch`), since
    // agent-produced diffs can have slightly-stale line numbers or shifted context.
    let mut current: Vec<String> = orig.to_vec();

    for h in hunks {
        let (pattern, pattern_len) = hunk_preimage_pattern(h);
        let preferred = h.old_start.saturating_sub(1);

        let start = find_hunk_start(&current, &pattern, preferred).ok_or_else(|| {
            anyhow::anyhow!(
                "Could not locate hunk context (preferred_start={}, pattern_len={})",
                preferred,
                pattern_len
            )
        })?;

        let mut pos = start;
        let mut segment: Vec<String> = Vec::new();
        for hl in &h.lines {
            match hl.kind {
                HunkLineKind::Context => {
                    let line = current.get(pos).ok_or_else(|| {
                        anyhow::anyhow!("Context line beyond EOF at pos {}", pos)
                    })?;
                    if line != &hl.text {
                        anyhow::bail!(
                            "Context mismatch at pos {}: expected {:?}, found {:?}",
                            pos,
                            hl.text,
                            line
                        );
                    }
                    segment.push(line.clone());
                    pos += 1;
                }
                HunkLineKind::Remove => {
                    let line = current.get(pos).ok_or_else(|| {
                        anyhow::anyhow!("Remove line beyond EOF at pos {}", pos)
                    })?;
                    if line != &hl.text {
                        anyhow::bail!(
                            "Remove mismatch at pos {}: expected {:?}, found {:?}",
                            pos,
                            hl.text,
                            line
                        );
                    }
                    pos += 1;
                }
                HunkLineKind::Add => {
                    segment.push(hl.text.clone());
                }
            }
        }

        let mut next: Vec<String> = Vec::with_capacity(current.len() + segment.len());
        next.extend_from_slice(&current[..start]);
        next.extend(segment);
        next.extend_from_slice(&current[pos..]);
        current = next;
    }

    Ok(current)
}

fn hunk_preimage_pattern(h: &Hunk) -> (Vec<&str>, usize) {
    // Pre-image = context + removed lines in order.
    let mut pattern: Vec<&str> = Vec::new();
    for hl in &h.lines {
        match hl.kind {
            HunkLineKind::Context | HunkLineKind::Remove => pattern.push(hl.text.as_str()),
            HunkLineKind::Add => {}
        }
    }
    let len = pattern.len();
    (pattern, len)
}

fn find_hunk_start(lines: &[String], pattern: &[&str], preferred: usize) -> Option<usize> {
    if pattern.is_empty() {
        return Some(preferred.min(lines.len()));
    }
    if lines.len() < pattern.len() {
        return None;
    }

    // Try preferred first, then search with a bounded fuzz window, then fall back to full scan.
    let try_at = |i: usize| -> bool {
        if i + pattern.len() > lines.len() {
            return false;
        }
        for (j, needle) in pattern.iter().enumerate() {
            if lines[i + j].as_str() != *needle {
                return false;
            }
        }
        true
    };

    if try_at(preferred) {
        return Some(preferred);
    }

    let fuzz: usize = 100;
    let start = preferred.saturating_sub(fuzz);
    let end = (preferred + fuzz).min(lines.len().saturating_sub(pattern.len()));
    for i in start..=end {
        if try_at(i) {
            return Some(i);
        }
    }

    for i in 0..=lines.len().saturating_sub(pattern.len()) {
        if try_at(i) {
            return Some(i);
        }
    }

    None
}

