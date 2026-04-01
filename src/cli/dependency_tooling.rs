use anyhow::{Context, Result, bail};
use chrono::Utc;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const DEPENDENCY_SCHEMA: &str = "reen.dependencies/v1";
const DEPENDENCY_MANIFEST_NAME: &str = "dependencies.yml";
const TOOLING_DIR: &str = ".reen/tooling";
const TOOLING_MANIFEST_NAME: &str = "Cargo.toml";
const TOOLING_STUB_LIB: &str = "src/lib.rs";
const SYMBOLS_FILE_NAME: &str = "rust-symbols.json";

#[derive(Clone, Debug, Default, Deserialize)]
pub struct DependencyManifest {
    #[serde(default)]
    pub schema: Option<String>,
    #[serde(default)]
    pub packages: Vec<DependencyPackage>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DependencyPackage {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolingPaths {
    pub dependency_manifest_path: PathBuf,
    pub tooling_root: PathBuf,
    pub cargo_toml_path: PathBuf,
    pub stub_lib_path: PathBuf,
    pub symbols_path: PathBuf,
}

#[derive(Clone, Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoMetadataPackage>,
}

#[derive(Clone, Debug, Deserialize)]
struct CargoMetadataPackage {
    name: String,
    version: String,
    #[serde(default)]
    targets: Vec<CargoMetadataTarget>,
}

#[derive(Clone, Debug, Deserialize)]
struct CargoMetadataTarget {
    #[serde(default)]
    kind: Vec<String>,
    src_path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RustSymbolsInventory {
    pub generated_at: String,
    pub manifest_path: String,
    pub source_dependencies_path: String,
    pub packages: Vec<RustDependencySymbols>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RustDependencySymbols {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub symbols: Vec<RustSymbol>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RustSymbol {
    pub kind: String,
    pub name: String,
    pub module: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

pub fn dependency_manifest_path(drafts_root: &Path) -> PathBuf {
    drafts_root.join(DEPENDENCY_MANIFEST_NAME)
}

pub fn tooling_paths(drafts_root: &Path, artifact_workspace_root: &Path) -> ToolingPaths {
    let tooling_root = artifact_workspace_root.join(TOOLING_DIR);
    ToolingPaths {
        dependency_manifest_path: dependency_manifest_path(drafts_root),
        cargo_toml_path: tooling_root.join(TOOLING_MANIFEST_NAME),
        stub_lib_path: tooling_root.join(TOOLING_STUB_LIB),
        symbols_path: tooling_root.join(SYMBOLS_FILE_NAME),
        tooling_root,
    }
}

pub fn load_dependency_manifest(path: &Path) -> Result<Option<DependencyManifest>> {
    if !path.exists() {
        return Ok(None);
    }

    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let mut manifest: DependencyManifest = serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    validate_dependency_manifest(&mut manifest, path)?;
    Ok(Some(manifest))
}

pub fn merge_manifest_dependencies(
    dependencies: &mut HashMap<String, String>,
    manifest: &DependencyManifest,
) {
    for package in &manifest.packages {
        dependencies.insert(package.name.clone(), package.version.clone());
    }
}

pub fn render_dependency_entries(dependencies: &BTreeMap<String, String>) -> String {
    let mut content = String::new();
    for (name, version) in dependencies {
        if version.trim_start().starts_with('{') {
            content.push_str(&format!("{name} = {version}\n"));
        } else {
            content.push_str(&format!("{name} = \"{version}\"\n"));
        }
    }
    content
}

pub fn ensure_tooling_artifacts_fresh(
    drafts_root: &Path,
    artifact_workspace_root: &Path,
    verbose: bool,
) -> Result<()> {
    ensure_tooling_artifacts_fresh_with_runner(
        drafts_root,
        artifact_workspace_root,
        verbose,
        run_cargo_metadata,
    )
}

pub fn load_symbols_context(primary_root: &Path) -> Result<Option<Value>> {
    let artifact_workspace_root = artifact_workspace_root_from_primary_root(primary_root);
    let symbols_path = artifact_workspace_root
        .join(TOOLING_DIR)
        .join(SYMBOLS_FILE_NAME);
    if !symbols_path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&symbols_path)
        .with_context(|| format!("Failed to read {}", symbols_path.display()))?;
    let value = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse {}", symbols_path.display()))?;
    Ok(Some(value))
}

pub(crate) fn ensure_tooling_artifacts_fresh_with_runner<F>(
    drafts_root: &Path,
    artifact_workspace_root: &Path,
    verbose: bool,
    mut cargo_metadata_runner: F,
) -> Result<()>
where
    F: FnMut(&Path) -> Result<String>,
{
    let paths = tooling_paths(drafts_root, artifact_workspace_root);
    let Some(manifest) = load_dependency_manifest(&paths.dependency_manifest_path)? else {
        return Ok(());
    };

    if !tooling_needs_refresh(&paths)? {
        if verbose {
            println!(
                "Dependency tooling artifacts are up to date: {}",
                paths.symbols_path.display()
            );
        }
        return Ok(());
    }

    if verbose {
        println!(
            "Refreshing dependency tooling artifacts from {}",
            paths.dependency_manifest_path.display()
        );
    }

    write_tooling_manifest(&paths, &manifest)?;
    let metadata_json = cargo_metadata_runner(&paths.cargo_toml_path)?;
    let inventory = build_symbols_inventory(&paths, &manifest, &metadata_json)?;
    if let Some(parent) = paths.symbols_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    fs::write(
        &paths.symbols_path,
        serde_json::to_string_pretty(&inventory)
            .context("Failed to serialize symbols inventory")?,
    )
    .with_context(|| format!("Failed to write {}", paths.symbols_path.display()))?;

    Ok(())
}

pub(crate) fn tooling_needs_refresh(paths: &ToolingPaths) -> Result<bool> {
    if !paths.dependency_manifest_path.exists() {
        return Ok(false);
    }
    if !paths.cargo_toml_path.exists() || !paths.symbols_path.exists() {
        return Ok(true);
    }

    let dependency_modified = fs::metadata(&paths.dependency_manifest_path)
        .with_context(|| {
            format!(
                "Failed to inspect {}",
                paths.dependency_manifest_path.display()
            )
        })?
        .modified()
        .with_context(|| {
            format!(
                "Failed to read mtime for {}",
                paths.dependency_manifest_path.display()
            )
        })?;
    let cargo_modified = fs::metadata(&paths.cargo_toml_path)
        .with_context(|| format!("Failed to inspect {}", paths.cargo_toml_path.display()))?
        .modified()
        .with_context(|| {
            format!(
                "Failed to read mtime for {}",
                paths.cargo_toml_path.display()
            )
        })?;
    let symbols_modified = fs::metadata(&paths.symbols_path)
        .with_context(|| format!("Failed to inspect {}", paths.symbols_path.display()))?
        .modified()
        .with_context(|| format!("Failed to read mtime for {}", paths.symbols_path.display()))?;

    Ok(cargo_modified < dependency_modified || symbols_modified < dependency_modified)
}

fn validate_dependency_manifest(manifest: &mut DependencyManifest, path: &Path) -> Result<()> {
    if let Some(schema) = manifest.schema.as_deref() {
        if schema != DEPENDENCY_SCHEMA {
            bail!(
                "Unsupported dependency manifest schema '{}' in {}",
                schema,
                path.display()
            );
        }
    }

    let mut seen = BTreeSet::new();
    for package in &mut manifest.packages {
        package.name = package.name.trim().to_string();
        package.version = package.version.trim().to_string();
        package.capabilities = package
            .capabilities
            .iter()
            .map(|capability| capability.trim().to_string())
            .filter(|capability| !capability.is_empty())
            .collect();
        package.capabilities.sort();
        package.capabilities.dedup();

        if package.name.is_empty() {
            bail!(
                "Dependency package name cannot be empty in {}",
                path.display()
            );
        }
        if package.version.is_empty() {
            bail!(
                "Dependency package '{}' is missing a version in {}",
                package.name,
                path.display()
            );
        }

        let canonical = canonicalize_name(&package.name);
        if !seen.insert(canonical) {
            bail!(
                "Duplicate dependency package '{}' in {}",
                package.name,
                path.display()
            );
        }
    }

    Ok(())
}

fn write_tooling_manifest(paths: &ToolingPaths, manifest: &DependencyManifest) -> Result<()> {
    fs::create_dir_all(&paths.tooling_root)
        .with_context(|| format!("Failed to create {}", paths.tooling_root.display()))?;
    if let Some(parent) = paths.stub_lib_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    let dependency_map = manifest
        .packages
        .iter()
        .map(|package| (package.name.clone(), package.version.clone()))
        .collect::<BTreeMap<_, _>>();

    let mut content = String::new();
    content.push_str(
        "[package]\n\
name = \"reen_tooling_symbols\"\n\
version = \"0.1.0\"\n\
edition = \"2024\"\n\
publish = false\n\
\n\
[lib]\n\
path = \"src/lib.rs\"\n\
\n\
[dependencies]\n",
    );
    content.push_str(&render_dependency_entries(&dependency_map));

    fs::write(&paths.cargo_toml_path, content)
        .with_context(|| format!("Failed to write {}", paths.cargo_toml_path.display()))?;
    fs::write(
        &paths.stub_lib_path,
        "// Auto-generated by reen for dependency symbol extraction.\n",
    )
    .with_context(|| format!("Failed to write {}", paths.stub_lib_path.display()))?;

    Ok(())
}

fn run_cargo_metadata(manifest_path: &Path) -> Result<String> {
    let output = Command::new("cargo")
        .arg("metadata")
        .arg("--format-version")
        .arg("1")
        .arg("--manifest-path")
        .arg(manifest_path)
        .output()
        .with_context(|| {
            format!(
                "Failed to execute cargo metadata for {}",
                manifest_path.display()
            )
        })?;

    if !output.status.success() {
        bail!(
            "cargo metadata failed for {}: {}",
            manifest_path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    String::from_utf8(output.stdout).context("cargo metadata output was not valid UTF-8")
}

fn build_symbols_inventory(
    paths: &ToolingPaths,
    manifest: &DependencyManifest,
    metadata_json: &str,
) -> Result<RustSymbolsInventory> {
    let metadata: CargoMetadata =
        serde_json::from_str(metadata_json).context("Failed to parse cargo metadata JSON")?;
    let packages_by_canonical = metadata
        .packages
        .iter()
        .map(|package| (canonicalize_name(&package.name), package))
        .collect::<HashMap<_, _>>();

    let mut packages = Vec::new();
    for dependency in &manifest.packages {
        let canonical = canonicalize_name(&dependency.name);
        let metadata_package = packages_by_canonical
            .get(&canonical)
            .copied()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Dependency '{}' was not resolved by cargo metadata from {}",
                    dependency.name,
                    paths.cargo_toml_path.display()
                )
            })?;
        packages.push(RustDependencySymbols {
            name: metadata_package.name.clone(),
            version: metadata_package.version.clone(),
            capabilities: dependency.capabilities.clone(),
            symbols: extract_public_symbols(metadata_package)?,
        });
    }

    packages.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(RustSymbolsInventory {
        generated_at: Utc::now().to_rfc3339(),
        manifest_path: paths.cargo_toml_path.to_string_lossy().into_owned(),
        source_dependencies_path: paths
            .dependency_manifest_path
            .to_string_lossy()
            .into_owned(),
        packages,
    })
}

fn extract_public_symbols(package: &CargoMetadataPackage) -> Result<Vec<RustSymbol>> {
    let mut symbols = Vec::new();
    let mut seen = BTreeSet::new();
    let mut scanned_roots = BTreeSet::new();

    for target in &package.targets {
        if !target
            .kind
            .iter()
            .any(|kind| kind == "lib" || kind == "proc-macro")
        {
            continue;
        }

        let src_path = PathBuf::from(&target.src_path);
        let Some(root_dir) = src_path.parent() else {
            continue;
        };
        if !scanned_roots.insert(root_dir.to_path_buf()) {
            continue;
        }

        let mut files = Vec::new();
        collect_rust_files(root_dir, &mut files)?;
        files.sort();

        for file in files {
            let content = fs::read_to_string(&file)
                .with_context(|| format!("Failed to read {}", file.display()))?;
            let module = module_path_for_file(root_dir, &file);
            collect_symbols_from_content(&content, &module, &mut symbols, &mut seen);
        }
    }

    symbols.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then_with(|| a.module.cmp(&b.module))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(symbols)
}

fn collect_rust_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("Failed to read {}", dir.display()))? {
        let path = entry
            .with_context(|| format!("Failed to inspect {}", dir.display()))?
            .path();
        if path.is_dir() {
            collect_rust_files(&path, files)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            files.push(path);
        }
    }
    Ok(())
}

fn collect_symbols_from_content(
    content: &str,
    module: &str,
    output: &mut Vec<RustSymbol>,
    seen: &mut BTreeSet<String>,
) {
    let patterns = [
        (
            "struct",
            Regex::new(r"(?m)^\s*pub\s+struct\s+([A-Za-z_][A-Za-z0-9_]*)").expect("struct regex"),
        ),
        (
            "enum",
            Regex::new(r"(?m)^\s*pub\s+enum\s+([A-Za-z_][A-Za-z0-9_]*)").expect("enum regex"),
        ),
        (
            "trait",
            Regex::new(r"(?m)^\s*pub\s+trait\s+([A-Za-z_][A-Za-z0-9_]*)").expect("trait regex"),
        ),
        (
            "type_alias",
            Regex::new(r"(?m)^\s*pub\s+type\s+([A-Za-z_][A-Za-z0-9_]*)").expect("type regex"),
        ),
        (
            "constant",
            Regex::new(r"(?m)^\s*pub\s+const\s+([A-Za-z_][A-Za-z0-9_]*)").expect("const regex"),
        ),
        (
            "static",
            Regex::new(r"(?m)^\s*pub\s+static\s+([A-Za-z_][A-Za-z0-9_]*)").expect("static regex"),
        ),
        (
            "module",
            Regex::new(r"(?m)^\s*pub\s+mod\s+([A-Za-z_][A-Za-z0-9_]*)").expect("mod regex"),
        ),
        (
            "function",
            Regex::new(r"(?m)^\s*pub\s+(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)")
                .expect("fn regex"),
        ),
        (
            "re_export",
            Regex::new(r"(?m)^\s*pub\s+use\s+(.+);").expect("use regex"),
        ),
    ];

    for (kind, regex) in patterns {
        for captures in regex.captures_iter(content) {
            let Some(matched) = captures.get(1) else {
                continue;
            };
            let name = matched.as_str().trim().to_string();
            let signature = captures
                .get(0)
                .map(|full| full.as_str().trim().to_string())
                .filter(|line| !line.is_empty());
            let key = format!("{kind}:{module}:{name}");
            if seen.insert(key) {
                output.push(RustSymbol {
                    kind: kind.to_string(),
                    name,
                    module: module.to_string(),
                    signature,
                });
            }
        }
    }
}

fn module_path_for_file(root_dir: &Path, file: &Path) -> String {
    let Ok(relative) = file.strip_prefix(root_dir) else {
        return "crate".to_string();
    };

    let mut parts = relative
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .map(|part| part.to_string())
        .collect::<Vec<_>>();
    if parts.is_empty() {
        return "crate".to_string();
    }

    let file_name = parts.pop().unwrap_or_default();
    if file_name != "lib.rs" && file_name != "main.rs" && file_name != "mod.rs" {
        let stem = file_name.strip_suffix(".rs").unwrap_or(&file_name);
        parts.push(stem.to_string());
    }

    if parts.is_empty() {
        "crate".to_string()
    } else {
        parts.join("::")
    }
}

fn canonicalize_name(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| {
            ch.to_ascii_lowercase()
                .to_string()
                .chars()
                .collect::<Vec<_>>()
        })
        .collect()
}

fn artifact_workspace_root_from_primary_root(primary_root: &Path) -> PathBuf {
    primary_root
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("reen_dependency_tooling_{prefix}_{nanos}"))
    }

    #[test]
    fn parses_dependency_manifest_and_normalizes_capabilities() {
        let root = temp_dir("parse_manifest");
        fs::create_dir_all(root.join("drafts")).expect("mkdir drafts");
        let manifest_path = root.join("drafts/dependencies.yml");
        fs::write(
            &manifest_path,
            "schema: reen.dependencies/v1\npackages:\n  - name: serde_json\n    version: \"1.0\"\n    capabilities: [json, \" json \"]\n",
        )
        .expect("write manifest");

        let manifest = load_dependency_manifest(&manifest_path)
            .expect("load manifest")
            .expect("manifest exists");
        assert_eq!(manifest.packages.len(), 1);
        assert_eq!(manifest.packages[0].capabilities, vec!["json".to_string()]);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_invalid_dependency_manifest_entries() {
        let root = temp_dir("invalid_manifest");
        fs::create_dir_all(root.join("drafts")).expect("mkdir drafts");
        let manifest_path = root.join("drafts/dependencies.yml");
        fs::write(
            &manifest_path,
            "schema: reen.dependencies/v1\npackages:\n  - name: tokio\n    version: \"\"\n",
        )
        .expect("write manifest");

        let error =
            load_dependency_manifest(&manifest_path).expect_err("expected validation error");
        assert!(error.to_string().contains("missing a version"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn render_dependency_entries_sorts_and_quotes_versions() {
        let deps = BTreeMap::from([
            ("serde".to_string(), "1.0".to_string()),
            (
                "tokio".to_string(),
                "{ version = \"1.40\", features = [\"macros\"] }".to_string(),
            ),
        ]);

        let rendered = render_dependency_entries(&deps);
        assert!(rendered.contains("serde = \"1.0\""));
        assert!(rendered.contains("tokio = { version = \"1.40\", features = [\"macros\"] }"));
    }

    #[test]
    fn tooling_needs_refresh_when_outputs_missing_or_stale() {
        let root = temp_dir("freshness");
        let drafts = root.join("drafts");
        let artifact_root = root.join("workspace");
        fs::create_dir_all(&drafts).expect("mkdir drafts");
        fs::create_dir_all(&artifact_root).expect("mkdir workspace");

        let paths = tooling_paths(&drafts, &artifact_root);
        fs::write(&paths.dependency_manifest_path, "packages: []\n").expect("write manifest");
        assert!(tooling_needs_refresh(&paths).expect("refresh missing outputs"));

        fs::create_dir_all(paths.tooling_root.join("src")).expect("mkdir tooling src");
        fs::write(
            &paths.cargo_toml_path,
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .expect("write cargo");
        fs::write(&paths.symbols_path, "{}").expect("write symbols");
        std::thread::sleep(Duration::from_millis(20));
        fs::write(
            &paths.dependency_manifest_path,
            "packages:\n  - name: serde_json\n    version: \"1.0\"\n",
        )
        .expect("rewrite manifest");

        assert!(tooling_needs_refresh(&paths).expect("refresh stale outputs"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ensure_tooling_artifacts_writes_manifest_and_symbols_inventory() {
        let root = temp_dir("preflight");
        let drafts = root.join("drafts");
        let artifact_root = root.join("workspace");
        let crate_root = root.join("fake_registry/serde_json");
        fs::create_dir_all(&drafts).expect("mkdir drafts");
        fs::create_dir_all(crate_root.join("src")).expect("mkdir fake crate");
        fs::create_dir_all(&artifact_root).expect("mkdir workspace");

        fs::write(
            drafts.join("dependencies.yml"),
            "schema: reen.dependencies/v1\npackages:\n  - name: serde_json\n    version: \"1.0\"\n    capabilities: [json]\n",
        )
        .expect("write manifest");
        fs::write(
            crate_root.join("src/lib.rs"),
            "pub struct Value;\npub enum Number {}\npub fn from_str() {}\n",
        )
        .expect("write fake crate");

        let metadata_json = format!(
            r#"{{
  "packages": [
    {{
      "name": "serde_json",
      "version": "1.0.145",
      "manifest_path": "{}",
      "targets": [{{ "kind": ["lib"], "src_path": "{}" }}]
    }}
  ]
}}"#,
            crate_root.join("Cargo.toml").display(),
            crate_root.join("src/lib.rs").display()
        );

        ensure_tooling_artifacts_fresh_with_runner(&drafts, &artifact_root, false, |_| {
            Ok(metadata_json.clone())
        })
        .expect("preflight succeeds");

        let paths = tooling_paths(&drafts, &artifact_root);
        assert!(paths.cargo_toml_path.exists());
        assert!(paths.symbols_path.exists());

        let symbols: RustSymbolsInventory =
            serde_json::from_str(&fs::read_to_string(&paths.symbols_path).expect("read symbols"))
                .expect("parse symbols");
        assert_eq!(symbols.packages.len(), 1);
        assert_eq!(symbols.packages[0].name, "serde_json");
        assert!(
            symbols.packages[0]
                .symbols
                .iter()
                .any(|symbol| symbol.kind == "struct" && symbol.name == "Value")
        );
        assert!(
            symbols.packages[0]
                .symbols
                .iter()
                .any(|symbol| symbol.kind == "function" && symbol.name == "from_str")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn fresh_tooling_artifacts_skip_regeneration() {
        let root = temp_dir("skip_refresh");
        let drafts = root.join("drafts");
        let artifact_root = root.join("workspace");
        fs::create_dir_all(&drafts).expect("mkdir drafts");
        fs::create_dir_all(&artifact_root).expect("mkdir workspace");

        let paths = tooling_paths(&drafts, &artifact_root);
        fs::create_dir_all(paths.tooling_root.join("src")).expect("mkdir tooling");
        fs::write(
            &paths.dependency_manifest_path,
            "packages:\n  - name: serde_json\n    version: \"1.0\"\n",
        )
        .expect("write manifest");
        std::thread::sleep(Duration::from_millis(20));
        fs::write(
            &paths.cargo_toml_path,
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .expect("write cargo");
        fs::write(&paths.symbols_path, "{}").expect("write symbols");

        let mut called = false;
        ensure_tooling_artifacts_fresh_with_runner(&drafts, &artifact_root, false, |_| {
            called = true;
            Ok(String::new())
        })
        .expect("preflight succeeds");
        assert!(!called);

        let _ = fs::remove_dir_all(root);
    }
}
