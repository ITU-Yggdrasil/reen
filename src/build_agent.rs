use crate::agent_runner::{AgentRequest, AgentRunner, SystemBlock};
use crate::build_compile_agent::apply_agent_compile_fixes;
use crate::build_tracker::{BuildTracker, hash_string};
use crate::compile_repair::{
    COMPILE_FIX_MAX_ROUNDS, apply_compile_fix, parse_compile_errors, run_cargo_build,
};
use crate::workspace::{GENERATED_MANIFEST, Workspace};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct BuildOptions {
    pub selection: crate::workspace::Selection,
    pub fix: bool,
    pub verbose: bool,
    pub debug: bool,
    pub dry_run: bool,
}

#[derive(Debug, Clone)]
struct TodoSite {
    file: PathBuf,
    todo_marker: String,
    fn_signature: String,
    description: String,
}

const SYSTEM_PROMPT: &str = r#"You are a Rust implementation assistant for the Reen DCI build system.

You are given the full scaffolded Rust source for a workspace, the original DCI-English draft prose, and the prepared YAML artifacts. Your task is to implement a single method body.

## CRITICAL output format

Your ENTIRE response must be ONLY the Rust statements that go inside the function body.
No explanation. No commentary. No markdown fences. No alternative approaches.
Do not repeat the function signature. Do not include the opening or closing brace.
Do not discuss trade-offs or limitations — just write the code.

Bad response (has explanation):
  Looking at the YAML, I need to call foo.
  ```rust
  self.foo()
  ```

Good response (code only):
  self.foo()

## Code rules
- Use only types visible in the scaffolded source, standard library types, or Cargo.toml dependencies.
- Do not add comments unless something non-obvious is happening.
- Do not use `todo!()` or `unimplemented!()`.
- Produce valid, idiomatic Rust that will compile against the scaffolded signatures as-is.
- Implement ONLY the single method you are asked about. Do not produce code for other methods.

## Mutability — the `mutable` field in the prepared YAML
Every prepared artifact has a `mutable: true/false` field. This is the primary signal for receivers:

| `mutable` | Type kind  | Default functionality receiver |
|-----------|------------|-------------------------------|
| `true`    | context    | `&mut self`                   |
| `false`   | projection | `&self`                       |
| `false`   | data       | `&self`                       |

Use the receiver that is **already in the scaffolded signature** — do not change it. The scaffold was generated from the prepared YAML, so the receiver is already correct for the type. This rule is here so you understand why you see `&mut self` on context methods.

## DCI architecture — role methods
This codebase follows DCI (Data-Context-Interaction). Contexts orchestrate use cases.

**Role methods** are private methods on the context struct, named `<role>_<method>` in snake_case.
Their signature is:
  `fn <role>_<method>(&self, <role>_: &<RolePlayerType>, <other params>) -> <Return>`

- Role methods always have `&self` as receiver, regardless of the context's `mutable` flag.
  The context struct itself is never mutated inside a role method call.
- The **first explicit parameter** is the role player, named `<role>_` (e.g. `board_: &Board`).
  It is an immutable borrow by default. If a mutable borrow is needed, the parameter type becomes
  `&mut <RolePlayerType>` — in that case the receiver also becomes `&mut self`.
- The body delegates to the role player: call `<role>_.<method>(other_args)`.

To **call** a role method from a functionality, pass `&self.<role>` as the first argument:
  `self.<role>_<method>(&self.<role>, other_args)`

## Behavioral description in prepared YAML
When a method has natural-language flow steps (i.e. could not be reduced to machine-readable IR),
the prepared YAML contains:
- `flow`: numbered happy-path steps (already stripped of `N. ` prefix)
- `extensions`: alternative-path entries (e.g. `1a. No keys → flow ends`)
- `guarantee`: post-condition invariants
- `references`: classified identifiers (roles, props, types, role_methods) from the flow text

Use these fields as the behavioral specification for the implementation. The `references` list tells
you which role methods, types, and props the method needs; look up their signatures in the scaffolded source.

## Other conventions
- For constructors (`new`): no receiver, initialise all struct fields from parameters.
- For getters: `&self`, return the field value or a reference matching the declared return type."#;

pub fn build_workspace(workspace: &Workspace, options: &BuildOptions) -> Result<()> {
    let manifest_path = workspace.root.join(GENERATED_MANIFEST);
    if !manifest_path.exists() {
        bail!("No scaffolded files found; run `reen scaffold` first");
    }

    let manifest: GeneratedManifest = {
        let content = fs::read_to_string(&manifest_path)
            .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse {}", manifest_path.display()))?
    };

    let allowed_files: HashSet<String> = manifest.files.iter().cloned().collect();

    let sites = collect_todo_sites(workspace, &manifest)?;
    if !sites.is_empty() {
        let runner = AgentRunner::from_env()?;
        let mut tracker = BuildTracker::load(&workspace.root)?;

        let cached_context = build_cached_context(workspace, &manifest)?;

        if options.verbose {
            eprintln!(
                "build-agent: {} todo site(s) to implement (model: {})",
                sites.len(),
                runner.model()
            );
        }

        let system_blocks = vec![
            SystemBlock::cached(SYSTEM_PROMPT),
            SystemBlock::cached(&cached_context),
        ];

        let mut implemented = 0usize;
        for site in &sites {
            let track_key = format!("build:{}:{}", site.file.display(), site.fn_signature);
            let input_hash = hash_string(&format!("{}{}", cached_context, site.fn_signature));
            if tracker.is_current("build", &track_key, &input_hash) {
                if options.verbose {
                    eprintln!("  skip {} (up to date)", site.fn_signature);
                }
                continue;
            }

            let user_message = format!(
                "Implement the body of this function:\n\n```rust\n{}\n```\n\n\
                 The todo description is: {}",
                site.fn_signature, site.description
            );

            if options.verbose {
                eprintln!("  implementing {} ...", site.fn_signature);
            }

            if options.dry_run {
                continue;
            }

            let body = runner.run(&AgentRequest {
                system: system_blocks.clone(),
                user_content: &user_message,
                temperature: 0.2,
                max_tokens: 4096,
            })?;

            let body = strip_code_fences(&body);
            replace_todo_in_file(&site.file, &site.todo_marker, &body)?;

            tracker.update("build", track_key, input_hash);
            implemented += 1;

            if options.verbose {
                eprintln!("  wrote body for {}", site.fn_signature);
            }
        }

        if !options.dry_run {
            tracker.save(&workspace.root)?;
        }

        if options.verbose {
            eprintln!("build-agent: implemented {} method(s)", implemented);
        }
    } else if options.verbose {
        println!("No todo!() sites found — nothing to implement");
    }

    if !options.dry_run {
        verify_project_compiles(workspace, options, &allowed_files)?;
    }

    Ok(())
}

fn verify_project_compiles(
    workspace: &Workspace,
    options: &BuildOptions,
    manifest_files: &HashSet<String>,
) -> Result<()> {
    let mut result = run_cargo_build(workspace)?;
    if result.success {
        return Ok(());
    }

    if !options.fix {
        eprint!("{}", result.stderr);
        bail!(
            "Project failed to compile after `reen build`; re-run with --fix to attempt auto-repair"
        );
    }

    let mut last_stderr = result.stderr;
    for round in 1..=COMPILE_FIX_MAX_ROUNDS {
        let fixes = parse_compile_errors(&last_stderr);
        if !fixes.is_empty() {
            if options.verbose {
                eprintln!(
                    "build --fix round {round}: applying {} deterministic fix(es)",
                    fixes.len()
                );
            }
            for fix in &fixes {
                apply_compile_fix(workspace, fix)?;
                if options.verbose {
                    eprintln!("  {}", fix.description());
                }
            }
            result = run_cargo_build(workspace)?;
            if result.success {
                return Ok(());
            }
            last_stderr = result.stderr;
            continue;
        }

        let runner = AgentRunner::from_env().context(
            "ANTHROPIC_API_KEY is required for `reen build --fix` when deterministic repair does not apply",
        )?;
        let wrote = apply_agent_compile_fixes(
            workspace,
            manifest_files,
            &last_stderr,
            &runner,
            options.verbose,
        )?;
        if !wrote {
            eprint!("{}", last_stderr);
            bail!(
                "Project failed to compile: no deterministic fixes applied, and the compile-fix agent made no file changes"
            );
        }

        result = run_cargo_build(workspace)?;
        if result.success {
            return Ok(());
        }
        last_stderr = result.stderr;
    }

    eprint!("{}", last_stderr);
    bail!(
        "Project still fails to compile after {} `build --fix` rounds",
        COMPILE_FIX_MAX_ROUNDS
    );
}

#[derive(Debug, Deserialize, Serialize)]
struct GeneratedManifest {
    files: Vec<String>,
}

fn collect_todo_sites(
    workspace: &Workspace,
    manifest: &GeneratedManifest,
) -> Result<Vec<TodoSite>> {
    let mut sites = Vec::new();
    for relative in &manifest.files {
        let path = workspace.root.join(relative);
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let lines: Vec<&str> = content.lines().collect();
        let mut current_fn: Option<(usize, String)> = None;

        for (idx, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.contains("fn ") && trimmed.ends_with('{') {
                current_fn = Some((idx, trimmed.to_string()));
            }
            if trimmed.starts_with("todo!(") {
                let description = extract_todo_description(trimmed);
                let fn_sig = current_fn
                    .as_ref()
                    .map(|(_, sig)| sig.clone())
                    .unwrap_or_else(|| format!("unknown at line {}", idx + 1));
                sites.push(TodoSite {
                    file: path.clone(),
                    todo_marker: trimmed.to_string(),
                    fn_signature: fn_sig,
                    description,
                });
            }
        }
    }
    Ok(sites)
}

fn extract_todo_description(line: &str) -> String {
    if let Some(start) = line.find("todo!(\"") {
        let after = &line[start + 7..];
        if let Some(end) = after.find("\")") {
            return after[..end].to_string();
        }
    }
    "implement this method".to_string()
}

fn build_cached_context(workspace: &Workspace, manifest: &GeneratedManifest) -> Result<String> {
    let mut context = String::new();

    context.push_str("# Scaffolded Rust Source\n\n");
    for relative in &manifest.files {
        let path = workspace.root.join(relative);
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        context.push_str(&format!("## {}\n```rust\n{}\n```\n\n", relative, content));
    }

    context.push_str("# Draft Prose\n\n");
    let draft_dir = &workspace.drafts_dir;
    if draft_dir.is_dir() {
        let mut draft_files = Vec::new();
        collect_files_recursive(draft_dir, "md", &mut draft_files)?;
        draft_files.sort();
        for path in &draft_files {
            let relative = path.strip_prefix(&workspace.root).unwrap_or(path);
            if relative.starts_with("drafts/prepare") {
                continue;
            }
            let content = fs::read_to_string(path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            context.push_str(&format!("## {}\n{}\n\n", relative.display(), content));
        }
    }

    context.push_str("# Prepared Artifacts (YAML)\n\n");
    if workspace.prepared_dir.is_dir() {
        let mut yml_files = Vec::new();
        collect_files_recursive(&workspace.prepared_dir, "yml", &mut yml_files)?;
        yml_files.sort();
        for path in &yml_files {
            let relative = path.strip_prefix(&workspace.root).unwrap_or(path);
            let content = fs::read_to_string(path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            context.push_str(&format!(
                "## {}\n```yaml\n{}\n```\n\n",
                relative.display(),
                content
            ));
        }
    }

    Ok(context)
}

fn collect_files_recursive(dir: &Path, extension: &str, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files_recursive(&path, extension, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some(extension) {
            out.push(path);
        }
    }
    Ok(())
}

fn strip_code_fences(text: &str) -> String {
    let trimmed = text.trim();

    // If the entire response is wrapped in a single code fence, unwrap it.
    if trimmed.starts_with("```") {
        let after_open = trimmed.strip_prefix("```").unwrap();
        let after_lang = after_open.trim_start_matches(|c: char| c.is_alphanumeric() || c == '_');
        let after_lang = after_lang.strip_prefix('\n').unwrap_or(after_lang);
        if let Some(end) = after_lang.rfind("```") {
            let inner = after_lang[..end].trim();
            if !inner.contains("```") {
                return inner.to_string();
            }
        }
    }

    // Multiple code blocks mixed with prose: take the longest fenced block
    // (the LLM often shows fragments before arriving at the final answer).
    let blocks = extract_fenced_blocks(trimmed);
    if !blocks.is_empty() {
        return blocks.into_iter().max_by_key(|b| b.len()).unwrap();
    }

    // No fences at all — reject if this is clearly prose, otherwise accept as code.
    if looks_like_prose(trimmed) {
        return format!("todo!(\"agent returned prose, not code\")");
    }

    trimmed.to_string()
}

fn extract_fenced_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") && trimmed.len() > 3 {
            let mut block = String::new();
            for inner in lines.by_ref() {
                if inner.trim() == "```" {
                    break;
                }
                if !block.is_empty() {
                    block.push('\n');
                }
                block.push_str(inner);
            }
            if !block.is_empty() {
                blocks.push(block);
            }
        }
    }
    blocks
}

/// Heuristic: does this line look like executable Rust (or a tight expression), not English?
fn line_has_rust_tokens(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    if t.contains(';') || t.contains("::") {
        return true;
    }
    if t.contains('(') || t.contains('[') || t.contains('{') || t.contains('}') || t.contains("->")
    {
        return true;
    }
    if t.starts_with("let ")
        || t.starts_with("const ")
        || t.starts_with("static ")
        || t.starts_with("return")
        || t.starts_with("if ")
        || t.starts_with("match ")
        || t.starts_with("for ")
        || t.starts_with("while ")
        || t.starts_with("loop ")
        || t.starts_with("unsafe ")
        || t.starts_with("break")
        || t.starts_with("continue")
        || t.starts_with("self.")
        || t.starts_with("Self::")
        || t.starts_with("super::")
        || t.starts_with("crate::")
        || t.starts_with("&mut ")
        || t.starts_with("&'")
        || t.starts_with("&self")
        || t.starts_with("move |")
        || t.starts_with('|')
    {
        return true;
    }
    if t.contains("self.") || t.contains("Self::") {
        return true;
    }
    // Char literal: `'x'`, `' '`, `'\n'`
    if t.starts_with('\'') && t.ends_with('\'') && t.len() >= 3 {
        return true;
    }
    // Macros: `todo!(...)`, `vec![...]`
    if t.contains('!') && (t.contains('(') || t.contains('[') || t.contains('{')) {
        return true;
    }
    // Single expression without spaces: `foo`, `a.b()`, `snake_.x()`
    if !t.contains(' ') && (t.contains('.') || t.contains("->")) {
        return true;
    }
    // Short expressions with operators / punctuation (e.g. `x + 1`, `Ok(())` already caught)
    let word_count = t.split_whitespace().count();
    if word_count <= 6
        && t.chars().any(|c| {
            matches!(
                c,
                '+' | '-' | '*' | '/' | '%' | '|' | '&' | '=' | '?' | '<' | '>'
            )
        })
    {
        return true;
    }
    // Common bare literals / keywords as method bodies
    if matches!(t, "true" | "false" | "None" | "()") {
        return true;
    }
    if t.parse::<i128>().is_ok() || t.parse::<f64>().is_ok() {
        return true;
    }
    false
}

/// High-confidence “this line is English explanation”, not Rust. Keep the list narrow so
/// `The`, `For`, `I` at column 0 do not false-positive on short code.
fn line_is_prose_explanation_only(line: &str) -> bool {
    let t = line.trim();
    const PROSE_OPENERS: &[&str] = &[
        "Looking ",
        "Based on",
        "However,",
        "However ",
        "I can ",
        "I'll ",
        "I think",
        "Let me ",
        "We need ",
        "We should ",
        "Note:",
        "Since the",
        "Since we",
        "The problem",
        "This means",
        "Here is",
        "Here are",
        "I'm going",
        "You've ",
        "It looks",
        "Alternatively",
        "Therefore,",
        "To implement",
        "First,",
        "Next,",
    ];
    if !PROSE_OPENERS.iter().any(|s| t.starts_with(s)) {
        return false;
    }
    !line_has_rust_tokens(t)
}

fn looks_like_prose(text: &str) -> bool {
    let lines: Vec<&str> = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return true;
    }

    let prose_only_lines = lines
        .iter()
        .filter(|l| line_is_prose_explanation_only(l))
        .count();

    // Single line: reject only if it reads as pure explanation, not code.
    if lines.len() == 1 {
        return prose_only_lines == 1;
    }

    // Multiple lines: two+ explanation sentences ⇒ not acceptable as a method body.
    if prose_only_lines >= 2 {
        return true;
    }

    // One explanation line plus other content (often a trailing `' '` or snippet) ⇒ reject.
    if prose_only_lines == 1 && lines.len() >= 2 {
        return true;
    }

    // Longer rambles without obvious Rust: many long lines without `;` / calls / paths.
    let vague_lines = lines
        .iter()
        .filter(|l| {
            l.len() > 72
                && !line_has_rust_tokens(l)
                && !l.contains(';')
                && !l.contains('(')
                && !l.contains('{')
        })
        .count();
    vague_lines * 3 > lines.len()
}

fn replace_todo_in_file(path: &Path, todo_marker: &str, body: &str) -> Result<()> {
    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let lines: Vec<&str> = content.lines().collect();

    let todo_idx = lines
        .iter()
        .position(|line| line.trim() == todo_marker)
        .with_context(|| format!("Could not find `{}` in {}", todo_marker, path.display()))?;

    let indent = lines[todo_idx]
        .chars()
        .take_while(|c| c.is_whitespace())
        .collect::<String>();

    let mut result = String::new();
    for (idx, line) in lines.iter().enumerate() {
        if idx == todo_idx {
            for body_line in body.lines() {
                if body_line.trim().is_empty() {
                    result.push('\n');
                } else {
                    result.push_str(&indent);
                    result.push_str(body_line.trim_start());
                    result.push('\n');
                }
            }
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }

    fs::write(path, result).with_context(|| format!("Failed to write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_todo_description_quoted() {
        assert_eq!(extract_todo_description(r#"        todo!("tick")"#), "tick");
    }

    #[test]
    fn extract_todo_description_fallback() {
        assert_eq!(
            extract_todo_description("        todo!()"),
            "implement this method"
        );
    }

    #[test]
    fn strip_code_fences_rust() {
        let input = "```rust\nlet x = 1;\n```";
        assert_eq!(strip_code_fences(input), "let x = 1;");
    }

    #[test]
    fn strip_code_fences_plain() {
        let input = "let x = 1;";
        assert_eq!(strip_code_fences(input), "let x = 1;");
    }

    #[test]
    fn strip_code_fences_multi_block_takes_longest() {
        let input = "Here's a fragment:\n\n```rust\nlet a = 1;\n```\n\nActual implementation:\n\n```rust\nlet a = 1;\nlet b = 2;\nlet c = a + b;\n```\n";
        assert_eq!(
            strip_code_fences(input),
            "let a = 1;\nlet b = 2;\nlet c = a + b;"
        );
    }

    #[test]
    fn strip_code_fences_single_wrapped() {
        let input = "```rust\nlet x = 1;\nlet y = 2;\n```";
        assert_eq!(strip_code_fences(input), "let x = 1;\nlet y = 2;");
    }

    #[test]
    fn strip_code_fences_prose_becomes_todo() {
        let input = "Looking at the YAML, this method should do X.\nI think the approach is to call foo.\nHowever we also need to consider bar.\nThe implementation would be complex.\nBased on my analysis here is what I suggest.\nNote: this is tricky.\nSince we have limited info let me explain.";
        assert_eq!(
            strip_code_fences(input),
            "todo!(\"agent returned prose, not code\")"
        );
    }

    #[test]
    fn strip_code_fences_short_prose_with_trailing_literal_becomes_todo() {
        let input =
            "Looking at the YAML, I need a placeholder.\n\nBased on the flow, use a space:\n\n' '";
        assert_eq!(
            strip_code_fences(input),
            "todo!(\"agent returned prose, not code\")"
        );
    }

    #[test]
    fn strip_code_fences_bare_code_passes() {
        let input = "let keys = self.stdin_source_read_available(&self.stdin_source);\nfor key in keys {\n    self.buffer.push_back(key);\n}";
        assert_eq!(strip_code_fences(input), input);
    }

    #[test]
    fn strip_code_fences_short_expression_passes() {
        let input = "snake_.body().clone()";
        assert_eq!(strip_code_fences(input), input);
    }

    #[test]
    fn strip_code_fences_short_delegation_passes() {
        let input = "game_state_.game_started()";
        assert_eq!(strip_code_fences(input), input);
    }

    #[test]
    fn strip_code_fences_multiline_short_impl_passes() {
        let input = "self.game_state_tick(&mut self.game_state);\nself.game_state_tick(&mut self.game_state);\n0";
        assert_eq!(strip_code_fences(input), input);
    }

    #[test]
    fn strip_code_fences_bare_none_and_numeric_pass() {
        assert_eq!(strip_code_fences("None"), "None");
        assert_eq!(strip_code_fences("0"), "0");
    }

    #[test]
    fn replace_todo_in_content() {
        let dir = std::env::temp_dir().join("reen_replace_test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.rs");
        fs::write(&path, "fn foo() {\n    todo!(\"foo\")\n}\n").unwrap();
        replace_todo_in_file(&path, "todo!(\"foo\")", "42").unwrap();
        let result = fs::read_to_string(&path).unwrap();
        assert!(
            result.contains("    42"),
            "body should be indented: {result}"
        );
        assert!(!result.contains("todo!"), "todo should be gone: {result}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn replace_todo_survives_line_shifts() {
        let dir = std::env::temp_dir().join("reen_replace_shift_test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.rs");
        fs::write(
            &path,
            "fn foo() {\n    todo!(\"foo\")\n}\nfn bar() {\n    todo!(\"bar\")\n}\n",
        )
        .unwrap();
        replace_todo_in_file(
            &path,
            "todo!(\"foo\")",
            "let a = 1;\nlet b = 2;\nlet c = 3;",
        )
        .unwrap();
        replace_todo_in_file(&path, "todo!(\"bar\")", "99").unwrap();
        let result = fs::read_to_string(&path).unwrap();
        assert!(
            !result.contains("todo!"),
            "all todos should be gone: {result}"
        );
        assert!(
            result.contains("    let a = 1;"),
            "foo body present: {result}"
        );
        assert!(result.contains("    99"), "bar body present: {result}");
        let _ = fs::remove_dir_all(&dir);
    }
}
