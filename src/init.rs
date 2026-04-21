//! `reen init` — install, upgrade, and check the bundle in a target project.

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::bundle::{Bundle, BundleFile, Classification};

/// On-disk bundle manifest written to `.reen-bundle.yml`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct BundleManifest {
    pub schema: String,
    pub reen_version: String,
    pub installed_at: String,
    pub managed_files: Vec<ManagedFileEntry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ManagedFileEntry {
    pub path: String,
    pub sha256: String,
}

/// Options for `reen init`.
pub struct InitOptions {
    pub target_dir: PathBuf,
    pub upgrade: bool,
    pub check: bool,
    pub force: bool,
}

/// Exit codes for `--check` mode.
pub const CHECK_UP_TO_DATE: i32 = 0;
pub const CHECK_CHANGES_PENDING: i32 = 1;
pub const CHECK_BLOCKED_BY_EDITS: i32 = 2;

/// Run `reen init` with the given options.
pub fn run_init(options: &InitOptions) -> Result<i32> {
    let bundle = Bundle::load()?;
    let manifest_path = options.target_dir.join(".reen-bundle.yml");
    let has_manifest = manifest_path.is_file();

    if options.upgrade || options.check {
        if !has_manifest {
            bail!(
                "No .reen-bundle.yml found in {}. Run `reen init` first to install the bundle.",
                options.target_dir.display()
            );
        }
        if options.check {
            return run_check(&bundle, &options.target_dir, &manifest_path);
        }
        return run_upgrade(&bundle, &options.target_dir, &manifest_path, options.force);
    }

    // Fresh install path.
    if has_manifest {
        bail!(
            ".reen-bundle.yml already exists in {}. Use `reen init --upgrade` to refresh managed files.",
            options.target_dir.display()
        );
    }

    run_fresh_install(&bundle, &options.target_dir)
}

fn run_fresh_install(bundle: &Bundle, target: &Path) -> Result<i32> {
    // Refuse if any managed file already exists on disk.
    let mut existing_managed = Vec::new();
    for file in bundle.managed_files() {
        let dest = target.join(&file.path);
        if dest.exists() {
            existing_managed.push(file.path.clone());
        }
    }
    if !existing_managed.is_empty() {
        eprintln!("error: managed files already exist on disk but no .reen-bundle.yml is present.");
        eprintln!("This looks like a project with manually copied Reen files.");
        eprintln!("Run `reen init --upgrade` to adopt bundle management.\n");
        eprintln!("Existing managed files:");
        for path in &existing_managed {
            eprintln!("  {path}");
        }
        bail!("Cannot perform fresh install — managed files already exist. Run `reen init --upgrade` to adopt.");
    }

    let mut managed_count = 0;
    let mut seed_count = 0;
    let mut manifest_entries = Vec::new();

    for file in bundle.files() {
        let dest = target.join(&file.path);
        ensure_parent_dir(&dest)?;

        match file.classification {
            Classification::Managed => {
                write_file(&dest, file.contents)?;
                manifest_entries.push(ManagedFileEntry {
                    path: file.path.clone(),
                    sha256: file.sha256(),
                });
                managed_count += 1;
            }
            Classification::Seed => {
                if !dest.exists() {
                    write_file(&dest, file.contents)?;
                    seed_count += 1;
                }
            }
        }
    }

    // Merge .gitignore.fragment
    merge_gitignore(target)?;

    // Write .reen-bundle.yml
    write_bundle_manifest(target, bundle.reen_version(), &manifest_entries)?;

    println!(
        "Installed {managed_count} managed files, {seed_count} seed files, updated .gitignore."
    );
    Ok(0)
}

fn run_upgrade(bundle: &Bundle, target: &Path, manifest_path: &Path, force: bool) -> Result<i32> {
    let existing = read_bundle_manifest(manifest_path)?;

    // Build a map of stored managed files: path → sha256
    let stored_hashes: BTreeMap<String, String> = existing
        .managed_files
        .iter()
        .map(|e| (e.path.clone(), e.sha256.clone()))
        .collect();

    // Build a set of paths in the new bundle's managed files.
    let new_managed: BTreeMap<String, &BundleFile> = bundle
        .managed_files()
        .into_iter()
        .map(|f| (f.path.clone(), f))
        .collect();

    let mut written = 0;
    let mut added = 0;
    let mut removed = 0;
    let mut blocked: Vec<String> = Vec::new();
    let mut manifest_entries = Vec::new();

    // Process new bundle's managed files.
    for (path, bundle_file) in &new_managed {
        let dest = target.join(path);

        if dest.exists() {
            let disk_hash = sha256_file(&dest)?;
            if let Some(stored_hash) = stored_hashes.get(path) {
                if disk_hash == *stored_hash {
                    // User hasn't edited → overwrite from bundle.
                    write_file(&dest, bundle_file.contents)?;
                    written += 1;
                } else {
                    // User has edited this file.
                    if force {
                        eprintln!("  overwriting user-edited file: {path}");
                        write_file(&dest, bundle_file.contents)?;
                        written += 1;
                    } else {
                        blocked.push(path.clone());
                        // Keep the user's version in the manifest with their hash
                        // so the next upgrade can still track it.
                        manifest_entries.push(ManagedFileEntry {
                            path: path.clone(),
                            sha256: disk_hash,
                        });
                        continue;
                    }
                }
            } else {
                // File exists on disk but wasn't in the stored manifest.
                // This is a new managed file that happens to already exist.
                let disk_hash_val = sha256_file(&dest)?;
                if disk_hash_val != bundle_file.sha256() {
                    if force {
                        eprintln!("  overwriting existing file (new managed): {path}");
                        write_file(&dest, bundle_file.contents)?;
                        written += 1;
                    } else {
                        blocked.push(path.clone());
                        manifest_entries.push(ManagedFileEntry {
                            path: path.clone(),
                            sha256: disk_hash_val,
                        });
                        continue;
                    }
                } else {
                    // Already matches bundle content — just adopt it.
                    written += 1;
                }
            }
        } else {
            // Absent on disk → write from bundle (new managed file).
            ensure_parent_dir(&dest)?;
            write_file(&dest, bundle_file.contents)?;
            added += 1;
        }

        manifest_entries.push(ManagedFileEntry {
            path: path.clone(),
            sha256: bundle_file.sha256(),
        });
    }

    // Delete managed files present in stored manifest but absent from new bundle.
    for stored in &existing.managed_files {
        if !new_managed.contains_key(&stored.path) {
            let dest = target.join(&stored.path);
            if dest.exists() {
                std::fs::remove_file(&dest)
                    .with_context(|| format!("Failed to remove {}", dest.display()))?;
                removed += 1;
                eprintln!("  removed (no longer in bundle): {}", stored.path);
            }
        }
    }

    // Merge .gitignore.fragment (idempotent).
    merge_gitignore(target)?;

    // Sort manifest entries for determinism.
    manifest_entries.sort_by(|a, b| a.path.cmp(&b.path));

    // Rewrite .reen-bundle.yml.
    write_bundle_manifest(target, bundle.reen_version(), &manifest_entries)?;

    // Summary.
    let mut parts = Vec::new();
    if written > 0 {
        parts.push(format!("{written} updated"));
    }
    if added > 0 {
        parts.push(format!("{added} added"));
    }
    if removed > 0 {
        parts.push(format!("{removed} removed"));
    }
    if parts.is_empty() && blocked.is_empty() {
        println!("Already up to date.");
    } else if !parts.is_empty() {
        println!("Managed files: {}.", parts.join(", "));
    }

    if !blocked.is_empty() {
        eprintln!(
            "\n{} managed file(s) have been edited and were not overwritten:",
            blocked.len()
        );
        for path in &blocked {
            eprintln!("  {path}");
        }
        eprintln!("\nRun `reen init --upgrade --force` to overwrite them.");
        return Ok(1);
    }

    Ok(0)
}

fn run_check(bundle: &Bundle, target: &Path, manifest_path: &Path) -> Result<i32> {
    let existing = read_bundle_manifest(manifest_path)?;
    let stored_hashes: BTreeMap<String, String> = existing
        .managed_files
        .iter()
        .map(|e| (e.path.clone(), e.sha256.clone()))
        .collect();
    let new_managed: BTreeMap<String, &BundleFile> = bundle
        .managed_files()
        .into_iter()
        .map(|f| (f.path.clone(), f))
        .collect();

    let mut to_add: Vec<String> = Vec::new();
    let mut to_update: Vec<String> = Vec::new();
    let mut to_remove: Vec<String> = Vec::new();
    let mut blocked: Vec<String> = Vec::new();

    for (path, bundle_file) in &new_managed {
        let dest = target.join(path);
        if dest.exists() {
            let disk_hash = sha256_file(&dest)?;
            let bundle_hash = bundle_file.sha256();
            if disk_hash == bundle_hash {
                // Already matches — no action needed.
                continue;
            }
            if let Some(stored_hash) = stored_hashes.get(path) {
                if disk_hash == *stored_hash {
                    // Not edited by user → would update cleanly.
                    to_update.push(path.clone());
                } else {
                    // User has edited → would be blocked.
                    blocked.push(path.clone());
                }
            } else {
                // New managed file that already exists on disk with different content.
                blocked.push(path.clone());
            }
        } else {
            to_add.push(path.clone());
        }
    }

    for stored in &existing.managed_files {
        if !new_managed.contains_key(&stored.path) {
            to_remove.push(stored.path.clone());
        }
    }

    if to_add.is_empty() && to_update.is_empty() && to_remove.is_empty() && blocked.is_empty() {
        println!("Up to date.");
        return Ok(CHECK_UP_TO_DATE);
    }

    if !to_add.is_empty() {
        println!("Would add:");
        for p in &to_add {
            println!("  {p}");
        }
    }
    if !to_update.is_empty() {
        println!("Would update:");
        for p in &to_update {
            println!("  {p}");
        }
    }
    if !to_remove.is_empty() {
        println!("Would remove:");
        for p in &to_remove {
            println!("  {p}");
        }
    }
    if !blocked.is_empty() {
        println!("Blocked by user edits (use --force to override):");
        for p in &blocked {
            println!("  {p}");
        }
        return Ok(CHECK_BLOCKED_BY_EDITS);
    }

    Ok(CHECK_CHANGES_PENDING)
}

// --- Helpers ---

fn sha256_file(path: &Path) -> Result<String> {
    let contents = std::fs::read(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&contents);
    Ok(format!("{:x}", hasher.finalize()))
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }
    }
    Ok(())
}

fn write_file(path: &Path, contents: &[u8]) -> Result<()> {
    std::fs::write(path, contents)
        .with_context(|| format!("Failed to write {}", path.display()))
}

fn merge_gitignore(target: &Path) -> Result<()> {
    let fragment = crate::bundle::gitignore_fragment()?;
    let gitignore_path = target.join(".gitignore");

    let existing = if gitignore_path.is_file() {
        std::fs::read_to_string(&gitignore_path)
            .with_context(|| format!("Failed to read {}", gitignore_path.display()))?
    } else {
        String::new()
    };

    let existing_lines: std::collections::HashSet<&str> =
        existing.lines().collect();

    let mut to_append = Vec::new();
    for line in fragment.lines() {
        if !line.is_empty() && !existing_lines.contains(line) {
            to_append.push(line);
        }
    }

    if to_append.is_empty() {
        return Ok(());
    }

    let mut content = existing;
    // Ensure we start appending on a new line.
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    for line in &to_append {
        content.push_str(line);
        content.push('\n');
    }

    std::fs::write(&gitignore_path, content)
        .with_context(|| format!("Failed to write {}", gitignore_path.display()))
}

fn write_bundle_manifest(
    target: &Path,
    reen_version: &str,
    managed_files: &[ManagedFileEntry],
) -> Result<()> {
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let manifest = BundleManifest {
        schema: "reen.bundle/v1".to_string(),
        reen_version: reen_version.to_string(),
        installed_at: now,
        managed_files: managed_files.to_vec(),
    };

    let yaml = serde_yaml::to_string(&manifest).context("Failed to serialise bundle manifest")?;
    let path = target.join(".reen-bundle.yml");
    std::fs::write(&path, yaml)
        .with_context(|| format!("Failed to write {}", path.display()))
}

fn read_bundle_manifest(path: &Path) -> Result<BundleManifest> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    serde_yaml::from_str(&raw)
        .with_context(|| format!("Failed to parse {}", path.display()))
}
