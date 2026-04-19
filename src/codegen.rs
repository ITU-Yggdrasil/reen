use crate::build_tracker::{BuildTracker, hash_string};
use crate::compile_repair::{
    COMPILE_FIX_MAX_ROUNDS, apply_compile_fix, canonicalize_stderr_for_compare,
    is_structural_error, run_cargo_build,
};
use crate::prepared::{
    Ambiguity, Body, CollectionEntry, Expression, FieldSpec, GetterSpec, MethodSpec,
    PreparedArtifact, Statement,
};
use crate::prepared_contracts::collect_contract_issues;
use crate::workspace::{GENERATED_MANIFEST, Workspace};
use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

/// Shape of a role method's first parameter (the role player).
///
/// Determines how call sites spell the receiver argument: `&self.<role>` vs `&mut self.<role>`
/// vs `self.<role>.clone()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RolePlayerCallShape {
    ImmutableRef,
    MutableRef,
    Owned,
}

thread_local! {
    /// Per-composite-file map from `(role_field_name, method_name)` → call-site shape.
    /// Populated at the start of `generate_composite_file` and consulted by
    /// `render_expression` for `Expression::CallRoleMethod`. Cleared when the file is done.
    static ROLE_PLAYER_SHAPES: RefCell<BTreeMap<(String, String), RolePlayerCallShape>> =
        RefCell::new(BTreeMap::new());
}

fn classify_role_player_shape(param_rust: &str) -> RolePlayerCallShape {
    let trimmed = param_rust.trim_start();
    if let Some(rest) = trimmed.strip_prefix('&') {
        if rest.trim_start().starts_with("mut ") {
            RolePlayerCallShape::MutableRef
        } else {
            RolePlayerCallShape::ImmutableRef
        }
    } else {
        RolePlayerCallShape::Owned
    }
}

fn load_role_player_shapes(artifact: &PreparedArtifact) {
    ROLE_PLAYER_SHAPES.with(|cell| {
        let mut map = cell.borrow_mut();
        map.clear();
        for role in &artifact.roles {
            for method in &role.methods {
                let Some(first) = method.parameters.first() else {
                    continue;
                };
                let Some(rust) = first.type_status.rust() else {
                    continue;
                };
                let shape = classify_role_player_shape(rust);
                map.insert((role.name.clone(), method.name.clone()), shape);
            }
        }
    });
}

fn lookup_role_player_shape(role: &str, method: &str) -> RolePlayerCallShape {
    ROLE_PLAYER_SHAPES.with(|cell| {
        cell.borrow()
            .get(&(role.to_string(), method.to_string()))
            .copied()
            .unwrap_or(RolePlayerCallShape::ImmutableRef)
    })
}

fn clear_role_player_shapes() {
    ROLE_PLAYER_SHAPES.with(|cell| cell.borrow_mut().clear());
}
#[derive(Debug, Clone)]
pub struct ScaffoldOptions {
    pub selection: crate::workspace::Selection,
    pub fix: bool,
    pub verbose: bool,
    pub debug: bool,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct GeneratedFilesManifest {
    files: Vec<String>,
}

#[derive(Debug, Clone)]
struct LoadedArtifact {
    prepared_path: PathBuf,
    artifact: PreparedArtifact,
    output_path: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct DependencyManifest {
    #[serde(default)]
    packages: Vec<DependencyPackage>,
}

#[derive(Debug, Clone, Deserialize)]
struct DependencyPackage {
    name: String,
    version: String,
}

const SCAFFOLD_TRACKER_VERSION: &str = "scaffold:v2:spec-doc-comments";

pub fn scaffold_workspace(workspace: &Workspace, options: &ScaffoldOptions) -> Result<()> {
    let prepared_paths = workspace.prepared_paths(&options.selection)?;
    let mut tracker = BuildTracker::load(&workspace.root)?;
    let loaded = load_prepared_artifacts(workspace, &prepared_paths)?;
    validate_loaded_artifacts(&loaded)?;
    if options.fix {
        ensure_manifests_cover_prepared(workspace, &loaded)?;
    }
    let dependency_manifest = load_dependency_manifest(workspace)?;
    let aggregate_hash = hash_string(&format!(
        "{SCAFFOLD_TRACKER_VERSION}:{}",
        combined_hash(&loaded)?
    ));
    if tracker.is_current("scaffold", "scaffold:workspace", &aggregate_hash)
        && workspace.root.join(GENERATED_MANIFEST).exists()
    {
        if options.verbose {
            println!("Scaffold outputs are up to date");
        }
        return Ok(());
    }

    let order = topo_sort(&loaded)?;
    let package_name = package_name(&workspace.root);
    let library_crate = package_name.replace('-', "_");
    let generated = generate_workspace_files(
        &loaded,
        &order,
        &package_name,
        &library_crate,
        &dependency_manifest,
    )?;

    if options.dry_run {
        if options.verbose {
            println!("[dry-run] would generate {} file(s)", generated.len());
        }
        return Ok(());
    }

    clear_generated_outputs(workspace, false)?;
    for (path, content) in &generated {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        fs::write(path, content).with_context(|| format!("Failed to write {}", path.display()))?;
    }
    write_generated_manifest(workspace, generated.keys())?;

    tracker.clear_stage("build");
    tracker.save(&workspace.root)?;
    if options.debug {
        write_debug_dump(workspace, &loaded)?;
    }

    let result = run_cargo_build(workspace)?;
    if result.success {
        tracker.update("scaffold", "scaffold:workspace", aggregate_hash);
        tracker.save(&workspace.root)?;
        return Ok(());
    }

    if !options.fix {
        eprint!("{}", result.stderr);
        bail!("Generated project failed to compile; re-run with --fix to attempt auto-repair");
    }

    let max_rounds = COMPILE_FIX_MAX_ROUNDS;
    let mut last_stderr = result.stderr;
    let mut last_diagnostics = result.diagnostics;
    let mut last_canonical = canonicalize_stderr_for_compare(&last_stderr);
    let mut stuck_rounds: u32 = 0;
    for round in 1..=max_rounds {
        if is_structural_error(&last_stderr) {
            eprint!("{}", last_stderr);
            bail!(
                "Scaffold failed to compile with a structural error (missing field / duplicate \
                 method / unknown type). This indicates drift between prepared YAML and the \
                 generated scaffold — re-run `reen prepare --fix` before retrying `reen scaffold --fix`."
            );
        }

        let error_count = count_compile_errors_text(&last_stderr);
        if options.verbose {
            eprintln!(
                "scaffold --fix round {round}/{max_rounds} ({error_count} compile error(s) remaining)"
            );
        }

        let fixes =
            crate::compile_repair::collect_all_compile_fixes(workspace, &last_stderr, &last_diagnostics);
        if fixes.is_empty() {
            eprint!("{}", last_stderr);
            bail!("Generated project failed to compile but no auto-fixable errors were found");
        }
        if options.verbose {
            eprintln!("  deterministic: applying {} fix(es)", fixes.len());
        }
        for fix in &fixes {
            apply_compile_fix(workspace, fix)?;
            if options.verbose {
                eprintln!("    {}", fix.description());
            }
        }
        let result = run_cargo_build(workspace)?;
        if result.success {
            tracker.update("scaffold", "scaffold:workspace", aggregate_hash);
            tracker.save(&workspace.root)?;
            return Ok(());
        }
        let new_stderr = result.stderr;
        let new_diagnostics = result.diagnostics;
        let new_canonical = canonicalize_stderr_for_compare(&new_stderr);
        let new_error_count = count_compile_errors_text(&new_stderr);
        if new_canonical == last_canonical {
            stuck_rounds += 1;
            if options.verbose {
                eprintln!("  no progress after fix(es) (stuck round {stuck_rounds})");
            }
            if stuck_rounds >= 3 {
                eprint!("{}", new_stderr);
                bail!(
                    "Scaffold fix loop bailing: {} consecutive rounds produced no change in compiler output",
                    stuck_rounds
                );
            }
        } else {
            stuck_rounds = 0;
            if options.verbose {
                let delta = new_error_count as i64 - error_count as i64;
                eprintln!("  after fix(es): {new_error_count} error(s) ({delta:+})");
            }
        }
        last_stderr = new_stderr;
        last_diagnostics = new_diagnostics;
        last_canonical = new_canonical;
    }
    eprint!("{}", last_stderr);
    bail!("Generated project still fails to compile after {max_rounds} fix rounds");
}

fn count_compile_errors_text(stderr: &str) -> usize {
    stderr
        .lines()
        .filter(|line| {
            let t = line.trim_start();
            t.starts_with("error[") || t.starts_with("error:")
        })
        .count()
}

/// Ensure every external path referenced by the loaded prepared artifacts has a corresponding
/// entry in `drafts/dependencies.yml` and `drafts/types-manifest.yml`.
///
/// Called only when `scaffold --fix` is in effect; failures here are soft — we only log and
/// continue, since the subsequent `collect_required_dependencies` check still produces the
/// authoritative error if a crate remains unregistered.
fn ensure_manifests_cover_prepared(workspace: &Workspace, loaded: &[LoadedArtifact]) -> Result<()> {
    let mut seen_roots: BTreeSet<String> = BTreeSet::new();
    for item in loaded {
        let artifact = &item.artifact;
        for rust_type in artifact_type_strings(artifact) {
            let trimmed = rust_type.trim_start_matches('&').trim_start();
            let trimmed = trimmed.strip_prefix("mut ").unwrap_or(trimmed);
            if !trimmed.contains("::") {
                continue;
            }
            let root = trimmed.split("::").next().unwrap_or_default().to_string();
            if root.is_empty()
                || matches!(
                    root.as_str(),
                    "std" | "core" | "alloc" | "crate" | "self" | "super"
                )
            {
                continue;
            }
            if !seen_roots.insert(root.clone()) {
                continue;
            }
            let _ = crate::manifest::ensure_external_dependency_for_type(workspace, trimmed);
        }
    }
    Ok(())
}

fn artifact_type_strings(artifact: &PreparedArtifact) -> Vec<String> {
    let mut out = Vec::new();
    for field in &artifact.fields {
        if let Some(rust) = field.type_status.rust() {
            out.push(rust.to_string());
        }
    }
    for role in &artifact.roles {
        if let Some(rust) = role.type_status.rust() {
            out.push(rust.to_string());
        }
        for method in &role.methods {
            if let Some(rust) = method.return_status.rust() {
                out.push(rust.to_string());
            }
            for param in &method.parameters {
                if let Some(rust) = param.type_status.rust() {
                    out.push(rust.to_string());
                }
            }
        }
    }
    for prop in &artifact.props {
        if let Some(rust) = prop.type_status.rust() {
            out.push(rust.to_string());
        }
    }
    for collab in &artifact.collaborators {
        if let Some(rust) = collab.type_status.rust() {
            out.push(rust.to_string());
        }
    }
    for functionality in &artifact.functionalities {
        if let Some(rust) = functionality.return_status.rust() {
            out.push(rust.to_string());
        }
        for param in &functionality.parameters {
            if let Some(rust) = param.type_status.rust() {
                out.push(rust.to_string());
            }
        }
    }
    out
}

pub fn clear_generated_outputs(workspace: &Workspace, dry_run: bool) -> Result<()> {
    let manifest_path = workspace.root.join(GENERATED_MANIFEST);
    if !manifest_path.exists() {
        return Ok(());
    }
    let content = fs::read_to_string(&manifest_path)
        .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
    let manifest: GeneratedFilesManifest = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;
    for file in &manifest.files {
        let path = workspace.root.join(file);
        if dry_run {
            println!("[dry-run] would remove {}", path.display());
            continue;
        }
        if path.is_file() {
            fs::remove_file(&path)
                .with_context(|| format!("Failed to remove {}", path.display()))?;
        }
    }
    if !dry_run {
        if let Some(src) = workspace
            .root
            .join("src")
            .canonicalize()
            .ok()
            .filter(|path| path.is_dir())
        {
            let _ = prune_empty_dirs(&src, &workspace.root);
        }
        fs::remove_file(&manifest_path)
            .with_context(|| format!("Failed to remove {}", manifest_path.display()))?;
        let mut tracker = BuildTracker::load(&workspace.root)?;
        tracker.clear_stage("scaffold");
        tracker.save(&workspace.root)?;
    }
    Ok(())
}

fn write_debug_dump(workspace: &Workspace, loaded: &[LoadedArtifact]) -> Result<()> {
    let path = workspace
        .state_dir
        .join("debug")
        .join("build")
        .join("loaded.yml");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let yaml =
        serde_yaml::to_string(&loaded.iter().map(|item| &item.artifact).collect::<Vec<_>>())?;
    fs::write(&path, yaml).with_context(|| format!("Failed to write {}", path.display()))
}

fn load_prepared_artifacts(
    workspace: &Workspace,
    prepared_paths: &[PathBuf],
) -> Result<Vec<LoadedArtifact>> {
    let mut loaded = Vec::new();
    for path in prepared_paths {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let mut artifact: PreparedArtifact = serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse prepared artifact {}", path.display()))?;
        artifact.refresh_ambiguity_index();
        artifact.validate()?;
        if artifact.blocking_ambiguities().next().is_some() {
            let messages = artifact
                .blocking_ambiguities()
                .map(|ambiguity| format!("{}: {}", ambiguity.path, ambiguity.message))
                .collect::<Vec<_>>()
                .join("\n");
            bail!(
                "Prepared artifact {} contains blocking ambiguities:\n{}",
                path.display(),
                messages
            );
        }
        let output_path = output_path_for_artifact(workspace, &artifact)?;
        loaded.push(LoadedArtifact {
            prepared_path: path.clone(),
            artifact,
            output_path,
        });
    }
    Ok(loaded)
}

fn validate_loaded_artifacts(loaded: &[LoadedArtifact]) -> Result<()> {
    let exports = loaded
        .iter()
        .map(|item| item.artifact.export.name.clone())
        .collect::<BTreeSet<_>>();
    for item in loaded {
        for name in item.artifact.referenced_type_names() {
            if exports.contains(&name) || is_builtin_type(&name) {
                continue;
            }
            bail!(
                "Prepared artifact {} references unknown type `{}`",
                item.prepared_path.display(),
                name
            );
        }
    }

    let artifacts = loaded
        .iter()
        .map(|item| item.artifact.clone())
        .collect::<Vec<_>>();
    let contract_issues = collect_contract_issues(&artifacts);
    if !contract_issues.is_empty() {
        let rendered = contract_issues
            .iter()
            .map(|issue| {
                let item = &loaded[issue.artifact_index];
                format!(
                    "{}: {}: {}",
                    item.prepared_path.display(),
                    issue.ambiguity.path,
                    issue.ambiguity.message
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        bail!("Prepared artifacts contain contract mismatches:\n{rendered}");
    }
    Ok(())
}

fn combined_hash(loaded: &[LoadedArtifact]) -> Result<String> {
    let mut contents = String::new();
    for item in loaded {
        let yaml = serde_yaml::to_string(&item.artifact)?;
        contents.push_str(&item.artifact.source.path);
        contents.push('\n');
        contents.push_str(&yaml);
        contents.push('\n');
    }
    Ok(hash_string(&contents))
}

fn load_dependency_manifest(workspace: &Workspace) -> Result<BTreeMap<String, String>> {
    let path = workspace.drafts_dir.join("dependencies.yml");
    if !path.is_file() {
        return Ok(BTreeMap::new());
    }

    let content =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    let manifest: DependencyManifest = serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(manifest
        .packages
        .into_iter()
        .map(|package| (package.name, package.version))
        .collect())
}

fn topo_sort(loaded: &[LoadedArtifact]) -> Result<Vec<usize>> {
    let mut export_to_index = BTreeMap::new();
    for (idx, item) in loaded.iter().enumerate() {
        export_to_index.insert(item.artifact.export.name.clone(), idx);
    }

    let mut indegree = vec![0usize; loaded.len()];
    let mut edges = vec![Vec::<usize>::new(); loaded.len()];

    for (idx, item) in loaded.iter().enumerate() {
        for dep in item.artifact.referenced_type_names() {
            if dep == item.artifact.export.name {
                continue;
            }
            if let Some(dep_idx) = export_to_index.get(&dep).copied() {
                indegree[idx] += 1;
                edges[dep_idx].push(idx);
            }
        }
    }

    let mut queue = VecDeque::new();
    for (idx, count) in indegree.iter().enumerate() {
        if *count == 0 {
            queue.push_back(idx);
        }
    }
    let mut ordered = Vec::new();
    while let Some(idx) = queue.pop_front() {
        ordered.push(idx);
        for next in &edges[idx] {
            indegree[*next] -= 1;
            if indegree[*next] == 0 {
                queue.push_back(*next);
            }
        }
    }
    if ordered.len() != loaded.len() {
        bail!("Prepared artifacts contain a cyclic type dependency");
    }
    Ok(ordered)
}

fn output_path_for_artifact(
    _workspace: &Workspace,
    artifact: &PreparedArtifact,
) -> Result<PathBuf> {
    let relative = artifact
        .source
        .path
        .strip_prefix("drafts/")
        .unwrap_or(&artifact.source.path);
    let mut parts = relative
        .split('/')
        .map(|part| part.to_string())
        .collect::<Vec<_>>();
    match artifact.source.kind.as_str() {
        "data" => {
            parts.remove(0);
            Ok(normalized_output_path("src/data", &parts))
        }
        "projection" => {
            parts.remove(0);
            Ok(normalized_output_path("src/projections", &parts))
        }
        "context" => {
            parts.remove(0);
            Ok(normalized_output_path("src/contexts", &parts))
        }
        "app" => Ok(PathBuf::from("src/main.rs")),
        other => bail!("Unsupported prepared artifact kind `{other}`"),
    }
}

fn normalized_output_path(prefix: &str, parts: &[String]) -> PathBuf {
    let mut path = PathBuf::from(prefix);
    for (idx, part) in parts.iter().enumerate() {
        if idx + 1 == parts.len() {
            let stem = part.trim_end_matches(".md");
            path.push(format!("{}.rs", sanitize_module_name(stem)));
        } else {
            path.push(sanitize_module_name(part));
        }
    }
    path
}

fn generate_workspace_files(
    loaded: &[LoadedArtifact],
    order: &[usize],
    package_name: &str,
    library_crate: &str,
    dependency_manifest: &BTreeMap<String, String>,
) -> Result<BTreeMap<PathBuf, String>> {
    let mut files = BTreeMap::new();
    let export_kinds = loaded
        .iter()
        .map(|item| {
            (
                item.artifact.export.name.clone(),
                item.artifact.source.kind.clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();

    for idx in order {
        let item = &loaded[*idx];
        let content = match item.artifact.source.kind.as_str() {
            "data" => generate_data_file(&item.artifact, &export_kinds),
            "projection" | "context" => generate_composite_file(&item.artifact, &export_kinds),
            "app" => generate_app_file(&item.artifact, &export_kinds, library_crate),
            other => bail!("Unsupported prepared artifact kind `{other}`"),
        }?;
        files.insert(item.output_path.clone(), content);
    }

    let library_artifacts = loaded
        .iter()
        .filter(|item| item.artifact.source.kind != "app")
        .collect::<Vec<_>>();
    if !library_artifacts.is_empty() {
        files.insert(
            PathBuf::from("src/lib.rs"),
            render_lib_rs(
                library_artifacts
                    .iter()
                    .map(|item| item.output_path.as_path())
                    .collect::<Vec<_>>()
                    .as_slice(),
            ),
        );
        extend_module_files(&mut files, &library_artifacts)?;
    } else {
        files.insert(PathBuf::from("src/lib.rs"), String::new());
    }

    let local_exports = loaded
        .iter()
        .map(|item| item.artifact.export.name.clone())
        .collect::<BTreeSet<_>>();
    let dependencies =
        collect_required_dependencies(&files, dependency_manifest, &local_exports, library_crate)?;
    files.insert(
        PathBuf::from("Cargo.toml"),
        render_cargo_toml(package_name, &dependencies),
    );
    Ok(files)
}

fn extend_module_files(
    files: &mut BTreeMap<PathBuf, String>,
    library_artifacts: &[&LoadedArtifact],
) -> Result<()> {
    let mut module_tree: BTreeMap<PathBuf, ModuleNode> = BTreeMap::new();
    for artifact in library_artifacts {
        let output = &artifact.output_path;
        let relative = output.strip_prefix("src").unwrap_or(output).to_path_buf();
        let parent = relative.parent().unwrap_or(Path::new(""));
        let stem = output
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_string();
        let entry = module_tree.entry(parent.to_path_buf()).or_default();
        entry
            .files
            .push((stem, artifact.artifact.export.name.clone()));
        let mut current = parent.to_path_buf();
        while let Some(parent_dir) = current.parent() {
            if current.as_os_str().is_empty() {
                break;
            }
            let dir_name = current
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_string();
            module_tree
                .entry(parent_dir.to_path_buf())
                .or_default()
                .dirs
                .insert(dir_name);
            current = parent_dir.to_path_buf();
        }
    }

    for (dir, node) in module_tree {
        let content = render_mod_rs(&node);
        if dir.as_os_str().is_empty() {
            continue;
        }
        files.insert(PathBuf::from("src").join(dir).join("mod.rs"), content);
    }
    Ok(())
}

#[derive(Debug, Default)]
struct ModuleNode {
    files: Vec<(String, String)>,
    dirs: BTreeSet<String>,
}

fn render_mod_rs(node: &ModuleNode) -> String {
    let mut out = String::new();
    for dir in &node.dirs {
        out.push_str(&format!("mod {};\n", dir));
        out.push_str(&format!("pub use {}::*;\n", dir));
    }
    for (stem, export) in &node.files {
        out.push_str(&format!("mod {};\n", stem));
        out.push_str(&format!("pub use {}::{};\n", stem, export));
    }
    out
}

fn render_lib_rs(paths: &[&Path]) -> String {
    let mut sections = BTreeSet::new();
    for path in paths {
        if let Some(component) = path
            .strip_prefix("src")
            .ok()
            .and_then(|value| value.components().next())
            .and_then(|component| component.as_os_str().to_str())
        {
            sections.insert(component.to_string());
        }
    }
    let mut out = String::new();
    for section in &sections {
        out.push_str(&format!("pub mod {};\n", section));
    }
    if !sections.is_empty() {
        out.push('\n');
    }
    for section in &sections {
        out.push_str(&format!("pub use {}::*;\n", section));
    }
    out
}

fn render_cargo_toml(package_name: &str, dependencies: &BTreeMap<String, String>) -> String {
    let mut out = format!(
        "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[lib]\npath = \"src/lib.rs\"\n\n[dependencies]\n",
        package_name
    );
    for (name, version) in dependencies {
        out.push_str(&format!(
            "{name} = {}\n",
            render_dependency_version(version)
        ));
    }
    out
}

fn collect_required_dependencies(
    files: &BTreeMap<PathBuf, String>,
    dependency_manifest: &BTreeMap<String, String>,
    local_exports: &BTreeSet<String>,
    library_crate: &str,
) -> Result<BTreeMap<String, String>> {
    let qualified_path_re = Regex::new(r"\b([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)+)")
        .expect("qualified path regex");
    let ignored_roots = ["crate", "self", "super", "std", "core", "alloc"]
        .into_iter()
        .collect::<BTreeSet<_>>();
    let local_module_roots = collect_local_module_roots(files);

    let mut required = BTreeMap::new();
    let mut unknown = BTreeSet::new();

    for (path, content) in files {
        if path.extension().and_then(|value| value.to_str()) != Some("rs") {
            continue;
        }
        for captures in qualified_path_re.captures_iter(content) {
            let Some(path_match) = captures.get(1) else {
                continue;
            };
            let root = path_match.as_str().split("::").next().unwrap_or_default();
            if ignored_roots.contains(root)
                || root == library_crate
                || local_exports.contains(root)
                || local_module_roots.contains(root)
            {
                continue;
            }
            if let Some(version) = dependency_manifest.get(root) {
                required.insert(root.to_string(), version.clone());
            } else {
                unknown.insert(root.to_string());
            }
        }
    }

    if !unknown.is_empty() {
        bail!(
            "Generated sources reference external crate(s) not declared in drafts/dependencies.yml: {}",
            unknown.into_iter().collect::<Vec<_>>().join(", ")
        );
    }

    Ok(required)
}

fn collect_local_module_roots(files: &BTreeMap<PathBuf, String>) -> BTreeSet<String> {
    let mut roots = BTreeSet::new();

    for path in files.keys() {
        let Some(stripped) = path.strip_prefix("src").ok() else {
            continue;
        };
        for component in stripped.components() {
            let value = component.as_os_str().to_string_lossy();
            let stem = value.trim_end_matches(".rs");
            if stem.is_empty() || matches!(stem, "lib" | "main" | "mod") {
                continue;
            }
            roots.insert(stem.to_string());
        }
    }

    roots
}

fn render_dependency_version(version: &str) -> String {
    let trimmed = version.trim();
    if trimmed.starts_with('{') {
        trimmed.to_string()
    } else {
        format!("{trimmed:?}")
    }
}

fn generate_data_file(
    artifact: &PreparedArtifact,
    export_kinds: &BTreeMap<String, String>,
) -> Result<String> {
    let imports = render_imports(artifact, export_kinds, "crate");
    let explicit_method_names = artifact
        .functionalities
        .iter()
        .filter(|method| method.name != "new")
        .map(|method| method.name.as_str())
        .collect::<BTreeSet<_>>();
    let mut out = String::new();
    if !imports.is_empty() {
        out.push_str(&imports);
        out.push('\n');
    }
    let derives = if artifact.derives.is_empty() {
        "#[derive(Debug, Clone)]\n".to_string()
    } else {
        format!("#[derive({})]\n", artifact.derives.join(", "))
    };
    out.push_str(&derives);
    out.push_str(&render_doc_comment(&artifact_doc_lines(artifact), 0));
    if !artifact.variants.is_empty() {
        out.push_str(&format!("pub enum {} {{\n", artifact.export.name));
        for variant in &artifact.variants {
            out.push_str(&render_doc_comment(
                &item_doc_lines(&variant.meaning, &variant.notes),
                1,
            ));
            if variant.payload_types.is_empty() {
                out.push_str(&format!("    {},\n", variant.name));
            } else {
                out.push_str(&format!(
                    "    {}({}),\n",
                    variant.name,
                    variant.payload_types.join(", ")
                ));
            }
        }
        out.push_str("}\n");
        return Ok(out);
    }

    out.push_str(&format!("pub struct {} {{\n", artifact.export.name));
    for (idx, field) in artifact.fields.iter().enumerate() {
        let ty = field
            .type_status
            .rust()
            .ok_or_else(|| anyhow::anyhow!("missing field type for `{}`", field.name))?;
        out.push_str(&fixme_comment(
            &artifact.ambiguities,
            &format!("fields[{idx}].type"),
            1,
        ));
        out.push_str(&render_doc_comment(
            &item_doc_lines(&field.meaning, &field.notes),
            1,
        ));
        out.push_str(&format!("    {}: {},\n", field.name, ty));
    }
    out.push_str("}\n\n");
    out.push_str(&format!("impl {} {{\n", artifact.export.name));
    if artifact.constructor.is_some() {
        out.push_str(&render_constructor(artifact, 1)?);
    }
    for getter in artifact
        .getters
        .iter()
        .filter(|getter| !explicit_method_names.contains(getter.name.as_str()))
    {
        out.push_str(&render_getter(artifact, getter, 1)?);
    }
    for (idx, method) in artifact.functionalities.iter().enumerate() {
        if method.name == "new" {
            continue;
        }
        out.push_str(&render_method_with_fixme(
            method,
            None,
            1,
            &artifact.ambiguities,
            &format!("functionalities[{idx}]"),
        )?);
    }
    out.push_str("}\n");
    Ok(out)
}

fn generate_composite_file(
    artifact: &PreparedArtifact,
    export_kinds: &BTreeMap<String, String>,
) -> Result<String> {
    load_role_player_shapes(artifact);
    let result = generate_composite_file_inner(artifact, export_kinds);
    clear_role_player_shapes();
    result
}

fn generate_composite_file_inner(
    artifact: &PreparedArtifact,
    export_kinds: &BTreeMap<String, String>,
) -> Result<String> {
    let imports = render_imports(artifact, export_kinds, "crate");
    let mut out = String::new();
    if !imports.is_empty() {
        out.push_str(&imports);
        out.push('\n');
    }
    out.push_str(&render_doc_comment(&artifact_doc_lines(artifact), 0));
    out.push_str(&format!("pub struct {} {{\n", artifact.export.name));
    for (idx, role) in artifact.roles.iter().enumerate() {
        let ty = role
            .type_status
            .rust()
            .ok_or_else(|| anyhow::anyhow!("missing role type for `{}`", role.name))?;
        out.push_str(&fixme_comment(
            &artifact.ambiguities,
            &format!("roles[{idx}].type"),
            1,
        ));
        out.push_str(&render_doc_comment(&role_doc_lines(role), 1));
        out.push_str(&format!("    {}: {},\n", role.name, ty));
    }
    for (idx, prop) in artifact.props.iter().enumerate() {
        let ty = prop
            .type_status
            .rust()
            .ok_or_else(|| anyhow::anyhow!("missing prop type for `{}`", prop.name))?;
        out.push_str(&fixme_comment(
            &artifact.ambiguities,
            &format!("props[{idx}].type"),
            1,
        ));
        out.push_str(&render_doc_comment(
            &item_doc_lines(&prop.meaning, &prop.notes),
            1,
        ));
        out.push_str(&format!("    {}: {},\n", prop.name, ty));
    }
    out.push_str("}\n\n");
    out.push_str(&format!("impl {} {{\n", artifact.export.name));
    for (idx, method) in artifact.functionalities.iter().enumerate() {
        out.push_str(&render_method_with_fixme(
            method,
            None,
            1,
            &artifact.ambiguities,
            &format!("functionalities[{idx}]"),
        )?);
    }
    for (ridx, role) in artifact.roles.iter().enumerate() {
        for (midx, method) in role.methods.iter().enumerate() {
            out.push_str(&render_method_with_fixme(
                method,
                Some(&role.name),
                1,
                &artifact.ambiguities,
                &format!("roles[{ridx}].methods[{midx}]"),
            )?);
        }
    }
    out.push_str("}\n");
    Ok(out)
}

fn generate_app_file(
    artifact: &PreparedArtifact,
    export_kinds: &BTreeMap<String, String>,
    library_crate: &str,
) -> Result<String> {
    let imports = render_app_imports(artifact, export_kinds, library_crate);
    let mut out = String::new();
    if !imports.is_empty() {
        out.push_str(&imports);
        out.push('\n');
    }
    let main = artifact
        .functionalities
        .iter()
        .find(|method| method.name == "main")
        .ok_or_else(|| anyhow::anyhow!("app artifact is missing main functionality"))?;
    out.push_str("fn main() {\n");
    match main.body.as_ref() {
        Some(body) => out.push_str(&render_body(body, 1)?),
        None => out.push_str("    todo!(\"main\")\n"),
    }
    out.push_str("}\n");
    Ok(out)
}

fn render_constructor(artifact: &PreparedArtifact, indent: usize) -> Result<String> {
    let mut out = String::new();
    let params = artifact
        .fields
        .iter()
        .map(|field| {
            let ty = field
                .type_status
                .rust()
                .ok_or_else(|| anyhow::anyhow!("missing field type for `{}`", field.name))?;
            Ok(format!("{}: {}", field.name, ty))
        })
        .collect::<Result<Vec<_>>>()?;
    out.push_str(&format!(
        "{}pub fn new({}) -> Self {{\n",
        indent_str(indent),
        params.join(", ")
    ));
    out.push_str(&format!("{}Self {{\n", indent_str(indent + 1)));
    for field in &artifact.fields {
        out.push_str(&format!("{}{},\n", indent_str(indent + 2), field.name));
    }
    out.push_str(&format!("{}}}\n", indent_str(indent + 1)));
    out.push_str(&format!("{}}}\n", indent_str(indent)));
    Ok(out)
}

fn render_getter(
    artifact: &PreparedArtifact,
    getter: &GetterSpec,
    indent: usize,
) -> Result<String> {
    let field = artifact
        .fields
        .iter()
        .find(|field| field.name == getter.field)
        .ok_or_else(|| anyhow::anyhow!("unknown getter field `{}`", getter.field))?;
    let field_type = field
        .type_status
        .rust()
        .ok_or_else(|| anyhow::anyhow!("missing field type for `{}`", field.name))?;
    let return_type = match getter.mode.as_str() {
        "copy" => field_type.to_string(),
        _ => format!("&{}", field_type),
    };
    let body = match getter.mode.as_str() {
        "copy" => format!("self.{}", field.name),
        _ => format!("&self.{}", field.name),
    };
    let mut out = String::new();
    out.push_str(&render_doc_comment(
        &getter_doc_lines(field, getter),
        indent,
    ));
    out.push_str(&format!(
        "{}pub fn {}(&self) -> {} {{\n{}{}\n{}}}\n",
        indent_str(indent),
        getter.name,
        return_type,
        indent_str(indent + 1),
        body,
        indent_str(indent)
    ));
    Ok(out)
}

fn render_method_with_fixme(
    method: &MethodSpec,
    role_name: Option<&str>,
    indent: usize,
    ambiguities: &[Ambiguity],
    base_path: &str,
) -> Result<String> {
    let mut out = String::new();
    out.push_str(&fixme_comment(
        ambiguities,
        &format!("{base_path}.signature"),
        indent,
    ));
    out.push_str(&fixme_comment(
        ambiguities,
        &format!("{base_path}.returns"),
        indent,
    ));
    out.push_str(&render_doc_comment(
        &method_doc_lines(method, role_name),
        indent,
    ));
    let visibility = if role_name.is_some() { "" } else { "pub " };
    let rust_name = role_name
        .map(|role| format!("{}_{}", role, method.name))
        .unwrap_or_else(|| method.name.clone());
    let receiver = method.receiver.clone().unwrap_or_default();
    let mut params = Vec::new();
    if !receiver.is_empty() {
        params.push(receiver);
    }
    for (pidx, parameter) in method.parameters.iter().enumerate() {
        let param_path = format!("{base_path}.parameters[{pidx}].type");
        if ambiguities
            .iter()
            .any(|a| a.path == param_path && a.severity == "fixed")
        {
            out.push_str(&fixme_comment(ambiguities, &param_path, indent));
        }
        let ty = parameter
            .type_status
            .rust()
            .ok_or_else(|| anyhow::anyhow!("missing type for parameter `{}`", parameter.name))?;
        params.push(format!("{}: {}", parameter.name, ty));
    }
    let return_type = method
        .return_status
        .rust()
        .ok_or_else(|| anyhow::anyhow!("missing return type for method `{}`", method.name))?;
    out.push_str(&format!(
        "{}{}fn {}({}) -> {} {{\n",
        indent_str(indent),
        visibility,
        rust_name,
        params.join(", "),
        return_type
    ));
    match method.body.as_ref() {
        Some(body) => out.push_str(&render_body(body, indent + 1)?),
        None => out.push_str(&format!(
            "{}todo!(\"{}\")\n",
            indent_str(indent + 1),
            rust_name
        )),
    }
    out.push_str(&format!("{}}}\n", indent_str(indent)));
    Ok(out)
}

fn render_body(body: &Body, indent: usize) -> Result<String> {
    let mut out = String::new();
    for step in &body.steps {
        out.push_str(&render_statement(step, indent)?);
    }
    Ok(out)
}

fn render_statement(step: &Statement, indent: usize) -> Result<String> {
    Ok(match step {
        Statement::Let { name, expr } => {
            format!(
                "{}let {} = {};\n",
                indent_str(indent),
                name,
                render_expression(expr)?
            )
        }
        Statement::AssignLocal { name, expr } => {
            format!(
                "{}{} = {};\n",
                indent_str(indent),
                name,
                render_expression(expr)?
            )
        }
        Statement::Call { expr } => {
            format!("{}{};\n", indent_str(indent), render_expression(expr)?)
        }
        Statement::If {
            condition,
            then_steps,
            else_steps,
        } => {
            let mut out = format!(
                "{}if {} {{\n",
                indent_str(indent),
                render_expression(condition)?
            );
            for step in then_steps {
                out.push_str(&render_statement(step, indent + 1)?);
            }
            if else_steps.is_empty() {
                out.push_str(&format!("{}}}\n", indent_str(indent)));
            } else {
                out.push_str(&format!("{}}} else {{\n", indent_str(indent)));
                for step in else_steps {
                    out.push_str(&render_statement(step, indent + 1)?);
                }
                out.push_str(&format!("{}}}\n", indent_str(indent)));
            }
            out
        }
        Statement::Match { expr, arms } => {
            let mut out = format!(
                "{}match {} {{\n",
                indent_str(indent),
                render_expression(expr)?
            );
            for arm in arms {
                out.push_str(&format!(
                    "{}{} => {{\n",
                    indent_str(indent + 1),
                    arm.pattern
                ));
                for step in &arm.steps {
                    out.push_str(&render_statement(step, indent + 2)?);
                }
                out.push_str(&format!("{}}},\n", indent_str(indent + 1)));
            }
            out.push_str(&format!("{}}}\n", indent_str(indent)));
            out
        }
        Statement::ForEach {
            binding,
            collection,
            body,
        } => {
            let mut out = format!(
                "{}for {} in {} {{\n",
                indent_str(indent),
                binding,
                render_expression(collection)?
            );
            for step in body {
                out.push_str(&render_statement(step, indent + 1)?);
            }
            out.push_str(&format!("{}}}\n", indent_str(indent)));
            out
        }
        Statement::Return { expr } => match expr {
            Some(expr) => format!(
                "{}return {};\n",
                indent_str(indent),
                render_expression(expr)?
            ),
            None => format!("{}return;\n", indent_str(indent)),
        },
        Statement::SleepMs { expr } => format!(
            "{}std::thread::sleep(std::time::Duration::from_millis(({}) as u64));\n",
            indent_str(indent),
            render_expression(expr)?
        ),
        Statement::ReadUtcNowMs { name } => format!(
            "{}let {} = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64;\n",
            indent_str(indent),
            name
        ),
    })
}

fn render_expression(expr: &Expression) -> Result<String> {
    Ok(match expr {
        Expression::Literal { kind, value } => match kind.as_str() {
            "string" => format!("{value:?}.to_string()"),
            "integer" | "bool" | "path" => value.clone(),
            "char" => format!("'{}'", value),
            other => bail!("Unsupported literal kind `{other}`"),
        },
        Expression::Var { name } => name.clone(),
        Expression::Field { base, name } => {
            if base == "self" {
                format!("self.{name}")
            } else {
                format!("{base}.{name}")
            }
        }
        Expression::ConstructStruct { type_name, fields } => {
            let rendered = fields
                .iter()
                .map(|field| {
                    Ok(format!(
                        "{}: {}",
                        field.name,
                        render_expression(&field.expr)?
                    ))
                })
                .collect::<Result<Vec<_>>>()?
                .join(", ");
            format!("{type_name} {{ {rendered} }}")
        }
        Expression::ConstructEnum { type_name, variant } => format!("{type_name}::{variant}"),
        Expression::CallRoleMethod { role, method, args } => {
            // Role methods take the role player as their first argument. Its call-site shape
            // (`&`, `&mut`, owned) is determined by the first parameter of the prepared role
            // method signature, cached in ROLE_PLAYER_SHAPES for this file.
            let domain_args = render_call_args(args)?;
            let first_arg = match lookup_role_player_shape(role, method) {
                RolePlayerCallShape::ImmutableRef => format!("&self.{role}"),
                RolePlayerCallShape::MutableRef => format!("&mut self.{role}"),
                RolePlayerCallShape::Owned => format!("self.{role}.clone()"),
            };
            let all_args = if domain_args.is_empty() {
                first_arg
            } else {
                format!("{first_arg}, {domain_args}")
            };
            format!("self.{role}_{method}({all_args})")
        }
        Expression::CallLocalMethod { name, args } => {
            if name.ends_with('!') {
                format!("{name}({})", render_macro_args(args)?)
            } else {
                format!("{name}({})", render_call_args(args)?)
            }
        }
        Expression::CallInstanceMethod {
            receiver,
            method,
            args,
        } => format!(
            "{}.{}({})",
            render_expression(receiver)?,
            method,
            render_call_args(args)?
        ),
        Expression::BinaryOp {
            operator,
            left,
            right,
        } => format!(
            "({} {} {})",
            render_expression(left)?,
            operator,
            render_expression(right)?
        ),
        Expression::UnaryOp { operator, expr } => {
            format!("({}{})", operator, render_expression(expr)?)
        }
        Expression::CollectionLiteral {
            kind,
            items,
            entries,
        } => match kind.as_str() {
            "vec" => format!(
                "vec![{}]",
                items
                    .iter()
                    .map(render_expression)
                    .collect::<Result<Vec<_>>>()?
                    .join(", ")
            ),
            "hash_map" => {
                let rendered = render_entries(entries)?;
                format!("std::collections::HashMap::from([{rendered}])")
            }
            other => bail!("Unsupported collection literal kind `{other}`"),
        },
    })
}

fn render_call_args(args: &[Expression]) -> Result<String> {
    args.iter()
        .map(render_expression)
        .collect::<Result<Vec<_>>>()
        .map(|items| items.join(", "))
}

fn render_macro_args(args: &[Expression]) -> Result<String> {
    args.iter()
        .map(|expr| match expr {
            Expression::Literal { kind, value } if kind == "string" => Ok(format!("{value:?}")),
            _ => render_expression(expr),
        })
        .collect::<Result<Vec<_>>>()
        .map(|items| items.join(", "))
}

fn render_entries(entries: &[CollectionEntry]) -> Result<String> {
    entries
        .iter()
        .map(|entry| {
            Ok(format!(
                "({}, {})",
                render_expression(&entry.key)?,
                render_expression(&entry.value)?
            ))
        })
        .collect::<Result<Vec<_>>>()
        .map(|items| items.join(", "))
}

fn render_imports(
    artifact: &PreparedArtifact,
    export_kinds: &BTreeMap<String, String>,
    root: &str,
) -> String {
    let mut imports = artifact
        .referenced_type_names()
        .into_iter()
        .filter(|name| export_kinds.contains_key(name) && *name != artifact.export.name)
        .collect::<Vec<_>>();
    imports.sort();
    if imports.is_empty() {
        return String::new();
    }
    format!("use {}::{{{}}};\n", root, imports.join(", "))
}

/// Imports for the generated `src/main.rs`.
///
/// The `app` artifact is the top-level orchestrator: its flow prose cannot enumerate every type
/// that its implementation will transitively reference (e.g. a return type of a collaborator
/// method such as `game_loop_context.tick() -> PlayerState`). To give the implementation agent
/// unambiguous access to the domain — and to avoid `E0433: use of undeclared type` errors after
/// the body is filled in — we import every library-crate export from the generated library.
fn render_app_imports(
    artifact: &PreparedArtifact,
    export_kinds: &BTreeMap<String, String>,
    library_crate: &str,
) -> String {
    let mut imports = export_kinds
        .iter()
        .filter(|(name, kind)| {
            kind.as_str() != "app" && name.as_str() != artifact.export.name.as_str()
        })
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    imports.sort();
    if imports.is_empty() {
        return String::new();
    }
    format!(
        "#[allow(unused_imports)]\nuse {}::{{{}}};\n",
        library_crate,
        imports.join(", ")
    )
}

fn package_name(root: &Path) -> String {
    let base = root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("generated-app");
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in base.chars() {
        let normalized = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            '-'
        };
        if normalized == '-' {
            if !prev_dash && !out.is_empty() {
                out.push('-');
            }
            prev_dash = true;
            continue;
        }
        prev_dash = false;
        out.push(normalized);
    }
    if out.is_empty() {
        "generated-app".to_string()
    } else {
        out.trim_matches('-').to_string()
    }
}

fn sanitize_module_name(value: &str) -> String {
    let mut out = String::new();
    let mut prev_underscore = false;
    for ch in value.chars() {
        let normalized = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            '_'
        };
        if normalized == '_' {
            if !prev_underscore && !out.is_empty() {
                out.push('_');
            }
            prev_underscore = true;
            continue;
        }
        prev_underscore = false;
        out.push(normalized);
    }
    out.trim_matches('_').to_string()
}

fn write_generated_manifest<'a>(
    workspace: &Workspace,
    files: impl Iterator<Item = &'a PathBuf>,
) -> Result<()> {
    let manifest = GeneratedFilesManifest {
        files: files
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .collect(),
    };
    let path = workspace.root.join(GENERATED_MANIFEST);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(&manifest)?;
    fs::write(&path, content).with_context(|| format!("Failed to write {}", path.display()))
}

fn indent_str(depth: usize) -> String {
    "    ".repeat(depth)
}

fn render_doc_comment(lines: &[String], indent: usize) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for line in lines {
        out.push_str(&format!("{}/// {}\n", indent_str(indent), line));
    }
    out
}

fn artifact_doc_lines(artifact: &PreparedArtifact) -> Vec<String> {
    vec![format!(
        "Generated from {} specification `{}`.",
        artifact.source.kind, artifact.source.path
    )]
}

fn item_doc_lines(summary: &str, notes: &[String]) -> Vec<String> {
    let mut lines = Vec::new();
    push_doc_text(&mut lines, summary);
    for note in notes {
        lines.push(format!("Note: {}", normalize_doc_text(note)));
    }
    lines
}

fn role_doc_lines(role: &crate::prepared::RoleSpec) -> Vec<String> {
    let mut lines = Vec::new();
    push_doc_text(&mut lines, &role.purpose);
    if !role.expected_behavior.trim().is_empty() {
        lines.push(format!(
            "Expected behavior: {}",
            normalize_doc_text(&role.expected_behavior)
        ));
    }
    lines
}

fn getter_doc_lines(field: &FieldSpec, getter: &GetterSpec) -> Vec<String> {
    let mut lines = Vec::new();
    if !field.meaning.trim().is_empty() {
        lines.push(format!("Returns {}.", normalize_doc_text(&field.meaning)));
    }
    if getter.mode == "copy" {
        lines.push("Getter returns the stored value by copy.".to_string());
    } else {
        lines.push("Getter returns a shared reference to the stored value.".to_string());
    }
    lines
}

fn method_doc_lines(method: &MethodSpec, role_name: Option<&str>) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(role_name) = role_name {
        lines.push(format!("Role method for `{role_name}`."));
    }
    if !method.flow.is_empty() {
        lines.push("Flow:".to_string());
        for step in &method.flow {
            lines.push(format!("- {}", normalize_doc_text(step)));
        }
    }
    if !method.extensions.is_empty() {
        lines.push("Extensions:".to_string());
        for step in &method.extensions {
            lines.push(format!("- {}", normalize_doc_text(step)));
        }
    }
    if !method.guarantee.is_empty() {
        lines.push("Guarantee:".to_string());
        for line in &method.guarantee {
            lines.push(format!("- {}", normalize_doc_text(line)));
        }
    }
    if let Some(references) = method.references.as_ref() {
        let mut parts = Vec::new();
        if !references.roles.is_empty() {
            parts.push(format!("roles={}", references.roles.join(", ")));
        }
        if !references.props.is_empty() {
            parts.push(format!("props={}", references.props.join(", ")));
        }
        if !references.types.is_empty() {
            parts.push(format!("types={}", references.types.join(", ")));
        }
        if !references.role_methods.is_empty() {
            parts.push(format!(
                "role_methods={}",
                references.role_methods.join(", ")
            ));
        }
        if !parts.is_empty() {
            lines.push(format!("References: {}.", parts.join("; ")));
        }
    }
    lines
}

fn push_doc_text(lines: &mut Vec<String>, text: &str) {
    let normalized = normalize_doc_text(text);
    if !normalized.is_empty() {
        lines.push(normalized);
    }
}

fn normalize_doc_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_builtin_type(name: &str) -> bool {
    matches!(
        name,
        "String"
            | "Vec"
            | "Option"
            | "Result"
            | "HashMap"
            | "BTreeMap"
            | "HashSet"
            | "BTreeSet"
            | "VecDeque"
            | "Box"
            | "Rc"
            | "Arc"
            | "Cow"
            | "Pin"
    )
}

fn fixme_comment(ambiguities: &[Ambiguity], path: &str, indent: usize) -> String {
    for amb in ambiguities {
        if amb.path == path && amb.severity == "fixed" {
            return format!("{}// FIXME(agent): {}\n", indent_str(indent), amb.message);
        }
    }
    String::new()
}

fn prune_empty_dirs(path: &Path, root: &Path) -> Result<()> {
    if path == root {
        return Ok(());
    }
    if path.read_dir()?.next().is_none() {
        fs::remove_dir(path).with_context(|| format!("Failed to remove {}", path.display()))?;
        if let Some(parent) = path.parent() {
            prune_empty_dirs(parent, root)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prepared::{
        Body, ExportInfo, Expression, FieldSpec, MethodReferences, MethodSpec, SourceInfo,
        Statement, ValueStatus, VariantSpec,
    };

    #[test]
    fn collect_required_dependencies_uses_manifest_and_ignores_local_roots() {
        let mut files = BTreeMap::new();
        files.insert(
            PathBuf::from("src/main.rs"),
            r#"
use demo_app::{Board, CommandInputContext};

fn main() {
    let _: Option<crossterm::event::KeyEvent> = None;
    let _ = chrono::Utc::now();
    let _ = Board::new();
}
"#
            .to_string(),
        );

        let manifest = BTreeMap::from([
            (
                "chrono".to_string(),
                r#"{ version = "0.4", features = ["serde"] }"#.to_string(),
            ),
            ("crossterm".to_string(), "0.27".to_string()),
        ]);
        let local_exports =
            BTreeSet::from(["Board".to_string(), "CommandInputContext".to_string()]);

        let dependencies =
            collect_required_dependencies(&files, &manifest, &local_exports, "demo_app").unwrap();

        assert_eq!(
            dependencies.get("crossterm").map(String::as_str),
            Some("0.27")
        );
        assert_eq!(
            dependencies.get("chrono").map(String::as_str),
            Some(r#"{ version = "0.4", features = ["serde"] }"#)
        );
        assert_eq!(dependencies.len(), 2);
    }

    #[test]
    fn collect_required_dependencies_errors_for_unknown_external_crate() {
        let files = BTreeMap::from([(
            PathBuf::from("src/lib.rs"),
            "pub fn now() -> foo::Bar { todo!() }\n".to_string(),
        )]);

        let error =
            collect_required_dependencies(&files, &BTreeMap::new(), &BTreeSet::new(), "demo_app")
                .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("external crate(s) not declared in drafts/dependencies.yml: foo")
        );
    }

    #[test]
    fn render_cargo_toml_formats_string_and_inline_table_versions() {
        let dependencies = BTreeMap::from([
            (
                "chrono".to_string(),
                r#"{ version = "0.4", features = ["serde"] }"#.to_string(),
            ),
            ("crossterm".to_string(), "0.27".to_string()),
        ]);

        let rendered = render_cargo_toml("demo-app", &dependencies);

        assert!(rendered.contains(r#"chrono = { version = "0.4", features = ["serde"] }"#));
        assert!(rendered.contains(r#"crossterm = "0.27""#));
    }

    #[test]
    fn generate_data_file_renders_tuple_variants() {
        let mut artifact = PreparedArtifact::empty(
            "data",
            "data/UserAction.md".to_string(),
            "UserAction".to_string(),
            "UserAction".to_string(),
            false,
        );
        artifact.source = SourceInfo {
            path: "data/UserAction.md".to_string(),
            kind: "data".to_string(),
            title: "UserAction".to_string(),
        };
        artifact.export = ExportInfo {
            name: "UserAction".to_string(),
        };
        artifact.variants = vec![
            VariantSpec {
                name: "Movement".to_string(),
                payload_types: vec!["Direction".to_string()],
                meaning: "move".to_string(),
                notes: Vec::new(),
            },
            VariantSpec {
                name: "Fire".to_string(),
                payload_types: Vec::new(),
                meaning: "fire".to_string(),
                notes: Vec::new(),
            },
        ];

        let rendered = generate_data_file(&artifact, &BTreeMap::new()).unwrap();

        assert!(rendered.contains("Movement(Direction)"));
        assert!(rendered.contains("Fire,"));
    }

    #[test]
    fn generate_data_file_skips_synthesized_getter_when_functionality_has_same_name() {
        let mut artifact = PreparedArtifact::empty(
            "data",
            "data/GameState.md".to_string(),
            "GameState".to_string(),
            "GameState".to_string(),
            false,
        );
        artifact.fields = vec![FieldSpec {
            name: "game_started".to_string(),
            meaning: "start timestamp".to_string(),
            type_status: ValueStatus::resolved("u64", "test"),
            getter_accessible: true,
            notes: Vec::new(),
        }];
        artifact.getters = vec![GetterSpec {
            name: "game_started".to_string(),
            field: "game_started".to_string(),
            mode: "copy".to_string(),
        }];
        artifact.functionalities = vec![MethodSpec {
            name: "game_started".to_string(),
            signature: ValueStatus::resolved("game_started(&self) -> u64", "test"),
            receiver: Some("&self".to_string()),
            parameters: Vec::new(),
            return_status: ValueStatus::resolved("u64", "test"),
            flow: Vec::new(),
            extensions: Vec::new(),
            guarantee: Vec::new(),
            references: None,
            body: Some(Body {
                steps: vec![Statement::Return {
                    expr: Some(Expression::Field {
                        base: "self".to_string(),
                        name: "game_started".to_string(),
                    }),
                }],
            }),
        }];

        let rendered = generate_data_file(&artifact, &BTreeMap::new()).unwrap();

        assert_eq!(
            rendered.matches("pub fn game_started").count(),
            1,
            "{rendered}"
        );
    }

    #[test]
    fn generate_composite_file_emits_spec_doc_comments_for_methods_and_roles() {
        let mut artifact = PreparedArtifact::empty(
            "context",
            "contexts/GameLoop.md".to_string(),
            "GameLoop".to_string(),
            "GameLoopContext".to_string(),
            true,
        );
        artifact.roles = vec![crate::prepared::RoleSpec {
            name: "command".to_string(),
            purpose: "Provides player input.".to_string(),
            expected_behavior: "Translates keys into user actions.".to_string(),
            type_status: ValueStatus::resolved("CommandInputContext", "test"),
            methods: Vec::new(),
        }];
        artifact.functionalities = vec![MethodSpec {
            name: "tick".to_string(),
            signature: ValueStatus::resolved("tick(&mut self) -> PlayerState", "test"),
            receiver: Some("&mut self".to_string()),
            parameters: Vec::new(),
            return_status: ValueStatus::resolved("PlayerState", "test"),
            flow: vec![
                "Read the next action from command.".to_string(),
                "Advance the snake.".to_string(),
            ],
            extensions: vec!["No action -> keep current direction.".to_string()],
            guarantee: vec!["Returns the updated player state.".to_string()],
            references: Some(MethodReferences {
                roles: vec!["command".to_string()],
                props: Vec::new(),
                types: vec!["PlayerState".to_string()],
                role_methods: vec!["command_next_action".to_string()],
            }),
            body: Some(Body {
                steps: vec![Statement::Return {
                    expr: Some(Expression::Var {
                        name: "PlayerState::Alive".to_string(),
                    }),
                }],
            }),
        }];

        let rendered = generate_composite_file(&artifact, &BTreeMap::new()).unwrap();

        assert!(rendered.contains("/// Provides player input."));
        assert!(rendered.contains("/// Expected behavior: Translates keys into user actions."));
        assert!(rendered.contains("/// Flow:"));
        assert!(rendered.contains("/// - Read the next action from command."));
        assert!(rendered.contains(
            "/// References: roles=command; types=PlayerState; role_methods=command_next_action."
        ));
    }
}
