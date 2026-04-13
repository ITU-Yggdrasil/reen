//! Deterministic allowed-types manifest (`drafts/types-manifest.yml`).
//!
//! See `plans/allowed-types-manifest.md`. Scans draft markdown, primitives, Cargo.toml
//! dependency names, and emits scoped allowlists for interface synthesis.

use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use super::draft_schema::{DraftKind, draft_title_from_markdown, infer_draft_kind};
use super::resolved_contract::primary_export_rust_identifier;

pub(crate) const MANIFEST_YAML_NAME: &str = "types-manifest.yml";
const LEGACY_MANIFEST_MD_NAME: &str = "types-manifest.md";
const META_VERSION: u32 = 2;

/// Paths under the drafts root that are outputs of this generator (excluded from tree fingerprint).
/// The markdown summary path is kept here only so legacy files do not perturb the fingerprint.
fn is_manifest_output(relative: &str) -> bool {
    relative == MANIFEST_YAML_NAME || relative == LEGACY_MANIFEST_MD_NAME
}

fn collect_markdown_files(drafts_root: &Path, dir: &Path, acc: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_markdown_files(drafts_root, &path, acc)?;
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        acc.push(path);
    }
    Ok(())
}

fn relative_draft_path(drafts_root: &Path, file: &Path) -> Result<String> {
    Ok(file
        .strip_prefix(drafts_root)
        .with_context(|| format!("draft path not under {}", drafts_root.display()))?
        .to_string_lossy()
        .replace('\\', "/"))
}

/// First `# <Title>` line only — matches [`super::draft_schema::draft_title_from_markdown`]
/// without loading the rest of the file.
fn read_first_heading_title(path: &Path) -> Result<String> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let reader = BufReader::new(file);
    for line in reader.lines() {
        let line = line.with_context(|| format!("read line {}", path.display()))?;
        if let Some(title) = line
            .trim()
            .strip_prefix("# ")
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            return Ok(title.to_string());
        }
    }
    Ok(String::new())
}

/// Fingerprint of **`drafts/**/*.md` path set** and each file’s **title** (first `# ` heading).
/// Body text does not affect this value.
pub(crate) fn drafts_tree_fingerprint(drafts_root: &Path) -> Result<String> {
    let mut files = Vec::new();
    collect_markdown_files(drafts_root, drafts_root, &mut files)?;
    files.sort();

    let mut hasher = Sha256::new();
    for path in files {
        let rel = relative_draft_path(drafts_root, &path)?;
        if is_manifest_output(&rel) {
            continue;
        }
        let title = read_first_heading_title(&path)?;
        hasher.update(rel.as_bytes());
        hasher.update(b"\0");
        hasher.update(title.as_bytes());
        hasher.update(b"\n");
    }
    Ok(hex::encode(hasher.finalize()))
}

fn static_primitives() -> Vec<String> {
    vec![
        "()".to_string(),
        "bool".to_string(),
        "char".to_string(),
        "f32".to_string(),
        "f64".to_string(),
        "i128".to_string(),
        "i16".to_string(),
        "i32".to_string(),
        "i64".to_string(),
        "i8".to_string(),
        "isize".to_string(),
        "str".to_string(),
        "String".to_string(),
        "u128".to_string(),
        "u16".to_string(),
        "u32".to_string(),
        "u64".to_string(),
        "u8".to_string(),
        "usize".to_string(),
    ]
}

/// Default phrase → concrete type, aligned with `resolve_semantic_type` defaults.
fn static_semantic_defaults() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("integer".to_string(), "i32".to_string());
    m.insert("non_negative_integer".to_string(), "u32".to_string());
    m.insert("positive_whole_number".to_string(), "u32".to_string());
    m.insert("timestamp_millis".to_string(), "u64".to_string());
    m
}

fn static_external_prefixes() -> Vec<String> {
    vec![
        "alloc::".to_string(),
        "anyhow::".to_string(),
        "core::".to_string(),
        "std::".to_string(),
    ]
}

fn parse_dependency_keys_from_cargo_toml(text: &str) -> BTreeSet<String> {
    let dep_line = Regex::new(r"^\s*([A-Za-z0-9_][A-Za-z0-9_-]*)\s*=").expect("regex");
    let mut names = BTreeSet::new();
    let mut section: Option<String> = None;

    for raw_line in text.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') && line.ends_with(']') {
            let inner = line.trim_start_matches('[').trim_end_matches(']').trim();
            section = Some(inner.to_string());
            continue;
        }
        let sec = section.as_deref().unwrap_or("");
        let in_deps = sec == "dependencies"
            || sec == "dev-dependencies"
            || sec == "build-dependencies"
            || sec.ends_with(".dependencies");
        if !in_deps || line.is_empty() {
            continue;
        }
        if let Some(caps) = dep_line.captures(line) {
            let key = caps.get(1).unwrap().as_str();
            let base = key.split('.').next().unwrap_or(key);
            if base == "package" {
                continue;
            }
            names.insert(base.to_string());
        }
    }
    names
}

fn cargo_toml_candidates(drafts_root: &Path, artifact_workspace_root: &Path) -> Vec<PathBuf> {
    let mut v = vec![artifact_workspace_root.join("Cargo.toml")];
    if let Some(parent) = drafts_root.parent() {
        v.push(parent.join("Cargo.toml"));
    }
    v
}

fn dependency_crate_prefixes(drafts_root: &Path, artifact_workspace_root: &Path) -> Vec<String> {
    let mut names = BTreeSet::new();
    for path in cargo_toml_candidates(drafts_root, artifact_workspace_root) {
        if !path.is_file() {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        names.extend(parse_dependency_keys_from_cargo_toml(&text));
    }
    names.into_iter().map(|n| format!("{n}::")).collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct DraftTypeRow {
    pub(crate) relative_path: String,
    pub(crate) title: String,
    pub(crate) export_type: String,
    pub(crate) aliases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Allowlists {
    data: Vec<String>,
    projection: Vec<String>,
    context: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct TypesManifestRules {
    /// `allow_projection = allow_data ∪ D_projection`
    pub(crate) projection_includes_data: bool,
    /// `allow_context = allow_data ∪ D_context ∪ D_projection`
    pub(crate) context_includes_projections: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct TypesManifestMeta {
    pub(crate) version: u32,
    pub(crate) drafts_tree_fingerprint: String,
    pub(crate) rules: TypesManifestRules,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct TypesManifestDoc {
    meta: TypesManifestMeta,
    primitives: Vec<String>,
    semantic_defaults: BTreeMap<String, String>,
    /// `data`, `projection`, `context`, `api` — only scoped kinds populate allowlists.
    drafts: BTreeMap<String, Vec<DraftTypeRow>>,
    external_path_prefixes: Vec<String>,
    allowlists: Allowlists,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct TypesManifestScope {
    pub(crate) meta: TypesManifestMeta,
    pub(crate) draft_kind: String,
    pub(crate) allowlist: Vec<String>,
    pub(crate) semantic_defaults: BTreeMap<String, String>,
    pub(crate) external_path_prefixes: Vec<String>,
    pub(crate) draft_types: Vec<DraftTypeRow>,
}

fn normalize_prose_alias(phrase: &str) -> Option<String> {
    let tokens = phrase
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(" "))
    }
}

fn split_camel_case_phrase(ident: &str) -> Option<String> {
    let mut words = Vec::new();
    let chars = ident.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return None;
    }

    let mut current = String::new();
    for (idx, ch) in chars.iter().enumerate() {
        let previous = idx.checked_sub(1).and_then(|i| chars.get(i)).copied();
        let next = chars.get(idx + 1).copied();
        let starts_new_word = idx > 0
            && ch.is_ascii_uppercase()
            && (previous.is_some_and(|prev| prev.is_ascii_lowercase() || prev.is_ascii_digit())
                || next.is_some_and(|next| next.is_ascii_lowercase()));
        if starts_new_word && !current.is_empty() {
            words.push(current);
            current = String::new();
        }
        current.push(*ch);
    }
    if !current.is_empty() {
        words.push(current);
    }

    if words.is_empty() {
        None
    } else {
        Some(words.join(" "))
    }
}

fn draft_type_aliases(title: &str, export_type: &str) -> Vec<String> {
    let mut aliases = BTreeSet::new();

    if let Some(alias) = normalize_prose_alias(title) {
        aliases.insert(alias.clone());
        aliases.insert(alias.to_ascii_lowercase());
    }

    if let Some(alias) = split_camel_case_phrase(export_type) {
        aliases.insert(alias.clone());
        aliases.insert(alias.to_ascii_lowercase());
    }

    aliases.into_iter().collect()
}

fn kind_key(kind: DraftKind) -> Option<&'static str> {
    match kind {
        DraftKind::Data => Some("data"),
        DraftKind::Projection => Some("projection"),
        DraftKind::Context => Some("context"),
        DraftKind::Api | DraftKind::Root => None,
    }
}

fn merge_allowlists(
    primitives: &[String],
    semantic_defaults: &BTreeMap<String, String>,
    external: &[String],
    d_data: &BTreeSet<String>,
    d_projection: &BTreeSet<String>,
    d_context: &BTreeSet<String>,
) -> Allowlists {
    let mut pset: BTreeSet<String> = primitives.iter().cloned().collect();
    pset.extend(semantic_defaults.values().cloned());
    pset.extend(d_data.iter().cloned());
    pset.extend(external.iter().cloned());

    let allow_data: Vec<String> = pset.iter().cloned().collect();

    let mut proj = pset.clone();
    proj.extend(d_projection.iter().cloned());
    let allow_projection: Vec<String> = proj.iter().cloned().collect();

    let mut ctx = pset;
    ctx.extend(d_context.iter().cloned());
    ctx.extend(d_projection.iter().cloned());
    let allow_context: Vec<String> = ctx.iter().cloned().collect();

    Allowlists {
        data: allow_data,
        projection: allow_projection,
        context: allow_context,
    }
}

fn build_manifest_doc(
    drafts_root: &Path,
    artifact_workspace_root: &Path,
) -> Result<TypesManifestDoc> {
    let fingerprint = drafts_tree_fingerprint(drafts_root)?;
    let primitives = static_primitives();
    let semantic_defaults = static_semantic_defaults();
    let mut external = static_external_prefixes();
    external.extend(dependency_crate_prefixes(
        drafts_root,
        artifact_workspace_root,
    ));
    external.sort();
    external.dedup();

    let mut files = Vec::new();
    collect_markdown_files(drafts_root, drafts_root, &mut files)?;
    files.sort();

    let mut by_kind: BTreeMap<String, Vec<DraftTypeRow>> = BTreeMap::new();
    let mut d_data = BTreeSet::new();
    let mut d_projection = BTreeSet::new();
    let mut d_context = BTreeSet::new();
    let mut type_to_paths: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for path in files {
        let rel = relative_draft_path(drafts_root, &path)?;
        if is_manifest_output(&rel) {
            continue;
        }
        let kind = infer_draft_kind(&path, &drafts_root.to_string_lossy());
        let Some(kind_str) = kind_key(kind) else {
            continue;
        };

        let content =
            fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let Some(title) = draft_title_from_markdown(&content) else {
            bail!(
                "Missing `# <Title>` heading in draft {}; cannot derive export type",
                rel
            );
        };
        let export_type = primary_export_rust_identifier(&title);
        type_to_paths
            .entry(export_type.clone())
            .or_default()
            .push(rel.clone());

        let aliases = draft_type_aliases(&title, &export_type);
        by_kind
            .entry(kind_str.to_string())
            .or_default()
            .push(DraftTypeRow {
                relative_path: rel,
                title,
                export_type: export_type.clone(),
                aliases,
            });

        match kind {
            DraftKind::Data => {
                d_data.insert(export_type);
            }
            DraftKind::Projection => {
                d_projection.insert(export_type);
            }
            DraftKind::Context => {
                d_context.insert(export_type);
            }
            _ => {}
        }
    }

    for rows in by_kind.values_mut() {
        rows.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    }

    for (export_type, paths) in &type_to_paths {
        if paths.len() > 1 {
            bail!(
                "Duplicate export type '{}' from multiple drafts; disambiguate titles or paths:\n  - {}",
                export_type,
                paths.join("\n  - ")
            );
        }
    }

    let allowlists = merge_allowlists(
        &primitives,
        &semantic_defaults,
        &external,
        &d_data,
        &d_projection,
        &d_context,
    );

    Ok(TypesManifestDoc {
        meta: TypesManifestMeta {
            version: META_VERSION,
            drafts_tree_fingerprint: fingerprint,
            rules: TypesManifestRules {
                projection_includes_data: true,
                context_includes_projections: true,
            },
        },
        primitives,
        semantic_defaults,
        drafts: by_kind,
        external_path_prefixes: external,
        allowlists,
    })
}

fn load_manifest_doc(
    drafts_root: &Path,
    artifact_workspace_root: &Path,
) -> Result<TypesManifestDoc> {
    let yaml_path = drafts_root.join(MANIFEST_YAML_NAME);
    if yaml_path.is_file() {
        let yaml = fs::read_to_string(&yaml_path)
            .with_context(|| format!("read {}", yaml_path.display()))?;
        return serde_yaml::from_str(&yaml)
            .with_context(|| format!("parse {}", yaml_path.display()));
    }

    build_manifest_doc(drafts_root, artifact_workspace_root)
}

fn scope_key_for_specification_kind(specification_kind: &str) -> Option<&'static str> {
    match specification_kind.trim().to_ascii_lowercase().as_str() {
        "data" => Some("data"),
        "projection" => Some("projection"),
        "context" | "app" | "root" => Some("context"),
        _ => None,
    }
}

pub(crate) fn load_types_manifest_scope(
    drafts_root: &Path,
    artifact_workspace_root: &Path,
    specification_kind: &str,
) -> Result<Option<TypesManifestScope>> {
    let Some(scope_key) = scope_key_for_specification_kind(specification_kind) else {
        return Ok(None);
    };
    let doc = load_manifest_doc(drafts_root, artifact_workspace_root)?;
    let allowlist = match scope_key {
        "data" => doc.allowlists.data.clone(),
        "projection" => doc.allowlists.projection.clone(),
        "context" => doc.allowlists.context.clone(),
        _ => Vec::new(),
    };
    let draft_types = doc.drafts.get(scope_key).cloned().unwrap_or_default();

    Ok(Some(TypesManifestScope {
        meta: doc.meta,
        draft_kind: scope_key.to_string(),
        allowlist,
        semantic_defaults: doc.semantic_defaults,
        external_path_prefixes: doc.external_path_prefixes,
        draft_types,
    }))
}

/// Regenerates `drafts/types-manifest.yml` when inputs changed and removes any legacy markdown
/// summary if present.
pub(crate) fn ensure_types_manifest_current(
    drafts_root: &Path,
    artifact_workspace_root: &Path,
    dry_run: bool,
    verbose: bool,
) -> Result<()> {
    let doc = build_manifest_doc(drafts_root, artifact_workspace_root)?;
    let yaml_path = drafts_root.join(MANIFEST_YAML_NAME);
    let legacy_md_path = drafts_root.join(LEGACY_MANIFEST_MD_NAME);

    let yaml = serde_yaml::to_string(&doc).context("serialize types manifest")?;
    let stale_markdown_exists = legacy_md_path.is_file();

    let existing = if yaml_path.is_file() {
        fs::read_to_string(&yaml_path).ok()
    } else {
        None
    };

    if existing.as_deref() == Some(yaml.as_str()) && !stale_markdown_exists {
        if verbose {
            println!(
                "{}",
                super::progress::standard_text(format!(
                    "Types manifest up to date: {}",
                    yaml_path.display()
                ))
            );
        }
        return Ok(());
    }

    if dry_run {
        println!(
            "[DRY RUN] Would write types manifest to {}",
            yaml_path.display()
        );
        if stale_markdown_exists {
            println!(
                "[DRY RUN] Would remove legacy types manifest summary at {}",
                legacy_md_path.display()
            );
        }
        return Ok(());
    }

    if existing.as_deref() != Some(yaml.as_str()) {
        fs::write(&yaml_path, &yaml).with_context(|| format!("write {}", yaml_path.display()))?;
    }
    if stale_markdown_exists {
        fs::remove_file(&legacy_md_path)
            .with_context(|| format!("remove {}", legacy_md_path.display()))?;
    }

    println!(
        "{}",
        super::progress::standard_text(format!("Updated types manifest: {}", yaml_path.display()))
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch_drafts_dir(name: &str) -> PathBuf {
        let base =
            std::env::temp_dir().join(format!("reen_types_manifest_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).expect("mkdir scratch");
        base
    }

    #[test]
    fn drafts_fingerprint_changes_when_md_added() {
        let base = scratch_drafts_dir("fp");
        let root = base.join("drafts");
        fs::create_dir_all(root.join("data")).expect("mkdir");
        let a = root.join("data/A.md");
        fs::write(&a, "# A\n").expect("write");
        let fp1 = drafts_tree_fingerprint(&root).expect("fp1");
        let b = root.join("data/B.md");
        fs::write(&b, "# B\n").expect("write");
        let fp2 = drafts_tree_fingerprint(&root).expect("fp2");
        assert_ne!(fp1, fp2);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn drafts_fingerprint_ignores_body_when_title_unchanged() {
        let base = scratch_drafts_dir("body");
        let root = base.join("drafts");
        fs::create_dir_all(root.join("data")).expect("mkdir");
        let p = root.join("data/X.md");
        fs::write(&p, "# X\n\n## Description\n\nalpha\n").expect("write");
        let fp1 = drafts_tree_fingerprint(&root).expect("fp1");
        fs::write(&p, "# X\n\n## Description\n\nbeta and more prose\n").expect("rewrite");
        let fp2 = drafts_tree_fingerprint(&root).expect("fp2");
        assert_eq!(fp1, fp2);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn drafts_fingerprint_changes_when_title_changes() {
        let base = scratch_drafts_dir("title");
        let root = base.join("drafts");
        fs::create_dir_all(root.join("data")).expect("mkdir");
        let p = root.join("data/X.md");
        fs::write(&p, "# X\n").expect("write");
        let fp1 = drafts_tree_fingerprint(&root).expect("fp1");
        fs::write(&p, "# Y\n").expect("rewrite title");
        let fp2 = drafts_tree_fingerprint(&root).expect("fp2");
        assert_ne!(fp1, fp2);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn duplicate_export_types_error() {
        let base = scratch_drafts_dir("dup");
        let root = base.join("drafts");
        fs::create_dir_all(root.join("data/x")).expect("mkdir");
        fs::create_dir_all(root.join("data/y")).expect("mkdir");
        fs::write(root.join("data/x/a.md"), "# Foo\n").expect("w");
        fs::write(root.join("data/y/b.md"), "# Foo\n").expect("w");
        let err = build_manifest_doc(&root, &base).expect_err("dup");
        assert!(err.to_string().contains("Duplicate export type"), "{}", err);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn manifest_normalizes_export_type_and_records_prose_aliases() {
        let base = scratch_drafts_dir("aliases");
        let root = base.join("drafts");
        fs::create_dir_all(root.join("contexts")).expect("mkdir");
        fs::write(
            root.join("contexts/terminal_renderer.md"),
            "# Terminal Renderer\n",
        )
        .expect("write");

        let doc = build_manifest_doc(&root, &base).expect("manifest");
        let rows = doc.drafts.get("context").expect("context rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Terminal Renderer");
        assert_eq!(rows[0].export_type, "TerminalRenderer");
        assert!(rows[0].aliases.contains(&"Terminal Renderer".to_string()));
        assert!(rows[0].aliases.contains(&"terminal renderer".to_string()));
        assert!(
            doc.allowlists
                .context
                .contains(&"TerminalRenderer".to_string())
        );
        assert!(doc.external_path_prefixes.contains(&"anyhow::".to_string()));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn loads_context_scope_for_app_specifications() {
        let base = scratch_drafts_dir("scope");
        let root = base.join("drafts");
        fs::create_dir_all(root.join("contexts")).expect("mkdir");
        fs::write(
            root.join("contexts/terminal_renderer.md"),
            "# Terminal Renderer\n",
        )
        .expect("write");

        let scope = load_types_manifest_scope(&root, &base, "app")
            .expect("scope")
            .expect("context scope");
        assert_eq!(scope.draft_kind, "context");
        assert!(scope.allowlist.contains(&"TerminalRenderer".to_string()));

        let _ = fs::remove_dir_all(&base);
    }
}
