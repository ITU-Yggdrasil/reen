use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BuildTracker {
    entries: BTreeMap<String, TrackEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TrackEntry {
    stage: String,
    input_hash: String,
}

impl BuildTracker {
    pub fn load(workspace_root: &Path) -> Result<Self> {
        let path = tracker_path(workspace_root);
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        serde_json::from_str(&content).with_context(|| format!("Failed to parse {}", path.display()))
    }

    pub fn save(&self, workspace_root: &Path) -> Result<()> {
        let path = tracker_path(workspace_root);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        let content = serde_json::to_string_pretty(self)?;
        fs::write(&path, content).with_context(|| format!("Failed to write {}", path.display()))
    }

    pub fn is_current(&self, stage: &str, key: &str, input_hash: &str) -> bool {
        self.entries
            .get(key)
            .is_some_and(|entry| entry.stage == stage && entry.input_hash == input_hash)
    }

    pub fn update(&mut self, stage: &str, key: impl Into<String>, input_hash: impl Into<String>) {
        self.entries.insert(
            key.into(),
            TrackEntry {
                stage: stage.to_string(),
                input_hash: input_hash.into(),
            },
        );
    }

    pub fn clear_stage(&mut self, stage: &str) {
        self.entries.retain(|_, entry| entry.stage != stage);
    }
}

pub fn hash_string(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn hash_file(path: &Path) -> Result<String> {
    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(hash_string(&content))
}

fn tracker_path(workspace_root: &Path) -> std::path::PathBuf {
    workspace_root.join(".reen").join("build_tracker.json")
}
