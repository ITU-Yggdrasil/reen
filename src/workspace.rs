use crate::draft_parser::ArtifactKind;
use anyhow::{Context, Result, bail};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

pub const DRAFTS_DIR: &str = "drafts";
pub const PREPARED_DIR: &str = "drafts/prepare";
pub const STATE_DIR: &str = ".reen";
pub const GENERATED_MANIFEST: &str = ".reen/generated_files.json";

#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
    pub drafts_dir: PathBuf,
    pub prepared_dir: PathBuf,
    pub state_dir: PathBuf,
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

    pub fn ensure_drafts_exist(&self) -> Result<()> {
        if self.drafts_dir.is_dir() {
            return Ok(());
        }
        bail!(
            "No drafts directory found at {}",
            self.drafts_dir.display()
        );
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
        collect_prepared_artifacts(&self.prepared_dir, &self.prepared_dir, selection, &mut paths)?;
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
            let first = relative.components().next().and_then(|component| component.as_os_str().to_str());
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
        let stem = path.file_stem().and_then(|value| value.to_str()).unwrap_or_default();
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
        let stem = path.file_stem().and_then(|value| value.to_str()).unwrap_or_default();
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
