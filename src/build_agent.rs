use crate::agent_runner::{AgentRequest, AgentRunner, SystemBlock};
use crate::build_tracker::{BuildTracker, hash_string};
use crate::workspace::{GENERATED_MANIFEST, Workspace};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct BuildOptions {
    pub selection: crate::workspace::Selection,
    pub verbose: bool,
    pub debug: bool,
    pub dry_run: bool,
}

#[derive(Debug, Clone)]
struct TodoSite {
    file: PathBuf,
    line: usize,
    fn_signature: String,
    description: String,
}

const SYSTEM_PROMPT: &str = r#"You are a Rust implementation assistant for the Reen DCI build system.

You are given the full scaffolded Rust source for a workspace, the original DCI-English draft prose, and the prepared YAML artifacts. Your task is to implement a single method body.

## Output rules
- Return ONLY the lines that go between the opening `{` and closing `}` of the function. Do not include the function signature or braces.
- Use only types visible in the scaffolded source, standard library types, or types from the project's Cargo.toml dependencies.
- Do not add comments unless something non-obvious is happening.
- Do not use `todo!()` or `unimplemented!()`.
- Do not wrap the output in markdown fences.
- Produce valid, idiomatic Rust that will compile.

## DCI architecture — role methods
This codebase follows DCI (Data-Context-Interaction). Contexts orchestrate use cases.

**Role methods** are private methods on the context struct, named `<role>_<method>` in snake_case.
Their signature is:
  `fn <role>_<method>(&self, <role>_: &<RolePlayerType>, <other params>) -> <Return>`

- `self` refers to the **context** (always `&self`; the context struct is never mutated by a role method).
- The **first explicit parameter** is the role player, named `<role>_` (e.g. `board_: &Board`).
  It is an immutable borrow by default. If a mutable borrow is needed, the parameter type becomes `&mut <RolePlayerType>` — in that case also change `&self` to `&mut self`.
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
- For constructors (`new`): initialise all struct fields from parameters.
- For getters: return the field value or a reference, matching the declared return type.
- Prefer `&self` on functionalities unless the method clearly mutates the context's own fields."#;

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

    let sites = collect_todo_sites(workspace, &manifest)?;
    if sites.is_empty() {
        if options.verbose {
            println!("No todo!() sites found — nothing to implement");
        }
        return Ok(());
    }

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
        let track_key = format!(
            "build:{}:{}",
            site.file.display(),
            site.fn_signature
        );
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
        replace_todo_in_file(&site.file, site.line, &body)?;

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
    Ok(())
}

#[derive(Debug, Deserialize, Serialize)]
struct GeneratedManifest {
    files: Vec<String>,
}

fn collect_todo_sites(workspace: &Workspace, manifest: &GeneratedManifest) -> Result<Vec<TodoSite>> {
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
                    line: idx + 1,
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
            context.push_str(&format!(
                "## {}\n{}\n\n",
                relative.display(),
                content
            ));
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

    // Multiple code blocks mixed with prose: extract and join all fenced blocks.
    let blocks = extract_fenced_blocks(trimmed);
    if !blocks.is_empty() {
        return blocks.join("\n");
    }

    trimmed.to_string()
}

fn extract_fenced_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") && trimmed.len() > 3 {
            // Opening fence (e.g. ```rust) — collect until closing ```
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

fn replace_todo_in_file(path: &Path, line_number: usize, body: &str) -> Result<()> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let lines: Vec<&str> = content.lines().collect();
    let todo_idx = line_number - 1;

    if todo_idx >= lines.len() {
        bail!(
            "Line {} is out of range in {} ({} lines)",
            line_number,
            path.display(),
            lines.len()
        );
    }

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
        assert_eq!(
            extract_todo_description(r#"        todo!("tick")"#),
            "tick"
        );
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
    fn strip_code_fences_multi_block() {
        let input = "Here's step 1:\n\n```rust\nlet a = 1;\n```\n\nAnd step 2:\n\n```rust\nlet b = 2;\n```\n";
        assert_eq!(strip_code_fences(input), "let a = 1;\nlet b = 2;");
    }

    #[test]
    fn strip_code_fences_single_wrapped() {
        let input = "```rust\nlet x = 1;\nlet y = 2;\n```";
        assert_eq!(strip_code_fences(input), "let x = 1;\nlet y = 2;");
    }

    #[test]
    fn replace_todo_in_content() {
        let dir = std::env::temp_dir().join("reen_replace_test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.rs");
        fs::write(&path, "fn foo() {\n    todo!(\"foo\")\n}\n").unwrap();
        replace_todo_in_file(&path, 2, "42").unwrap();
        let result = fs::read_to_string(&path).unwrap();
        assert!(result.contains("    42"), "body should be indented: {result}");
        assert!(!result.contains("todo!"), "todo should be gone: {result}");
        let _ = fs::remove_dir_all(&dir);
    }
}
