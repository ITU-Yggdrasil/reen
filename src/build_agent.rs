use crate::agent_runner::{AgentRequest, AgentRunner, SystemBlock};
use crate::build_compile_agent::apply_agent_compile_fixes;
use crate::build_tracker::{BuildTracker, hash_string};
use crate::compile_repair::{
    COMPILE_FIX_MAX_ROUNDS, apply_compile_fix, canonicalize_stderr_for_compare, is_structural_error,
    parse_compile_errors, run_cargo_build,
};
use crate::spec_context::load_artifact_spec_context_for_generated_file;
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

### How role methods participate in the interaction
There are two distinct call shapes — do not conflate them:

1. **Functionality → role method.** A public functionality on the context dispatches to a role method by calling
   `self.<role>_<method>(&self.<role>, other_args)`.
   The role method lives on the context, so you are calling a method on `self`, not on the role player.

2. **Role method body → role player.** Inside `fn <role>_<method>(&self, <role>_: &<RolePlayerType>, …)`, you are *implementing* the use-case step that belongs to the `<role>` role. The body is use-case logic, not a thin wrapper over a same-named method on the role player.

### Implementing role method bodies — CRITICAL
When writing the body of `fn <role>_<method>(&self, <role>_: &<RolePlayerType>, …)`:

- You MAY call methods on the role player via `<role>_.<api_method>(args)` — but ONLY methods that are explicitly declared in the role player's prepared YAML (listed below under "Role player APIs"). The fact that the role method happens to share a name with a would-be role-player method does NOT mean that method exists. Check the API block.
- If the role player does not expose what you need, you MUST NOT invent methods on it. Data types are "barely smart" and are never extended to satisfy a use case. Compose the result from the role player's real API combined with other roles, props, and role methods visible on `self`.
- You MAY call other role methods on `self` to build up the logic: `self.<other_role>_<other_method>(&self.<other_role>, …)`.
- Never call `.unwrap()`, `.clone()`, `.expect(...)`, or any other method on a role player unless you can point to it in that role player's spec. Primitives and std types like `Option`/`Result` follow their own rules (see the role player's Rust type).

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

const BUILD_TRACKER_VERSION: &str = "build:v4:role-method-vs-roleplayer";

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
            let user_message = build_method_user_message(workspace, site)?;
            let track_key = format!("build:{}:{}", site.file.display(), site.fn_signature);
            let input_hash = hash_string(&format!(
                "{BUILD_TRACKER_VERSION}\n{SYSTEM_PROMPT}\n{cached_context}\n{user_message}"
            ));
            if tracker.is_current("build", &track_key, &input_hash) {
                if options.verbose {
                    eprintln!("  skip {} (up to date)", site.fn_signature);
                }
                continue;
            }

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
            if is_self_recursive_body(&body, &site.fn_signature) {
                if options.verbose {
                    eprintln!(
                        "  rejecting self-recursive body for {}; re-prompting with warning",
                        site.fn_signature
                    );
                }
                let retry_message = format!(
                    "{user_message}\n\nIMPORTANT: Your previous attempt produced a body that \
                     simply calls the same method it is implementing (`self.<same name>(...)`), \
                     which would cause infinite recursion. Re-read the draft and prepared YAML: \
                     the method's body MUST be implemented in terms of role methods \
                     (named `<role>_<method>` on `self`), fields on `self`, or collaborator \
                     methods — never the function being implemented itself."
                );
                let body = runner.run(&AgentRequest {
                    system: system_blocks.clone(),
                    user_content: &retry_message,
                    temperature: 0.2,
                    max_tokens: 4096,
                })?;
                let body = strip_code_fences(&body);
                if is_self_recursive_body(&body, &site.fn_signature) {
                    if options.verbose {
                        eprintln!(
                            "  still recursive after retry; leaving todo!() in place for {}",
                            site.fn_signature
                        );
                    }
                    continue;
                }
                replace_todo_in_file(&site.file, &site.todo_marker, &body)?;
            } else {
                replace_todo_in_file(&site.file, &site.todo_marker, &body)?;
            }

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

fn build_method_user_message(workspace: &Workspace, site: &TodoSite) -> Result<String> {
    let mut user_message = format!(
        "Implement the body of this function:\n\n```rust\n{}\n```\n\n\
         The todo description is: {}\n\n\
         Prefer the original draft and prepared artifact over implementation-shaped guesses. \
         If the draft implies a named workspace type or existing collaborator API, use that instead of inventing a structural substitute.\n\n",
        site.fn_signature, site.description
    );

    let spec = load_artifact_spec_context_for_generated_file(workspace, &site.file)?;
    if let Some(spec) = spec.as_ref() {
        user_message.push_str("# Relevant specification\n\n");
        user_message.push_str(&spec.render_prompt_block());
    }

    if let Some(role_player_api_block) = render_role_player_api_block(workspace, &site.file)? {
        user_message.push_str("# Role player APIs (authoritative — do not invent methods)\n\n");
        user_message.push_str(&role_player_api_block);
    }

    Ok(user_message)
}

/// Gather the real public APIs of every role player referenced by the enclosing artifact so the
/// LLM can see which methods actually exist on each collaborator. Returns `None` if the generated
/// file doesn't map to an artifact or the artifact has no roles/collaborators.
fn render_role_player_api_block(
    workspace: &Workspace,
    generated_file: &Path,
) -> Result<Option<String>> {
    use crate::prepared::PreparedArtifact;
    use crate::workspace::PREPARED_DIR;

    let Some(prepared_relative) =
        crate::spec_context::prepared_relative_for_generated_file(workspace, generated_file)
    else {
        return Ok(None);
    };
    let prepared_path = workspace.root.join(&prepared_relative);
    if !prepared_path.is_file() {
        return Ok(None);
    }
    let yaml = fs::read_to_string(&prepared_path)
        .with_context(|| format!("Failed to read {}", prepared_path.display()))?;
    let artifact: PreparedArtifact = match serde_yaml::from_str(&yaml) {
        Ok(a) => a,
        Err(_) => return Ok(None),
    };

    // Collect (role_field_name, rust_type) entries from roles + collaborators.
    let mut role_types: Vec<(String, String)> = Vec::new();
    for role in &artifact.roles {
        if let Some(ty) = role.type_status.rust() {
            role_types.push((role.name.clone(), ty.to_string()));
        }
    }
    for collab in &artifact.collaborators {
        if let Some(ty) = collab.type_status.rust() {
            role_types.push((collab.name.clone(), ty.to_string()));
        }
    }
    if role_types.is_empty() {
        return Ok(None);
    }

    // Find and summarize the public API of each local role-player type by scanning every
    // prepared artifact and matching on `export.name`.
    let prepared_dir = workspace.root.join(PREPARED_DIR);
    let all_artifacts = load_all_prepared_artifacts(&prepared_dir)?;

    let mut out = String::new();
    let mut seen = HashSet::new();
    for (role_name, rust_type) in &role_types {
        // Strip borrows and generic wrappers to get the export name we can match on.
        let simple = simple_export_name(rust_type);
        let Some(simple) = simple else {
            continue;
        };
        if !seen.insert(simple.clone()) {
            continue;
        }
        let Some(target) = all_artifacts
            .iter()
            .find(|artifact| artifact.export.name == simple)
        else {
            continue;
        };

        out.push_str(&format!(
            "## Role `{role_name}` → `{rust_type}`\n\n"
        ));
        let methods = summarize_public_methods(target);
        if methods.is_empty() {
            out.push_str(
                "_(No public methods declared on this type. Do not call any methods on it.)_\n\n",
            );
        } else {
            out.push_str("Allowed public methods (call ONLY these on the role player):\n");
            for sig in methods {
                out.push_str(&format!("- `{sig}`\n"));
            }
            out.push('\n');
        }
    }

    if out.is_empty() {
        Ok(None)
    } else {
        Ok(Some(out))
    }
}

fn simple_export_name(rust_type: &str) -> Option<String> {
    let mut s = rust_type.trim();
    while let Some(stripped) = s.strip_prefix('&') {
        s = stripped.trim_start();
    }
    s = s.strip_prefix("mut ").unwrap_or(s).trim_start();
    // Strip generic wrappers like `Option<T>` / `Vec<T>` by taking the content before `<`.
    let name = s.split('<').next()?.trim();
    // Take the last path segment (e.g. `crate::board::Board` → `Board`).
    let last = name.rsplit("::").next()?.trim();
    if last.is_empty()
        || last
            .chars()
            .next()
            .is_some_and(|ch| !ch.is_ascii_uppercase())
    {
        return None;
    }
    Some(last.to_string())
}

fn summarize_public_methods(artifact: &crate::prepared::PreparedArtifact) -> Vec<String> {
    let mut out = Vec::new();
    for getter in &artifact.getters {
        let Some(field) = artifact
            .fields
            .iter()
            .find(|field| field.name == getter.field)
        else {
            continue;
        };
        let Some(field_ty) = field.type_status.rust() else {
            continue;
        };
        let ret = if getter.mode == "copy" {
            field_ty.to_string()
        } else {
            format!("&{}", field_ty)
        };
        out.push(format!("fn {}(&self) -> {ret}", getter.name));
    }
    for method in &artifact.functionalities {
        if let Some(sig) = method.signature.rust() {
            out.push(format!("fn {}", sig));
            continue;
        }
        let Some(ret) = method.return_status.rust() else {
            continue;
        };
        let receiver = method.receiver.as_deref().unwrap_or("&self");
        let params = method
            .parameters
            .iter()
            .filter_map(|p| {
                p.type_status
                    .rust()
                    .map(|ty| format!("{}: {}", p.name, ty))
            })
            .collect::<Vec<_>>()
            .join(", ");
        let head = if params.is_empty() {
            receiver.to_string()
        } else {
            format!("{receiver}, {params}")
        };
        out.push(format!("fn {}({head}) -> {ret}", method.name));
    }
    out
}

fn load_all_prepared_artifacts(
    prepared_dir: &Path,
) -> Result<Vec<crate::prepared::PreparedArtifact>> {
    let mut out = Vec::new();
    if !prepared_dir.is_dir() {
        return Ok(out);
    }
    walk_prepared_dir(prepared_dir, &mut out)?;
    Ok(out)
}

fn walk_prepared_dir(
    dir: &Path,
    out: &mut Vec<crate::prepared::PreparedArtifact>,
) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("Failed to read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_prepared_dir(&path, out)?;
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("yml") {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        if let Ok(artifact) = serde_yaml::from_str::<crate::prepared::PreparedArtifact>(&raw) {
            out.push(artifact);
        }
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
    let mut last_canonical = canonicalize_stderr_for_compare(&last_stderr);
    let mut stuck_rounds: u32 = 0;
    for round in 1..=COMPILE_FIX_MAX_ROUNDS {
        if is_structural_error(&last_stderr) {
            eprint!("{}", last_stderr);
            bail!(
                "Project failed to compile with a structural error (spec/scaffold drift): \
                 compile-repair cannot resolve missing fields, duplicate definitions, or unknown \
                 types. Re-run `reen prepare --fix` and `reen scaffold --fix` to re-sync before \
                 retrying `reen build --fix`."
            );
        }

        let error_count = count_compile_errors(&last_stderr);
        if options.verbose {
            eprintln!(
                "build --fix round {round}/{} ({error_count} compile error(s) remaining)",
                COMPILE_FIX_MAX_ROUNDS
            );
        }

        let fixes = parse_compile_errors(workspace, &last_stderr);
        let round_action = if !fixes.is_empty() {
            if options.verbose {
                eprintln!(
                    "  deterministic: applying {} fix(es)",
                    fixes.len()
                );
            }
            for fix in &fixes {
                apply_compile_fix(workspace, fix)?;
                if options.verbose {
                    eprintln!("    {}", fix.description());
                }
            }
            "deterministic"
        } else {
            let runner = AgentRunner::from_env().context(
                "ANTHROPIC_API_KEY is required for `reen build --fix` when deterministic repair does not apply",
            )?;
            if options.verbose {
                eprintln!("  agent: invoking compile-fix agent (no deterministic fixes)");
            }
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
            "agent"
        };

        result = run_cargo_build(workspace)?;
        if result.success {
            if options.verbose {
                eprintln!("build --fix round {round}: {round_action} fix(es) succeeded");
            }
            return Ok(());
        }
        let new_stderr = result.stderr;
        let new_canonical = canonicalize_stderr_for_compare(&new_stderr);
        let new_error_count = count_compile_errors(&new_stderr);
        if new_canonical == last_canonical {
            stuck_rounds += 1;
            if options.verbose {
                eprintln!(
                    "  no progress after {round_action} fix(es) (stuck round {stuck_rounds})"
                );
            }
            if stuck_rounds >= 3 {
                eprint!("{}", new_stderr);
                bail!(
                    "Project failed to compile: {} consecutive rounds produced no change in compiler output; bailing early",
                    stuck_rounds
                );
            }
        } else {
            stuck_rounds = 0;
            if options.verbose {
                let delta = new_error_count as i64 - error_count as i64;
                eprintln!(
                    "  after {round_action}: {} error(s) ({:+})",
                    new_error_count, delta
                );
            }
        }
        last_stderr = new_stderr;
        last_canonical = new_canonical;
    }

    eprint!("{}", last_stderr);
    bail!(
        "Project still fails to compile after {} `build --fix` rounds",
        COMPILE_FIX_MAX_ROUNDS
    );
}

fn count_compile_errors(stderr: &str) -> usize {
    stderr
        .lines()
        .filter(|line| {
            let t = line.trim_start();
            t.starts_with("error[") || t.starts_with("error:")
        })
        .count()
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

fn extract_fn_name(fn_signature: &str) -> Option<String> {
    let after = fn_signature.find("fn ")?;
    let rest = &fn_signature[after + 3..];
    let end = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    if end == 0 {
        None
    } else {
        Some(rest[..end].to_string())
    }
}

fn is_self_recursive_body(body: &str, fn_signature: &str) -> bool {
    let Some(name) = extract_fn_name(fn_signature) else {
        return false;
    };
    let needle = format!("self.{name}(");
    let mut remaining = body;
    while let Some(idx) = remaining.find(&needle) {
        let before = remaining[..idx].trim_end();
        if !before.ends_with('.') && !before.ends_with('&') {
            return true;
        }
        remaining = &remaining[idx + needle.len()..];
    }
    false
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

    let deps_section = render_dependency_context(workspace)?;
    if !deps_section.is_empty() {
        context.push_str(&deps_section);
        context.push('\n');
    }

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

/// Render the "Dependencies" section for LLM prompt contexts, embedding crate versions from
/// `drafts/dependencies.yml` together with curated API-for-version notes for crates whose API
/// has changed in ways that frequently trip up code generation.
///
/// Emits an empty string when `drafts/dependencies.yml` is missing or unreadable.
pub(crate) fn render_dependency_context(workspace: &Workspace) -> Result<String> {
    let path = workspace.drafts_dir.join("dependencies.yml");
    if !path.is_file() {
        return Ok(String::new());
    }
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let value: serde_yaml::Value = match serde_yaml::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return Ok(String::new()),
    };
    let Some(packages) = value
        .as_mapping()
        .and_then(|m| m.get(serde_yaml::Value::String("packages".to_string())))
        .and_then(serde_yaml::Value::as_sequence)
    else {
        return Ok(String::new());
    };

    let mut crates: Vec<(String, String)> = Vec::new();
    for pkg in packages {
        let Some(map) = pkg.as_mapping() else { continue };
        let name = map
            .get(serde_yaml::Value::String("name".to_string()))
            .and_then(serde_yaml::Value::as_str)
            .unwrap_or("")
            .to_string();
        let version = map
            .get(serde_yaml::Value::String("version".to_string()))
            .and_then(serde_yaml::Value::as_str)
            .unwrap_or("*")
            .to_string();
        if !name.is_empty() {
            crates.push((name, version));
        }
    }
    if crates.is_empty() {
        return Ok(String::new());
    }

    let mut out = String::new();
    out.push_str("# Dependencies (drafts/dependencies.yml)\n\n");
    out.push_str("These are the ONLY crates that may be used in generated bodies. Versions are authoritative — use the APIs that exist in the listed version, not older names.\n\n");
    for (name, version) in &crates {
        out.push_str(&format!("- `{name}` = `{version}`\n"));
    }
    out.push('\n');

    let notes: Vec<String> = crates
        .iter()
        .filter_map(|(name, version)| crate_api_notes(name, version).map(str::to_string))
        .collect();
    if !notes.is_empty() {
        out.push_str("## Crate API notes\n\n");
        for note in notes {
            out.push_str(&note);
            out.push_str("\n\n");
        }
    }
    Ok(out)
}

/// Hand-curated migration cues for crates whose current API names differ from older versions.
///
/// The table lists crate/version pairs together with a short note that tells the LLM which
/// current-version symbols to use. Keep entries conservative and focused on symbols the model
/// is known to mis-generate (for example `rand::thread_rng` → `rand::rng`).
fn crate_api_notes(name: &str, _version: &str) -> Option<&'static str> {
    match name {
        "rand" => Some(
            "- `rand` (0.9+): use `rand::rng()` (not `rand::thread_rng()`). On any `Rng`, call `random_range(low..high)` (not `gen_range(..)`). Use `random()` / `random_iter()` / `random_bool(p)` for the other generators.",
        ),
        "crossterm" => Some(
            "- `crossterm` (0.27+): enable raw mode with `crossterm::terminal::enable_raw_mode()?`. Poll events via `crossterm::event::poll(Duration::from_millis(..))?` + `crossterm::event::read()?`. Use `execute!(stdout, ...)` for screen-control macros.",
        ),
        "tokio" => Some(
            "- `tokio` (1+): prefer `#[tokio::main]` for `async fn main`. Task spawning is `tokio::spawn(fut)`; timers are in `tokio::time`.",
        ),
        "anyhow" => Some(
            "- `anyhow` (1+): use `anyhow::Result<T>` for the return types and `anyhow::bail!(..)`/`anyhow::ensure!(..)` for errors. `.context(\"...\")` attaches context.",
        ),
        _ => None,
    }
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

    #[test]
    fn simple_export_name_strips_borrows_and_paths() {
        assert_eq!(simple_export_name("Board"), Some("Board".into()));
        assert_eq!(simple_export_name("&Board"), Some("Board".into()));
        assert_eq!(simple_export_name("&mut Board"), Some("Board".into()));
        assert_eq!(
            simple_export_name("crate::data::Board"),
            Some("Board".into())
        );
        assert_eq!(simple_export_name("Option<Board>"), Some("Option".into()));
        assert_eq!(simple_export_name("u32"), None, "primitives skipped");
        assert_eq!(simple_export_name("&u32"), None);
    }

    #[test]
    fn role_player_api_block_lists_only_real_methods_from_prepared_yaml() {
        let dir = std::env::temp_dir().join(format!(
            "reen_role_api_block_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("drafts/prepare/data")).unwrap();
        fs::create_dir_all(dir.join("drafts/prepare/projections")).unwrap();
        fs::create_dir_all(dir.join("src/projections")).unwrap();
        fs::write(dir.join("drafts/prepare/data/Board.yml"), r#"schema: reen.prepare/v1
source:
  path: data/Board.md
  kind: data
  title: Board
export:
  name: Board
mutable: false
fields:
- name: width
  meaning: Width
  type:
    status: resolved
    rust: u32
    source: test
- name: height
  meaning: Height
  type:
    status: resolved
    rust: u32
    source: test
getters:
- name: width
  field: width
  mode: copy
- name: height
  field: height
  mode: copy
"#).unwrap();
        fs::write(
            dir.join("drafts/prepare/projections/string_renderer.yml"),
            r#"schema: reen.prepare/v1
source:
  path: projections/string_renderer.md
  kind: projection
  title: StringRenderer
export:
  name: StringRenderer
mutable: false
roles:
- name: board
  purpose: Supplies the picture
  expected_behavior: Read symbols
  type:
    status: resolved
    rust: Board
    source: name_match
  methods:
  - name: symbol_at
    signature:
      status: fixed
      rust: 'symbol_at(&self, board_: &Board, x: usize, y: usize) -> char'
      source: fix.agent
    receiver: '&self'
    parameters:
    - name: board_
      type:
        status: resolved
        rust: '&Board'
        source: prepare.role_player
    - name: x
      type:
        status: resolved
        rust: usize
        source: fix.agent
    - name: y
      type:
        status: resolved
        rust: usize
        source: fix.agent
    returns:
      status: resolved
      rust: char
      source: fix.agent
"#,
        )
        .unwrap();
        let workspace = crate::workspace::Workspace::discover(dir.clone()).unwrap();
        let generated = dir.join("src/projections/string_renderer.rs");
        let block = render_role_player_api_block(&workspace, &generated)
            .unwrap()
            .expect("expected a block when a role is present");
        assert!(
            block.contains("Role `board` → `Board`"),
            "block should name the role: {block}"
        );
        assert!(
            block.contains("fn width(&self) -> u32"),
            "block should list width getter: {block}"
        );
        assert!(
            block.contains("fn height(&self) -> u32"),
            "block should list height getter: {block}"
        );
        assert!(
            !block.contains("symbol_at"),
            "symbol_at must NOT appear because Board does not declare it: {block}"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
