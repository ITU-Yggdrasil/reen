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
    pub stderr: String,
}

pub(crate) fn run_cargo_build(workspace: &Workspace) -> Result<CompileResult> {
    let output = Command::new("cargo")
        .args(["build", "--message-format=short"])
        .env("RUSTFLAGS", "-Awarnings")
        .current_dir(&workspace.root)
        .output()
        .context("Failed to invoke cargo build")?;
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    Ok(CompileResult {
        success: output.status.success(),
        stderr,
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
    AddExternalCrate { crate_root: String },
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
                format!(
                    "replace `{from}` with `{to}` at {}:{line}",
                    file.display()
                )
            }
            CompileFix::StripUnwrapCall { file, line, method } => {
                format!(
                    "strip spurious `.{method}()` at {}:{line}",
                    file.display()
                )
            }
            CompileFix::AddTraitImport { file, trait_path } => {
                format!(
                    "add `use {trait_path};` to {} so trait methods resolve",
                    file.display()
                )
            }
            CompileFix::AddExternalCrate { crate_root } => {
                format!(
                    "register external crate `{crate_root}` in dependencies.yml and Cargo.toml"
                )
            }
        }
    }
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
    matches
        .into_iter()
        .next()
        .and_then(|path| path.strip_prefix(&workspace.root).ok().map(Path::to_path_buf))
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

/// Known trait methods that come into scope only via `use <trait>;`. Used to auto-add the import
/// when rustc reports "no method named X found for struct Y" and Y is known to implement the
/// trait in question (e.g. `rand::rngs::ThreadRng: rand::Rng`).
fn known_trait_methods() -> &'static [(&'static str, &'static str, &'static [&'static str])] {
    // (method_name, trait_path, receiver_type_substrings that indicate this trait)
    &[
        (
            "random_range",
            "rand::Rng",
            &["ThreadRng", "rand::rngs::", "rand::prelude::"],
        ),
        (
            "random_bool",
            "rand::Rng",
            &["ThreadRng", "rand::rngs::", "rand::prelude::"],
        ),
        (
            "random",
            "rand::Rng",
            &["ThreadRng", "rand::rngs::", "rand::prelude::"],
        ),
        (
            "sample",
            "rand::Rng",
            &["ThreadRng", "rand::rngs::", "rand::prelude::"],
        ),
        (
            "gen_range",
            "rand::Rng",
            &["ThreadRng", "rand::rngs::", "rand::prelude::"],
        ),
    ]
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
    if !line.contains("error[E0308]:") || !line.contains("expected `") || !line.contains(", found `&")
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
            let updated = add_crate_import(&content, type_name).unwrap_or(content);
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
            fs::write(&path, updated)
                .with_context(|| format!("Failed to write {}", path.display()))
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
            fs::write(&path, updated)
                .with_context(|| format!("Failed to write {}", path.display()))
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
    }
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
    let mut updated = cargo_raw.clone();
    if let Some(idx) = updated.find("[dependencies]\n") {
        let insert_at = idx + "[dependencies]\n".len();
        let entry = format!("{crate_root} = \"{version}\"\n");
        updated.insert_str(insert_at, &entry);
    } else {
        updated.push_str(&format!(
            "\n[dependencies]\n{crate_root} = \"{version}\"\n"
        ));
    }
    fs::write(&cargo_path, updated)
        .with_context(|| format!("Failed to write {}", cargo_path.display()))
}

fn extract_crate_version(deps_yaml: &str, crate_root: &str) -> Option<String> {
    let value: serde_yaml::Value = serde_yaml::from_str(deps_yaml).ok()?;
    let packages = value.get("packages")?.as_sequence()?;
    for pkg in packages {
        let m = pkg.as_mapping()?;
        let name = m.get(serde_yaml::Value::String("name".to_string()))?.as_str()?;
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

fn add_crate_import(content: &str, type_name: &str) -> Option<String> {
    let single_import = format!("use crate::{type_name};");
    if content.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == single_import || crate_brace_import_contains(trimmed, type_name)
    }) {
        return None;
    }

    let mut lines: Vec<String> = content.lines().map(String::from).collect();
    if let Some(idx) = lines.iter().position(|line| {
        let trimmed = line.trim();
        trimmed.starts_with("use crate::{") && trimmed.ends_with("};")
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
        lines[idx] = format!("use crate::{{{}}};", names.join(", "));
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

fn crate_brace_import_contains(line: &str, type_name: &str) -> bool {
    let Some(rest) = line.strip_prefix("use crate::{") else {
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
pub(crate) fn collect_error_rs_paths(stderr: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for line in stderr.lines() {
        if let Some(path) = parse_short_diagnostic_path(line) {
            let key = path.to_string_lossy().replace('\\', "/");
            if seen.insert(key) {
                out.push(path);
            }
        }
    }
    out
}

/// Match `path/to/file.rs:12:34:` at the start of a diagnostic line.
fn parse_short_diagnostic_path(line: &str) -> Option<PathBuf> {
    let rest = line.trim_start();
    let dot_rs = rest.find(".rs:")?;
    let path_end = dot_rs + ".rs".len();
    let path_str = &rest[..path_end];
    if !path_str.ends_with(".rs") || path_str.contains(' ') {
        return None;
    }
    // After `.rs` expect `:line:col:`
    let after = &rest[path_end..];
    if !after.starts_with(':') {
        return None;
    }
    let after = &after[1..];
    let mut parts = after.splitn(3, ':');
    parts.next()?.parse::<usize>().ok()?;
    parts.next()?.parse::<usize>().ok()?;
    Some(PathBuf::from(path_str))
}

/// Normalize a path for comparison with manifest entries (forward slashes).
pub(crate) fn normalize_manifest_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Keep only paths present in `allowed` (manifest file list).
pub(crate) fn filter_paths_by_manifest(
    paths: Vec<PathBuf>,
    allowed: &HashSet<String>,
) -> Vec<PathBuf> {
    paths
        .into_iter()
        .filter(|p| {
            let n = normalize_manifest_path(p);
            allowed.contains(&n)
        })
        .collect()
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
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parse_short_diagnostic_extracts_path() {
        let line = "src/contexts/game_loop.rs:28:16: error: lifetime may not live long enough";
        assert_eq!(
            parse_short_diagnostic_path(line),
            Some(PathBuf::from("src/contexts/game_loop.rs"))
        );
    }

    #[test]
    fn collect_error_rs_paths_dedupes() {
        let stderr = r#"src/a.rs:1:1: error[E0001]: foo
src/a.rs:2:2: error[E0002]: bar
src/b.rs:1:1: note: blah
"#;
        let paths = collect_error_rs_paths(stderr);
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&PathBuf::from("src/a.rs")));
        assert!(paths.contains(&PathBuf::from("src/b.rs")));
    }

    #[test]
    fn parse_compile_errors_detects_unique_local_type_import_fix() {
        let root = temp_root("compile_repair_direction");
        fs::create_dir_all(root.join("src/data")).unwrap();
        fs::create_dir_all(root.join("src/contexts")).unwrap();
        fs::write(root.join("src/data/direction.rs"), "pub enum Direction {}\n").unwrap();
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
        fs::write(root.join("src/data/direction.rs"), "pub enum Direction {}\n").unwrap();
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

    #[test]
    fn parse_compile_errors_skips_ambiguous_local_type_imports() {
        let root = temp_root("compile_repair_ambiguous");
        fs::create_dir_all(root.join("src/data")).unwrap();
        fs::create_dir_all(root.join("src/contexts")).unwrap();
        fs::write(root.join("src/data/direction.rs"), "pub enum Direction {}\n").unwrap();
        fs::write(root.join("src/data/other_direction.rs"), "pub struct Direction;\n").unwrap();
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
    fn parse_compile_errors_detects_trait_method_missing() {
        let root = temp_root("compile_repair_trait_method");
        fs::create_dir_all(root.join("src/contexts")).unwrap();
        let workspace = Workspace::discover(root).unwrap();
        let stderr = "src/contexts/game_loop.rs:182:25: error[E0599]: no method named `random_range` found for struct `ThreadRng` in the current scope\n";

        let fixes = parse_compile_errors(&workspace, stderr);

        assert!(fixes.iter().any(|f| matches!(
            f,
            CompileFix::AddTraitImport { trait_path, file }
                if trait_path == "rand::Rng"
                    && file == &PathBuf::from("src/contexts/game_loop.rs")
        )));
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

    fn temp_root(prefix: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("reen_{prefix}_{stamp}"))
    }
}
