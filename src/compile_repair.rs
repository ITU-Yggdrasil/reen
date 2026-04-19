//! Shared `cargo build` invocation and deterministic compile-error repair (used by
//! `scaffold --fix` and `build --fix`).

use crate::workspace::Workspace;
use anyhow::{Context, Result};
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) const COMPILE_FIX_MAX_ROUNDS: usize = 5;

pub(crate) struct CompileResult {
    pub success: bool,
    /// Human-readable build log suitable for eprintln!-ing on failure. Contains the short-format
    /// single-line summaries rustc emits when `--message-format=short` is requested, plus any
    /// non-JSON output cargo writes to stderr.
    pub stderr: String,
    /// Structured rustc diagnostics parsed from cargo's JSON stream. Populated when rustc emits
    /// `compiler-message` events; empty when the compiler didn't produce any. These carry
    /// `suggested_replacement` spans that [`parse_compile_errors`] consumes to build
    /// [`CompileFix::ApplyRustcSuggestion`] fixes.
    pub diagnostics: Vec<RustcDiagnostic>,
}

pub(crate) fn run_cargo_build(workspace: &Workspace) -> Result<CompileResult> {
    // Run twice: once with `--message-format=json` so we get structured diagnostics with
    // `suggested_replacement` spans (MachineApplicable patches, help/note children); once with
    // `--message-format=short` so the user-facing log stays the same single-line-per-error format
    // that scaffold/build already surface. The JSON run is first so cargo's incremental cache
    // makes the second run essentially free.
    let json = Command::new("cargo")
        .args(["build", "--message-format=json"])
        .env("RUSTFLAGS", "-Awarnings")
        .current_dir(&workspace.root)
        .output()
        .context("Failed to invoke cargo build (json pass)")?;
    let json_stdout = String::from_utf8_lossy(&json.stdout).to_string();
    let json_stderr = String::from_utf8_lossy(&json.stderr).to_string();
    let diagnostics = parse_cargo_json_diagnostics(&json_stdout);

    let short = Command::new("cargo")
        .args(["build", "--message-format=short"])
        .env("RUSTFLAGS", "-Awarnings")
        .current_dir(&workspace.root)
        .output()
        .context("Failed to invoke cargo build (short pass)")?;
    let stderr_short = String::from_utf8_lossy(&short.stderr).to_string();
    // Prefer the short pass's stderr (the log format callers already parse). Fall back to the
    // json pass's stderr when cargo emitted nothing short (rare, but can happen when the build
    // fails before the short pass ever ran a rustc).
    let stderr = if !stderr_short.trim().is_empty() {
        stderr_short
    } else {
        json_stderr
    };
    Ok(CompileResult {
        success: short.status.success() && json.status.success(),
        stderr,
        diagnostics,
    })
}

// ---------------------------------------------------------------------------
// Structured rustc diagnostics (parsed from `cargo build --message-format=json`)
// ---------------------------------------------------------------------------

/// A single `compiler-message` event from cargo's JSON stream, reduced to the fields we care
/// about for compile-repair. Child diagnostics carry the actual `suggested_replacement` patches.
#[derive(Debug, Clone)]
#[allow(dead_code)] // `code`/`rendered` are consumed by Phase 4 (LLM prompt enrichment).
pub(crate) struct RustcDiagnostic {
    pub code: Option<String>,
    pub level: String,
    pub message: String,
    pub rendered: Option<String>,
    pub spans: Vec<RustcSpan>,
    pub children: Vec<RustcDiagnostic>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // `is_primary`/`label` are consumed by Phase 3/4 (revert-to-todo + prompt).
pub(crate) struct RustcSpan {
    pub file_name: String,
    pub line_start: usize,
    pub line_end: usize,
    pub column_start: usize,
    pub column_end: usize,
    pub is_primary: bool,
    pub suggested_replacement: Option<String>,
    pub suggestion_applicability: Option<String>,
    pub label: Option<String>,
}

/// Parse cargo's `--message-format=json` stdout into a flat list of rustc diagnostics.
/// Each stdout line is one event; we keep only `reason == "compiler-message"` entries.
/// Malformed lines are skipped silently — the build-repair loop treats an empty diagnostic
/// vector as "no structured fixes available" and falls back to the string-match parsers.
pub(crate) fn parse_cargo_json_diagnostics(stdout: &str) -> Vec<RustcDiagnostic> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() || !line.starts_with('{') {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if value.get("reason").and_then(|v| v.as_str()) != Some("compiler-message") {
            continue;
        }
        let Some(message) = value.get("message") else {
            continue;
        };
        if let Some(diag) = parse_rustc_diagnostic(message) {
            out.push(diag);
        }
    }
    out
}

fn parse_rustc_diagnostic(value: &serde_json::Value) -> Option<RustcDiagnostic> {
    let message = value.get("message")?.as_str()?.to_string();
    let level = value
        .get("level")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let code = value
        .get("code")
        .and_then(|c| c.get("code"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let rendered = value
        .get("rendered")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let spans = value
        .get("spans")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(parse_rustc_span).collect())
        .unwrap_or_default();
    let children = value
        .get("children")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(parse_rustc_diagnostic).collect())
        .unwrap_or_default();
    Some(RustcDiagnostic {
        code,
        level,
        message,
        rendered,
        spans,
        children,
    })
}

fn parse_rustc_span(value: &serde_json::Value) -> Option<RustcSpan> {
    let file_name = value.get("file_name")?.as_str()?.to_string();
    let line_start = value.get("line_start")?.as_u64()? as usize;
    let line_end = value
        .get("line_end")
        .and_then(|v| v.as_u64())
        .unwrap_or(line_start as u64) as usize;
    let column_start = value.get("column_start")?.as_u64()? as usize;
    let column_end = value
        .get("column_end")
        .and_then(|v| v.as_u64())
        .unwrap_or(column_start as u64) as usize;
    let is_primary = value
        .get("is_primary")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let suggested_replacement = value
        .get("suggested_replacement")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let suggestion_applicability = value
        .get("suggestion_applicability")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let label = value
        .get("label")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    Some(RustcSpan {
        file_name,
        line_start,
        line_end,
        column_start,
        column_end,
        is_primary,
        suggested_replacement,
        suggestion_applicability,
        label,
    })
}

// ---------------------------------------------------------------------------
// Deterministic compile-error repair
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub(crate) enum CompileFix {
    RemoveDerive {
        file: PathBuf,
        line: usize,
        trait_name: String,
    },
    AddLifetimeToMethod {
        file: PathBuf,
        line: usize,
    },
    AddLocalTypeImport {
        file: PathBuf,
        line: usize,
        type_name: String,
    },
    ReplaceRandThreadRngCall {
        file: PathBuf,
        line: usize,
    },
    RemoveBorrowForOwnedArgument {
        file: PathBuf,
        line: usize,
        type_name: String,
    },
    /// Add one or more derives to the type declared in `file`.
    ///
    /// Emitted by scanning rustc errors such as E0277 ("the trait bound X: Eq is not satisfied"),
    /// E0369 ("binary operation `!=` cannot be applied to type X"), and the `help: consider
    /// annotating ... with #[derive(...)]` hint lines.
    AddDerive {
        file: PathBuf,
        trait_names: Vec<String>,
    },
    /// Replace a known-stale crate method call with its current-version name.
    ///
    /// Used for crate-API migrations (for example `rand::Rng::gen_range` → `random_range`).
    ReplaceMethodCall {
        file: PathBuf,
        line: usize,
        from: String,
        to: String,
    },
    /// Strip a `.unwrap()` (or `.unwrap_or_default()`, `.expect(...)`) that was appended to a
    /// value whose type is not `Option`/`Result`. Triggered by
    /// `no method named 'unwrap'/'expect' found for struct X`.
    StripUnwrapCall {
        file: PathBuf,
        line: usize,
        method: String,
    },
    /// Add a `use <trait_path>;` statement so a trait method becomes in-scope.
    ///
    /// Triggered by "no method named X found for struct/type Y" where X is a known trait method
    /// (see `known_trait_methods`).
    AddTraitImport {
        file: PathBuf,
        trait_path: String,
    },
    /// Add an external crate to both `drafts/dependencies.yml` and the project `Cargo.toml`.
    ///
    /// Triggered by "use of unresolved module or unlinked crate `X`" when `X` is declared in
    /// `drafts/capability_registry.yml`.
    AddExternalCrate {
        crate_root: String,
    },
    /// Replace a function's brace-balanced body with `todo!("<description>")` and queue the
    /// enclosing method for re-implementation by the next build-agent pass.
    ///
    /// Triggered by E0277 "the `?` operator can only be used in a method that returns
    /// `Result` or `Option`" — the only safe move in that case is to surrender the body back
    /// to the LLM, because stripping `?` in isolation would leave a `while poll(...)?` with an
    /// incompatible operand type or discard a real error branch.
    RevertBodyToTodo {
        file: PathBuf,
        fn_signature_line: usize,
        todo_description: String,
    },
    /// Apply an edit that rustc itself suggested via `suggested_replacement` in a JSON
    /// `compiler-message` event. This is the most trustworthy source of patches because the
    /// compiler controls both the span and the replacement text.
    ///
    /// `applicability` is the raw string rustc emitted (`MachineApplicable`,
    /// `MaybeIncorrect`, `HasPlaceholders`, `Unspecified`); the repair loop only applies
    /// `MachineApplicable` suggestions by default.
    ApplyRustcSuggestion {
        file: PathBuf,
        line_start: usize,
        line_end: usize,
        column_start: usize,
        column_end: usize,
        replacement: String,
        applicability: String,
        /// Short human-readable summary (e.g. the parent diagnostic's `message` field), used
        /// only in the verbose log.
        summary: String,
    },
    /// Convert a helper method into an associated function when its receiver is unused and the
    /// current receiver creates a borrow conflict like `self.helper(&mut self.field)`.
    ConvertHelperToAssociatedFn {
        file: PathBuf,
        method_name: String,
    },
}

impl CompileFix {
    pub(crate) fn description(&self) -> String {
        match self {
            CompileFix::RemoveDerive {
                file, trait_name, ..
            } => {
                format!("remove `{trait_name}` derive from {}", file.display())
            }
            CompileFix::AddLifetimeToMethod { file, line } => {
                format!(
                    "add lifetime parameter to method at {}:{line}",
                    file.display()
                )
            }
            CompileFix::AddLocalTypeImport {
                file,
                line,
                type_name,
            } => {
                format!(
                    "add local type import `{type_name}` to {}:{line}",
                    file.display()
                )
            }
            CompileFix::ReplaceRandThreadRngCall { file, line } => {
                format!(
                    "replace `rand::thread_rng()` with `rand::rng()` at {}:{line}",
                    file.display()
                )
            }
            CompileFix::RemoveBorrowForOwnedArgument {
                file,
                line,
                type_name,
            } => {
                format!(
                    "remove borrow for owned `{type_name}` argument at {}:{line}",
                    file.display()
                )
            }
            CompileFix::AddDerive { file, trait_names } => {
                format!(
                    "add derive(s) [{}] to {}",
                    trait_names.join(", "),
                    file.display()
                )
            }
            CompileFix::ReplaceMethodCall {
                file,
                line,
                from,
                to,
            } => {
                format!("replace `{from}` with `{to}` at {}:{line}", file.display())
            }
            CompileFix::StripUnwrapCall { file, line, method } => {
                format!("strip spurious `.{method}()` at {}:{line}", file.display())
            }
            CompileFix::AddTraitImport { file, trait_path } => {
                format!(
                    "add `use {trait_path};` to {} so trait methods resolve",
                    file.display()
                )
            }
            CompileFix::AddExternalCrate { crate_root } => {
                format!("register external crate `{crate_root}` in dependencies.yml and Cargo.toml")
            }
            CompileFix::ApplyRustcSuggestion {
                file,
                line_start,
                column_start,
                summary,
                applicability,
                ..
            } => {
                format!(
                    "apply rustc suggestion ({applicability}) at {}:{line_start}:{column_start} — {summary}",
                    file.display()
                )
            }
            CompileFix::ConvertHelperToAssociatedFn { file, method_name } => {
                format!(
                    "convert helper `{method_name}` into an associated function in {}",
                    file.display()
                )
            }
            CompileFix::RevertBodyToTodo {
                file,
                fn_signature_line,
                todo_description,
            } => {
                format!(
                    "revert body to todo!(\"{todo_description}\") at {}:{fn_signature_line} for build-agent re-implementation",
                    file.display()
                )
            }
        }
    }
}

/// Walk the structured rustc diagnostics and produce `ApplyRustcSuggestion` fixes for every
/// span that carries a `suggested_replacement`. Only `MachineApplicable` suggestions are
/// accepted by default — rustc itself reserves that label for patches it is willing to apply
/// automatically (this is the same bar `cargo fix` uses).
///
/// Also emits `RevertBodyToTodo` fixes for E0277 "`?` in a non-`Result` method" errors, which
/// rustc can't deterministically rewrite — the only safe recovery is to wipe the body and let
/// the build-agent re-implement it with the correct control flow.
pub(crate) fn diagnostic_suggestions_to_fixes(diagnostics: &[RustcDiagnostic]) -> Vec<CompileFix> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();

    // Tier 1: every MachineApplicable suggestion.
    for diag in diagnostics {
        if diag.level != "error" {
            continue;
        }
        collect_suggestions_in_diagnostic(
            diag,
            &diag.message,
            &["MachineApplicable"],
            &mut out,
            &mut seen,
        );
    }

    // Tier 2: fall back to MaybeIncorrect only when no MachineApplicable patches landed.
    // Rustc uses this label for the "consider importing this trait" family
    // (`use rand::RngExt;`, `use std::convert::From;`, …) which is overwhelmingly correct in
    // practice. Applying rustc's own version is strictly safer than the curated
    // `known_trait_methods` table, which drifts with crate versions.
    if out.is_empty() {
        for diag in diagnostics {
            if diag.level != "error" {
                continue;
            }
            collect_suggestions_in_diagnostic(
                diag,
                &diag.message,
                &["MaybeIncorrect"],
                &mut out,
                &mut seen,
            );
        }
    }

    // RevertBodyToTodo is orthogonal to suggestion tiers — always consider.
    for diag in diagnostics {
        if diag.level != "error" {
            continue;
        }
        if let Some(fix) = detect_question_mark_in_non_result(diag) {
            let key = fix.description();
            if seen.insert(key) {
                out.push(fix);
            }
        }
    }

    out
}

/// Recognize the family of errors that produce:
/// ```text
/// error[E0277]: the `?` operator can only be used in ... that returns `Result` or `Option`
/// ```
/// When found, locate the enclosing `fn ... {` line so `apply_compile_fix` can brace-balance
/// the body and replace it with `todo!(...)`.
fn detect_question_mark_in_non_result(diag: &RustcDiagnostic) -> Option<CompileFix> {
    if diag.code.as_deref() != Some("E0277") {
        return None;
    }
    if !diag.message.contains("`?` operator can only be used") {
        return None;
    }
    // The primary span points at the `?` token; its enclosing function's signature line is
    // reported in one of the `notes`/`help` children as "this function should return `Result`"
    // with a span starting at the `fn` keyword. Prefer that; fall back to the primary span's
    // start line (the signature is usually a few lines above — we search upward in the file
    // when applying the fix).
    let primary = diag.spans.iter().find(|s| s.is_primary)?;

    let mut fn_sig_line: Option<(String, usize)> = None;
    for child in &diag.children {
        for span in &child.spans {
            if span.file_name != primary.file_name {
                continue;
            }
            // Rustc tends to label this exact span with phrases like "this function should return
            // `Result` or `Option`..." — accept any span in the same file whose start line is at
            // or before the primary.
            if span.line_start <= primary.line_start {
                fn_sig_line = Some((span.file_name.clone(), span.line_start));
            }
        }
    }
    let (file, fn_line) =
        fn_sig_line.unwrap_or_else(|| (primary.file_name.clone(), primary.line_start));

    Some(CompileFix::RevertBodyToTodo {
        file: PathBuf::from(file),
        fn_signature_line: fn_line,
        todo_description: "revert body: ? used in a non-Result return type".to_string(),
    })
}

fn collect_suggestions_in_diagnostic(
    diag: &RustcDiagnostic,
    parent_message: &str,
    allowed_applicabilities: &[&str],
    out: &mut Vec<CompileFix>,
    seen: &mut BTreeSet<String>,
) {
    for span in &diag.spans {
        let (Some(replacement), Some(applicability)) = (
            span.suggested_replacement.as_ref(),
            span.suggestion_applicability.as_ref(),
        ) else {
            continue;
        };
        if !allowed_applicabilities.contains(&applicability.as_str()) {
            continue;
        }
        let fix = CompileFix::ApplyRustcSuggestion {
            file: PathBuf::from(&span.file_name),
            line_start: span.line_start,
            line_end: span.line_end,
            column_start: span.column_start,
            column_end: span.column_end,
            replacement: replacement.clone(),
            applicability: applicability.clone(),
            summary: parent_message.to_string(),
        };
        let key = format!(
            "{}|{}|{}|{}|{}|{}",
            span.file_name,
            span.line_start,
            span.line_end,
            span.column_start,
            span.column_end,
            replacement
        );
        if seen.insert(key) {
            out.push(fix);
        }
    }
    for child in &diag.children {
        collect_suggestions_in_diagnostic(
            child,
            parent_message,
            allowed_applicabilities,
            out,
            seen,
        );
    }
}

/// Union the two deterministic fix sources — rustc's own JSON suggestions and the string-match
/// parsers — preserving the order (JSON first, string-match second) and deduplicating by
/// fix-description.
///
/// The two passes cover largely disjoint error families: JSON handles MachineApplicable/
/// MaybeIncorrect patches rustc is willing to apply itself (type casts, trait imports, etc.);
/// string-match covers structural edits rustc can't suggest (stripping `.unwrap()` on plain
/// `T`, converting a helper to `Self::` to break a borrow conflict, reverting a broken body
/// to `todo!()`). Running only the JSON pass when it happens to find *any* suggestion — which
/// is what an earlier "prefer JSON, fall back otherwise" layering did — silently hid every
/// string-match fix for the rest of that round, leaving orthogonal errors (most visibly the
/// E0502 `cannot borrow self.X as mutable` pattern) stuck across retries.
pub(crate) fn collect_all_compile_fixes(
    workspace: &Workspace,
    stderr: &str,
    diagnostics: &[RustcDiagnostic],
) -> Vec<CompileFix> {
    let mut out = diagnostic_suggestions_to_fixes(diagnostics);
    let mut seen: BTreeSet<String> = out.iter().map(|fix| fix.description()).collect();
    for fix in parse_compile_errors(workspace, stderr) {
        let key = fix.description();
        if seen.insert(key) {
            out.push(fix);
        }
    }
    out
}

pub(crate) fn parse_compile_errors(workspace: &Workspace, stderr: &str) -> Vec<CompileFix> {
    let mut fixes = Vec::new();
    let mut seen = BTreeSet::new();
    let mut pending_derives: std::collections::BTreeMap<PathBuf, BTreeSet<String>> =
        std::collections::BTreeMap::new();
    for line in stderr.lines() {
        if let Some(fix) = parse_derive_error(line, "E0204", "Copy") {
            let key = format!("{}:{}", fix.description(), "");
            if seen.insert(key) {
                fixes.push(fix);
            }
        }
        if let Some(fix) = parse_derive_error(line, "E0277", "Debug") {
            let key = format!("{}:{}", fix.description(), "");
            if seen.insert(key) {
                fixes.push(fix);
            }
        }
        if line.contains("error: lifetime may not live long enough")
            && let Some(fix) = parse_lifetime_error(line)
        {
            let key = format!("{}:{}", fix.description(), "");
            if seen.insert(key) {
                fixes.push(fix);
            }
        }
        if let Some(fix) = parse_undeclared_local_type_error(workspace, line) {
            let key = format!("{}:{}", fix.description(), "");
            if seen.insert(key) {
                fixes.push(fix);
            }
        }
        if let Some(fix) = parse_missing_rand_thread_rng_error(line) {
            let key = format!("{}:{}", fix.description(), "");
            if seen.insert(key) {
                fixes.push(fix);
            }
        }
        if let Some(fix) = parse_owned_argument_borrow_mismatch(line) {
            let key = format!("{}:{}", fix.description(), "");
            if seen.insert(key) {
                fixes.push(fix);
            }
        }
        if let Some(fix) = parse_method_rename_error(line) {
            let key = format!("{}:{}", fix.description(), "");
            if seen.insert(key) {
                fixes.push(fix);
            }
        }
        if let Some(fix) = parse_spurious_unwrap_error(line) {
            let key = format!("{}:{}", fix.description(), "");
            if seen.insert(key) {
                fixes.push(fix);
            }
        }
        if let Some(fix) = parse_trait_method_missing_error(line) {
            let key = format!("{}:{}", fix.description(), "");
            if seen.insert(key) {
                fixes.push(fix);
            }
        }
        if let Some(fix) = parse_local_missing_method_error(workspace, line) {
            let key = format!("{}:{}", fix.description(), "");
            if seen.insert(key) {
                fixes.push(fix);
            }
        }
        if let Some(fix) = parse_syntax_error_revert_body(workspace, line) {
            let key = format!("{}:{}", fix.description(), "");
            if seen.insert(key) {
                fixes.push(fix);
            }
        }
        if let Some(fix) = parse_self_borrow_helper_conflict(workspace, line) {
            let key = format!("{}:{}", fix.description(), "");
            if seen.insert(key) {
                fixes.push(fix);
            }
        }
        if let Some(fix) = parse_unresolved_external_crate_error(workspace, line) {
            let key = format!("{}:{}", fix.description(), "");
            if seen.insert(key) {
                fixes.push(fix);
            }
        }
        // Add-derive fixes accumulate across all lines so we coalesce by file.
        for (file, traits) in parse_missing_derive_hints(workspace, line) {
            let entry = pending_derives.entry(file).or_default();
            for trait_name in traits {
                entry.insert(trait_name);
            }
        }
    }
    for (file, traits) in pending_derives {
        let trait_names: Vec<String> = traits.into_iter().collect();
        let fix = CompileFix::AddDerive {
            file: file.clone(),
            trait_names,
        };
        let key = fix.description();
        if seen.insert(key) {
            fixes.push(fix);
        }
    }
    fixes
}

/// Parse `src/data/snake.rs:3:17: error[E0204]: ...` style lines
fn parse_derive_error(line: &str, error_code: &str, trait_name: &str) -> Option<CompileFix> {
    let marker = format!("error[{error_code}]:");
    if !line.contains(&marker) {
        return None;
    }
    let (location, _) = line.split_once(": error")?;
    let parts: Vec<&str> = location.split(':').collect();
    if parts.len() < 2 {
        return None;
    }
    let file = PathBuf::from(parts[0]);
    let line_no: usize = parts[1].parse().ok()?;
    Some(CompileFix::RemoveDerive {
        file,
        line: line_no,
        trait_name: trait_name.to_string(),
    })
}

/// Parse `src/contexts/game_loop.rs:28:16: error: lifetime may not live long enough`
fn parse_lifetime_error(line: &str) -> Option<CompileFix> {
    let (location, _) = line.split_once(": error:")?;
    let parts: Vec<&str> = location.split(':').collect();
    if parts.len() < 2 {
        return None;
    }
    let file = PathBuf::from(parts[0]);
    let line_no: usize = parts[1].parse().ok()?;
    Some(CompileFix::AddLifetimeToMethod {
        file,
        line: line_no,
    })
}

fn parse_undeclared_local_type_error(workspace: &Workspace, line: &str) -> Option<CompileFix> {
    if !line.contains("error[E0433]:") || !line.contains("use of undeclared type `") {
        return None;
    }
    let (location, _) = line.split_once(": error")?;
    let parts: Vec<&str> = location.split(':').collect();
    if parts.len() < 2 {
        return None;
    }
    let file = PathBuf::from(parts[0]);
    let line_no: usize = parts[1].parse().ok()?;
    let marker = "use of undeclared type `";
    let start = line.find(marker)? + marker.len();
    let end = line[start..].find('`')? + start;
    let type_name = line[start..end].trim();
    if type_name.is_empty() || !has_unique_local_type_declaration(workspace, type_name, &file) {
        return None;
    }
    Some(CompileFix::AddLocalTypeImport {
        file,
        line: line_no,
        type_name: type_name.to_string(),
    })
}

fn parse_missing_rand_thread_rng_error(line: &str) -> Option<CompileFix> {
    if !line.contains("error[E0425]:")
        || !line.contains("cannot find function `thread_rng` in crate `rand`")
    {
        return None;
    }
    let (location, _) = line.split_once(": error")?;
    let parts: Vec<&str> = location.split(':').collect();
    if parts.len() < 2 {
        return None;
    }
    let file = PathBuf::from(parts[0]);
    let line_no: usize = parts[1].parse().ok()?;
    Some(CompileFix::ReplaceRandThreadRngCall {
        file,
        line: line_no,
    })
}

/// Parse rustc error lines that imply a missing derive on a locally-declared type.
///
/// Handles three families:
/// - `error[E0277]: the trait bound `module::Type: Trait` is not satisfied`
/// - `error[E0369]: binary operation `==`/`!=` cannot be applied to type `module::Type``
/// - `help: consider annotating ``module::Type`` with `#[derive(Trait[, Trait2, ...])]``
///
/// Returns `(declaration_file, trait_names)` pairs; the caller coalesces per-file.
fn parse_missing_derive_hints(workspace: &Workspace, line: &str) -> Vec<(PathBuf, Vec<String>)> {
    let mut results = Vec::new();
    let trimmed = line.trim_start();

    if let Some(help_info) = parse_help_consider_annotating(trimmed)
        && let Some(file) = locate_local_type_file(workspace, &help_info.0)
    {
        results.push((file, help_info.1));
    }

    if trimmed.contains("error[E0277]:")
        && trimmed.contains("the trait bound `")
        && trimmed.contains("is not satisfied")
        && let Some((type_ref, trait_name)) = parse_e0277_trait_bound(trimmed)
        && is_derivable_trait(&trait_name)
        && let Some(file) = locate_local_type_file(workspace, &type_ref)
    {
        results.push((file, vec![trait_name]));
    }

    if trimmed.contains("error[E0369]:")
        && (trimmed.contains("binary operation `==`") || trimmed.contains("binary operation `!=`"))
        && let Some(type_ref) = parse_e0369_operand_type(trimmed)
        && let Some(file) = locate_local_type_file(workspace, &type_ref)
    {
        results.push((file, vec!["PartialEq".to_string()]));
    }

    if trimmed.contains("error[E0507]:")
        && let Some(type_ref) = parse_e0507_copy_operand_type(trimmed)
        && let Some(file) = locate_local_type_file(workspace, &type_ref)
    {
        results.push((file, vec!["Clone".to_string(), "Copy".to_string()]));
    }

    results
}

fn parse_help_consider_annotating(line: &str) -> Option<(String, Vec<String>)> {
    let idx = line.find("help: consider annotating `")?;
    let after = &line[idx + "help: consider annotating `".len()..];
    let end = after.find('`')?;
    let type_ref = after[..end].to_string();
    let marker = "with `#[derive(";
    let mid = after.find(marker)?;
    let after_marker = &after[mid + marker.len()..];
    let close = after_marker.find(")]`")?;
    let traits_str = &after_marker[..close];
    let traits: Vec<String> = traits_str
        .split(',')
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect();
    if traits.is_empty() {
        return None;
    }
    Some((type_ref, traits))
}

fn parse_e0277_trait_bound(line: &str) -> Option<(String, String)> {
    let marker = "the trait bound `";
    let start = line.find(marker)? + marker.len();
    let tail = &line[start..];
    let close = tail.find('`')?;
    let inner = &tail[..close];
    let (type_ref, trait_name) = inner.rsplit_once(':')?;
    Some((type_ref.trim().to_string(), trait_name.trim().to_string()))
}

fn parse_e0369_operand_type(line: &str) -> Option<String> {
    let marker = "cannot be applied to type `";
    let start = line.find(marker)? + marker.len();
    let tail = &line[start..];
    let close = tail.find('`')?;
    Some(tail[..close].to_string())
}

fn parse_e0507_copy_operand_type(line: &str) -> Option<String> {
    if !line.contains("does not implement the `Copy` trait")
        && !line.contains("does not implement the Copy trait")
    {
        return None;
    }
    let marker = "type `";
    if let Some(start) = line.find(marker) {
        let start = start + marker.len();
        let tail = &line[start..];
        let close = tail.find('`')?;
        return Some(tail[..close].to_string());
    }
    let marker = "type ";
    let start = line.find(marker)? + marker.len();
    let tail = &line[start..];
    let close = tail.find(", which")?;
    Some(tail[..close].trim().to_string())
}

fn is_derivable_trait(trait_name: &str) -> bool {
    matches!(
        trait_name,
        "Eq" | "Hash" | "PartialEq" | "PartialOrd" | "Ord" | "Copy" | "Clone" | "Debug" | "Default"
    )
}

fn locate_local_type_file(workspace: &Workspace, type_ref: &str) -> Option<PathBuf> {
    // `module::Type` or just `Type` — take the last segment.
    let simple = type_ref.rsplit("::").next()?.trim();
    if simple.is_empty() {
        return None;
    }
    let mut matches = Vec::new();
    let src_dir = workspace.root.join("src");
    collect_local_type_declarations(&src_dir, simple, &mut matches);
    if matches.len() != 1 {
        return None;
    }
    matches.into_iter().next().and_then(|path| {
        path.strip_prefix(&workspace.root)
            .ok()
            .map(Path::to_path_buf)
    })
}

/// Curated crate-API migration table. Returns a ReplaceMethodCall fix when a known-renamed
/// method is referenced in a "method not found" compile error.
///
/// Keep this table small and conservative — only entries whose replacement is universally correct
/// across current versions.
fn parse_method_rename_error(line: &str) -> Option<CompileFix> {
    let rename_map: &[(&str, &str, &str)] = &[
        // (error marker, from, to)
        (
            "no method named `gen_range` found",
            "gen_range",
            "random_range",
        ),
        (
            "no function or associated item named `gen_range` found",
            "gen_range",
            "random_range",
        ),
    ];
    for (marker, from, to) in rename_map {
        if line.contains(marker) {
            let (location, _) = line.split_once(": error")?;
            let parts: Vec<&str> = location.split(':').collect();
            if parts.len() < 2 {
                continue;
            }
            let file = PathBuf::from(parts[0]);
            let line_no: usize = parts[1].parse().ok()?;
            return Some(CompileFix::ReplaceMethodCall {
                file,
                line: line_no,
                from: from.to_string(),
                to: to.to_string(),
            });
        }
    }
    None
}

/// Detect `.unwrap()` / `.expect(...)` / `.unwrap_or_default()` applied to values whose type is
/// not `Option`/`Result` — rustc reports these as `no method named 'unwrap' found for struct X`.
///
/// The generated build-agent body probably guessed that a getter returns `Option<T>` when it
/// actually returns `T` directly. Stripping the unwrap is conservative: the value is already the
/// inner type, so the surrounding expression keeps compiling as-is.
fn parse_spurious_unwrap_error(line: &str) -> Option<CompileFix> {
    if !line.contains("error[E0599]:") {
        return None;
    }
    let method = if line.contains("no method named `unwrap` found") {
        "unwrap"
    } else if line.contains("no method named `expect` found") {
        "expect"
    } else if line.contains("no method named `unwrap_or_default` found") {
        "unwrap_or_default"
    } else if line.contains("no method named `unwrap_or` found") {
        "unwrap_or"
    } else {
        return None;
    };
    // Only trigger when the receiver type is NOT `Option`/`Result`. The error reads
    // "found for struct/enum/reference/type `X`" — skip the fix if `X` mentions Option/Result.
    let for_marker = " found for ";
    if let Some(idx) = line.find(for_marker) {
        let tail = &line[idx + for_marker.len()..];
        let lowered = tail.to_lowercase();
        if lowered.contains("option<") || lowered.contains("result<") {
            return None;
        }
    }
    let (location, _) = line.split_once(": error")?;
    let parts: Vec<&str> = location.split(':').collect();
    if parts.len() < 2 {
        return None;
    }
    let file = PathBuf::from(parts[0]);
    let line_no: usize = parts[1].parse().ok()?;
    Some(CompileFix::StripUnwrapCall {
        file,
        line: line_no,
        method: method.to_string(),
    })
}

/// Known trait methods that come into scope only via `use <trait>;`. Used as a last-resort
/// fallback when rustc reports "no method named X found for struct Y" and Y is known to
/// implement the trait in question, *and* rustc did not emit its own structured suggestion
/// (the JSON-diagnostic pipeline is preferred because it tracks upstream renames — e.g. in
/// `rand` 0.9 the trait became `rand::RngExt` and the compiler's suggestion reflects that).
///
/// Only legacy-API entries remain here so the table can't produce a wrong import when the
/// on-disk `rand` has moved on. Modern API entries (`random_range`, `random_bool`, …) rely on
/// rustc's `MaybeIncorrect` suggestion that the JSON pipeline now consumes.
fn known_trait_methods() -> &'static [(&'static str, &'static str, &'static [&'static str])] {
    // (method_name, trait_path, receiver_type_substrings that indicate this trait)
    &[(
        "gen_range",
        "rand::Rng",
        &["ThreadRng", "rand::rngs::", "rand::prelude::"],
    )]
}

fn parse_trait_method_missing_error(line: &str) -> Option<CompileFix> {
    if !line.contains("error[E0599]:") || !line.contains("no method named `") {
        return None;
    }
    let method_marker = "no method named `";
    let start = line.find(method_marker)? + method_marker.len();
    let end = line[start..].find('`')? + start;
    let method = &line[start..end];

    // Identify the receiver type after "found for ...".
    let for_marker = " found for ";
    let for_idx = line.find(for_marker)?;
    let receiver_tail = &line[for_idx + for_marker.len()..];

    for (known_method, trait_path, receiver_hints) in known_trait_methods() {
        if *known_method != method {
            continue;
        }
        if !receiver_hints
            .iter()
            .any(|hint| receiver_tail.contains(hint))
        {
            continue;
        }
        let (location, _) = line.split_once(": error")?;
        let parts: Vec<&str> = location.split(':').collect();
        if parts.len() < 2 {
            return None;
        }
        let file = PathBuf::from(parts[0]);
        return Some(CompileFix::AddTraitImport {
            file,
            trait_path: trait_path.to_string(),
        });
    }
    None
}

fn parse_local_missing_method_error(workspace: &Workspace, line: &str) -> Option<CompileFix> {
    if !line.contains("error[E0599]:") {
        return None;
    }
    let method_marker = if line.contains("no method named `") {
        "no method named `"
    } else if line.contains("no function or associated item named `") {
        "no function or associated item named `"
    } else {
        return None;
    };
    let method_start = line.find(method_marker)? + method_marker.len();
    let method_end = line[method_start..].find('`')? + method_start;
    let method_name = line[method_start..method_end].trim();
    if method_name.is_empty() {
        return None;
    }
    // `Option`/`Result` accessor calls on a plain `T` look like E0599 "no method named `unwrap`
    // found for struct `Foo`" — same error code as a genuinely missing local method. Those cases
    // are already owned by `parse_spurious_unwrap_error`, which produces a conservative
    // `StripUnwrapCall` fix. If we also queued a `RevertBodyToTodo` here the two would be applied
    // in sequence and the whole enclosing function body would be discarded (undoing the clean
    // strip and forcing an unnecessary LLM re-implementation). Skip those methods so the
    // spurious-unwrap fixer is the sole owner.
    if is_option_result_shape_method(method_name) {
        return None;
    }
    let receiver_type = extract_missing_method_receiver_type(line)?;
    let _type_file = locate_local_type_file(workspace, &receiver_type)?;
    let (file, line_no) = parse_line_location(line)?;
    let content = fs::read_to_string(workspace.root.join(&file)).ok()?;
    let fn_signature_line = find_enclosing_fn_signature_line(&content, line_no)?;
    let simple = receiver_type
        .rsplit("::")
        .next()
        .unwrap_or(receiver_type.as_str())
        .trim();
    Some(CompileFix::RevertBodyToTodo {
        file,
        fn_signature_line,
        todo_description: format!(
            "re-implement body: called nonexistent local method `{method_name}` on `{simple}`"
        ),
    })
}

/// Methods that only make sense on `Option`/`Result`. When rustc reports "no method named X
/// found for struct/enum T" with one of these names, T is a plain value that was never wrapped
/// — the strip/conversion is handled by dedicated fixers (`parse_spurious_unwrap_error`) and
/// must NOT trigger a body revert.
fn is_option_result_shape_method(method: &str) -> bool {
    matches!(
        method,
        "unwrap"
            | "expect"
            | "unwrap_or"
            | "unwrap_or_default"
            | "unwrap_or_else"
            | "ok"
            | "err"
            | "is_some"
            | "is_none"
            | "is_ok"
            | "is_err"
            | "ok_or"
            | "ok_or_else"
    )
}

fn extract_missing_method_receiver_type(line: &str) -> Option<String> {
    for marker in [
        " found for struct `",
        " found for enum `",
        " found for reference `",
        " found for mutable reference `",
        " found for type `",
    ] {
        let Some(start) = line.find(marker) else {
            continue;
        };
        let start = start + marker.len();
        let tail = &line[start..];
        let Some(end) = tail.find('`') else {
            continue;
        };
        return Some(tail[..end].trim().to_string());
    }
    None
}

/// Recognize parse-level errors that rustc reports without an error code, e.g.
///
/// ```text
/// src/contexts/game_loop.rs:42:70: error: expected expression, found `.`
/// src/contexts/game_loop.rs:42:70: error: expected one of `,`, `.`, …, found `'f'`
/// ```
///
/// These almost always come from an LLM emitting token-order bugs inside a generated function
/// body (stray commas, misplaced `.clone()`, dangling tokens). The whole body is syntactically
/// invalid so no surgical patch is safe; reverting it to `todo!("…")` lets the build agent
/// re-implement the function from scratch. We deliberately scope this to a small whitelist of
/// parse-error phrasings so we don't accidentally claim errors that carry a specific code (those
/// are owned by the structured parsers above).
fn parse_syntax_error_revert_body(workspace: &Workspace, line: &str) -> Option<CompileFix> {
    // Skip anything with a rustc error code — those have dedicated handlers.
    if line.contains("error[") {
        return None;
    }
    // `parse_lifetime_error` already claims this one.
    if line.contains("lifetime may not live long enough") {
        return None;
    }
    // The " error:" pattern has to exist and be followed by a recognized parse-error phrase.
    let error_body = line.split(": error:").nth(1)?;
    let lowered = error_body.to_lowercase();
    let recognized = [
        "expected expression, found",
        "expected one of",
        "expected `;`",
        "expected identifier, found",
        "expected pattern, found",
        "expected item, found",
        "unexpected token",
        "mismatched closing delimiter",
        "unclosed delimiter",
    ];
    if !recognized.iter().any(|phrase| lowered.contains(phrase)) {
        return None;
    }
    let (file, line_no) = parse_line_location(line)?;
    let content = fs::read_to_string(workspace.root.join(&file)).ok()?;
    let fn_signature_line = find_enclosing_fn_signature_line(&content, line_no)?;
    Some(CompileFix::RevertBodyToTodo {
        file,
        fn_signature_line,
        todo_description: format!(
            "re-implement body: parse error near line {line_no} — the LLM emitted invalid Rust tokens"
        ),
    })
}

fn parse_self_borrow_helper_conflict(workspace: &Workspace, line: &str) -> Option<CompileFix> {
    if !line.contains("error[E0502]:")
        || !line.contains("cannot borrow `self.")
        || !line.contains("as mutable because it is also borrowed as immutable")
    {
        return None;
    }
    let (file, line_no) = parse_line_location(line)?;
    let path = workspace.root.join(&file);
    let content = fs::read_to_string(&path).ok()?;
    let source_line = content
        .lines()
        .nth(line_no.checked_sub(1)?)
        .unwrap_or_default();
    let call_re = regex::Regex::new(
        r"self\.([A-Za-z_][A-Za-z0-9_]*)\(\s*&mut\s+self\.([A-Za-z_][A-Za-z0-9_]*)\s*\)",
    )
    .ok()?;
    let captures = call_re.captures(source_line)?;
    let method_name = captures.get(1)?.as_str().to_string();
    if !helper_can_drop_receiver(&content, &method_name) {
        return None;
    }
    Some(CompileFix::ConvertHelperToAssociatedFn { file, method_name })
}

/// Detect `error[E0433]: failed to resolve: use of unresolved module or unlinked crate `X``.
/// If `X` is declared in `drafts/capability_registry.yml`, we can auto-add the dep.
fn parse_unresolved_external_crate_error(workspace: &Workspace, line: &str) -> Option<CompileFix> {
    if !line.contains("error[E0433]:") || !line.contains("unlinked crate `") {
        return None;
    }
    let marker = "unlinked crate `";
    let start = line.find(marker)? + marker.len();
    let end = line[start..].find('`')? + start;
    let crate_root = line[start..end].trim().to_string();
    if crate_root.is_empty()
        || matches!(
            crate_root.as_str(),
            "std" | "core" | "alloc" | "crate" | "self" | "super"
        )
    {
        return None;
    }
    if !crate_root_in_registry(workspace, &crate_root) {
        return None;
    }
    Some(CompileFix::AddExternalCrate { crate_root })
}

fn crate_root_in_registry(workspace: &Workspace, crate_root: &str) -> bool {
    let registry_path = workspace.drafts_dir.join("capability_registry.yml");
    let Ok(raw) = fs::read_to_string(&registry_path) else {
        return false;
    };
    let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&raw) else {
        return false;
    };
    let Some(mapping) = value.as_mapping() else {
        return false;
    };
    let Some(providers) = mapping
        .get(serde_yaml::Value::String("providers".to_string()))
        .and_then(serde_yaml::Value::as_sequence)
    else {
        return false;
    };
    providers.iter().any(|provider| {
        provider
            .as_mapping()
            .and_then(|m| m.get(serde_yaml::Value::String("crate".to_string())))
            .and_then(serde_yaml::Value::as_str)
            .is_some_and(|name| name == crate_root)
    })
}

fn parse_owned_argument_borrow_mismatch(line: &str) -> Option<CompileFix> {
    if !line.contains("error[E0308]:")
        || !line.contains("expected `")
        || !line.contains(", found `&")
    {
        return None;
    }
    let (location, _) = line.split_once(": error")?;
    let parts: Vec<&str> = location.split(':').collect();
    if parts.len() < 2 {
        return None;
    }
    let file = PathBuf::from(parts[0]);
    let line_no: usize = parts[1].parse().ok()?;
    let expected_marker = "expected `";
    let expected_start = line.find(expected_marker)? + expected_marker.len();
    let expected_end = line[expected_start..].find('`')? + expected_start;
    let type_name = line[expected_start..expected_end].trim();
    let found_marker = ", found `&";
    let found_start = line.find(found_marker)? + found_marker.len();
    let found_end = line[found_start..].find('`')? + found_start;
    let found_type = line[found_start..found_end].trim();
    if type_name.is_empty() || found_type != type_name {
        return None;
    }
    Some(CompileFix::RemoveBorrowForOwnedArgument {
        file,
        line: line_no,
        type_name: type_name.to_string(),
    })
}

fn parse_line_location(line: &str) -> Option<(PathBuf, usize)> {
    let (location, _) = line.split_once(": error")?;
    let parts: Vec<&str> = location.split(':').collect();
    if parts.len() < 2 {
        return None;
    }
    let file = PathBuf::from(parts[0]);
    let line_no: usize = parts[1].parse().ok()?;
    Some((file, line_no))
}

fn find_enclosing_fn_signature_line(content: &str, hint_line: usize) -> Option<usize> {
    let lines: Vec<&str> = content.lines().collect();
    if hint_line == 0 || hint_line > lines.len() {
        return None;
    }
    for idx in (0..hint_line).rev() {
        if looks_like_fn_signature(lines[idx]) {
            return Some(idx + 1);
        }
    }
    None
}

fn helper_can_drop_receiver(content: &str, method_name: &str) -> bool {
    let Some((signature_line, body_start, body_end)) =
        find_method_signature_and_body(content, method_name)
    else {
        return false;
    };
    let signature = content
        .lines()
        .nth(signature_line.saturating_sub(1))
        .unwrap_or("");
    if !signature.contains("(&self")
        && !signature.contains("(&mut self")
        && !signature.contains("(self")
    {
        return false;
    }
    let body = &content[body_start..body_end];
    !body.contains("self.")
}

fn convert_helper_to_associated_fn(content: &str, method_name: &str) -> Result<String> {
    if !helper_can_drop_receiver(content, method_name) {
        anyhow::bail!("helper `{method_name}` still uses `self` in its body");
    }
    let Some((signature_line, _, _)) = find_method_signature_and_body(content, method_name) else {
        anyhow::bail!("could not find helper `{method_name}`");
    };
    let mut lines: Vec<String> = content.lines().map(String::from).collect();
    let idx = signature_line.saturating_sub(1);
    lines[idx] = remove_receiver_from_signature_line(&lines[idx], method_name);
    for line in &mut lines {
        if line.trim_start().starts_with("///") || line.trim_start().starts_with("//!") {
            continue;
        }
        let needle = format!("self.{method_name}(");
        if line.contains(&needle) {
            *line = line.replace(&needle, &format!("Self::{method_name}("));
        }
    }
    Ok(lines.join("\n") + "\n")
}

fn remove_receiver_from_signature_line(line: &str, method_name: &str) -> String {
    let needle = format!("fn {method_name}");
    let Some(fn_idx) = line.find(&needle) else {
        return line.to_string();
    };
    let Some(open_idx_rel) = line[fn_idx..].find('(') else {
        return line.to_string();
    };
    let open_idx = fn_idx + open_idx_rel;
    let prefix = &line[..open_idx + 1];
    let after = &line[open_idx + 1..];
    for pattern in ["&self, ", "&mut self, ", "self, "] {
        if let Some(rest) = after.strip_prefix(pattern) {
            return format!("{prefix}{rest}");
        }
    }
    for pattern in ["&self", "&mut self", "self"] {
        if let Some(rest) = after.strip_prefix(pattern) {
            return format!("{prefix}{rest}");
        }
    }
    line.to_string()
}

fn find_method_signature_and_body(
    content: &str,
    method_name: &str,
) -> Option<(usize, usize, usize)> {
    let lines: Vec<&str> = content.lines().collect();
    let signature_line = lines.iter().enumerate().find_map(|(idx, line)| {
        let trimmed = line.trim_start();
        if trimmed.starts_with(&format!("fn {method_name}("))
            || trimmed.starts_with(&format!("fn {method_name}<"))
            || trimmed.starts_with(&format!("pub fn {method_name}("))
            || trimmed.starts_with(&format!("pub fn {method_name}<"))
        {
            Some(idx + 1)
        } else {
            None
        }
    })?;

    let lines = content.lines().collect::<Vec<_>>();
    let sig_idx = signature_line - 1;
    let mut open_line = None;
    let mut open_col_byte = None;
    if let Some(pos) = lines[sig_idx].find('{') {
        open_line = Some(sig_idx);
        open_col_byte = Some(pos);
    } else {
        for (idx, line) in lines.iter().enumerate().skip(sig_idx + 1) {
            if let Some(pos) = line.find('{') {
                open_line = Some(idx);
                open_col_byte = Some(pos);
                break;
            }
        }
    }
    let open_line = open_line?;
    let open_col_byte = open_col_byte?;
    let body_open_byte = byte_offset_of_line_column(content, open_line + 1, open_col_byte + 1)?;
    let bytes = content.as_bytes();
    let mut depth = 1i32;
    let mut i = body_open_byte + 1;
    while i < bytes.len() && depth > 0 {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    if depth != 0 {
        return None;
    }
    Some((signature_line, body_open_byte + 1, i - 1))
}

pub(crate) fn apply_compile_fix(workspace: &Workspace, fix: &CompileFix) -> Result<()> {
    match fix {
        CompileFix::RemoveDerive {
            file,
            line,
            trait_name,
        } => {
            let path = workspace.root.join(file);
            let content = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            let mut lines: Vec<String> = content.lines().map(String::from).collect();
            let idx = line.checked_sub(1).context("invalid line number")?;
            if idx < lines.len() {
                lines[idx] = remove_derive_trait(&lines[idx], trait_name);
            }
            let result = lines.join("\n") + "\n";
            fs::write(&path, result).with_context(|| format!("Failed to write {}", path.display()))
        }
        CompileFix::AddLifetimeToMethod { file, line } => {
            let path = workspace.root.join(file);
            let content = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            let mut lines: Vec<String> = content.lines().map(String::from).collect();
            let sig_idx = line.checked_sub(2).unwrap_or(0);
            if sig_idx < lines.len() && lines[sig_idx].contains("fn ") {
                lines[sig_idx] = add_lifetime_to_signature(&lines[sig_idx]);
            }
            let result = lines.join("\n") + "\n";
            fs::write(&path, result).with_context(|| format!("Failed to write {}", path.display()))
        }
        CompileFix::AddLocalTypeImport {
            file, type_name, ..
        } => {
            let path = workspace.root.join(file);
            let content = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            // `src/main.rs` is the binary crate root; `crate::` there refers to the binary and
            // cannot see library-crate types. Import via the library crate name instead.
            let import_root = if file == Path::new("src/main.rs") {
                library_crate_name(workspace).unwrap_or_else(|| "crate".to_string())
            } else {
                "crate".to_string()
            };
            let updated =
                add_crate_import(&content, type_name, &import_root).unwrap_or(content);
            fs::write(&path, updated).with_context(|| format!("Failed to write {}", path.display()))
        }
        CompileFix::ReplaceRandThreadRngCall { file, line } => {
            let path = workspace.root.join(file);
            let content = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            let mut lines: Vec<String> = content.lines().map(String::from).collect();
            let idx = line.checked_sub(1).context("invalid line number")?;
            if idx < lines.len() {
                lines[idx] = lines[idx].replace("rand::thread_rng()", "rand::rng()");
            }
            let result = lines.join("\n") + "\n";
            fs::write(&path, result).with_context(|| format!("Failed to write {}", path.display()))
        }
        CompileFix::RemoveBorrowForOwnedArgument {
            file,
            line,
            type_name,
        } => {
            let path = workspace.root.join(file);
            let content = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            let mut lines: Vec<String> = content.lines().map(String::from).collect();
            let idx = line.checked_sub(1).context("invalid line number")?;
            if idx < lines.len() {
                lines[idx] = remove_borrow_for_owned_argument(&lines[idx], type_name);
            }
            let result = lines.join("\n") + "\n";
            fs::write(&path, result).with_context(|| format!("Failed to write {}", path.display()))
        }
        CompileFix::AddDerive { file, trait_names } => {
            let path = workspace.root.join(file);
            let content = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            let updated = add_derives_to_file(&content, trait_names);
            fs::write(&path, updated).with_context(|| format!("Failed to write {}", path.display()))
        }
        CompileFix::ReplaceMethodCall {
            file,
            line,
            from,
            to,
        } => {
            let path = workspace.root.join(file);
            let content = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            let mut lines: Vec<String> = content.lines().map(String::from).collect();
            let idx = line.checked_sub(1).context("invalid line number")?;
            if idx < lines.len() {
                // Replace only the exact method-name token to avoid collateral damage on
                // substrings that happen to contain `from`.
                lines[idx] = replace_whole_token(&lines[idx], from, to);
            }
            let result = lines.join("\n") + "\n";
            fs::write(&path, result).with_context(|| format!("Failed to write {}", path.display()))
        }
        CompileFix::StripUnwrapCall { file, line, method } => {
            let path = workspace.root.join(file);
            let content = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            let mut lines: Vec<String> = content.lines().map(String::from).collect();
            let idx = line.checked_sub(1).context("invalid line number")?;
            if idx < lines.len() {
                lines[idx] = strip_unwrap_call(&lines[idx], method);
            }
            let result = lines.join("\n") + "\n";
            fs::write(&path, result).with_context(|| format!("Failed to write {}", path.display()))
        }
        CompileFix::AddTraitImport { file, trait_path } => {
            let path = workspace.root.join(file);
            let content = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            let updated = ensure_use_statement(&content, trait_path);
            fs::write(&path, updated).with_context(|| format!("Failed to write {}", path.display()))
        }
        CompileFix::AddExternalCrate { crate_root } => {
            // Register in drafts/dependencies.yml via the capability registry.
            let synthetic = format!("{crate_root}::Placeholder");
            let _ = crate::manifest::ensure_external_dependency_for_type(workspace, &synthetic)?;
            // Also patch Cargo.toml if the dep is missing; the build uses that file directly
            // and we don't re-run scaffold between compile-repair rounds.
            patch_cargo_toml_with_dependency(workspace, crate_root)?;
            Ok(())
        }
        CompileFix::RevertBodyToTodo {
            file,
            fn_signature_line,
            todo_description,
        } => {
            let path = workspace.root.join(file);
            let content = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            let updated = revert_fn_body_to_todo(&content, *fn_signature_line, todo_description)
                .with_context(|| {
                    format!(
                        "Failed to revert body to todo!() at {}:{fn_signature_line}",
                        path.display()
                    )
                })?;
            fs::write(&path, updated).with_context(|| format!("Failed to write {}", path.display()))
        }
        CompileFix::ApplyRustcSuggestion {
            file,
            line_start,
            line_end,
            column_start,
            column_end,
            replacement,
            ..
        } => {
            let path = workspace.root.join(file);
            let content = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            let updated = apply_span_replacement(
                &content,
                *line_start,
                *column_start,
                *line_end,
                *column_end,
                replacement,
            )
            .with_context(|| {
                format!(
                    "Failed to apply rustc suggestion at {}:{line_start}:{column_start}",
                    path.display()
                )
            })?;
            fs::write(&path, updated).with_context(|| format!("Failed to write {}", path.display()))
        }
        CompileFix::ConvertHelperToAssociatedFn { file, method_name } => {
            let path = workspace.root.join(file);
            let content = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            let updated = convert_helper_to_associated_fn(&content, method_name).with_context(|| {
                format!(
                    "Failed to convert helper `{method_name}` into an associated function in {}",
                    path.display()
                )
            })?;
            fs::write(&path, updated).with_context(|| format!("Failed to write {}", path.display()))
        }
    }
}

/// Splice `replacement` into `content` at the rustc span `[line_start:column_start,
/// line_end:column_end)`. Rustc columns are 1-based and count UTF-8 characters, not bytes, so
/// we walk the file characterwise to translate them into byte offsets.
pub(crate) fn apply_span_replacement(
    content: &str,
    line_start: usize,
    column_start: usize,
    line_end: usize,
    column_end: usize,
    replacement: &str,
) -> Result<String> {
    let start_byte = locate_byte_offset(content, line_start, column_start)
        .context("start span out of bounds")?;
    let end_byte =
        locate_byte_offset(content, line_end, column_end).context("end span out of bounds")?;
    if end_byte < start_byte {
        anyhow::bail!("end span precedes start span");
    }
    let mut out = String::with_capacity(content.len() + replacement.len());
    out.push_str(&content[..start_byte]);
    out.push_str(replacement);
    out.push_str(&content[end_byte..]);
    Ok(out)
}

/// Find the `fn ...` signature line that encloses `hint_line`, then splice its brace-balanced
/// body with `{ todo!("<description>") }`. Searches upward from `hint_line` if the hint lands
/// inside the body itself (rustc's `?` span is inside the body, not on the signature).
pub(crate) fn revert_fn_body_to_todo(
    content: &str,
    hint_line: usize,
    description: &str,
) -> Result<String> {
    let lines: Vec<&str> = content.lines().collect();
    if hint_line == 0 || hint_line > lines.len() {
        anyhow::bail!("hint line {hint_line} is outside the file");
    }
    // Walk upward to the nearest `fn ` signature line. We stop at the first line that
    // textually contains `fn ` followed by an identifier and an open paren — good enough for
    // reen-generated code which uses a single line per signature.
    let mut sig_idx = None;
    for idx in (0..hint_line).rev() {
        let line = lines[idx];
        if looks_like_fn_signature(line) {
            sig_idx = Some(idx);
            break;
        }
    }
    let sig_idx = sig_idx.context("no enclosing `fn` signature found above the hint line")?;
    let sig_line = lines[sig_idx];
    let indent: String = sig_line.chars().take_while(|c| c.is_whitespace()).collect();

    // Find the `{` that opens the body. It is typically on the signature line; if not, scan
    // forward until we find one.
    let mut open_line = None;
    let mut open_col_byte = None;
    if let Some(pos) = sig_line.find('{') {
        open_line = Some(sig_idx);
        open_col_byte = Some(pos);
    } else {
        for j in (sig_idx + 1)..lines.len() {
            if let Some(pos) = lines[j].find('{') {
                open_line = Some(j);
                open_col_byte = Some(pos);
                break;
            }
        }
    }
    let open_line = open_line.context("no opening brace for function body")?;
    let open_col_byte = open_col_byte.unwrap();

    // Translate (open_line, open_col_byte) to a byte offset in the full content.
    let body_open_byte = byte_offset_of_line_column(content, open_line + 1, open_col_byte + 1)
        .context("failed to resolve body open byte")?;

    // Brace-balance to find the closing `}`.
    let bytes = content.as_bytes();
    let mut depth = 1i32;
    let mut i = body_open_byte + 1;
    while i < bytes.len() && depth > 0 {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    if depth != 0 {
        anyhow::bail!("unbalanced braces while searching for function body end");
    }
    let body_close_byte = i; // one past the `}`

    let sanitized_desc = description.replace('\\', "\\\\").replace('"', "\\\"");
    let replacement = format!("{{\n{indent}    todo!(\"{sanitized_desc}\")\n{indent}}}");

    let mut out = String::with_capacity(content.len());
    out.push_str(&content[..body_open_byte]);
    out.push_str(&replacement);
    out.push_str(&content[body_close_byte..]);
    Ok(out)
}

fn byte_offset_of_line_column(
    content: &str,
    line_1based: usize,
    column_1based: usize,
) -> Option<usize> {
    locate_byte_offset(content, line_1based, column_1based)
}

fn looks_like_fn_signature(line: &str) -> bool {
    let trimmed = line.trim_start();
    // Allow visibility / async / unsafe / pub(crate) prefixes before `fn `.
    trimmed.starts_with("fn ")
        || trimmed.starts_with("pub fn ")
        || trimmed.starts_with("pub(crate) fn ")
        || trimmed.starts_with("pub(super) fn ")
        || trimmed.starts_with("pub(self) fn ")
        || trimmed.starts_with("async fn ")
        || trimmed.starts_with("pub async fn ")
        || trimmed.starts_with("unsafe fn ")
        || trimmed.starts_with("pub unsafe fn ")
}

fn locate_byte_offset(content: &str, line_1based: usize, column_1based: usize) -> Option<usize> {
    if line_1based == 0 || column_1based == 0 {
        return None;
    }
    let target_line = line_1based - 1;
    let mut current_line = 0usize;
    let mut line_start_byte = 0usize;
    for (idx, ch) in content.char_indices() {
        if current_line == target_line {
            // Count characters from the start of this line until we reach `column_1based - 1`.
            let mut chars_seen = 0usize;
            for (byte_offset, _) in content[line_start_byte..].char_indices() {
                if chars_seen == column_1based - 1 {
                    return Some(line_start_byte + byte_offset);
                }
                chars_seen += 1;
            }
            // Column is at end-of-line (or beyond): return the line's terminating byte offset.
            let end_of_line = content[line_start_byte..]
                .find('\n')
                .map(|off| line_start_byte + off)
                .unwrap_or(content.len());
            if chars_seen <= column_1based - 1 {
                return Some(end_of_line);
            }
            return None;
        }
        if ch == '\n' {
            current_line += 1;
            line_start_byte = idx + 1;
        }
    }
    // Empty file or target line equals number-of-lines: the span is at the very end.
    if current_line == target_line {
        let mut chars_seen = 0usize;
        for (byte_offset, _) in content[line_start_byte..].char_indices() {
            if chars_seen == column_1based - 1 {
                return Some(line_start_byte + byte_offset);
            }
            chars_seen += 1;
        }
        if chars_seen <= column_1based - 1 {
            return Some(content.len());
        }
    }
    None
}

/// Strip `.<method>()` or `.<method>(...)` tokens. Conservative: only removes the first
/// occurrence on the line that matches `.<method>(` with a balanced `)`.
fn strip_unwrap_call(line: &str, method: &str) -> String {
    let needle = format!(".{method}(");
    let Some(start) = line.find(&needle) else {
        return line.to_string();
    };
    let open = start + needle.len();
    let bytes = line.as_bytes();
    let mut depth = 1i32;
    let mut i = open;
    while i < bytes.len() && depth > 0 {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    if depth != 0 {
        return line.to_string();
    }
    let before = &line[..start];
    let after = &line[i..];
    format!("{before}{after}")
}

/// Insert `use <trait_path>;` near the top of the file (after the last top-level `use` line,
/// or at the very top if none). Idempotent: returns content unchanged if the use already exists.
fn ensure_use_statement(content: &str, trait_path: &str) -> String {
    let target = format!("use {trait_path};");
    if content.lines().any(|l| l.trim() == target) {
        return content.to_string();
    }
    let mut lines: Vec<String> = content.lines().map(String::from).collect();
    let mut insert_at = 0usize;
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("use ") && trimmed.ends_with(';') {
            insert_at = idx + 1;
        } else if !trimmed.is_empty() && !trimmed.starts_with("//") && insert_at > 0 {
            break;
        }
    }
    lines.insert(insert_at, target);
    lines.join("\n") + "\n"
}

/// Ensure the project `Cargo.toml` lists `crate_root` under `[dependencies]`. We pull the
/// version from `drafts/dependencies.yml` (written by `ensure_external_dependency_for_type`).
fn patch_cargo_toml_with_dependency(workspace: &Workspace, crate_root: &str) -> Result<()> {
    let cargo_path = workspace.root.join("Cargo.toml");
    let cargo_raw = fs::read_to_string(&cargo_path)
        .with_context(|| format!("Failed to read {}", cargo_path.display()))?;
    // Skip if already declared.
    let needle_start = format!("\n{crate_root} =");
    if cargo_raw.contains(&needle_start) || cargo_raw.starts_with(&format!("{crate_root} =")) {
        return Ok(());
    }
    let deps_path = workspace.drafts_dir.join("dependencies.yml");
    let deps_raw = fs::read_to_string(&deps_path).unwrap_or_default();
    let version = extract_crate_version(&deps_raw, crate_root).unwrap_or_else(|| "*".to_string());
    let rendered_version = render_dependency_version_for_toml(&version);
    let mut updated = cargo_raw.clone();
    if let Some(idx) = updated.find("[dependencies]\n") {
        let insert_at = idx + "[dependencies]\n".len();
        let entry = format!("{crate_root} = {rendered_version}\n");
        updated.insert_str(insert_at, &entry);
    } else {
        updated.push_str(&format!(
            "\n[dependencies]\n{crate_root} = {rendered_version}\n"
        ));
    }
    fs::write(&cargo_path, updated)
        .with_context(|| format!("Failed to write {}", cargo_path.display()))
}

fn render_dependency_version_for_toml(version: &str) -> String {
    let trimmed = version.trim();
    if trimmed.starts_with('{') {
        trimmed.to_string()
    } else {
        format!("{trimmed:?}")
    }
}

fn extract_crate_version(deps_yaml: &str, crate_root: &str) -> Option<String> {
    let value: serde_yaml::Value = serde_yaml::from_str(deps_yaml).ok()?;
    let packages = value.get("packages")?.as_sequence()?;
    for pkg in packages {
        let m = pkg.as_mapping()?;
        let name = m
            .get(serde_yaml::Value::String("name".to_string()))?
            .as_str()?;
        if name == crate_root {
            let version = m
                .get(serde_yaml::Value::String("version".to_string()))?
                .as_str()?;
            return Some(version.to_string());
        }
    }
    None
}

/// Merge `trait_names` into the first `#[derive(...)]` attribute in `content`, or insert one
/// above the `pub struct`/`pub enum` declaration if none exists.
fn add_derives_to_file(content: &str, trait_names: &[String]) -> String {
    if trait_names.is_empty() {
        return content.to_string();
    }
    let mut lines: Vec<String> = content.lines().map(String::from).collect();

    // Look for an existing top-level `#[derive(...)]` attribute immediately above the type
    // declaration. Scan for the first `pub struct`/`pub enum` line, then look upward for a
    // derive attribute.
    let decl_idx = lines.iter().position(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("pub struct ") || trimmed.starts_with("pub enum ")
    });

    if let Some(decl_idx) = decl_idx {
        let mut above = decl_idx;
        // Walk up through doc comments and attributes until we hit the first non-attribute line.
        while above > 0 {
            let prev = lines[above - 1].trim_start();
            if prev.starts_with("///") || prev.starts_with("//!") || prev.starts_with("#[") {
                above -= 1;
            } else {
                break;
            }
        }

        // Scan above..decl_idx for an existing derive attribute.
        let mut found = None;
        for (offset, line) in lines[above..decl_idx].iter().enumerate() {
            if line.trim_start().starts_with("#[derive(") {
                found = Some(above + offset);
                break;
            }
        }

        if let Some(idx) = found {
            lines[idx] = merge_derive_line(&lines[idx], trait_names);
            return lines.join("\n") + "\n";
        }

        let indent: String = lines[decl_idx]
            .chars()
            .take_while(|ch| ch.is_whitespace())
            .collect();
        lines.insert(
            decl_idx,
            format!("{indent}#[derive({})]", trait_names.join(", ")),
        );
        return lines.join("\n") + "\n";
    }

    content.to_string()
}

fn merge_derive_line(line: &str, extra_traits: &[String]) -> String {
    let Some(start) = line.find("#[derive(") else {
        return line.to_string();
    };
    let inner_start = start + "#[derive(".len();
    let Some(end) = line[inner_start..].find(")]") else {
        return line.to_string();
    };
    let prefix = &line[..start];
    let suffix = &line[inner_start + end + ")]".len()..];
    let existing: Vec<&str> = line[inner_start..inner_start + end]
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    let mut merged: Vec<String> = existing.iter().map(|s| s.to_string()).collect();
    for extra in extra_traits {
        let extra = extra.trim();
        if extra.is_empty() {
            continue;
        }
        if !merged.iter().any(|existing| existing == extra) {
            merged.push(extra.to_string());
        }
    }
    merged.sort();
    merged.dedup();
    format!("{prefix}#[derive({})]{suffix}", merged.join(", "))
}

fn replace_whole_token(line: &str, from: &str, to: &str) -> String {
    let mut result = String::with_capacity(line.len());
    let bytes = line.as_bytes();
    let from_bytes = from.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx..].starts_with(from_bytes) {
            let before_ok = idx == 0 || !is_ident_char(bytes[idx - 1] as char);
            let after_end = idx + from_bytes.len();
            let after_ok = after_end == bytes.len() || !is_ident_char(bytes[after_end] as char);
            if before_ok && after_ok {
                result.push_str(to);
                idx += from_bytes.len();
                continue;
            }
        }
        result.push(bytes[idx] as char);
        idx += 1;
    }
    result
}

fn is_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn remove_derive_trait(line: &str, trait_name: &str) -> String {
    let Some(start) = line.find("#[derive(") else {
        return line.to_string();
    };
    let prefix = &line[..start];
    let inner_start = start + "#[derive(".len();
    let Some(end) = line[inner_start..].find(")]") else {
        return line.to_string();
    };
    let inner = &line[inner_start..inner_start + end];
    let traits: Vec<&str> = inner.split(',').map(|t| t.trim()).collect();
    let filtered: Vec<&str> = traits.into_iter().filter(|t| *t != trait_name).collect();
    if filtered.is_empty() {
        String::new()
    } else {
        format!("{prefix}#[derive({})]", filtered.join(", "))
    }
}

fn add_lifetime_to_signature(line: &str) -> String {
    if line.contains("<'a>") {
        return line.to_string();
    }
    let Some(fn_pos) = line.find("fn ") else {
        return line.to_string();
    };
    let after_fn = &line[fn_pos + 3..];
    let paren_pos = match after_fn.find('(') {
        Some(p) => fn_pos + 3 + p,
        None => return line.to_string(),
    };
    let fn_name_end = paren_pos;

    let mut result = String::with_capacity(line.len() + 20);
    result.push_str(&line[..fn_name_end]);
    result.push_str("<'a>");
    let rest = &line[fn_name_end..];

    let rest = annotate_borrows_with_lifetime(rest);
    result.push_str(&rest);
    result
}

fn annotate_borrows_with_lifetime(sig: &str) -> String {
    let mut out = String::with_capacity(sig.len() + 10);
    let chars: Vec<char> = sig.chars().collect();
    let len = chars.len();
    let mut i = 0;
    while i < len {
        if chars[i] == '&' {
            let rest: String = chars[i..].iter().collect();
            if rest.starts_with("&self") || rest.starts_with("&mut self") {
                out.push('&');
                i += 1;
                continue;
            }
            if rest.starts_with("&'") {
                out.push('&');
                i += 1;
                continue;
            }
            out.push_str("&'a ");
            i += 1;
            while i < len && chars[i] == ' ' {
                i += 1;
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn has_unique_local_type_declaration(
    workspace: &Workspace,
    type_name: &str,
    requesting_file: &Path,
) -> bool {
    let mut matches = Vec::new();
    let src_dir = workspace.root.join("src");
    collect_local_type_declarations(&src_dir, type_name, &mut matches);
    matches.retain(|path| {
        path.strip_prefix(&workspace.root)
            .ok()
            .map(|relative| relative != requesting_file)
            .unwrap_or(true)
    });
    matches.len() == 1
}

fn collect_local_type_declarations(root: &Path, type_name: &str, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_local_type_declarations(&path, type_name, out);
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) != Some("rs") {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        if declares_named_type(&content, type_name) {
            out.push(path);
        }
    }
}

fn declares_named_type(content: &str, type_name: &str) -> bool {
    content.lines().any(|line| {
        let trimmed = line.trim_start();
        [
            format!("pub struct {type_name}"),
            format!("pub enum {type_name}"),
            format!("pub trait {type_name}"),
            format!("pub type {type_name}"),
        ]
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
    })
}

fn add_crate_import(content: &str, type_name: &str, import_root: &str) -> Option<String> {
    let single_import = format!("use {import_root}::{type_name};");
    let brace_prefix = format!("use {import_root}::{{");
    if content.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == single_import || brace_import_contains(trimmed, &brace_prefix, type_name)
    }) {
        return None;
    }

    let mut lines: Vec<String> = content.lines().map(String::from).collect();
    if let Some(idx) = lines.iter().position(|line| {
        let trimmed = line.trim();
        trimmed.starts_with(&brace_prefix) && trimmed.ends_with("};")
    }) {
        let trimmed = lines[idx].trim();
        let start = trimmed.find('{')? + 1;
        let end = trimmed.rfind('}')?;
        let mut names = trimmed[start..end]
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(String::from)
            .collect::<Vec<_>>();
        names.push(type_name.to_string());
        names.sort();
        names.dedup();
        lines[idx] = format!("{brace_prefix}{}}};", names.join(", "));
        return Some(lines.join("\n") + "\n");
    }

    let insert_at = lines
        .iter()
        .rposition(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("use ") || trimmed.is_empty()
        })
        .map(|idx| idx + 1)
        .unwrap_or(0);
    lines.insert(insert_at, single_import);
    Some(lines.join("\n") + "\n")
}

/// Derive the library crate name for a workspace by reading `Cargo.toml`'s `[package] name`
/// field and normalising dashes to underscores (Rust crate names are snake_case).
///
/// Returns `None` when `Cargo.toml` cannot be read or does not declare a package name —
/// callers should fall back to `crate` in that case.
fn library_crate_name(workspace: &Workspace) -> Option<String> {
    let cargo_toml = workspace.root.join("Cargo.toml");
    let content = fs::read_to_string(&cargo_toml).ok()?;
    let mut in_package = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("name") {
            let rest = rest.trim_start();
            let Some(rest) = rest.strip_prefix('=') else {
                continue;
            };
            let value = rest.trim().trim_matches('"').trim_matches('\'');
            if value.is_empty() {
                return None;
            }
            return Some(value.replace('-', "_"));
        }
    }
    None
}

fn brace_import_contains(line: &str, brace_prefix: &str, type_name: &str) -> bool {
    let Some(rest) = line.strip_prefix(brace_prefix) else {
        return false;
    };
    let Some(inner) = rest.strip_suffix("};") else {
        return false;
    };
    inner.split(',').any(|name| name.trim() == type_name)
}

fn remove_borrow_for_owned_argument(line: &str, type_name: &str) -> String {
    let patterns = [
        format!("&self.{field}", field = type_name_to_field(type_name)),
        "&self.".to_string(),
        "&mut self.".to_string(),
    ];
    for pattern in &patterns {
        if let Some(start) = line.find(pattern) {
            let ampersand = start;
            let mut end = start + pattern.len();
            while end < line.len() {
                let ch = line.as_bytes()[end] as char;
                if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' {
                    end += 1;
                } else {
                    break;
                }
            }
            let mut updated = line.to_string();
            updated.remove(ampersand);
            return updated;
        }
    }

    if let Some(start) = line.find('&') {
        let mut end = start + 1;
        while end < line.len() {
            let ch = line.as_bytes()[end] as char;
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' {
                end += 1;
            } else {
                break;
            }
        }
        if end > start + 1 {
            let mut updated = line.to_string();
            updated.remove(start);
            return updated;
        }
    }
    line.to_string()
}

fn type_name_to_field(type_name: &str) -> String {
    let mut out = String::new();
    for (idx, ch) in type_name.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if idx > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// Paths to `.rs` files mentioned in cargo `--message-format=short` diagnostics (deduplicated).
pub(crate) fn collect_error_paths(stderr: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for line in stderr.lines() {
        if let Some(path) = parse_diagnostic_path(line) {
            let key = path.to_string_lossy().replace('\\', "/");
            if seen.insert(key) {
                out.push(path);
            }
        }
    }
    out
}

/// Match either `path/to/file.rs:12:34:` or pretty-diagnostic arrows like `--> Cargo.toml:10:24`.
fn parse_diagnostic_path(line: &str) -> Option<PathBuf> {
    let rest = line.trim_start();
    let rest = rest
        .strip_prefix("-->")
        .map(str::trim_start)
        .unwrap_or(rest);
    let location_re =
        regex::Regex::new(r"^(?P<path>.+?):(?P<line>\d+):(?P<col>\d+)(?::|\s|$)").ok()?;
    let captures = location_re.captures(rest)?;
    captures.name("line")?.as_str().parse::<usize>().ok()?;
    captures.name("col")?.as_str().parse::<usize>().ok()?;
    let path = captures.name("path")?.as_str().trim();
    if path.is_empty() {
        return None;
    }
    Some(PathBuf::from(path))
}

/// Normalize a path for comparison with manifest entries (forward slashes).
pub(crate) fn normalize_manifest_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Map a compiler-emitted path (often absolute, or `./`-prefixed) to the workspace-relative form
/// used in `.reen/generated_files.json`, so it can be matched against the manifest.
pub(crate) fn normalize_compiler_path_for_manifest(workspace: &Workspace, path: &Path) -> String {
    if path.is_absolute() {
        resolve_compiler_path_for_manifest(workspace, path).unwrap_or_else(|| {
            normalize_manifest_path(path)
                .trim_start_matches("./")
                .to_string()
        })
    } else {
        normalize_manifest_path(path)
            .trim_start_matches("./")
            .to_string()
    }
}

/// Keep only paths present in `allowed` (manifest file list).
pub(crate) fn filter_paths_by_manifest(
    workspace: &Workspace,
    paths: Vec<PathBuf>,
    allowed: &HashSet<String>,
) -> Vec<PathBuf> {
    let normalized_allowed = allowed
        .iter()
        .map(|path| {
            normalize_manifest_path(Path::new(path))
                .trim_start_matches("./")
                .to_string()
        })
        .collect::<Vec<_>>();
    let allowed_set = normalized_allowed.iter().cloned().collect::<HashSet<_>>();
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for p in paths {
        let normalized = normalize_compiler_path_for_manifest(workspace, &p);
        let allow_suffix_match = !p.is_absolute();
        let Some(n) = resolve_manifest_match(
            &normalized,
            &allowed_set,
            &normalized_allowed,
            allow_suffix_match,
        ) else {
            continue;
        };
        if seen.insert(n.clone()) {
            out.push(PathBuf::from(n));
        }
    }
    out
}

fn resolve_compiler_path_for_manifest(workspace: &Workspace, path: &Path) -> Option<String> {
    let workspace_roots = [
        Some(workspace.root.clone()),
        workspace.root.canonicalize().ok(),
    ];
    let path_candidates = [Some(path.to_path_buf()), path.canonicalize().ok()];
    for candidate in path_candidates.into_iter().flatten() {
        if !candidate.is_absolute() {
            continue;
        }
        for root in workspace_roots.iter().flatten() {
            if let Ok(relative) = candidate.strip_prefix(root) {
                let normalized = normalize_manifest_path(relative)
                    .trim_start_matches("./")
                    .to_string();
                if !normalized.is_empty() {
                    return Some(normalized);
                }
            }
        }
    }
    None
}

fn resolve_manifest_match(
    compiler_path: &str,
    allowed_set: &HashSet<String>,
    allowed_list: &[String],
    allow_suffix_match: bool,
) -> Option<String> {
    if allowed_set.contains(compiler_path) {
        return Some(compiler_path.to_string());
    }
    if !allow_suffix_match {
        return None;
    }
    // Accept crate-prefixed or canonicalized compiler paths like `snake/src/a.rs` by matching
    // them to the unique manifest entry they end with. Restrict suffix matching to relative
    // compiler paths so an external dependency's absolute `.../src/lib.rs` cannot alias the
    // workspace's own `src/lib.rs`.
    let mut matches = allowed_list
        .iter()
        .filter(|entry| entry.contains('/') && compiler_path.ends_with(&format!("/{entry}")))
        .cloned()
        .collect::<Vec<_>>();
    matches.sort_by_key(|entry| std::cmp::Reverse(entry.len()));
    let best = matches.first()?.clone();
    if matches.get(1).is_some_and(|next| next.len() == best.len()) {
        return None;
    }
    Some(best)
}

/// Detect "structural" errors that deterministic compile-repair can't fix because they indicate
/// a mismatch between the prepared artefact and the generated scaffold.
///
/// Used by the fix loop to bail early rather than burning LLM rounds on errors the LLM also
/// cannot resolve.
pub(crate) fn is_structural_error(stderr: &str) -> bool {
    stderr.lines().any(|line| {
        let trimmed = line.trim_start();
        // Unknown field — the method body references a field that doesn't exist on `self`.
        if trimmed.contains("error[E0609]:") && trimmed.contains("no field") {
            return true;
        }
        // Duplicate definition — usually a getter/functionality collision.
        if trimmed.contains("error[E0592]:") {
            return true;
        }
        // "cannot find type X in this scope" with no local declaration — spec drift.
        if trimmed.contains("error[E0412]:") {
            return true;
        }
        false
    })
}

/// Strip location noise (line/column numbers) so two stderrs that differ only in positions
/// can be compared for "same errors as last round".
pub(crate) fn canonicalize_stderr_for_compare(stderr: &str) -> String {
    let re = regex::Regex::new(r":\d+:\d+").expect("stderr position regex");
    re.replace_all(stderr, ":L:C").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Workspace;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn normalize_compiler_path_for_manifest_strips_workspace_and_dot_slash() {
        let root = temp_root("norm_compiler_path");
        fs::create_dir_all(root.join("src/contexts")).unwrap();
        let ws = Workspace::discover(root.clone()).unwrap();
        let abs = root.join("src/contexts/game_loop.rs");
        assert_eq!(
            normalize_compiler_path_for_manifest(&ws, &abs),
            "src/contexts/game_loop.rs"
        );
        assert_eq!(
            normalize_compiler_path_for_manifest(&ws, Path::new("./src/a.rs")),
            "src/a.rs"
        );
    }

    #[test]
    fn parse_diagnostic_path_extracts_short_format_path() {
        let line = "src/contexts/game_loop.rs:28:16: error: lifetime may not live long enough";
        assert_eq!(
            parse_diagnostic_path(line),
            Some(PathBuf::from("src/contexts/game_loop.rs"))
        );
    }

    #[test]
    fn parse_diagnostic_path_extracts_arrow_format_path() {
        let line = "  --> Cargo.toml:10:24";
        assert_eq!(
            parse_diagnostic_path(line),
            Some(PathBuf::from("Cargo.toml"))
        );
    }

    #[test]
    fn collect_error_paths_dedupes_rust_files() {
        let stderr = r#"src/a.rs:1:1: error[E0001]: foo
src/a.rs:2:2: error[E0002]: bar
src/b.rs:1:1: note: blah
"#;
        let paths = collect_error_paths(stderr)
            .into_iter()
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("rs"))
            .collect::<Vec<_>>();
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&PathBuf::from("src/a.rs")));
        assert!(paths.contains(&PathBuf::from("src/b.rs")));
    }

    #[test]
    fn collect_error_paths_keeps_non_rust_manifest_files() {
        let stderr =
            "error: unexpected key or value, expected newline, `#`\n  --> Cargo.toml:10:24\n";
        let paths = collect_error_paths(stderr);
        assert_eq!(paths, vec![PathBuf::from("Cargo.toml")]);
    }

    #[test]
    fn filter_paths_by_manifest_matches_prefixed_relative_paths() {
        let root = temp_root("compile_repair_prefixed_path");
        fs::create_dir_all(root.join("src/contexts")).unwrap();
        let workspace = Workspace::discover(root).unwrap();
        let allowed = HashSet::from(["src/contexts/game_loop.rs".to_string()]);

        let paths = filter_paths_by_manifest(
            &workspace,
            vec![PathBuf::from("snake/src/contexts/game_loop.rs")],
            &allowed,
        );

        assert_eq!(paths, vec![PathBuf::from("src/contexts/game_loop.rs")]);
    }

    #[test]
    fn parse_compile_errors_detects_unique_local_type_import_fix() {
        let root = temp_root("compile_repair_direction");
        fs::create_dir_all(root.join("src/data")).unwrap();
        fs::create_dir_all(root.join("src/contexts")).unwrap();
        fs::write(
            root.join("src/data/direction.rs"),
            "pub enum Direction {}\n",
        )
        .unwrap();
        fs::write(
            root.join("src/contexts/command_input.rs"),
            "use crate::{UserAction};\n",
        )
        .unwrap();
        let workspace = Workspace::discover(root.clone()).unwrap();
        let stderr = "src/contexts/command_input.rs:26:42: error[E0433]: failed to resolve: use of undeclared type `Direction`: use of undeclared type `Direction`\n";

        let fixes = parse_compile_errors(&workspace, stderr);

        assert_eq!(fixes.len(), 1);
        match &fixes[0] {
            CompileFix::AddLocalTypeImport {
                file,
                line,
                type_name,
            } => {
                assert_eq!(file, &PathBuf::from("src/contexts/command_input.rs"));
                assert_eq!(*line, 26);
                assert_eq!(type_name, "Direction");
            }
            other => panic!("unexpected fix: {other:?}"),
        }
    }

    #[test]
    fn apply_compile_fix_adds_local_type_to_crate_import_list() {
        let root = temp_root("compile_repair_apply");
        fs::create_dir_all(root.join("src/data")).unwrap();
        fs::create_dir_all(root.join("src/contexts")).unwrap();
        fs::write(
            root.join("src/data/direction.rs"),
            "pub enum Direction {}\n",
        )
        .unwrap();
        let target = root.join("src/contexts/command_input.rs");
        fs::write(&target, "use crate::{UserAction};\n\nfn f() {}\n").unwrap();
        let workspace = Workspace::discover(root.clone()).unwrap();

        apply_compile_fix(
            &workspace,
            &CompileFix::AddLocalTypeImport {
                file: PathBuf::from("src/contexts/command_input.rs"),
                line: 1,
                type_name: "Direction".to_string(),
            },
        )
        .unwrap();

        let updated = fs::read_to_string(&target).unwrap();
        assert!(updated.starts_with("use crate::{Direction, UserAction};\n"));
    }

    /// `src/main.rs` is the binary crate root — `use crate::Type;` does not reach the library
    /// crate. The compile fixer must instead import from `<library_crate>::Type`.
    #[test]
    fn apply_compile_fix_adds_local_type_to_main_via_library_crate() {
        let root = temp_root("compile_repair_apply_main");
        fs::create_dir_all(root.join("src/data")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"my-snake-app\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("src/data/direction.rs"),
            "pub enum Direction {}\n",
        )
        .unwrap();
        let target = root.join("src/main.rs");
        fs::write(
            &target,
            "use my_snake_app::{UserAction};\n\nfn main() {}\n",
        )
        .unwrap();
        let workspace = Workspace::discover(root.clone()).unwrap();

        apply_compile_fix(
            &workspace,
            &CompileFix::AddLocalTypeImport {
                file: PathBuf::from("src/main.rs"),
                line: 1,
                type_name: "Direction".to_string(),
            },
        )
        .unwrap();

        let updated = fs::read_to_string(&target).unwrap();
        assert!(
            updated.starts_with("use my_snake_app::{Direction, UserAction};\n"),
            "expected library-crate import, got:\n{updated}"
        );
    }

    #[test]
    fn patch_cargo_toml_with_dependency_preserves_inline_table_versions() {
        let root = temp_root("compile_repair_patch_cargo_inline");
        fs::create_dir_all(root.join("drafts")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[dependencies]\nanyhow = \"1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("drafts/dependencies.yml"),
            "schema: reen.dependencies/v1\npackages:\n- name: chrono\n  version: '{ version = \"0.4\", features = [\"serde\"] }'\n",
        )
        .unwrap();
        let workspace = Workspace::discover(root).unwrap();

        patch_cargo_toml_with_dependency(&workspace, "chrono").unwrap();

        let updated = fs::read_to_string(workspace.root.join("Cargo.toml")).unwrap();
        assert!(updated.contains("chrono = { version = \"0.4\", features = [\"serde\"] }\n"));
        assert!(!updated.contains("chrono = \"{ version = \"0.4\", features = [\"serde\"] }\""));
    }

    #[test]
    fn patch_cargo_toml_with_dependency_quotes_plain_versions() {
        let root = temp_root("compile_repair_patch_cargo_plain");
        fs::create_dir_all(root.join("drafts")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("drafts/dependencies.yml"),
            "schema: reen.dependencies/v1\npackages:\n- name: anyhow\n  version: '1.0'\n",
        )
        .unwrap();
        let workspace = Workspace::discover(root).unwrap();

        patch_cargo_toml_with_dependency(&workspace, "anyhow").unwrap();

        let updated = fs::read_to_string(workspace.root.join("Cargo.toml")).unwrap();
        assert!(updated.contains("[dependencies]\nanyhow = \"1.0\"\n"));
    }

    #[test]
    fn parse_compile_errors_skips_ambiguous_local_type_imports() {
        let root = temp_root("compile_repair_ambiguous");
        fs::create_dir_all(root.join("src/data")).unwrap();
        fs::create_dir_all(root.join("src/contexts")).unwrap();
        fs::write(
            root.join("src/data/direction.rs"),
            "pub enum Direction {}\n",
        )
        .unwrap();
        fs::write(
            root.join("src/data/other_direction.rs"),
            "pub struct Direction;\n",
        )
        .unwrap();
        fs::write(
            root.join("src/contexts/command_input.rs"),
            "use crate::{UserAction};\n",
        )
        .unwrap();
        let workspace = Workspace::discover(root).unwrap();
        let stderr = "src/contexts/command_input.rs:26:42: error[E0433]: failed to resolve: use of undeclared type `Direction`: use of undeclared type `Direction`\n";

        let fixes = parse_compile_errors(&workspace, stderr);

        assert!(fixes.is_empty());
    }

    #[test]
    fn parse_compile_errors_detects_rand_thread_rng_replacement() {
        let root = temp_root("compile_repair_rand_rng");
        fs::create_dir_all(root.join("src/contexts")).unwrap();
        let workspace = Workspace::discover(root).unwrap();
        let stderr = "src/contexts/game_loop.rs:128:29: error[E0425]: cannot find function `thread_rng` in crate `rand`: not found in `rand`\n";

        let fixes = parse_compile_errors(&workspace, stderr);

        assert_eq!(fixes.len(), 1);
        match &fixes[0] {
            CompileFix::ReplaceRandThreadRngCall { file, line } => {
                assert_eq!(file, &PathBuf::from("src/contexts/game_loop.rs"));
                assert_eq!(*line, 128);
            }
            other => panic!("unexpected fix: {other:?}"),
        }
    }

    #[test]
    fn apply_compile_fix_replaces_rand_thread_rng_call() {
        let root = temp_root("compile_repair_rand_apply");
        fs::create_dir_all(root.join("src/contexts")).unwrap();
        let target = root.join("src/contexts/game_loop.rs");
        fs::write(&target, "fn f() { let rng = rand::thread_rng(); }\n").unwrap();
        let workspace = Workspace::discover(root).unwrap();

        apply_compile_fix(
            &workspace,
            &CompileFix::ReplaceRandThreadRngCall {
                file: PathBuf::from("src/contexts/game_loop.rs"),
                line: 1,
            },
        )
        .unwrap();

        let updated = fs::read_to_string(&target).unwrap();
        assert_eq!(updated, "fn f() { let rng = rand::rng(); }\n");
    }

    #[test]
    fn parse_compile_errors_detects_owned_argument_borrow_mismatch() {
        let root = temp_root("compile_repair_owned_borrow");
        fs::create_dir_all(root.join("src/contexts")).unwrap();
        let workspace = Workspace::discover(root).unwrap();
        let stderr = "src/contexts/command_input.rs:13:53: error[E0308]: mismatched types: expected `Stdin`, found `&Stdin`\n";

        let fixes = parse_compile_errors(&workspace, stderr);

        assert_eq!(fixes.len(), 1);
        match &fixes[0] {
            CompileFix::RemoveBorrowForOwnedArgument {
                file,
                line,
                type_name,
            } => {
                assert_eq!(file, &PathBuf::from("src/contexts/command_input.rs"));
                assert_eq!(*line, 13);
                assert_eq!(type_name, "Stdin");
            }
            other => panic!("unexpected fix: {other:?}"),
        }
    }

    #[test]
    fn apply_compile_fix_removes_borrow_for_owned_argument() {
        let root = temp_root("compile_repair_owned_borrow_apply");
        fs::create_dir_all(root.join("src/contexts")).unwrap();
        let target = root.join("src/contexts/command_input.rs");
        fs::write(
            &target,
            "let keys = self.stdin_source_read_available(&self.stdin_source);\n",
        )
        .unwrap();
        let workspace = Workspace::discover(root).unwrap();

        apply_compile_fix(
            &workspace,
            &CompileFix::RemoveBorrowForOwnedArgument {
                file: PathBuf::from("src/contexts/command_input.rs"),
                line: 1,
                type_name: "Stdin".to_string(),
            },
        )
        .unwrap();

        let updated = fs::read_to_string(&target).unwrap();
        assert_eq!(
            updated,
            "let keys = self.stdin_source_read_available(self.stdin_source);\n"
        );
    }

    #[test]
    fn parse_compile_errors_detects_spurious_unwrap() {
        let root = temp_root("compile_repair_unwrap");
        fs::create_dir_all(root.join("src/contexts")).unwrap();
        let workspace = Workspace::discover(root).unwrap();
        let stderr = "src/contexts/game_loop.rs:107:79: error[E0599]: no method named `unwrap` found for struct `snake::Snake` in the current scope: method not found in `snake::Snake`\n";

        let fixes = parse_compile_errors(&workspace, stderr);

        assert_eq!(fixes.len(), 1);
        match &fixes[0] {
            CompileFix::StripUnwrapCall { file, line, method } => {
                assert_eq!(file, &PathBuf::from("src/contexts/game_loop.rs"));
                assert_eq!(*line, 107);
                assert_eq!(method, "unwrap");
            }
            other => panic!("unexpected fix: {other:?}"),
        }
    }

    #[test]
    fn parse_compile_errors_skips_unwrap_on_option_or_result() {
        let root = temp_root("compile_repair_unwrap_option");
        fs::create_dir_all(root.join("src/contexts")).unwrap();
        let workspace = Workspace::discover(root).unwrap();
        let stderr = "src/a.rs:1:1: error[E0599]: no method named `unwrap` found for enum `Option<u32>` in the current scope\n";
        let fixes = parse_compile_errors(&workspace, stderr);
        // Option<T> does have unwrap so this specific stderr wouldn't happen in real life, but we
        // still want to defensively skip stripping when the type name mentions Option/Result.
        assert!(
            fixes
                .iter()
                .all(|f| !matches!(f, CompileFix::StripUnwrapCall { .. }))
        );
    }

    #[test]
    fn apply_compile_fix_strips_unwrap_call() {
        let root = temp_root("compile_repair_unwrap_apply");
        fs::create_dir_all(root.join("src/contexts")).unwrap();
        let target = root.join("src/contexts/game_loop.rs");
        fs::write(&target, "let head = self.snake.body().unwrap().first();\n").unwrap();
        let workspace = Workspace::discover(root).unwrap();

        apply_compile_fix(
            &workspace,
            &CompileFix::StripUnwrapCall {
                file: PathBuf::from("src/contexts/game_loop.rs"),
                line: 1,
                method: "unwrap".to_string(),
            },
        )
        .unwrap();

        let updated = fs::read_to_string(&target).unwrap();
        assert_eq!(updated, "let head = self.snake.body().first();\n");
    }

    #[test]
    fn parse_compile_errors_spurious_unwrap_wins_over_local_missing_method_revert() {
        // Regression: when both `parse_spurious_unwrap_error` and
        // `parse_local_missing_method_error` fire for the same `.unwrap()` call on a plain value
        // (struct with a same-named source file, e.g. `snake::Snake` ↔ `src/data/snake.rs`),
        // the order of application would strip `.unwrap()` first and then revert the entire
        // enclosing function body — discarding the clean fix and forcing a pointless LLM
        // re-implementation. The local-missing-method parser must defer to the spurious-unwrap
        // parser for Option/Result-shape methods.
        let root = temp_root("compile_repair_unwrap_over_revert");
        fs::create_dir_all(root.join("src/data")).unwrap();
        fs::create_dir_all(root.join("src/contexts")).unwrap();
        fs::write(root.join("src/data/snake.rs"), "pub struct Snake {}\n").unwrap();
        fs::write(
            root.join("src/contexts/game_loop.rs"),
            "impl GameLoopContext {\n    pub fn tick(&mut self) {\n        self.snake = Snake::new(body, dir).unwrap();\n    }\n}\n",
        )
        .unwrap();
        let workspace = Workspace::discover(root).unwrap();
        let stderr = "src/contexts/game_loop.rs:3:45: error[E0599]: no method named `unwrap` found for struct `snake::Snake` in the current scope: method not found in `snake::Snake`\n";

        let fixes = parse_compile_errors(&workspace, stderr);

        assert!(
            fixes
                .iter()
                .any(|f| matches!(f, CompileFix::StripUnwrapCall { .. })),
            "expected StripUnwrapCall, got {fixes:?}"
        );
        assert!(
            !fixes
                .iter()
                .any(|f| matches!(f, CompileFix::RevertBodyToTodo { .. })),
            "local-missing-method must not queue a body revert for spurious .unwrap(), got {fixes:?}"
        );
    }

    #[test]
    fn parse_compile_errors_reverts_body_for_llm_syntax_error() {
        // LLMs sometimes emit token-order bugs inside a generated body, e.g.
        // `board.with_symbol_at(food.position(),.clone() 'f')`. rustc reports these as
        // code-less parse errors ("expected expression, found `.`"). Recovery is to revert the
        // enclosing function body to todo!() so the build agent re-implements it.
        let root = temp_root("compile_repair_syntax_error_revert");
        fs::create_dir_all(root.join("src/contexts")).unwrap();
        fs::write(
            root.join("src/contexts/game_loop.rs"),
            "impl Ctx {\n    pub fn current_board(&self) -> Board {\n        let mut bp = self.board.clone();\n        bp = bp.with_symbol_at(food.position(),.clone() 'f');\n        bp\n    }\n}\n",
        )
        .unwrap();
        let workspace = Workspace::discover(root).unwrap();
        let stderr = "src/contexts/game_loop.rs:4:43: error: expected expression, found `.`: expected expression\n";

        let fixes = parse_compile_errors(&workspace, stderr);

        assert!(
            fixes.iter().any(|f| matches!(
                f,
                CompileFix::RevertBodyToTodo { file, fn_signature_line, .. }
                    if file == &PathBuf::from("src/contexts/game_loop.rs") && *fn_signature_line == 2
            )),
            "expected RevertBodyToTodo pointing at the enclosing fn, got {fixes:?}"
        );
    }

    #[test]
    fn parse_compile_errors_syntax_revert_ignores_coded_errors() {
        // Errors that carry a rustc code (e.g. `error[E0308]`) must remain the responsibility of
        // their dedicated handlers. The syntax-error revert is strictly for code-less parse errors.
        let root = temp_root("compile_repair_syntax_error_skip_coded");
        fs::create_dir_all(root.join("src/contexts")).unwrap();
        fs::write(
            root.join("src/contexts/game_loop.rs"),
            "impl Ctx {\n    pub fn f(&self) -> u16 { 0u32 }\n}\n",
        )
        .unwrap();
        let workspace = Workspace::discover(root).unwrap();
        let stderr = "src/contexts/game_loop.rs:2:34: error[E0308]: mismatched types: expected `u16`, found `u32`\n";

        let fixes = parse_compile_errors(&workspace, stderr);

        assert!(
            !fixes.iter().any(|f| matches!(
                f,
                CompileFix::RevertBodyToTodo { todo_description, .. }
                    if todo_description.contains("parse error")
            )),
            "syntax-error revert must not claim coded errors; got {fixes:?}"
        );
    }

    #[test]
    fn parse_compile_errors_detects_legacy_trait_method_missing() {
        // `gen_range` is the last legacy entry kept in `known_trait_methods` as a safety net
        // when rustc's JSON suggestion pipeline isn't available. Modern method names
        // (`random_range`, `random_bool`, …) now rely on rustc's own `use rand::RngExt;`
        // suggestion via `diagnostic_suggestions_to_fixes`, not the curated table.
        let root = temp_root("compile_repair_trait_method");
        fs::create_dir_all(root.join("src/contexts")).unwrap();
        let workspace = Workspace::discover(root).unwrap();
        let stderr = "src/contexts/game_loop.rs:182:25: error[E0599]: no method named `gen_range` found for struct `ThreadRng` in the current scope\n";

        let fixes = parse_compile_errors(&workspace, stderr);

        assert!(fixes.iter().any(|f| matches!(
            f,
            CompileFix::AddTraitImport { trait_path, file }
                if trait_path == "rand::Rng"
                    && file == &PathBuf::from("src/contexts/game_loop.rs")
        )));
    }

    #[test]
    fn parse_compile_errors_skips_modern_rand_methods_from_curated_table() {
        // Regression: `random_range` used to live in `known_trait_methods` pointing at
        // `rand::Rng`, but in `rand` 0.9 the method moved to `rand::RngExt`. Applying the old
        // curated import would silently fail the build in a stuck loop. Rustc's own JSON
        // suggestion is the right source; the curated fallback must stay out of the way.
        let root = temp_root("compile_repair_modern_rand");
        fs::create_dir_all(root.join("src/contexts")).unwrap();
        let workspace = Workspace::discover(root).unwrap();
        let stderr = "src/contexts/game_loop.rs:182:25: error[E0599]: no method named `random_range` found for struct `ThreadRng` in the current scope\n";

        let fixes = parse_compile_errors(&workspace, stderr);

        assert!(
            !fixes
                .iter()
                .any(|f| matches!(f, CompileFix::AddTraitImport { .. })),
            "curated fallback must not produce a trait import for modern rand methods; got {fixes:?}"
        );
    }

    #[test]
    fn apply_compile_fix_adds_trait_import() {
        let root = temp_root("compile_repair_trait_import_apply");
        fs::create_dir_all(root.join("src/contexts")).unwrap();
        let target = root.join("src/contexts/game_loop.rs");
        fs::write(
            &target,
            "use crate::Board;\nuse std::collections::HashMap;\n\nfn body() {}\n",
        )
        .unwrap();
        let workspace = Workspace::discover(root).unwrap();

        apply_compile_fix(
            &workspace,
            &CompileFix::AddTraitImport {
                file: PathBuf::from("src/contexts/game_loop.rs"),
                trait_path: "rand::Rng".to_string(),
            },
        )
        .unwrap();

        let updated = fs::read_to_string(&target).unwrap();
        assert!(updated.contains("use rand::Rng;"));
        // Idempotent: applying again is a no-op.
        apply_compile_fix(
            &workspace,
            &CompileFix::AddTraitImport {
                file: PathBuf::from("src/contexts/game_loop.rs"),
                trait_path: "rand::Rng".to_string(),
            },
        )
        .unwrap();
        let again = fs::read_to_string(&target).unwrap();
        assert_eq!(updated, again);
    }

    #[test]
    fn parse_compile_errors_detects_local_missing_method_revert() {
        let root = temp_root("compile_repair_local_missing_method");
        fs::create_dir_all(root.join("src/data")).unwrap();
        fs::create_dir_all(root.join("src/contexts")).unwrap();
        fs::write(root.join("src/data/board.rs"), "pub struct Board {}\n").unwrap();
        fs::write(
            root.join("src/contexts/game_loop.rs"),
            "impl GameLoopContext {\n    pub fn current_board(&self) -> Board {\n        board.set_cell(position, 's');\n        board\n    }\n}\n",
        )
        .unwrap();
        let workspace = Workspace::discover(root).unwrap();
        let stderr = "src/contexts/game_loop.rs:3:15: error[E0599]: no method named `set_cell` found for struct `board::Board` in the current scope: method not found in `board::Board`\n";

        let fixes = parse_compile_errors(&workspace, stderr);

        assert!(fixes.iter().any(|fix| matches!(
            fix,
            CompileFix::RevertBodyToTodo {
                file,
                fn_signature_line,
                todo_description,
            }
                if file == &PathBuf::from("src/contexts/game_loop.rs")
                    && *fn_signature_line == 2
                    && todo_description.contains("set_cell")
                    && todo_description.contains("Board")
        )));
    }

    #[test]
    fn parse_compile_errors_detects_self_borrow_helper_conflict() {
        let root = temp_root("compile_repair_self_borrow_helper");
        fs::create_dir_all(root.join("src/contexts")).unwrap();
        fs::write(
            root.join("src/contexts/game_loop.rs"),
            "impl GameLoopContext {\n    pub fn tick(&mut self) {\n        self.command_capture(&mut self.command);\n    }\n    fn command_capture(&self, command_: &mut CommandInputContext) {\n        command_.capture();\n    }\n}\n",
        )
        .unwrap();
        let workspace = Workspace::discover(root).unwrap();
        let stderr = "src/contexts/game_loop.rs:3:30: error[E0502]: cannot borrow `self.command` as mutable because it is also borrowed as immutable: mutable borrow occurs here\n";

        let fixes = parse_compile_errors(&workspace, stderr);

        assert!(fixes.iter().any(|fix| matches!(
            fix,
            CompileFix::ConvertHelperToAssociatedFn { file, method_name }
                if file == &PathBuf::from("src/contexts/game_loop.rs")
                    && method_name == "command_capture"
        )));
    }

    #[test]
    fn apply_compile_fix_converts_helper_to_associated_fn() {
        let root = temp_root("compile_repair_convert_helper");
        fs::create_dir_all(root.join("src/contexts")).unwrap();
        let target = root.join("src/contexts/game_loop.rs");
        fs::write(
            &target,
            "impl GameLoopContext {\n    pub fn tick(&mut self) {\n        self.command_capture(&mut self.command);\n    }\n    fn command_capture(&self, command_: &mut CommandInputContext) {\n        command_.capture();\n    }\n}\n",
        )
        .unwrap();
        let workspace = Workspace::discover(root).unwrap();

        apply_compile_fix(
            &workspace,
            &CompileFix::ConvertHelperToAssociatedFn {
                file: PathBuf::from("src/contexts/game_loop.rs"),
                method_name: "command_capture".to_string(),
            },
        )
        .unwrap();

        let updated = fs::read_to_string(&target).unwrap();
        assert!(updated.contains("Self::command_capture(&mut self.command);"));
        assert!(updated.contains("fn command_capture(command_: &mut CommandInputContext)"));
        assert!(!updated.contains("fn command_capture(&self"));
    }

    #[test]
    fn parse_compile_errors_detects_copy_clone_derive_from_move_out() {
        let root = temp_root("compile_repair_copy_clone");
        fs::create_dir_all(root.join("src/data")).unwrap();
        fs::create_dir_all(root.join("src/contexts")).unwrap();
        fs::write(
            root.join("src/data/position.rs"),
            "pub struct Position {}\n",
        )
        .unwrap();
        let workspace = Workspace::discover(root).unwrap();
        let stderr = "src/contexts/game_loop.rs:165:14: error[E0507]: cannot move out of index of `Vec<position::Position>`: move occurs because value has type `position::Position`, which does not implement the `Copy` trait\n";

        let fixes = parse_compile_errors(&workspace, stderr);

        assert!(fixes.iter().any(|fix| matches!(
            fix,
            CompileFix::AddDerive { file, trait_names }
                if file == &PathBuf::from("src/data/position.rs")
                    && trait_names.contains(&"Copy".to_string())
                    && trait_names.contains(&"Clone".to_string())
        )));
    }

    #[test]
    fn parse_compile_errors_detects_unresolved_external_crate() {
        let root = temp_root("compile_repair_unlinked_crate");
        fs::create_dir_all(root.join("drafts")).unwrap();
        fs::write(
            root.join("drafts/capability_registry.yml"),
            "providers:\n  - crate: chrono\n    external_path_prefixes:\n      - chrono::\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        let workspace = Workspace::discover(root).unwrap();
        let stderr = "src/contexts/game_loop.rs:88:22: error[E0433]: failed to resolve: use of unresolved module or unlinked crate `chrono`: use of unresolved module or unlinked crate `chrono`\n";

        let fixes = parse_compile_errors(&workspace, stderr);

        assert!(fixes.iter().any(|f| matches!(
            f,
            CompileFix::AddExternalCrate { crate_root } if crate_root == "chrono"
        )));
    }

    #[test]
    fn parse_compile_errors_skips_unlinked_crate_not_in_registry() {
        let root = temp_root("compile_repair_unlinked_crate_unregistered");
        fs::create_dir_all(root.join("drafts")).unwrap();
        fs::write(
            root.join("drafts/capability_registry.yml"),
            "providers: []\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        let workspace = Workspace::discover(root).unwrap();
        let stderr = "src/a.rs:1:1: error[E0433]: failed to resolve: use of unresolved module or unlinked crate `nonexistent`: use of unresolved module or unlinked crate `nonexistent`\n";
        let fixes = parse_compile_errors(&workspace, stderr);
        assert!(
            fixes
                .iter()
                .all(|f| !matches!(f, CompileFix::AddExternalCrate { .. }))
        );
    }

    #[test]
    fn maybe_incorrect_suggestion_is_used_as_fallback_when_no_machine_applicable_fix_exists() {
        // Mirrors the snake-project `use rand::RngExt;` case: rustc tags the import suggestion
        // as `MaybeIncorrect` (because it can't prove the user really intended this trait),
        // but there's no other MachineApplicable fix, so Phase 1 should accept it rather than
        // falling through to the curated `known_trait_methods` mapping.
        let event = serde_json::json!({
            "reason": "compiler-message",
            "message": {
                "message": "no method named `random_range` found for struct `ThreadRng`",
                "level": "error",
                "code": {"code": "E0599"},
                "rendered": "error[E0599]…",
                "spans": [{
                    "file_name": "src/contexts/game_loop.rs",
                    "line_start": 196,
                    "line_end": 196,
                    "column_start": 25,
                    "column_end": 37,
                    "is_primary": true
                }],
                "children": [{
                    "message": "the following trait is implemented but not in scope; perhaps add a `use` for it:",
                    "level": "help",
                    "code": null,
                    "rendered": null,
                    "spans": [{
                        "file_name": "src/contexts/game_loop.rs",
                        "line_start": 1,
                        "line_end": 1,
                        "column_start": 1,
                        "column_end": 1,
                        "is_primary": false,
                        "suggested_replacement": "use rand::RngExt;\n",
                        "suggestion_applicability": "MaybeIncorrect"
                    }],
                    "children": []
                }]
            }
        });
        let line = serde_json::to_string(&event).unwrap();
        let diags = parse_cargo_json_diagnostics(&line);
        let fixes = diagnostic_suggestions_to_fixes(&diags);
        assert!(
            fixes.iter().any(|f| matches!(
                f,
                CompileFix::ApplyRustcSuggestion { replacement, applicability, .. }
                    if replacement == "use rand::RngExt;\n" && applicability == "MaybeIncorrect"
            )),
            "expected MaybeIncorrect `use rand::RngExt;` fallback, got {fixes:?}"
        );
    }

    #[test]
    fn collect_all_compile_fixes_unions_json_and_string_match_sources() {
        // Regression: `build --fix` used to prefer JSON suggestions and fall back to
        // `parse_compile_errors` only when JSON returned nothing. That hid every string-match
        // fix whenever *any* JSON suggestion was present — so e.g. the `use rand::RngExt;`
        // MaybeIncorrect suggestion for `random_range` suppressed
        // `parse_self_borrow_helper_conflict` for an orthogonal E0502 borrow error, leaving
        // the borrow error stuck across all 5 repair rounds. Both sources must now run.
        let root = temp_root("compile_repair_union_sources");
        fs::create_dir_all(root.join("src/contexts")).unwrap();
        fs::write(
            root.join("src/contexts/game_loop.rs"),
            "impl GameLoopContext {\n    pub fn tick(&mut self) {\n        self.command_capture(&mut self.command);\n    }\n    fn command_capture(&self, command_: &mut CommandInputContext) {\n        command_.capture();\n    }\n}\n",
        )
        .unwrap();
        let workspace = Workspace::discover(root).unwrap();

        // JSON pass: one MaybeIncorrect `use rand::RngExt;` suggestion (same shape as the real
        // snake project's `random_range` error).
        let event = serde_json::json!({
            "reason": "compiler-message",
            "message": {
                "message": "no method named `random_range` found for struct `ThreadRng`",
                "level": "error",
                "code": {"code": "E0599"},
                "rendered": "error[E0599]",
                "spans": [{
                    "file_name": "src/contexts/game_loop.rs",
                    "line_start": 174,
                    "line_end": 174,
                    "column_start": 25,
                    "column_end": 37,
                    "is_primary": true
                }],
                "children": [{
                    "message": "help",
                    "level": "help",
                    "code": null,
                    "rendered": null,
                    "spans": [{
                        "file_name": "src/contexts/game_loop.rs",
                        "line_start": 1,
                        "line_end": 1,
                        "column_start": 1,
                        "column_end": 1,
                        "is_primary": false,
                        "suggested_replacement": "use rand::RngExt;\n",
                        "suggestion_applicability": "MaybeIncorrect"
                    }],
                    "children": []
                }]
            }
        });
        let diagnostics = parse_cargo_json_diagnostics(&serde_json::to_string(&event).unwrap());

        // String-match pass: an orthogonal E0502 borrow error on a different line.
        let stderr = "src/contexts/game_loop.rs:3:30: error[E0502]: cannot borrow `self.command` as mutable because it is also borrowed as immutable: mutable borrow occurs here\n";

        let fixes = collect_all_compile_fixes(&workspace, stderr, &diagnostics);

        assert!(
            fixes.iter().any(|f| matches!(
                f,
                CompileFix::ApplyRustcSuggestion { replacement, .. } if replacement.contains("use rand::RngExt;")
            )),
            "JSON suggestion must still be applied, got {fixes:?}"
        );
        assert!(
            fixes.iter().any(|f| matches!(
                f,
                CompileFix::ConvertHelperToAssociatedFn { method_name, .. } if method_name == "command_capture"
            )),
            "string-match borrow-helper fix must fire alongside a JSON suggestion, got {fixes:?}"
        );
    }

    #[test]
    fn machine_applicable_suggestion_suppresses_maybe_incorrect_fallback() {
        // When a MachineApplicable suggestion is available for any error in the batch, the
        // MaybeIncorrect tier must not activate for other errors in the same run — otherwise
        // a speculative `use Foo;` could be applied alongside a trusted patch and confuse the
        // next repair round.
        let machine = serde_json::json!({
            "reason": "compiler-message",
            "message": {
                "message": "clone required",
                "level": "error",
                "code": {"code": "E0507"},
                "rendered": "error[E0507]",
                "spans": [{
                    "file_name": "src/a.rs",
                    "line_start": 5,
                    "line_end": 5,
                    "column_start": 1,
                    "column_end": 10,
                    "is_primary": true,
                    "suggested_replacement": "foo.clone()",
                    "suggestion_applicability": "MachineApplicable"
                }],
                "children": []
            }
        });
        let maybe = serde_json::json!({
            "reason": "compiler-message",
            "message": {
                "message": "trait not in scope",
                "level": "error",
                "code": {"code": "E0599"},
                "rendered": "error[E0599]",
                "spans": [{
                    "file_name": "src/b.rs",
                    "line_start": 1,
                    "line_end": 1,
                    "column_start": 1,
                    "column_end": 1,
                    "is_primary": true
                }],
                "children": [{
                    "message": "help",
                    "level": "help",
                    "code": null,
                    "rendered": null,
                    "spans": [{
                        "file_name": "src/b.rs",
                        "line_start": 1,
                        "line_end": 1,
                        "column_start": 1,
                        "column_end": 1,
                        "is_primary": false,
                        "suggested_replacement": "use crate::Foo;\n",
                        "suggestion_applicability": "MaybeIncorrect"
                    }],
                    "children": []
                }]
            }
        });
        let stdout = format!(
            "{}\n{}\n",
            serde_json::to_string(&machine).unwrap(),
            serde_json::to_string(&maybe).unwrap()
        );
        let diags = parse_cargo_json_diagnostics(&stdout);
        let fixes = diagnostic_suggestions_to_fixes(&diags);
        assert!(fixes.iter().any(|f| matches!(
            f,
            CompileFix::ApplyRustcSuggestion { applicability, .. } if applicability == "MachineApplicable"
        )));
        assert!(fixes.iter().all(|f| !matches!(
            f,
            CompileFix::ApplyRustcSuggestion { applicability, .. } if applicability == "MaybeIncorrect"
        )), "MaybeIncorrect tier must stay silent while a MachineApplicable fix is available; got {fixes:?}");
    }

    #[test]
    fn parse_cargo_json_diagnostic_extracts_suggested_replacement() {
        // A minimal rustc json event with a MachineApplicable suggestion child.
        let event = serde_json::json!({
            "reason": "compiler-message",
            "message": {
                "message": "no method named `random_range` found for mutable reference `&mut ThreadRng`",
                "level": "error",
                "code": {"code": "E0599"},
                "rendered": "error[E0599]: no method named `random_range`\n  --> src/a.rs:10:5",
                "spans": [
                    {
                        "file_name": "src/a.rs",
                        "line_start": 10,
                        "line_end": 10,
                        "column_start": 5,
                        "column_end": 17,
                        "is_primary": true
                    }
                ],
                "children": [
                    {
                        "message": "the following trait defines an item `random_range`, perhaps you need to implement it",
                        "level": "help",
                        "code": null,
                        "rendered": null,
                        "spans": [
                            {
                                "file_name": "src/a.rs",
                                "line_start": 1,
                                "line_end": 1,
                                "column_start": 1,
                                "column_end": 1,
                                "is_primary": false,
                                "suggested_replacement": "use rand::RngExt;\n",
                                "suggestion_applicability": "MachineApplicable"
                            }
                        ],
                        "children": []
                    }
                ]
            }
        });
        let line = serde_json::to_string(&event).unwrap();
        let diags = parse_cargo_json_diagnostics(&line);
        assert_eq!(diags.len(), 1);
        let fixes = diagnostic_suggestions_to_fixes(&diags);
        assert!(fixes.iter().any(|f| matches!(
            f,
            CompileFix::ApplyRustcSuggestion { replacement, .. } if replacement == "use rand::RngExt;\n"
        )));
    }

    #[test]
    fn apply_span_replacement_splices_multibyte_content() {
        // A unicode character occupies more than one byte but rustc counts chars for columns.
        let content = "let name = \"résumé\";\nprintln!();\n";
        // Replace `résumé` (columns 13..19 on line 1 when counting chars) with `name`.
        let updated = apply_span_replacement(content, 1, 13, 1, 19, "name").unwrap();
        assert_eq!(updated, "let name = \"name\";\nprintln!();\n");
    }

    #[test]
    fn revert_fn_body_to_todo_replaces_body_balanced() {
        let content = r#"pub fn foo(x: i32) -> i32 {
    let y = x + 1;
    while y > 0 {
        break;
    }
    y
}
"#;
        // Hint points inside the body (the `while` line).
        let updated = revert_fn_body_to_todo(content, 3, "reset body").unwrap();
        assert!(updated.contains("todo!(\"reset body\")"));
        assert!(!updated.contains("let y = x + 1;"));
        // Surrounding structure (signature and trailing newline) preserved.
        assert!(updated.starts_with("pub fn foo(x: i32) -> i32 {"));
        assert!(updated.ends_with("\n"));
    }

    #[test]
    fn detect_question_mark_in_non_result_produces_revert_fix() {
        let diag = RustcDiagnostic {
            code: Some("E0277".to_string()),
            level: "error".to_string(),
            message:
                "the `?` operator can only be used in a method that returns `Result` or `Option`"
                    .to_string(),
            rendered: Some("error[E0277]".to_string()),
            spans: vec![RustcSpan {
                file_name: "src/contexts/command_input.rs".to_string(),
                line_start: 42,
                line_end: 42,
                column_start: 30,
                column_end: 31,
                is_primary: true,
                suggested_replacement: None,
                suggestion_applicability: None,
                label: None,
            }],
            children: vec![RustcDiagnostic {
                code: None,
                level: "note".to_string(),
                message: "this function should return `Result` or `Option`".to_string(),
                rendered: None,
                spans: vec![RustcSpan {
                    file_name: "src/contexts/command_input.rs".to_string(),
                    line_start: 35,
                    line_end: 35,
                    column_start: 1,
                    column_end: 40,
                    is_primary: false,
                    suggested_replacement: None,
                    suggestion_applicability: None,
                    label: None,
                }],
                children: vec![],
            }],
        };
        let fixes = diagnostic_suggestions_to_fixes(std::slice::from_ref(&diag));
        let has_revert = fixes.iter().any(|f| {
            matches!(
                f,
                CompileFix::RevertBodyToTodo {
                    fn_signature_line: 35,
                    ..
                }
            )
        });
        assert!(has_revert, "expected RevertBodyToTodo, got {fixes:?}");
    }

    fn temp_root(prefix: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("reen_{prefix}_{stamp}"))
    }
}
