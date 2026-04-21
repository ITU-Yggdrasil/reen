//! Typed, read-only view of the templates embedded in the reen binary.
//!
//! The bundle is the single source of truth for what `reen init` installs.
//! No filesystem I/O happens in this module — it operates entirely over the
//! data compiled into the binary via `include_dir`.

use anyhow::{Context, Result, bail};
use include_dir::{Dir, include_dir};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

static TEMPLATES_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/templates");

/// File classification — drives upgrade behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Classification {
    /// Reen owns this file; overwritten on `--upgrade`.
    Managed,
    /// Reen drops this file once; never touched on upgrade.
    Seed,
}

/// A single file from the embedded bundle.
#[derive(Debug, Clone)]
pub struct BundleFile {
    /// Forward-slash path relative to the project root (e.g. ".claude/hooks.json").
    pub path: String,
    /// Raw file contents.
    pub contents: &'static [u8],
    /// Managed or seed.
    pub classification: Classification,
}

impl BundleFile {
    /// Lowercase hex sha256 of the raw bytes (no newline normalisation).
    pub fn sha256(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.contents);
        format!("{:x}", hasher.finalize())
    }
}

/// Parsed bundle — all files with their classifications.
pub struct Bundle {
    files: Vec<BundleFile>,
}

impl Bundle {
    /// Load the embedded bundle, parsing `bundle.toml` for classifications.
    ///
    /// Panics (via bail) if any template file is unclassified or if `bundle.toml`
    /// references a file that doesn't exist in the embedded directory.
    pub fn load() -> Result<Self> {
        let toml_file = TEMPLATES_DIR
            .get_file("bundle.toml")
            .context("bundle.toml missing from embedded templates")?;
        let toml_str = std::str::from_utf8(toml_file.contents())
            .context("bundle.toml is not valid UTF-8")?;

        let table: toml::Value =
            toml::from_str(toml_str).context("bundle.toml is not valid TOML")?;

        let files_table = table
            .get("files")
            .and_then(|v| v.as_table())
            .context("bundle.toml missing [files] table")?;

        let mut classifications: BTreeMap<String, Classification> = BTreeMap::new();
        for (path, value) in files_table {
            let class_str = value
                .as_str()
                .with_context(|| format!("bundle.toml: value for {path:?} is not a string"))?;
            let classification = match class_str {
                "managed" => Classification::Managed,
                "seed" => Classification::Seed,
                other => bail!("bundle.toml: unknown classification {other:?} for {path:?}"),
            };
            classifications.insert(path.clone(), classification);
        }

        let mut files = Vec::new();
        collect_files(&TEMPLATES_DIR, "", &classifications, &mut files)?;

        // Verify no classification entries are orphaned (reference non-existent files).
        let embedded_paths: std::collections::BTreeSet<String> =
            files.iter().map(|f| f.path.clone()).collect();
        for path in classifications.keys() {
            if !embedded_paths.contains(path) {
                bail!(
                    "bundle.toml classifies {path:?} but the file is not in the embedded templates"
                );
            }
        }

        // Sort by path for deterministic ordering.
        files.sort_by(|a, b| a.path.cmp(&b.path));

        Ok(Bundle { files })
    }

    /// All files in the bundle, sorted by path.
    pub fn files(&self) -> &[BundleFile] {
        &self.files
    }

    /// Only managed files, sorted by path.
    pub fn managed_files(&self) -> Vec<&BundleFile> {
        self.files
            .iter()
            .filter(|f| f.classification == Classification::Managed)
            .collect()
    }

    /// Only seed files, sorted by path.
    pub fn seed_files(&self) -> Vec<&BundleFile> {
        self.files
            .iter()
            .filter(|f| f.classification == Classification::Seed)
            .collect()
    }

    /// The reen binary version baked in at compile time.
    pub fn reen_version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
}

fn collect_files(
    dir: &Dir<'static>,
    prefix: &str,
    classifications: &BTreeMap<String, Classification>,
    out: &mut Vec<BundleFile>,
) -> Result<()> {
    for file in dir.files() {
        let path = file.path().to_string_lossy().replace('\\', "/");
        // Skip meta files that are not installed into target projects.
        if path == "bundle.toml" || path == ".gitignore.fragment" {
            continue;
        }
        let classification = classifications
            .get(&path)
            .copied()
            .with_context(|| {
                format!("template file {path:?} is not classified in bundle.toml — every file in templates/ must be listed")
            })?;
        out.push(BundleFile {
            path,
            contents: file.contents(),
            classification,
        });
    }
    for subdir in dir.dirs() {
        let sub_prefix = if prefix.is_empty() {
            subdir
                .path()
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        } else {
            format!(
                "{}/{}",
                prefix,
                subdir
                    .path()
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            )
        };
        collect_files(subdir, &sub_prefix, classifications, out)?;
    }
    Ok(())
}

/// Read the `.gitignore.fragment` from the embedded bundle.
pub fn gitignore_fragment() -> Result<&'static str> {
    let file = TEMPLATES_DIR
        .get_file(".gitignore.fragment")
        .context(".gitignore.fragment missing from embedded templates")?;
    std::str::from_utf8(file.contents()).context(".gitignore.fragment is not valid UTF-8")
}
