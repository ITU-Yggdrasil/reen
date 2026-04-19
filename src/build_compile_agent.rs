//! LLM-assisted repair when `cargo build` fails after `reen build` (used with `--fix`).

use crate::agent_runner::{AgentRequest, AgentRunner, SystemBlock};
use crate::compile_repair::{
    RustcDiagnostic, collect_error_paths, filter_paths_by_manifest, normalize_manifest_path,
};
use crate::spec_context::load_artifact_spec_context_for_generated_file;
use crate::workspace::Workspace;
use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub(crate) const COMPILE_FIX_PROMPT_VERSION: &str = "compile-fix:v4:block-delimited-multi-file";

/// Marker opening a file block in the compile-fix agent's output: `<<<FILE path/to.rs>>>`.
const FILE_OPEN_PREFIX: &str = "<<<FILE ";
const FILE_OPEN_SUFFIX: &str = ">>>";
/// Marker closing a file block.
const FILE_CLOSE: &str = "<<<END>>>";

const SYSTEM_PROMPT: &str = r#"You are a compiler/build error repair assistant for the Reen DCI code generator.

You are given:
- `cargo` stderr (single-line summary).
- Rendered rustc diagnostics: full multi-line error messages with spans, notes, and help text.
- The full contents of affected project files (paths are workspace-relative). These are usually Rust source files, but may also include files like `Cargo.toml` when cargo fails before rustc can point at a `.rs` file.
- For Rust files, the enclosing function body where the primary error span lives, so the fix can be local.
- For Rust files, the role-player API block: the authoritative list of public methods on each role player referenced by that file. Do NOT invent methods outside this list.
- For generated Rust files, matching specification context (the prepared YAML and original draft) describing the intended behavior.

Your task: fix the code so the project compiles.

## DCI semantic fix guidance

- When a type-mismatch error mentions a method on a role player (e.g. `game_state_.food()`), look up that role player's API block first. Pick the method whose return type matches the expected one. Do NOT fabricate intermediate values or invent a method on the role player to bridge the gap.
- When an error says `expected X, found Y`, compare the expression that produced `Y` to the spec: the spec usually points at which role method or projection should have produced `X`. Use that instead of coercing with `.clone()` or `.into()` unless rustc's own `help:` suggests the coercion.
- Role methods live on the context (`self.<role>_<method>`), not on the role player. If you need to delegate, call the role method on `self` with `&self.<role>` as the first argument.
- Never introduce `.unwrap()` or `.expect(...)` to paper over a type mismatch. Surface the error via the existing `Result`/`Option` flow if the spec allows it; otherwise restructure the expression.
- For non-Rust files such as `Cargo.toml`, make the smallest syntax/configuration change needed to unblock compilation.

## Output format

For every file you modify, emit a block in this EXACT format:

<<<FILE src/path/to/file.rs>>>
...full new file contents, verbatim...
<<<END>>>

Rules:
- The `<<<FILE ...>>>` and `<<<END>>>` markers MUST each be on their own line with no leading whitespace.
- The content between the markers is taken VERBATIM — do NOT wrap it in markdown fences, do NOT JSON-escape quotes or newlines, do NOT add any prefix or suffix lines.
- Each block must contain the COMPLETE file text (not a diff).
- The path must match one of the `##` headers in the user message and use forward slashes (e.g. `src/contexts/foo.rs`).
- Do not include files you did not change.
- Preserve project structure and public APIs unless the compiler forces a change.
- Do not add any explanatory text outside the file blocks.

Example (a fix that edits a single file):

<<<FILE src/contexts/game_loop.rs>>>
use rand::RngExt;

pub struct GameLoopContext { /* ... */ }

impl GameLoopContext {
    pub fn tick(&mut self) { /* ... */ }
}
<<<END>>>
"#;

/// Parsed output from the compile-fix agent: one entry per `<<<FILE ...>>> ... <<<END>>>` block.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct CompileFixBlock {
    pub path: String,
    pub content: String,
}

/// Parse the compile-fix agent's block-delimited output into a list of (path, content) pairs.
///
/// The format is strict-but-forgiving: markers must be on their own lines, but the parser
/// tolerates arbitrary prose between blocks, trailing whitespace on markers, and either LF or
/// CRLF line endings in the model's output (we normalize to LF in the returned content so the
/// on-disk files stay consistent). Content bytes between the markers are preserved verbatim —
/// no escaping is assumed or performed, which is the whole point of moving off JSON.
///
/// Duplicate paths are kept in emission order; the caller applies each write in sequence, so
/// the last wins. Empty `path` strings are skipped.
pub(crate) fn parse_compile_fix_output(text: &str) -> Vec<CompileFixBlock> {
    let mut out = Vec::new();
    let mut iter = text.lines();
    while let Some(line) = iter.next() {
        let trimmed = line.trim_end();
        let Some(rest) = trimmed.strip_prefix(FILE_OPEN_PREFIX) else {
            continue;
        };
        let Some(path) = rest.strip_suffix(FILE_OPEN_SUFFIX) else {
            continue;
        };
        let path = path.trim().to_string();
        if path.is_empty() {
            continue;
        }
        let mut content_lines = Vec::new();
        let mut closed = false;
        for inner in iter.by_ref() {
            if inner.trim_end() == FILE_CLOSE {
                closed = true;
                break;
            }
            content_lines.push(inner);
        }
        if !closed {
            // Unterminated block — skip it rather than silently writing a partial file.
            // Upstream error surfacing will report "no files parsed" with a response preview.
            break;
        }
        let mut content = content_lines.join("\n");
        if !content.ends_with('\n') {
            content.push('\n');
        }
        out.push(CompileFixBlock { path, content });
    }
    out
}

/// Run the compile-fix agent: writes manifest-listed files. Returns `Ok(true)` if at least one
/// file was written.
pub(crate) fn apply_agent_compile_fixes(
    workspace: &Workspace,
    allowed_files: &HashSet<String>,
    stderr: &str,
    diagnostics: &[RustcDiagnostic],
    runner: &AgentRunner,
    verbose: bool,
) -> Result<bool> {
    // Cargo `--message-format=short` may print workspace-relative paths or absolute paths under
    // `workspace.root`; manifest entries are always relative. Merge primary error spans from JSON
    // diagnostics so we still have candidate files if short-format lines are missing or odd.
    let mut raw_paths = collect_error_paths(stderr);
    for diag in diagnostics.iter().filter(|d| d.level == "error") {
        for span in diag.spans.iter().filter(|s| s.is_primary) {
            raw_paths.push(PathBuf::from(&span.file_name));
        }
    }
    let paths = filter_paths_by_manifest(workspace, raw_paths, allowed_files);
    if paths.is_empty() {
        bail!(
            "No compiler-reported file paths matched generated manifest files; cannot run compile-fix agent. First 400 chars of compiler output: {}",
            stderr.chars().take(400).collect::<String>()
        );
    }

    let mut user = String::new();
    // Opaque version tag so the compile-fix agent's cache invalidates when we change the
    // prompt shape. (The outer build-agent tracks keys by file+fn; this lives in the log.)
    user.push_str(&format!("<!-- {COMPILE_FIX_PROMPT_VERSION} -->\n"));
    let deps_section = crate::build_agent::render_dependency_context(workspace)?;
    if !deps_section.is_empty() {
        user.push_str(&deps_section);
        user.push('\n');
    }
    user.push_str("# Cargo build errors (short stderr)\n\n```\n");
    user.push_str(stderr);
    user.push_str("\n```\n\n");

    if !diagnostics.is_empty() {
        user.push_str("# Rendered rustc diagnostics (full)\n\n");
        for diag in diagnostics.iter().filter(|d| d.level == "error") {
            if let Some(rendered) = diag.rendered.as_deref() {
                user.push_str("```\n");
                user.push_str(rendered.trim_end_matches('\n'));
                user.push_str("\n```\n\n");
            }
        }
    }

    user.push_str("# Files\n\n");
    for p in &paths {
        let rel = normalize_manifest_path(p);
        let full = workspace.root.join(p);
        let content = fs::read_to_string(&full)
            .with_context(|| format!("Failed to read {}", full.display()))?;
        let fence = match full.file_name().and_then(|name| name.to_str()) {
            Some("Cargo.toml") => "toml",
            _ if rel.ends_with(".rs") => "rust",
            _ => "",
        };
        user.push_str(&format!("## {rel}\n\n```{fence}\n{content}\n```\n\n"));

        if rel.ends_with(".rs") {
            if let Some(body_block) = render_enclosing_function_bodies(&content, &rel, diagnostics)
            {
                user.push_str("### Enclosing function bodies where errors land\n\n");
                user.push_str(&body_block);
                user.push('\n');
            }

            if let Some(api_block) =
                crate::build_agent::render_role_player_api_block(workspace, &full)?
            {
                user.push_str(
                    "### Workspace type APIs for this file (authoritative — do not invent methods)\n\n",
                );
                user.push_str(&api_block);
                user.push('\n');
            }
        }
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
        "Return ONLY the file blocks described in the system prompt (`<<<FILE ...>>>` / `<<<END>>>`). \
         Paths must match the ## headers above.",
    );

    if verbose {
        eprintln!(
            "build --fix: compile-fix agent ({} file(s), model: {})",
            paths.len(),
            runner.model()
        );
    }

    let raw = runner.run(&AgentRequest {
        system: vec![SystemBlock::new(SYSTEM_PROMPT)],
        user_content: &user,
        temperature: 0.2,
        // Compile-fix patches carry the full rewritten file text between `<<<FILE>>>` /
        // `<<<END>>>` markers — no JSON escaping, so the token count tracks the source length
        // much more closely than under the old JSON protocol. 32k is still chosen for two
        // reasons: it comfortably covers two medium files in one shot, and it fits under the
        // Opus-4 per-call cap (32k) so the runner's model-aware clamp is a no-op on every
        // model we support. Truncations now surface as a clear
        // `stop_reason == "max_tokens"` error from `AgentRunner::run` instead of a silent
        // JSON parse failure.
        max_tokens: 32_000,
    })?;

    let blocks = parse_compile_fix_output(&raw);
    if blocks.is_empty() {
        bail!(
            "compile-fix agent returned no `<<<FILE ...>>>` blocks. First 400 chars of response: {}",
            raw.chars().take(400).collect::<String>()
        );
    }

    let mut wrote = false;
    let normalized_allowed = allowed_files
        .iter()
        .map(|path| path.replace('\\', "/").trim_start_matches("./").to_string())
        .collect::<HashSet<_>>();
    for block in blocks {
        let normalized = block
            .path
            .replace('\\', "/")
            .trim_start_matches("./")
            .to_string();
        validate_relative_workspace_path(&normalized)?;
        if !normalized_allowed.contains(&normalized) {
            bail!(
                "compile-fix agent wrote path {:?} which is not listed in the generated manifest",
                block.path
            );
        }
        let dest = safe_join_under_root(&workspace.root, &normalized)?;
        if verbose {
            eprintln!("  write {}", normalized);
        }
        fs::write(&dest, block.content)
            .with_context(|| format!("Failed to write {}", dest.display()))?;
        wrote = true;
    }

    Ok(wrote)
}

/// For every primary-error span in `diagnostics` that lands in `file_rel`, extract the enclosing
/// `fn ... { ... }` block from `file_content` and render it under a labeled heading so the LLM
/// sees exactly the scope it needs to edit without wading through the whole file.
///
/// Returns `None` if no diagnostic points at this file, or if the file doesn't contain any
/// recognizable `fn` signatures around the span.
fn render_enclosing_function_bodies(
    file_content: &str,
    file_rel: &str,
    diagnostics: &[RustcDiagnostic],
) -> Option<String> {
    let mut seen_lines = HashSet::<usize>::new();
    let mut blocks = Vec::new();

    let file_rel_slash = file_rel.replace('\\', "/");
    for diag in diagnostics.iter().filter(|d| d.level == "error") {
        for span in diag.spans.iter().filter(|s| s.is_primary) {
            let span_file = span.file_name.replace('\\', "/");
            let matches_file =
                span_file == file_rel_slash || span_file.ends_with(&format!("/{file_rel_slash}"));
            if !matches_file {
                continue;
            }
            let Some((sig_line_1based, body)) =
                extract_enclosing_fn_body(file_content, span.line_start)
            else {
                continue;
            };
            if !seen_lines.insert(sig_line_1based) {
                continue;
            }
            let label = match &diag.code {
                Some(code) => format!(
                    "fn at {file_rel}:{sig_line_1based} (error {code} on line {})",
                    span.line_start
                ),
                None => format!(
                    "fn at {file_rel}:{sig_line_1based} (error on line {})",
                    span.line_start
                ),
            };
            blocks.push(format!("**{label}**\n\n```rust\n{body}\n```"));
        }
    }

    if blocks.is_empty() {
        None
    } else {
        Some(blocks.join("\n\n"))
    }
}

/// Walk upward from `hint_line` (1-based) to the nearest `fn ...` signature, then brace-balance
/// forward to find the matching `}`. Returns `(signature_line_1based, full_fn_block_text)`.
fn extract_enclosing_fn_body(content: &str, hint_line: usize) -> Option<(usize, String)> {
    let lines: Vec<&str> = content.lines().collect();
    if hint_line == 0 || hint_line > lines.len() {
        return None;
    }
    let mut sig_idx = None;
    for idx in (0..hint_line).rev() {
        let trimmed = lines[idx].trim_start();
        if trimmed.starts_with("fn ")
            || trimmed.starts_with("pub fn ")
            || trimmed.starts_with("pub(crate) fn ")
            || trimmed.starts_with("pub(super) fn ")
            || trimmed.starts_with("async fn ")
            || trimmed.starts_with("pub async fn ")
            || trimmed.starts_with("unsafe fn ")
        {
            sig_idx = Some(idx);
            break;
        }
    }
    let sig_idx = sig_idx?;

    let mut depth = 0i32;
    let mut started = false;
    let mut end_idx = sig_idx;
    for (i, line) in lines.iter().enumerate().skip(sig_idx) {
        for ch in line.chars() {
            match ch {
                '{' => {
                    depth += 1;
                    started = true;
                }
                '}' => {
                    depth -= 1;
                }
                _ => {}
            }
        }
        if started && depth == 0 {
            end_idx = i;
            break;
        }
    }
    if !started {
        return None;
    }
    let body = lines[sig_idx..=end_idx].join("\n");
    Some((sig_idx + 1, body))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile_repair::{RustcDiagnostic, RustcSpan};

    #[test]
    fn parse_compile_fix_output_returns_single_block_verbatim() {
        let response = "<<<FILE src/a.rs>>>\nuse std::fmt;\n\npub fn ok() {}\n<<<END>>>\n";
        let blocks = parse_compile_fix_output(response);
        assert_eq!(
            blocks,
            vec![CompileFixBlock {
                path: "src/a.rs".to_string(),
                content: "use std::fmt;\n\npub fn ok() {}\n".to_string(),
            }]
        );
    }

    #[test]
    fn parse_compile_fix_output_handles_multiple_blocks() {
        let response = "<<<FILE src/a.rs>>>\npub fn a() {}\n<<<END>>>\n\
             \n\
             <<<FILE src/b.rs>>>\npub fn b() {}\n<<<END>>>\n";
        let blocks = parse_compile_fix_output(response);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].path, "src/a.rs");
        assert_eq!(blocks[1].path, "src/b.rs");
    }

    #[test]
    fn parse_compile_fix_output_preserves_quotes_and_backslashes_verbatim() {
        // Regression for the JSON-escaping failure mode that motivated moving to block
        // delimiters: the old JSON protocol broke when the model emitted source containing
        // unescaped `"`. Under the new protocol any Rust punctuation must flow through
        // untouched so `println!("hi");` lands on disk byte-for-byte.
        let response = "<<<FILE src/a.rs>>>\nfn main() { println!(\"hi {}\", 1); }\n<<<END>>>\n";
        let blocks = parse_compile_fix_output(response);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].content, "fn main() { println!(\"hi {}\", 1); }\n");
    }

    #[test]
    fn parse_compile_fix_output_tolerates_preamble_and_postamble_prose() {
        // Models sometimes add a short sentence before the first block despite the prompt.
        // The parser must ignore everything that isn't inside a `<<<FILE ...>>>` block so one
        // stray chat-style intro can't wipe out a useful patch.
        let response = "Sure, here is the fix:\n\
             \n\
             <<<FILE src/a.rs>>>\npub fn ok() {}\n<<<END>>>\n\
             \n\
             Hope that helps!\n";
        let blocks = parse_compile_fix_output(response);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].path, "src/a.rs");
    }

    #[test]
    fn parse_compile_fix_output_tolerates_crlf_line_endings() {
        // Some clients add \r\n; we normalize to \n-terminated content without losing data.
        let response = "<<<FILE src/a.rs>>>\r\npub fn ok() {}\r\n<<<END>>>\r\n";
        let blocks = parse_compile_fix_output(response);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].path, "src/a.rs");
        assert_eq!(blocks[0].content, "pub fn ok() {}\n");
    }

    #[test]
    fn parse_compile_fix_output_skips_unterminated_block_instead_of_writing_partial_file() {
        // If the model emits `<<<FILE ...>>>` and runs out of tokens before `<<<END>>>`, we
        // MUST NOT write a partial file to disk — better to surface "no blocks parsed" and
        // let the caller retry. This guards against exactly the failure mode that the old
        // JSON protocol produced (a truncated response silently mis-parsed).
        let response = "<<<FILE src/a.rs>>>\npub fn ok() {\n    // truncated here\n";
        let blocks = parse_compile_fix_output(response);
        assert!(
            blocks.is_empty(),
            "unterminated block must not produce a write; got {blocks:?}"
        );
    }

    #[test]
    fn parse_compile_fix_output_rejects_blocks_with_empty_path() {
        let response = "<<<FILE   >>>\npub fn nothing() {}\n<<<END>>>\n";
        let blocks = parse_compile_fix_output(response);
        assert!(blocks.is_empty());
    }

    #[test]
    fn parse_compile_fix_output_preserves_empty_file() {
        // Deleting a file's body down to nothing is degenerate but the parser should still
        // produce a block with an empty (or just-newline) content so the caller can decide.
        let response = "<<<FILE src/a.rs>>>\n<<<END>>>\n";
        let blocks = parse_compile_fix_output(response);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].content, "\n");
    }

    #[test]
    fn extract_enclosing_fn_body_finds_signature_above_hint() {
        let content = r#"pub fn alpha() {
    // body
}

pub fn beta(x: i32) -> i32 {
    let y = x + 1;
    y * 2
}
"#;
        // Hint on the `let y` line (line 6, 1-based) — should resolve to `beta`.
        let (sig_line, body) = extract_enclosing_fn_body(content, 6).unwrap();
        assert_eq!(sig_line, 5);
        assert!(body.starts_with("pub fn beta"));
        assert!(body.trim_end().ends_with('}'));
        assert!(body.contains("let y = x + 1;"));
    }

    #[test]
    fn render_enclosing_function_bodies_skips_files_without_primary_span() {
        let content = "pub fn only() {\n    let _ = 1;\n}\n";
        let diagnostics = vec![RustcDiagnostic {
            code: Some("E0308".to_string()),
            level: "error".to_string(),
            message: "type mismatch".to_string(),
            rendered: None,
            spans: vec![RustcSpan {
                file_name: "src/other.rs".to_string(),
                line_start: 2,
                line_end: 2,
                column_start: 1,
                column_end: 1,
                is_primary: true,
                suggested_replacement: None,
                suggestion_applicability: None,
                label: None,
            }],
            children: vec![],
        }];
        let block = render_enclosing_function_bodies(content, "src/this.rs", &diagnostics);
        assert!(block.is_none());
    }

    #[test]
    fn render_enclosing_function_bodies_picks_matching_file() {
        let content = "pub fn only() {\n    let x = 1;\n    x\n}\n";
        let diagnostics = vec![RustcDiagnostic {
            code: Some("E0308".to_string()),
            level: "error".to_string(),
            message: "type mismatch".to_string(),
            rendered: None,
            spans: vec![RustcSpan {
                file_name: "src/this.rs".to_string(),
                line_start: 2,
                line_end: 2,
                column_start: 1,
                column_end: 6,
                is_primary: true,
                suggested_replacement: None,
                suggestion_applicability: None,
                label: None,
            }],
            children: vec![],
        }];
        let block = render_enclosing_function_bodies(content, "src/this.rs", &diagnostics).unwrap();
        assert!(block.contains("fn at src/this.rs:1"));
        assert!(block.contains("let x = 1;"));
    }
}
