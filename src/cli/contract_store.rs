use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(crate) struct SemanticContract {
    pub(crate) kind: String,
    pub(crate) title: String,
    pub(crate) summary: Option<Value>,
    pub(crate) behavior_contract: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ResolvedType {
    pub(crate) semantic_type: String,
    pub(crate) rust_type: String,
    pub(crate) source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct NameBinding {
    pub(crate) semantic_name: String,
    pub(crate) rust_identifier: String,
    pub(crate) export_name: String,
    pub(crate) reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct InterfaceParameter {
    pub(crate) semantic_name: String,
    pub(crate) rust_name: String,
    pub(crate) type_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct InterfaceField {
    pub(crate) semantic_name: String,
    pub(crate) rust_name: String,
    pub(crate) export_name: String,
    pub(crate) type_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct InterfaceMethod {
    pub(crate) semantic_name: String,
    pub(crate) rust_name: String,
    pub(crate) export_name: String,
    pub(crate) receiver: String,
    pub(crate) parameters: Vec<InterfaceParameter>,
    pub(crate) return_type: String,
    pub(crate) failure_shape: String,
    pub(crate) signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct InterfaceType {
    pub(crate) semantic_name: String,
    pub(crate) rust_name: String,
    pub(crate) export_name: String,
    pub(crate) kind: String,
    pub(crate) fields: Vec<InterfaceField>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(crate) struct ResolvedInterface {
    pub(crate) version: String,
    pub(crate) interface_fingerprint: String,
    pub(crate) primary_export_name: String,
    pub(crate) artifact_kind: String,
    pub(crate) exported_types: Vec<InterfaceType>,
    pub(crate) exported_methods: Vec<InterfaceMethod>,
    /// Dependency-facing role API surface (e.g. `stdin_source.read_available`).
    #[serde(default)]
    pub(crate) role_method_exports: Vec<InterfaceMethod>,
    pub(crate) name_bindings: Vec<NameBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(crate) struct InterfaceIr {
    pub(crate) version: String,
    pub(crate) draft_identity: String,
    pub(crate) draft_relative_path: String,
    pub(crate) specification_kind: String,
    pub(crate) artifact_kind: String,
    pub(crate) interface_fingerprint: String,
    pub(crate) primary_export_name: String,
    pub(crate) exported_types: Vec<InterfaceType>,
    pub(crate) exported_methods: Vec<InterfaceMethod>,
    #[serde(default)]
    pub(crate) role_method_exports: Vec<InterfaceMethod>,
    #[serde(default)]
    pub(crate) name_bindings: Vec<NameBinding>,
    #[serde(default)]
    pub(crate) dependency_bindings: Vec<DependencyBinding>,
    #[serde(default)]
    pub(crate) resolved_types: Vec<ResolvedType>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct UpstreamInterfaceRef {
    pub(crate) path: String,
    pub(crate) source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct DependencyMethodBinding {
    pub(crate) role_method: String,
    pub(crate) upstream_method: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct DependencyBinding {
    pub(crate) semantic_dependency: String,
    pub(crate) rust_dependency: String,
    pub(crate) spec_path: String,
    pub(crate) interface_name: String,
    pub(crate) method_bindings: Vec<DependencyMethodBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct AmbiguityEntry {
    pub(crate) class: String,
    pub(crate) subject: String,
    pub(crate) detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct DecisionSource {
    pub(crate) subject: String,
    pub(crate) source_kind: String,
    pub(crate) detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct ContractBundle {
    pub(crate) draft_identity: String,
    pub(crate) draft_relative_path: String,
    pub(crate) draft_fingerprint: String,
    pub(crate) draft_summary: Option<Value>,
    pub(crate) behavior_contract: Value,
    pub(crate) contract_artifact: Value,
    pub(crate) implementation_plan: Value,
    pub(crate) plan_validation: Value,
    pub(crate) target_output_hints: Vec<String>,
    #[serde(default)]
    pub(crate) semantic_contract: SemanticContract,
    #[serde(default)]
    pub(crate) resolved_interface: ResolvedInterface,
    #[serde(default)]
    pub(crate) type_decisions: Vec<ResolvedType>,
    #[serde(default)]
    pub(crate) name_bindings: Vec<NameBinding>,
    #[serde(default)]
    pub(crate) dependency_bindings: Vec<DependencyBinding>,
    #[serde(default)]
    pub(crate) ambiguity_report: Vec<AmbiguityEntry>,
    #[serde(default)]
    pub(crate) decision_sources: Vec<DecisionSource>,
    pub(crate) required_upstream_interface_references: Vec<UpstreamInterfaceRef>,
    pub(crate) blocking_diagnostics: Vec<String>,
    pub(crate) unresolved_assumptions: Vec<String>,
    pub(crate) contract_markdown: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct LevelPolicy {
    pub(crate) stage: String,
    pub(crate) level_hash: String,
    pub(crate) artifact_paths: Vec<String>,
    pub(crate) canonical_names: Vec<String>,
    pub(crate) import_roots: Vec<String>,
    pub(crate) feature_names: Vec<String>,
    pub(crate) shared_type_choices: Vec<String>,
    pub(crate) collaborator_abstractions: Vec<String>,
    pub(crate) conflict_resolutions: Vec<String>,
    #[serde(default)]
    pub(crate) name_bindings: Vec<NameBinding>,
    #[serde(default)]
    pub(crate) container_shapes: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ContractStore {
    root: PathBuf,
}

impl ContractStore {
    pub(crate) fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub(crate) fn contracts_root(&self) -> PathBuf {
        self.root.join("contracts")
    }

    pub(crate) fn interfaces_root(&self) -> PathBuf {
        self.root.join("interfaces")
    }

    pub(crate) fn plans_root(&self) -> PathBuf {
        self.root.join("plans")
    }

    pub(crate) fn debug_root(&self) -> PathBuf {
        self.root.join("debug")
    }

    pub(crate) fn coordination_root(&self) -> PathBuf {
        self.root.join("coordination")
    }

    pub(crate) fn contract_bundle_path(&self, draft_relative_path: &Path) -> PathBuf {
        self.contracts_root()
            .join(draft_relative_path)
            .with_extension("contract.json")
    }

    pub(crate) fn implementation_plan_path(&self, draft_relative_path: &Path) -> PathBuf {
        self.plans_root()
            .join(draft_relative_path)
            .with_extension("implementation_plan.json")
    }

    pub(crate) fn interface_ir_path(&self, draft_relative_path: &Path) -> PathBuf {
        self.interfaces_root()
            .join(draft_relative_path)
            .with_extension("interface.json")
    }

    pub(crate) fn debug_bundle_path(&self, draft_relative_path: &Path) -> PathBuf {
        self.debug_root()
            .join("contracts")
            .join(draft_relative_path)
            .with_extension("contract.json")
    }

    pub(crate) fn debug_plan_path(&self, draft_relative_path: &Path) -> PathBuf {
        self.debug_root()
            .join("plans")
            .join(draft_relative_path)
            .with_extension("implementation_plan.json")
    }

    pub(crate) fn hidden_spec_path(&self, draft_relative_path: &Path) -> PathBuf {
        self.debug_root()
            .join("specifications")
            .join(draft_relative_path)
    }

    pub(crate) fn level_policy_path(&self, stage: &str, level_hash: &str) -> PathBuf {
        self.coordination_root()
            .join(stage)
            .join(format!("{level_hash}.json"))
    }

    pub(crate) fn write_debug_bundle(
        &self,
        draft_relative_path: &Path,
        bundle: &ContractBundle,
    ) -> Result<PathBuf> {
        let bundle_path = self.debug_bundle_path(draft_relative_path);
        self.write_json(&bundle_path, bundle)?;
        let plan_path = self.debug_plan_path(draft_relative_path);
        self.write_json(&plan_path, &bundle.implementation_plan)?;
        let hidden_spec_path = self.hidden_spec_path(draft_relative_path);
        self.write_text(&hidden_spec_path, &bundle.contract_markdown)?;
        Ok(bundle_path)
    }

    pub(crate) fn write_interface_ir(
        &self,
        draft_relative_path: &Path,
        interface_ir: &InterfaceIr,
    ) -> Result<PathBuf> {
        let path = self.interface_ir_path(draft_relative_path);
        self.write_json(&path, interface_ir)?;
        Ok(path)
    }

    pub(crate) fn read_interface_ir(&self, draft_relative_path: &Path) -> Result<InterfaceIr> {
        let path = self.interface_ir_path(draft_relative_path);
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse {}", path.display()))
    }

    pub(crate) fn write_level_policy(&self, policy: &LevelPolicy) -> Result<PathBuf> {
        let path = self.level_policy_path(&policy.stage, &policy.level_hash);
        self.write_json(&path, policy)?;
        Ok(path)
    }

    pub(crate) fn read_level_policy(&self, stage: &str, level_hash: &str) -> Result<LevelPolicy> {
        let path = self.level_policy_path(stage, level_hash);
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse {}", path.display()))
    }

    pub(crate) fn write_text(&self, path: &Path, content: &str) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        fs::write(path, content).with_context(|| format!("Failed to write {}", path.display()))
    }

    pub(crate) fn write_json<T: Serialize>(&self, path: &Path, value: &T) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        let content = serde_json::to_string_pretty(value)
            .with_context(|| format!("Failed to serialize {}", path.display()))?;
        fs::write(path, content).with_context(|| format!("Failed to write {}", path.display()))
    }
}

pub(crate) fn draft_relative_path(draft_file: &Path, drafts_root: &Path) -> Result<PathBuf> {
    draft_file
        .strip_prefix(drafts_root)
        .map(|path| path.to_path_buf())
        .with_context(|| {
            format!(
                "Failed to determine draft-relative path for {} from {}",
                draft_file.display(),
                drafts_root.display()
            )
        })
}

pub(crate) fn level_hash(paths: &[PathBuf]) -> String {
    let mut ordered = paths
        .iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    ordered.sort();
    let mut hasher = Sha256::new();
    for path in ordered {
        hasher.update(path.as_bytes());
        hasher.update([0]);
    }
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::{
        ContractBundle, ContractStore, LevelPolicy, NameBinding, ResolvedInterface,
        SemanticContract, draft_relative_path, level_hash,
    };
    use serde_json::json;
    use std::path::{Path, PathBuf};

    #[test]
    fn contract_store_uses_hidden_reen_layout() {
        let store = ContractStore::new(".reen");
        let rel = Path::new("contexts/game_loop.md");
        assert_eq!(
            store.interface_ir_path(rel),
            PathBuf::from(".reen/interfaces/contexts/game_loop.interface.json")
        );
        assert_eq!(
            store.contract_bundle_path(rel),
            PathBuf::from(".reen/contracts/contexts/game_loop.contract.json")
        );
        assert_eq!(
            store.implementation_plan_path(rel),
            PathBuf::from(".reen/plans/contexts/game_loop.implementation_plan.json")
        );
        assert_eq!(
            store.hidden_spec_path(rel),
            PathBuf::from(".reen/debug/specifications/contexts/game_loop.md")
        );
        assert_eq!(
            store.debug_bundle_path(rel),
            PathBuf::from(".reen/debug/contracts/contexts/game_loop.contract.json")
        );
    }

    #[test]
    fn level_hash_is_order_independent() {
        let a = level_hash(&[
            PathBuf::from("drafts/data/board.md"),
            PathBuf::from("drafts/data/position.md"),
        ]);
        let b = level_hash(&[
            PathBuf::from("drafts/data/position.md"),
            PathBuf::from("drafts/data/board.md"),
        ]);
        assert_eq!(a, b);
    }

    #[test]
    fn draft_relative_path_uses_drafts_root() {
        let rel = draft_relative_path(
            Path::new("drafts/contexts/game_loop.md"),
            Path::new("drafts"),
        )
        .expect("relative path");
        assert_eq!(rel, PathBuf::from("contexts/game_loop.md"));
    }

    #[test]
    fn bundle_round_trip_serializes() {
        let bundle = ContractBundle {
            draft_identity: "game_loop".to_string(),
            draft_relative_path: "contexts/game_loop.md".to_string(),
            draft_fingerprint: "abc".to_string(),
            draft_summary: Some(json!({"kind": "context"})),
            behavior_contract: json!({"kind": "context"}),
            contract_artifact: json!({"roles": []}),
            implementation_plan: json!({"ordered_tasks": []}),
            plan_validation: json!({"ok": true}),
            target_output_hints: vec!["src/contexts/game_loop.rs".to_string()],
            semantic_contract: SemanticContract {
                kind: "context".to_string(),
                title: "GameLoopContext".to_string(),
                summary: Some(json!({"kind": "context"})),
                behavior_contract: json!({"kind": "context"}),
            },
            resolved_interface: ResolvedInterface {
                version: "reen.interface/v2".to_string(),
                interface_fingerprint: "fp".to_string(),
                primary_export_name: "GameLoopContext".to_string(),
                artifact_kind: "context_module".to_string(),
                exported_types: Vec::new(),
                exported_methods: Vec::new(),
                role_method_exports: Vec::new(),
                name_bindings: Vec::new(),
            },
            type_decisions: Vec::new(),
            name_bindings: Vec::new(),
            dependency_bindings: Vec::new(),
            ambiguity_report: Vec::new(),
            decision_sources: Vec::new(),
            required_upstream_interface_references: Vec::new(),
            blocking_diagnostics: Vec::new(),
            unresolved_assumptions: Vec::new(),
            contract_markdown: "# GameLoop".to_string(),
        };
        let encoded = serde_json::to_string(&bundle).expect("serialize bundle");
        let decoded: ContractBundle = serde_json::from_str(&encoded).expect("deserialize bundle");
        assert_eq!(decoded, bundle);
    }

    #[test]
    fn level_policy_serializes() {
        let policy = LevelPolicy {
            stage: "contract".to_string(),
            level_hash: "abc".to_string(),
            artifact_paths: vec!["drafts/data/board.md".to_string()],
            canonical_names: vec!["Board".to_string()],
            import_roots: vec!["crate::data::Board".to_string()],
            feature_names: Vec::new(),
            shared_type_choices: Vec::new(),
            collaborator_abstractions: Vec::new(),
            conflict_resolutions: Vec::new(),
            name_bindings: vec![NameBinding {
                semantic_name: "type".to_string(),
                rust_identifier: "r#type".to_string(),
                export_name: "type".to_string(),
                reason: "keyword_escape".to_string(),
            }],
            container_shapes: vec!["board_picture = mapping<coordinate, symbol>".to_string()],
        };
        let encoded = serde_json::to_string(&policy).expect("serialize policy");
        let decoded: LevelPolicy = serde_json::from_str(&encoded).expect("deserialize policy");
        assert_eq!(decoded, policy);
    }
}
