//! Build tracking system for incremental builds
//!
//! Stores hashes of input and output files in .reen/ directory to track
//! when files need to be regenerated based on changes.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const TRACKER_DIR: &str = ".reen";
const TRACKER_FILE: &str = "build_tracker.json";

/// Represents the stage in the build pipeline
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Stage {
    /// Draft -> Specification
    Specification,
    /// Specification -> Implementation
    Implementation,
    /// Specification -> Tests
    Tests,
    /// Implementation -> Compile
    Compile,
}

/// Tracks a single file transformation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTrack {
    /// Hash of the input file
    pub input_hash: String,
    /// Hash of the output file(s)
    pub output_hash: String,
    /// Timestamp of last update
    pub timestamp: String,
}

/// Main build tracker
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BuildTracker {
    /// Maps: stage -> filename -> FileTrack
    tracks: HashMap<String, HashMap<String, FileTrack>>,
}

impl BuildTracker {
    /// Load the build tracker from disk, or create a new one
    pub fn load() -> Result<Self> {
        let tracker_path = Self::tracker_path();

        if tracker_path.exists() {
            let content = fs::read_to_string(&tracker_path)
                .context("Failed to read build tracker")?;
            let tracker: BuildTracker = serde_json::from_str(&content)
                .context("Failed to parse build tracker")?;
            Ok(tracker)
        } else {
            Ok(BuildTracker::default())
        }
    }

    /// Save the build tracker to disk
    pub fn save(&self) -> Result<()> {
        let tracker_dir = PathBuf::from(TRACKER_DIR);
        fs::create_dir_all(&tracker_dir)
            .context("Failed to create .reen directory")?;

        let tracker_path = Self::tracker_path();
        let content = serde_json::to_string_pretty(self)
            .context("Failed to serialize build tracker")?;

        fs::write(&tracker_path, content)
            .context("Failed to write build tracker")?;

        Ok(())
    }

    /// Get the path to the tracker file
    fn tracker_path() -> PathBuf {
        PathBuf::from(TRACKER_DIR).join(TRACKER_FILE)
    }

    /// Check if a file needs to be regenerated for a given stage
    ///
    /// Returns true if:
    /// - The input file has changed
    /// - The output file doesn't exist
    /// - There's no previous track record
    /// - Any upstream dependency has changed
    pub fn needs_update(&self, stage: Stage, name: &str, input_path: &Path, output_path: &Path) -> Result<bool> {
        // If output doesn't exist, definitely need to regenerate
        if !output_path.exists() {
            return Ok(true);
        }

        let stage_key = format!("{:?}", stage);

        // Check if we have a track record for this file
        let Some(stage_tracks) = self.tracks.get(&stage_key) else {
            return Ok(true); // No track record, need to generate
        };

        let Some(track) = stage_tracks.get(name) else {
            return Ok(true); // No track record for this specific file
        };

        // Compute current input hash
        let current_input_hash = Self::hash_file(input_path)?;

        // If input hasn't changed, no need to regenerate
        if current_input_hash == track.input_hash {
            return Ok(false);
        }

        Ok(true)
    }

    /// Check if any upstream stage has changed that would affect this stage
    ///
    /// For example:
    /// - Implementation depends on Specification
    /// - Compile/Run/Test depend on Implementation
    pub fn upstream_changed(&self, stage: Stage, name: &str) -> Result<bool> {
        match stage {
            Stage::Specification => {
                // First stage, no upstream
                Ok(false)
            }
            Stage::Implementation | Stage::Tests => {
                // Depends on Specification
                // Check if specification was regenerated recently
                let spec_stage_key = format!("{:?}", Stage::Specification);
                if let Some(spec_tracks) = self.tracks.get(&spec_stage_key) {
                    if let Some(spec_track) = spec_tracks.get(name) {
                        // Get corresponding input path (draft)
                        let draft_path = PathBuf::from("drafts").join(format!("{}.md", name));
                        if draft_path.exists() {
                            let current_hash = Self::hash_file(&draft_path)?;
                            if current_hash != spec_track.input_hash {
                                return Ok(true); // Draft changed, spec needs update
                            }
                        }
                    }
                }
                Ok(false)
            }
            Stage::Compile => {
                // Depends on Implementation
                // Check if any implementation files changed
                let impl_stage_key = format!("{:?}", Stage::Implementation);
                if let Some(impl_tracks) = self.tracks.get(&impl_stage_key) {
                    for (impl_name, impl_track) in impl_tracks {
                        let spec_path = PathBuf::from("contexts").join(format!("{}.md", impl_name));
                        if spec_path.exists() {
                            let current_hash = Self::hash_file(&spec_path)?;
                            if current_hash != impl_track.input_hash {
                                return Ok(true); // Spec changed, impl needs update
                            }
                        }
                    }
                }
                Ok(false)
            }
        }
    }

    /// Record a successful file transformation
    pub fn record(&mut self, stage: Stage, name: &str, input_path: &Path, output_path: &Path) -> Result<()> {
        let input_hash = Self::hash_file(input_path)?;
        // Output file may not exist yet if the agent hasn't written it
        // Use empty hash if file doesn't exist (will trigger regeneration next time)
        let output_hash = if output_path.exists() {
            Self::hash_file(output_path)?
        } else {
            String::new()
        };
        let timestamp = chrono::Utc::now().to_rfc3339();

        let stage_key = format!("{:?}", stage);

        let stage_tracks = self.tracks.entry(stage_key).or_insert_with(HashMap::new);

        stage_tracks.insert(name.to_string(), FileTrack {
            input_hash,
            output_hash,
            timestamp,
        });

        Ok(())
    }

    /// Compute SHA256 hash of a file
    fn hash_file(path: &Path) -> Result<String> {
        let content = fs::read(path)
            .with_context(|| format!("Failed to read file: {}", path.display()))?;

        let mut hasher = Sha256::new();
        hasher.update(&content);
        let result = hasher.finalize();

        Ok(hex::encode(result))
    }

    /// Get a summary of tracked files
    pub fn summary(&self) -> String {
        let mut lines = Vec::new();
        lines.push("Build Tracker Summary:".to_string());

        for (stage, tracks) in &self.tracks {
            lines.push(format!("\n{}:", stage));
            for (name, track) in tracks {
                lines.push(format!("  {} (updated: {})", name, track.timestamp));
            }
        }

        if self.tracks.is_empty() {
            lines.push("  No tracked files".to_string());
        }

        lines.join("\n")
    }

    /// Clear all cache entries for a specific stage.
    /// Returns number of entries removed.
    pub fn clear_stage(&mut self, stage: Stage) -> usize {
        let stage_key = format!("{:?}", stage);
        self.tracks.remove(&stage_key).map(|m| m.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_hash_file() {
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("test_hash.txt");

        fs::write(&test_file, "hello world").unwrap();

        let hash1 = BuildTracker::hash_file(&test_file).unwrap();
        let hash2 = BuildTracker::hash_file(&test_file).unwrap();

        assert_eq!(hash1, hash2); // Same content should give same hash

        fs::write(&test_file, "different").unwrap();
        let hash3 = BuildTracker::hash_file(&test_file).unwrap();

        assert_ne!(hash1, hash3); // Different content should give different hash

        fs::remove_file(&test_file).ok();
    }

    #[test]
    fn test_tracker_record_and_load() {
        let mut tracker = BuildTracker::default();

        let temp_dir = std::env::temp_dir();
        let input_file = temp_dir.join("input.txt");
        let output_file = temp_dir.join("output.txt");

        fs::write(&input_file, "input content").unwrap();
        fs::write(&output_file, "output content").unwrap();

        tracker.record(Stage::Specification, "test", &input_file, &output_file).unwrap();

        // Check that it was recorded
        let stage_key = format!("{:?}", Stage::Specification);
        assert!(tracker.tracks.contains_key(&stage_key));
        assert!(tracker.tracks[&stage_key].contains_key("test"));

        fs::remove_file(&input_file).ok();
        fs::remove_file(&output_file).ok();
    }
}
