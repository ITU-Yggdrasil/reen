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
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "kebab-case")]
pub struct ReenConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<bool>,
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
    /// Root-level fallback for [`RefineConfig::min_severity`]. Accepted at the root so a user
    /// can write a short `min-severity: 90` line without nesting under `refine:`. The nested
    /// `refine.min-severity` takes precedence when both are set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_severity: Option<u8>,
    /// Root-level fallback for [`RefineConfig::skip_llm_review`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_llm_review: Option<bool>,
    /// Root-level fallback for [`RefineConfig::require_llm_review`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_llm_review: Option<bool>,
    #[serde(skip_serializing_if = "CommandConfig::is_empty")]
    pub prepare: CommandConfig,
    #[serde(skip_serializing_if = "CommandConfig::is_empty")]
    pub scaffold: CommandConfig,
    #[serde(skip_serializing_if = "CommandConfig::is_empty")]
    pub build: CommandConfig,
    #[serde(skip_serializing_if = "CommandConfig::is_empty")]
    pub compile: CommandConfig,
    #[serde(skip_serializing_if = "CommandConfig::is_empty")]
    pub run: CommandConfig,
    #[serde(skip_serializing_if = "CommandConfig::is_empty")]
    pub test: CommandConfig,
    #[serde(skip_serializing_if = "CommandConfig::is_empty")]
    pub clear: CommandConfig,
    #[serde(skip_serializing_if = "RefineConfig::is_empty")]
    pub refine: RefineConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "kebab-case")]
pub struct RefineConfig {
    /// Minimum behavioral-ambiguity severity (0..=100). `None` → use the library default from
    /// [`crate::draft_refine_llm::DEFAULT_MIN_SEVERITY`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_severity: Option<u8>,
    /// Disable the LLM-backed behavioral review entirely.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_llm_review: Option<bool>,
    /// Treat LLM unavailability as a hard error rather than a silent skip.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_llm_review: Option<bool>,
}

impl RefineConfig {
    pub fn is_empty(&self) -> bool {
        self.min_severity.is_none()
            && self.skip_llm_review.is_none()
            && self.require_llm_review.is_none()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "kebab-case")]
pub struct CommandConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<bool>,
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
        self.fix.is_none()
            && self.verbose.is_none()
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
            return matches!(
                kind,
                ArtifactKind::Context
                    | ArtifactKind::Projection
                    | ArtifactKind::Data
                    | ArtifactKind::App
            );
        }

        match kind {
            ArtifactKind::Context => self.contexts,
            ArtifactKind::Projection => self.projections,
            ArtifactKind::Data => self.data,
            ArtifactKind::App => self.app,
            ArtifactKind::UnsupportedApi => false,
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
        Ok(Self {
            drafts_dir: root.join(DRAFTS_DIR),
            prepared_dir: root.join(PREPARED_DIR),
            state_dir: root.join(STATE_DIR),
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
            if matches!(first, Some("prepare")) {
                continue;
            }
            if matches!(first, Some("apis" | "external_apis")) {
                if dir_contains_markdown(&path)? {
                    bail!(
                        "Unsupported draft scope under {}. V1 does not support drafts/apis or drafts/external_apis.",
                        path.display()
                    );
                }
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

fn dir_contains_markdown(dir: &Path) -> Result<bool> {
    for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() && dir_contains_markdown(&path)? {
            return Ok(true);
        }
        if path.extension().and_then(|value| value.to_str()) == Some("md") {
            return Ok(true);
        }
    }
    Ok(false)
}

fn normalize_name(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}
