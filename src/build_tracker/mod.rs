//! Incremental build tracker for the reen pipeline.
//!
//! Tracks method-level provenance via `.reen/build.json`. Each artefact
//! (functionality, role method, data operation, projection query, role type,
//! data type) has a state: `not_implemented`, `fresh`, or `stale`.
//!
//! The tracker is populated during `reen prepare` and consumed by scaffold,
//! implement, verify, and audit.

pub mod edges;
pub mod keys;

use keys::{ArtefactEntryKind, ArtefactKey};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

/// Current schema version. The tracker refuses to load files with a different version.
pub const SCHEMA_VERSION: u32 = 1;

/// The tracker state for a single artefact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtefactState {
    NotImplemented,
    Fresh,
    Stale,
}

/// The reason an artefact entered its current state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StaleReason {
    DraftChanged,
    UpstreamChanged,
    AuditFinding,
    InitialScaffold,
}

/// A single artefact entry in the tracker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtefactEntry {
    pub kind: ArtefactEntryKind,
    pub prepared_hash: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub upstream: Vec<ArtefactKey>,
    pub state: ArtefactState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<StaleReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_finding: Option<String>,
}

/// The top-level build tracker, serialized as `.reen/build.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildTracker {
    pub schema_version: u32,
    /// Draft path (relative from repo root) → SHA-256 of raw bytes.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub drafts: BTreeMap<String, String>,
    /// Canonical artefact key → entry.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub artefacts: BTreeMap<ArtefactKey, ArtefactEntry>,
}

impl Default for BuildTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl BuildTracker {
    pub fn new() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            drafts: BTreeMap::new(),
            artefacts: BTreeMap::new(),
        }
    }

    // ── I/O ──────────────────────────────────────────────────────

    /// Load the tracker from `.reen/build.json`. Returns a fresh tracker if
    /// the file does not exist.
    pub fn load(state_dir: &Path) -> anyhow::Result<Self> {
        let path = state_dir.join("build.json");
        if !path.is_file() {
            return Ok(Self::new());
        }
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("failed to read {}: {}", path.display(), e))?;

        // Check schema version before full deserialization
        let version_check: SchemaVersionCheck = serde_json::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("failed to parse {}: {}", path.display(), e))?;
        if version_check.schema_version != SCHEMA_VERSION {
            anyhow::bail!(
                "build tracker schema version mismatch: expected {}, found {} in {}. \
                 Run `reen clean` and re-prepare.",
                SCHEMA_VERSION,
                version_check.schema_version,
                path.display()
            );
        }

        let tracker: Self = serde_json::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("failed to parse {}: {}", path.display(), e))?;
        Ok(tracker)
    }

    /// Persist the tracker to `.reen/build.json` atomically (write temp + rename).
    pub fn save(&self, state_dir: &Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(state_dir)
            .map_err(|e| anyhow::anyhow!("failed to create {}: {}", state_dir.display(), e))?;

        let json = serde_json::to_string_pretty(self)
            .map_err(|e| anyhow::anyhow!("failed to serialize tracker: {}", e))?;

        let final_path = state_dir.join("build.json");
        let tmp_path = state_dir.join("build.json.tmp");

        std::fs::write(&tmp_path, &json)
            .map_err(|e| anyhow::anyhow!("failed to write {}: {}", tmp_path.display(), e))?;
        std::fs::rename(&tmp_path, &final_path)
            .map_err(|e| anyhow::anyhow!("failed to rename to {}: {}", final_path.display(), e))?;

        Ok(())
    }

    // ── Mutation API ─────────────────────────────────────────────

    /// Insert or update an artefact entry. If the key already exists, the
    /// entry is replaced.
    pub fn upsert_artefact(&mut self, key: ArtefactKey, entry: ArtefactEntry) {
        self.artefacts.insert(key, entry);
    }

    /// Mark an artefact as stale with the given reason.
    pub fn mark_stale(&mut self, key: &ArtefactKey, reason: StaleReason) {
        if let Some(entry) = self.artefacts.get_mut(key) {
            entry.state = ArtefactState::Stale;
            entry.reason = Some(reason);
        }
    }

    /// Mark an artefact as stale due to an audit finding, attaching the
    /// finding text.
    pub fn mark_stale_audit(&mut self, key: &ArtefactKey, finding_text: String) {
        if let Some(entry) = self.artefacts.get_mut(key) {
            entry.state = ArtefactState::Stale;
            entry.reason = Some(StaleReason::AuditFinding);
            entry.audit_finding = Some(finding_text);
        }
    }

    /// Mark an artefact as fresh (e.g. after verify passes).
    pub fn mark_fresh(&mut self, key: &ArtefactKey) {
        if let Some(entry) = self.artefacts.get_mut(key) {
            entry.state = ArtefactState::Fresh;
            entry.reason = None;
            entry.audit_finding = None;
        }
    }

    /// Remove an artefact from the tracker (e.g. when it no longer exists
    /// in the prepared YAML).
    pub fn remove_artefact(&mut self, key: &ArtefactKey) {
        self.artefacts.remove(key);
    }

    /// Transitively cascade staleness from a set of root keys through the
    /// dependency graph. Roots should already be marked stale with their own
    /// reason; descendants get `UpstreamChanged`.
    pub fn cascade_stale(&mut self, root_keys: &[ArtefactKey]) {
        // Build a reverse-dependency map: for each upstream key, collect the
        // downstream keys that depend on it.
        let mut downstream: BTreeMap<ArtefactKey, Vec<ArtefactKey>> = BTreeMap::new();
        for (key, entry) in &self.artefacts {
            for up in &entry.upstream {
                downstream.entry(up.clone()).or_default().push(key.clone());
            }
        }

        // BFS from roots, collecting keys to mark stale
        let mut queue: VecDeque<ArtefactKey> = root_keys.iter().cloned().collect();
        let mut visited: BTreeSet<ArtefactKey> = root_keys.iter().cloned().collect();
        let mut to_mark: Vec<ArtefactKey> = Vec::new();

        while let Some(current) = queue.pop_front() {
            if let Some(deps) = downstream.get(&current) {
                for dep in deps {
                    if visited.insert(dep.clone()) {
                        to_mark.push(dep.clone());
                        queue.push_back(dep.clone());
                    }
                }
            }
        }

        // Apply staleness
        for key in to_mark {
            if let Some(entry) = self.artefacts.get_mut(&key) {
                if entry.state != ArtefactState::Stale {
                    entry.state = ArtefactState::Stale;
                    entry.reason = Some(StaleReason::UpstreamChanged);
                }
            }
        }
    }

    // ── Queries ──────────────────────────────────────────────────

    /// Count artefacts in each state.
    pub fn state_counts(&self) -> StateCounts {
        let mut counts = StateCounts::default();
        for entry in self.artefacts.values() {
            match entry.state {
                ArtefactState::NotImplemented => counts.not_implemented += 1,
                ArtefactState::Fresh => counts.fresh += 1,
                ArtefactState::Stale => counts.stale += 1,
            }
        }
        counts
    }

    /// Return all artefact keys with the given state.
    pub fn keys_in_state(&self, state: ArtefactState) -> Vec<&ArtefactKey> {
        self.artefacts
            .iter()
            .filter(|(_, e)| e.state == state)
            .map(|(k, _)| k)
            .collect()
    }

    /// Count stale artefacts by reason.
    pub fn stale_summary(&self) -> StaleSummary {
        let mut summary = StaleSummary::default();
        for entry in self.artefacts.values() {
            if entry.state == ArtefactState::Stale {
                match entry.reason {
                    Some(StaleReason::DraftChanged) => summary.direct += 1,
                    Some(StaleReason::UpstreamChanged) => summary.cascaded += 1,
                    Some(StaleReason::AuditFinding) => summary.audit += 1,
                    Some(StaleReason::InitialScaffold) => summary.initial += 1,
                    None => summary.direct += 1,
                }
            }
        }
        summary
    }
}

#[derive(Debug, Default)]
pub struct StateCounts {
    pub not_implemented: usize,
    pub fresh: usize,
    pub stale: usize,
}

#[derive(Debug, Default)]
pub struct StaleSummary {
    pub direct: usize,
    pub cascaded: usize,
    pub audit: usize,
    pub initial: usize,
}

impl StaleSummary {
    pub fn total(&self) -> usize {
        self.direct + self.cascaded + self.audit + self.initial
    }
}

/// Used to check schema version before full deserialization.
#[derive(Deserialize)]
struct SchemaVersionCheck {
    schema_version: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tracker() -> BuildTracker {
        let mut t = BuildTracker::new();
        t.drafts.insert(
            "drafts/contexts/money_transfer.md".to_string(),
            "abc123".to_string(),
        );

        let role_type_key = ArtefactKey::role_type("MoneyTransfer", "source");
        let role_method_key = ArtefactKey::role_method("MoneyTransfer", "source", "withdraw");
        let func_key = ArtefactKey::functionality("MoneyTransfer", "transfer");
        let data_type_key = ArtefactKey::data_type("Account");

        t.upsert_artefact(
            role_type_key.clone(),
            ArtefactEntry {
                kind: ArtefactEntryKind::RoleType,
                prepared_hash: "hash_role_type".into(),
                upstream: vec![],
                state: ArtefactState::Fresh,
                reason: None,
                audit_finding: None,
            },
        );

        t.upsert_artefact(
            role_method_key.clone(),
            ArtefactEntry {
                kind: ArtefactEntryKind::RoleMethod,
                prepared_hash: "hash_role_method".into(),
                upstream: vec![role_type_key.clone()],
                state: ArtefactState::Fresh,
                reason: None,
                audit_finding: None,
            },
        );

        t.upsert_artefact(
            func_key.clone(),
            ArtefactEntry {
                kind: ArtefactEntryKind::Functionality,
                prepared_hash: "hash_func".into(),
                upstream: vec![role_method_key.clone()],
                state: ArtefactState::Fresh,
                reason: None,
                audit_finding: None,
            },
        );

        t.upsert_artefact(
            data_type_key.clone(),
            ArtefactEntry {
                kind: ArtefactEntryKind::DataType,
                prepared_hash: "hash_data_type".into(),
                upstream: vec![],
                state: ArtefactState::Fresh,
                reason: None,
                audit_finding: None,
            },
        );

        t
    }

    #[test]
    fn serde_round_trip() {
        let tracker = sample_tracker();
        let json = serde_json::to_string_pretty(&tracker).unwrap();
        let parsed: BuildTracker = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.schema_version, SCHEMA_VERSION);
        assert_eq!(parsed.drafts.len(), tracker.drafts.len());
        assert_eq!(parsed.artefacts.len(), tracker.artefacts.len());

        for (key, entry) in &tracker.artefacts {
            let parsed_entry = parsed.artefacts.get(key).unwrap();
            assert_eq!(parsed_entry.kind, entry.kind);
            assert_eq!(parsed_entry.prepared_hash, entry.prepared_hash);
            assert_eq!(parsed_entry.upstream, entry.upstream);
            assert_eq!(parsed_entry.state, entry.state);
            assert_eq!(parsed_entry.reason, entry.reason);
        }
    }

    #[test]
    fn load_save_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let tracker = sample_tracker();
        tracker.save(dir.path()).unwrap();

        let loaded = BuildTracker::load(dir.path()).unwrap();
        assert_eq!(loaded.schema_version, tracker.schema_version);
        assert_eq!(loaded.drafts, tracker.drafts);
        assert_eq!(loaded.artefacts.len(), tracker.artefacts.len());
    }

    #[test]
    fn load_nonexistent_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let tracker = BuildTracker::load(dir.path()).unwrap();
        assert_eq!(tracker.schema_version, SCHEMA_VERSION);
        assert!(tracker.drafts.is_empty());
        assert!(tracker.artefacts.is_empty());
    }

    #[test]
    fn schema_version_mismatch_errors() {
        let dir = tempfile::tempdir().unwrap();
        let json = r#"{"schema_version": 999, "drafts": {}, "artefacts": {}}"#;
        std::fs::write(dir.path().join("build.json"), json).unwrap();

        let err = BuildTracker::load(dir.path()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("schema version mismatch"), "got: {}", msg);
        assert!(msg.contains("999"), "got: {}", msg);
    }

    #[test]
    fn schema_version_future_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let json = r#"{"schema_version": 0, "drafts": {}, "artefacts": {}}"#;
        std::fs::write(dir.path().join("build.json"), json).unwrap();

        let err = BuildTracker::load(dir.path()).unwrap_err();
        assert!(err.to_string().contains("schema version mismatch"));
    }

    #[test]
    fn mark_stale_and_fresh() {
        let mut tracker = sample_tracker();
        let key = ArtefactKey::functionality("MoneyTransfer", "transfer");

        assert_eq!(tracker.artefacts[&key].state, ArtefactState::Fresh);

        tracker.mark_stale(&key, StaleReason::DraftChanged);
        assert_eq!(tracker.artefacts[&key].state, ArtefactState::Stale);
        assert_eq!(tracker.artefacts[&key].reason, Some(StaleReason::DraftChanged));

        tracker.mark_fresh(&key);
        assert_eq!(tracker.artefacts[&key].state, ArtefactState::Fresh);
        assert_eq!(tracker.artefacts[&key].reason, None);
    }

    #[test]
    fn mark_stale_audit() {
        let mut tracker = sample_tracker();
        let key = ArtefactKey::functionality("MoneyTransfer", "transfer");

        tracker.mark_stale_audit(&key, "invariant not enforced".into());
        assert_eq!(tracker.artefacts[&key].state, ArtefactState::Stale);
        assert_eq!(tracker.artefacts[&key].reason, Some(StaleReason::AuditFinding));
        assert_eq!(
            tracker.artefacts[&key].audit_finding.as_deref(),
            Some("invariant not enforced")
        );

        // mark_fresh clears the audit finding
        tracker.mark_fresh(&key);
        assert_eq!(tracker.artefacts[&key].audit_finding, None);
    }

    #[test]
    fn cascade_stale_propagates() {
        let mut tracker = sample_tracker();

        // Mark the role type as stale — should cascade to role_method → functionality
        let role_type_key = ArtefactKey::role_type("MoneyTransfer", "source");
        tracker.mark_stale(&role_type_key, StaleReason::DraftChanged);
        tracker.cascade_stale(&[role_type_key.clone()]);

        let role_method_key = ArtefactKey::role_method("MoneyTransfer", "source", "withdraw");
        let func_key = ArtefactKey::functionality("MoneyTransfer", "transfer");
        let data_type_key = ArtefactKey::data_type("Account");

        assert_eq!(tracker.artefacts[&role_type_key].state, ArtefactState::Stale);
        assert_eq!(
            tracker.artefacts[&role_type_key].reason,
            Some(StaleReason::DraftChanged)
        );

        assert_eq!(tracker.artefacts[&role_method_key].state, ArtefactState::Stale);
        assert_eq!(
            tracker.artefacts[&role_method_key].reason,
            Some(StaleReason::UpstreamChanged)
        );

        assert_eq!(tracker.artefacts[&func_key].state, ArtefactState::Stale);
        assert_eq!(
            tracker.artefacts[&func_key].reason,
            Some(StaleReason::UpstreamChanged)
        );

        // data_type should be unaffected
        assert_eq!(tracker.artefacts[&data_type_key].state, ArtefactState::Fresh);
    }

    #[test]
    fn cascade_does_not_re_mark_already_stale() {
        let mut tracker = sample_tracker();

        // Pre-mark role_method as stale with a specific reason
        let role_method_key = ArtefactKey::role_method("MoneyTransfer", "source", "withdraw");
        tracker.mark_stale(&role_method_key, StaleReason::DraftChanged);

        // Now cascade from role_type — role_method should keep its existing stale reason
        let role_type_key = ArtefactKey::role_type("MoneyTransfer", "source");
        tracker.mark_stale(&role_type_key, StaleReason::DraftChanged);
        tracker.cascade_stale(&[role_type_key]);

        assert_eq!(
            tracker.artefacts[&role_method_key].reason,
            Some(StaleReason::DraftChanged),
            "should keep original stale reason"
        );
    }

    #[test]
    fn remove_artefact() {
        let mut tracker = sample_tracker();
        let key = ArtefactKey::data_type("Account");
        assert!(tracker.artefacts.contains_key(&key));
        tracker.remove_artefact(&key);
        assert!(!tracker.artefacts.contains_key(&key));
    }

    #[test]
    fn state_counts() {
        let mut tracker = sample_tracker();
        let counts = tracker.state_counts();
        assert_eq!(counts.fresh, 4);
        assert_eq!(counts.stale, 0);
        assert_eq!(counts.not_implemented, 0);

        tracker.mark_stale(
            &ArtefactKey::functionality("MoneyTransfer", "transfer"),
            StaleReason::DraftChanged,
        );
        let counts = tracker.state_counts();
        assert_eq!(counts.fresh, 3);
        assert_eq!(counts.stale, 1);
    }

    #[test]
    fn keys_in_state() {
        let mut tracker = sample_tracker();
        let key = ArtefactKey::functionality("MoneyTransfer", "transfer");
        tracker.mark_stale(&key, StaleReason::DraftChanged);

        let stale_keys = tracker.keys_in_state(ArtefactState::Stale);
        assert_eq!(stale_keys.len(), 1);
        assert_eq!(stale_keys[0], &key);
    }

    #[test]
    fn stale_summary() {
        let mut tracker = sample_tracker();
        let k1 = ArtefactKey::functionality("MoneyTransfer", "transfer");
        let k2 = ArtefactKey::role_method("MoneyTransfer", "source", "withdraw");

        tracker.mark_stale(&k1, StaleReason::DraftChanged);
        tracker.mark_stale(&k2, StaleReason::UpstreamChanged);

        let summary = tracker.stale_summary();
        assert_eq!(summary.direct, 1);
        assert_eq!(summary.cascaded, 1);
        assert_eq!(summary.total(), 2);
    }

    #[test]
    fn atomic_write_no_partial_state() {
        let dir = tempfile::tempdir().unwrap();
        let tracker = sample_tracker();
        tracker.save(dir.path()).unwrap();

        // Verify the temp file is cleaned up
        assert!(!dir.path().join("build.json.tmp").exists());
        assert!(dir.path().join("build.json").exists());
    }

    #[test]
    fn artefact_keys_deterministic_in_json() {
        let tracker = sample_tracker();
        let json1 = serde_json::to_string_pretty(&tracker).unwrap();
        let json2 = serde_json::to_string_pretty(&tracker).unwrap();

        // BTreeMap guarantees deterministic key order across serializations
        assert_eq!(json1, json2, "serialization should be deterministic");

        // All keys should appear in the output
        assert!(json1.contains("contexts/money_transfer/functionality/transfer"));
        assert!(json1.contains("contexts/money_transfer/role/source/type"));
        assert!(json1.contains("contexts/money_transfer/role/source/withdraw"));
        assert!(json1.contains("data/account/type"));
    }

    #[test]
    fn not_implemented_entry() {
        let mut tracker = BuildTracker::new();
        let key = ArtefactKey::functionality("Game", "play");
        tracker.upsert_artefact(
            key.clone(),
            ArtefactEntry {
                kind: ArtefactEntryKind::Functionality,
                prepared_hash: "hash1".into(),
                upstream: vec![],
                state: ArtefactState::NotImplemented,
                reason: Some(StaleReason::InitialScaffold),
                audit_finding: None,
            },
        );

        let json = serde_json::to_string_pretty(&tracker).unwrap();
        let parsed: BuildTracker = serde_json::from_str(&json).unwrap();
        assert_eq!(
            parsed.artefacts[&key].state,
            ArtefactState::NotImplemented
        );
        assert_eq!(
            parsed.artefacts[&key].reason,
            Some(StaleReason::InitialScaffold)
        );
    }
}
