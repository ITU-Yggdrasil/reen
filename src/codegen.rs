use crate::build_tracker::{BuildTracker, hash_string};
use crate::compile_repair::{
    COMPILE_FIX_MAX_ROUNDS, apply_compile_fix, parse_compile_errors, run_cargo_build,
};
use crate::prepared::{
    Ambiguity, Body, CollectionEntry, Expression, GetterSpec, MethodSpec, PreparedArtifact,
    Statement,
};
use crate::workspace::{GENERATED_MANIFEST, Workspace};
use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
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

pub fn scaffold_workspace(workspace: &Workspace, options: &ScaffoldOptions) -> Result<()> {
    let prepared_paths = workspace.prepared_paths(&options.selection)?;
    let mut tracker = BuildTracker::load(&workspace.root)?;
    let loaded = load_prepared_artifacts(workspace, &prepared_paths)?;
    validate_loaded_artifacts(&loaded)?;
    let dependency_manifest = load_dependency_manifest(workspace)?;
    let aggregate_hash = combined_hash(&loaded)?;
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
    for round in 1..=max_rounds {
        let fixes = parse_compile_errors(&last_stderr);
        if fixes.is_empty() {
            eprint!("{}", last_stderr);
            bail!("Generated project failed to compile but no auto-fixable errors were found");
        }
        if options.verbose {
            eprintln!(
                "scaffold --fix round {round}: applying {} fix(es)",
                fixes.len()
            );
        }
        for fix in &fixes {
            apply_compile_fix(workspace, fix)?;
            if options.verbose {
                eprintln!("  {}", fix.description());
            }
        }
        let result = run_cargo_build(workspace)?;
        if result.success {
            tracker.update("scaffold", "scaffold:workspace", aggregate_hash);
            tracker.save(&workspace.root)?;
            return Ok(());
        }
        last_stderr = result.stderr;
    }
    eprint!("{}", last_stderr);
    bail!("Generated project still fails to compile after {max_rounds} fix rounds");
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
    if !artifact.variants.is_empty() {
        out.push_str(&format!("pub enum {} {{\n", artifact.export.name));
        for variant in &artifact.variants {
            out.push_str(&format!("    {},\n", variant.name));
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
        out.push_str(&format!("    {}: {},\n", field.name, ty));
    }
    out.push_str("}\n\n");
    out.push_str(&format!("impl {} {{\n", artifact.export.name));
    if artifact.constructor.is_some() {
        out.push_str(&render_constructor(artifact, 1)?);
    }
    for getter in &artifact.getters {
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
    let imports = render_imports(artifact, export_kinds, "crate");
    let mut out = String::new();
    if !imports.is_empty() {
        out.push_str(&imports);
        out.push('\n');
    }
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
    let imports = render_imports(artifact, export_kinds, library_crate);
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
    Ok(format!(
        "{}pub fn {}(&self) -> {} {{\n{}{}\n{}}}\n",
        indent_str(indent),
        getter.name,
        return_type,
        indent_str(indent + 1),
        body,
        indent_str(indent)
    ))
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
            // Role methods take `&self.<role>` as their first argument (the role player).
            // Remaining args follow separated by commas.
            let domain_args = render_call_args(args)?;
            let all_args = if domain_args.is_empty() {
                format!("&self.{role}")
            } else {
                format!("&self.{role}, {domain_args}")
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
}
