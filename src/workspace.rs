use crate::draft_parser::ArtifactKind;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

pub const DRAFTS_DIR: &str = "drafts";
pub const PREPARED_DIR: &str = "drafts/prepare";
pub const STATE_DIR: &str = ".reen";
pub const GENERATED_MANIFEST: &str = ".reen/generated_files.json";
pub const CONFIG_FILE: &str = "reen.yml";

#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
    pub drafts_dir: PathBuf,
    pub prepared_dir: PathBuf,
    pub state_dir: PathBuf,
    pub layout: ProjectLayout,
}

/// Where generated source code lives in this project.
///
/// Detected at workspace-discovery time by inspecting `Cargo.toml`. A
/// `[workspace]`-only Cargo.toml (no `[package]`) with a sibling `crates/`
/// directory is a multi-crate layout; everything else is single-crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectLayout {
    SingleCrate { src_dir: PathBuf },
    MultiCrate { crates_dir: PathBuf },
}

impl ProjectLayout {
    /// Inspect `Cargo.toml` at `root` and decide which layout this project
    /// uses. Falls back to `SingleCrate` whenever Cargo.toml is missing,
    /// unparseable, or doesn't match the multi-crate shape.
    pub fn detect(root: &Path) -> Self {
        let cargo_path = root.join("Cargo.toml");
        if let Ok(raw) = fs::read_to_string(&cargo_path) {
            if let Ok(doc) = raw.parse::<toml_edit::DocumentMut>() {
                let has_workspace = doc.get("workspace").is_some();
                let has_package = doc.get("package").is_some();
                if has_workspace && !has_package {
                    let crates_dir = root.join("crates");
                    return ProjectLayout::MultiCrate { crates_dir };
                }
            }
        }
        ProjectLayout::SingleCrate { src_dir: root.join("src") }
    }

    pub fn is_multi_crate(&self) -> bool {
        matches!(self, ProjectLayout::MultiCrate { .. })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "kebab-case")]
pub struct ReenConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verbose: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debug: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projections: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(skip_serializing_if = "CommandConfig::is_empty")]
    pub prepare: CommandConfig,
    #[serde(skip_serializing_if = "CommandConfig::is_empty")]
    pub scaffold: CommandConfig,
    #[serde(skip_serializing_if = "CommandConfig::is_empty")]
    pub clear: CommandConfig,
    #[serde(skip_serializing_if = "CommandConfig::is_empty")]
    pub manifest: CommandConfig,
    #[serde(skip_serializing_if = "RefineConfig::is_empty")]
    pub refine: RefineConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "kebab-case")]
pub struct RefineConfig {
    // Standard section fields — mirror [`CommandConfig`] so a user can `reen config refine
    // --contexts --verbose` and have it behave exactly like the same flags on the sibling
    // `prepare:` / `scaffold:` sections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verbose: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debug: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projections: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,

    /// Persisted equivalent of `--drafts-only`. Mutually exclusive with `prepared_only`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drafts_only: Option<bool>,
    /// Persisted equivalent of `--prepared-only`. Mutually exclusive with `drafts_only`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prepared_only: Option<bool>,
}

impl RefineConfig {
    pub fn is_empty(&self) -> bool {
        self.verbose.is_none()
            && self.debug.is_none()
            && self.dry_run.is_none()
            && self.contexts.is_none()
            && self.projections.is_none()
            && self.data.is_none()
            && self.app.is_none()
            && self.profile.is_none()
            && self.drafts_only.is_none()
            && self.prepared_only.is_none()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "kebab-case")]
pub struct CommandConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verbose: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debug: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projections: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
}

impl CommandConfig {
    pub fn is_empty(&self) -> bool {
        self.verbose.is_none()
            && self.debug.is_none()
            && self.dry_run.is_none()
            && self.contexts.is_none()
            && self.projections.is_none()
            && self.data.is_none()
            && self.app.is_none()
            && self.profile.is_none()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Selection {
    contexts: bool,
    projections: bool,
    data: bool,
    app: bool,
    names: BTreeSet<String>,
}

impl Selection {
    pub fn new(
        contexts: bool,
        projections: bool,
        data: bool,
        app: bool,
        names: Vec<String>,
    ) -> Self {
        let names = names
            .into_iter()
            .map(|name| normalize_name(&name))
            .collect::<BTreeSet<_>>();
        Self {
            contexts,
            projections,
            data,
            app,
            names,
        }
    }

    pub fn includes(&self, kind: ArtifactKind) -> bool {
        if !self.has_explicit_kind_filter() {
            return true;
        }

        match kind {
            ArtifactKind::Context => self.contexts,
            ArtifactKind::Projection => self.projections,
            ArtifactKind::Data => self.data,
            ArtifactKind::Component | ArtifactKind::Brand | ArtifactKind::RoutingTable => {
                self.app
            }
            ArtifactKind::App => true, // App is always included when present
        }
    }

    pub fn matches_name(&self, name: &str) -> bool {
        self.names.is_empty() || self.names.contains(&normalize_name(name))
    }

    pub fn has_name_filter(&self) -> bool {
        !self.names.is_empty()
    }

    fn has_explicit_kind_filter(&self) -> bool {
        self.contexts || self.projections || self.data || self.app
    }
}

impl Workspace {
    pub fn discover(root: PathBuf) -> Result<Self> {
        let layout = ProjectLayout::detect(&root);
        Ok(Self {
            drafts_dir: root.join(DRAFTS_DIR),
            prepared_dir: root.join(PREPARED_DIR),
            state_dir: root.join(STATE_DIR),
            layout,
            root,
        })
    }

    pub fn config_path(&self) -> PathBuf {
        self.root.join(CONFIG_FILE)
    }

    pub fn load_config(&self) -> Result<ReenConfig> {
        let path = self.config_path();
        if !path.is_file() {
            return Ok(ReenConfig::default());
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let config = serde_yaml::from_str(&raw)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        Ok(config)
    }

    pub fn save_config(&self, config: &ReenConfig) -> Result<()> {
        let path = self.config_path();
        let yaml = serde_yaml::to_string(config)?;
        fs::write(&path, yaml).with_context(|| format!("Failed to write {}", path.display()))
    }

    pub fn ensure_drafts_exist(&self) -> Result<()> {
        if self.drafts_dir.is_dir() {
            return Ok(());
        }
        bail!("No drafts directory found at {}", self.drafts_dir.display());
    }

    pub fn raw_draft_paths(&self, selection: &Selection) -> Result<Vec<PathBuf>> {
        self.ensure_drafts_exist()?;
        let mut paths = Vec::new();
        collect_raw_drafts(&self.drafts_dir, &self.drafts_dir, selection, &mut paths)?;
        paths.sort();
        if selection.has_name_filter() && paths.is_empty() {
            bail!("No drafts matched the requested names");
        }
        Ok(paths)
    }

    pub fn prepared_paths(&self, selection: &Selection) -> Result<Vec<PathBuf>> {
        if !self.prepared_dir.is_dir() {
            bail!(
                "No prepared artifacts found at {}; run `reen prepare` first",
                self.prepared_dir.display()
            );
        }

        let mut paths = Vec::new();
        collect_prepared_artifacts(
            &self.prepared_dir,
            &self.prepared_dir,
            selection,
            &mut paths,
        )?;
        paths.sort();
        if selection.has_name_filter() && paths.is_empty() {
            bail!("No prepared artifacts matched the requested names");
        }
        Ok(paths)
    }

    pub fn prepared_output_path(&self, draft_path: &Path) -> Result<PathBuf> {
        let relative = draft_path.strip_prefix(&self.root).unwrap_or(draft_path);
        let under_drafts = relative
            .strip_prefix(DRAFTS_DIR)
            .with_context(|| format!("{} is not inside drafts/", draft_path.display()))?;
        Ok(self
            .root
            .join(PREPARED_DIR)
            .join(under_drafts)
            .with_extension("yml"))
    }

    pub fn matching_prepared_paths_for_raw(
        &self,
        raw_draft_paths: &[PathBuf],
    ) -> Result<Vec<PathBuf>> {
        let mut paths = raw_draft_paths
            .iter()
            .filter_map(|path| self.prepared_output_path(path).ok())
            .filter(|path| path.is_file())
            .collect::<Vec<_>>();
        paths.sort();
        paths.dedup();
        Ok(paths)
    }

    pub fn refine_report_path(&self) -> PathBuf {
        self.state_dir.join("refine").join("report.md")
    }
}

fn collect_raw_drafts(
    root: &Path,
    current: &Path,
    selection: &Selection,
    out: &mut Vec<PathBuf>,
) -> Result<()> {
    for entry in fs::read_dir(current).with_context(|| format!("read_dir {}", current.display()))? {
        let entry = entry?;
        let path = entry.path();
        let relative = path.strip_prefix(root).unwrap_or(&path);

        if path.is_dir() {
            let first = relative
                .components()
                .next()
                .and_then(|component| component.as_os_str().to_str());
            // Skip prepare/ output directory.
            if matches!(first, Some("prepare")) {
                continue;
            }
            // Also skip apis/ directory — boundary drafts are not DCI drafts
            if matches!(first, Some("apis")) {
                continue;
            }
            collect_raw_drafts(root, &path, selection, out)?;
            continue;
        }

        if path.extension().and_then(|value| value.to_str()) != Some("md") {
            continue;
        }

        let Some(kind) = ArtifactKind::from_draft_path(relative) else {
            continue;
        };
        if !selection.includes(kind) {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if !selection.matches_name(stem) {
            continue;
        }
        out.push(path);
    }

    Ok(())
}

fn collect_prepared_artifacts(
    root: &Path,
    current: &Path,
    selection: &Selection,
    out: &mut Vec<PathBuf>,
) -> Result<()> {
    for entry in fs::read_dir(current).with_context(|| format!("read_dir {}", current.display()))? {
        let entry = entry?;
        let path = entry.path();
        let relative = path.strip_prefix(root).unwrap_or(&path);

        if path.is_dir() {
            collect_prepared_artifacts(root, &path, selection, out)?;
            continue;
        }

        if path.extension().and_then(|value| value.to_str()) != Some("yml") {
            continue;
        }

        let Some(kind) = ArtifactKind::from_prepared_path(relative) else {
            continue;
        };
        if !selection.includes(kind) {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if !selection.matches_name(stem) {
            continue;
        }
        out.push(path);
    }

    Ok(())
}


fn normalize_name(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    use std::io::Write;

    fn write_cargo(dir: &Path, contents: &str) {
        let path = dir.join("Cargo.toml");
        let mut f = fs::File::create(&path).expect("create Cargo.toml");
        f.write_all(contents.as_bytes()).expect("write Cargo.toml");
    }

    #[test]
    fn layout_detects_single_crate_from_package_toml() {
        let tmp = tempdir();
        write_cargo(
            tmp.path(),
            "[package]\nname = \"snake\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        );
        let layout = ProjectLayout::detect(tmp.path());
        match layout {
            ProjectLayout::SingleCrate { src_dir } => {
                assert_eq!(src_dir, tmp.path().join("src"));
            }
            other => panic!("expected SingleCrate, got {:?}", other),
        }
    }

    #[test]
    fn layout_detects_multi_crate_from_workspace_only_toml() {
        let tmp = tempdir();
        write_cargo(
            tmp.path(),
            "[workspace]\nresolver = \"2\"\nmembers = [\n    \"crates/lupa-core\",\n    \"crates/lupa-server\",\n]\n",
        );
        // crates/ doesn't need to exist for detection.
        let layout = ProjectLayout::detect(tmp.path());
        match layout {
            ProjectLayout::MultiCrate { crates_dir } => {
                assert_eq!(crates_dir, tmp.path().join("crates"));
            }
            other => panic!("expected MultiCrate, got {:?}", other),
        }
    }

    #[test]
    fn layout_treats_workspace_with_top_level_package_as_single_crate() {
        // When Cargo.toml carries BOTH [workspace] and [package] (a workspace
        // root that is itself a package), we treat the project as
        // SingleCrate. The codegen contract is "machine-owns top-level src/",
        // and that file lives at the package root in this configuration.
        let tmp = tempdir();
        write_cargo(
            tmp.path(),
            "[workspace]\nmembers = [\".\"]\n\n[package]\nname = \"hybrid\"\nversion = \"0.1.0\"\n",
        );
        let layout = ProjectLayout::detect(tmp.path());
        assert!(
            matches!(layout, ProjectLayout::SingleCrate { .. }),
            "expected SingleCrate when [package] coexists with [workspace], got {:?}",
            layout,
        );
    }

    #[test]
    fn layout_falls_back_to_single_crate_when_cargo_missing() {
        let tmp = tempdir();
        let layout = ProjectLayout::detect(tmp.path());
        assert!(matches!(layout, ProjectLayout::SingleCrate { .. }));
    }

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir().expect("create tempdir")
    }
}
