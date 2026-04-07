use anyhow::{Context, Result};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::capability_registry::resolved_dependency_plan_context;
use super::contracts::{ContractArtifact, build_contract_artifact};
use super::dependency_graph::{
    DependencyArtifact, DependencyGraphCache, ExecutionNode, resolve_dependency_closure_with_cache,
};
use super::dependency_tooling::load_symbols_context;
use super::interface_capsules::{InterfaceCapsule, build_interface_capsule};

#[derive(Clone, Default)]
pub(crate) struct RunContextCache {
    inner: Arc<Mutex<RunContextCacheInner>>,
}

#[derive(Default)]
struct RunContextCacheInner {
    dependency_graph_cache: DependencyGraphCache,
    dependency_snapshots: HashMap<String, DependencySnapshot>,
    file_contents: HashMap<PathBuf, CachedFileContent>,
    contract_artifacts: HashMap<String, ContractArtifact>,
    interface_capsules: HashMap<String, InterfaceCapsule>,
    resolved_dependency_plans: HashMap<PathBuf, Option<Value>>,
    tooling_symbols: HashMap<PathBuf, Option<Value>>,
}

#[derive(Clone)]
pub(crate) struct DependencySnapshot {
    pub(crate) direct_dependencies: Vec<DependencyArtifact>,
    pub(crate) dependency_closure: Vec<DependencyArtifact>,
    pub(crate) dependency_fingerprint: String,
}

#[derive(Clone)]
struct CachedFileContent {
    sha256: String,
    content: String,
}

impl RunContextCache {
    pub(crate) fn dependency_snapshot(
        &self,
        node: &ExecutionNode,
        primary_root: &str,
        fallback_root: Option<&str>,
    ) -> Result<DependencySnapshot> {
        let key = dependency_snapshot_key(&node.input_path, primary_root, fallback_root);
        if let Some(snapshot) = self
            .inner
            .lock()
            .expect("run context cache mutex should not be poisoned")
            .dependency_snapshots
            .get(&key)
            .cloned()
        {
            return Ok(snapshot);
        }

        let direct_dependencies = node.resolve_direct_dependencies()?;
        let dependency_closure = {
            let mut inner = self
                .inner
                .lock()
                .expect("run context cache mutex should not be poisoned");
            resolve_dependency_closure_with_cache(
                node,
                primary_root,
                fallback_root,
                &mut inner.dependency_graph_cache,
            )?
        };
        let dependency_fingerprint = dependency_fingerprint(&dependency_closure);
        let snapshot = DependencySnapshot {
            direct_dependencies,
            dependency_closure,
            dependency_fingerprint,
        };
        self.inner
            .lock()
            .expect("run context cache mutex should not be poisoned")
            .dependency_snapshots
            .insert(key, snapshot.clone());
        Ok(snapshot)
    }

    pub(crate) fn resolved_dependency_plan(
        &self,
        drafts_root: &Path,
    ) -> Result<Option<Value>> {
        if let Some(value) = self
            .inner
            .lock()
            .expect("run context cache mutex should not be poisoned")
            .resolved_dependency_plans
            .get(drafts_root)
            .cloned()
        {
            return Ok(value);
        }

        let value = resolved_dependency_plan_context(drafts_root)?
            .map(|value| compact_resolved_dependency_plan_value(&value));
        self.inner
            .lock()
            .expect("run context cache mutex should not be poisoned")
            .resolved_dependency_plans
            .insert(drafts_root.to_path_buf(), value.clone());
        Ok(value)
    }

    pub(crate) fn tooling_symbols(&self, primary_root: &Path) -> Result<Option<Value>> {
        if let Some(value) = self
            .inner
            .lock()
            .expect("run context cache mutex should not be poisoned")
            .tooling_symbols
            .get(primary_root)
            .cloned()
        {
            return Ok(value);
        }

        let value = load_symbols_context(primary_root)?.map(|value| compact_tooling_symbols(&value));
        self.inner
            .lock()
            .expect("run context cache mutex should not be poisoned")
            .tooling_symbols
            .insert(primary_root.to_path_buf(), value.clone());
        Ok(value)
    }

    pub(crate) fn read_file(&self, path: &Path) -> Result<String> {
        Ok(self.read_file_with_hash(path)?.content)
    }

    fn read_file_with_hash(&self, path: &Path) -> Result<CachedFileContent> {
        if let Some(cached) = self
            .inner
            .lock()
            .expect("run context cache mutex should not be poisoned")
            .file_contents
            .get(path)
            .cloned()
        {
            return Ok(cached);
        }

        let content =
            fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
        let sha256 = file_sha256(&content);
        let cached = CachedFileContent { sha256, content };
        self.inner
            .lock()
            .expect("run context cache mutex should not be poisoned")
            .file_contents
            .insert(path.to_path_buf(), cached.clone());
        Ok(cached)
    }

    pub(crate) fn contract_artifact_without_context(
        &self,
        spec_path: &Path,
        output_path_hint: Option<&Path>,
    ) -> Result<ContractArtifact> {
        let cached = self.read_file_with_hash(spec_path)?;
        let output_hint = output_path_hint.map(|path| path.display().to_string());
        let key = format!(
            "{}|{}|{}",
            spec_path.display(),
            cached.sha256,
            output_hint.as_deref().unwrap_or("")
        );
        if let Some(contract) = self
            .inner
            .lock()
            .expect("run context cache mutex should not be poisoned")
            .contract_artifacts
            .get(&key)
            .cloned()
        {
            return Ok(contract);
        }

        let contract =
            build_contract_artifact(spec_path, &cached.content, output_path_hint, None);
        self.inner
            .lock()
            .expect("run context cache mutex should not be poisoned")
            .contract_artifacts
            .insert(key, contract.clone());
        Ok(contract)
    }

    pub(crate) fn interface_capsule_without_context(
        &self,
        spec_path: &Path,
        source_path: Option<&Path>,
        source_content: Option<&str>,
    ) -> Result<InterfaceCapsule> {
        let spec = self.read_file_with_hash(spec_path)?;
        let source_content_hash = source_content.map(file_sha256).unwrap_or_default();
        let source_path_key = source_path
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        let key = format!(
            "{}|{}|{}|{}",
            spec_path.display(),
            spec.sha256,
            source_path_key,
            source_content_hash
        );
        if let Some(capsule) = self
            .inner
            .lock()
            .expect("run context cache mutex should not be poisoned")
            .interface_capsules
            .get(&key)
            .cloned()
        {
            return Ok(capsule);
        }

        let contract = self.contract_artifact_without_context(spec_path, source_path)?;
        let capsule = build_interface_capsule(&contract, source_path, source_content);
        self.inner
            .lock()
            .expect("run context cache mutex should not be poisoned")
            .interface_capsules
            .insert(key, capsule.clone());
        Ok(capsule)
    }
}

pub(crate) fn compact_tooling_symbols(value: &Value) -> Value {
    let Some(packages) = value.get("packages").and_then(Value::as_array) else {
        return value.clone();
    };

    json!({
        "packages": packages.iter().map(|package| {
            json!({
                "name": package.get("name").cloned().unwrap_or(Value::Null),
                "version": package.get("version").cloned().unwrap_or(Value::Null),
                "capabilities": package.get("capabilities").cloned().unwrap_or(Value::Array(Vec::new())),
                "symbols": package
                    .get("symbols")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|symbol| {
                        json!({
                            "kind": symbol.get("kind").cloned().unwrap_or(Value::Null),
                            "name": symbol.get("name").cloned().unwrap_or(Value::Null),
                            "module": symbol.get("module").cloned().unwrap_or(Value::Null),
                        })
                    })
                    .collect::<Vec<_>>(),
            })
        }).collect::<Vec<_>>()
    })
}

pub(crate) fn compact_resolved_dependency_plan_value(value: &Value) -> Value {
    json!({
        "providers": value.get("providers").and_then(Value::as_array).cloned().unwrap_or_default().into_iter().map(|provider| {
            json!({
                "domain": provider.get("domain").cloned().unwrap_or(Value::Null),
                "crate_name": provider.get("crate_name").cloned().unwrap_or(Value::Null),
                "version": provider.get("version").cloned().unwrap_or(Value::Null),
                "default_features": provider.get("default_features").cloned().unwrap_or(Value::Null),
                "features": provider.get("features").cloned().unwrap_or(Value::Array(Vec::new())),
                "capabilities": provider.get("capabilities").cloned().unwrap_or(Value::Array(Vec::new())),
            })
        }).collect::<Vec<_>>(),
        "packages": value.get("packages").and_then(Value::as_array).cloned().unwrap_or_default().into_iter().map(|package| {
            json!({
                "name": package.get("name").cloned().unwrap_or(Value::Null),
                "version": package.get("version").cloned().unwrap_or(Value::Null),
                "capabilities": package.get("capabilities").cloned().unwrap_or(Value::Array(Vec::new())),
                "domain": package.get("domain").cloned().unwrap_or(Value::Null),
            })
        }).collect::<Vec<_>>(),
        "required_capabilities": value.get("required_capabilities").cloned().unwrap_or(Value::Array(Vec::new())),
        "unresolved_capabilities": value.get("unresolved_capabilities").cloned().unwrap_or(Value::Array(Vec::new())),
    })
}

fn dependency_snapshot_key(input_path: &Path, primary_root: &str, fallback_root: Option<&str>) -> String {
    format!(
        "{}|{}|{}",
        input_path.display(),
        primary_root,
        fallback_root.unwrap_or("")
    )
}

fn dependency_fingerprint(closure: &[DependencyArtifact]) -> String {
    if closure.is_empty() {
        return String::new();
    }
    let mut deps = closure
        .iter()
        .map(|dep| format!("{}:{}", dep.path, dep.sha256))
        .collect::<Vec<_>>();
    deps.sort();
    file_sha256(&deps.join("|"))
}

fn file_sha256(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::{RunContextCache, compact_resolved_dependency_plan_value, compact_tooling_symbols};
    use serde_json::json;

    #[test]
    fn compacts_tooling_symbols_to_prompt_relevant_fields() {
        let compacted = compact_tooling_symbols(&json!({
            "generated_at": "now",
            "manifest_path": "Cargo.toml",
            "source_dependencies_path": "drafts/dependencies.yml",
            "packages": [
                {
                    "name": "chrono",
                    "version": "0.4",
                    "capabilities": ["datetime_utc"],
                    "symbols": [
                        {
                            "kind": "function",
                            "name": "Utc",
                            "module": "chrono",
                            "signature": "pub fn now()",
                        }
                    ]
                }
            ]
        }));

        assert!(compacted.get("generated_at").is_none());
        assert!(
            compacted["packages"][0]["symbols"][0]
                .get("signature")
                .is_none()
        );
        assert_eq!(compacted["packages"][0]["symbols"][0]["module"], "chrono");
    }

    #[test]
    fn compacts_resolved_dependency_plan_to_prompt_relevant_fields() {
        let compacted = compact_resolved_dependency_plan_value(&json!({
            "registry_path": "drafts/capability_registry.yml",
            "dependency_manifest_path": "drafts/dependencies.yml",
            "providers": [{ "domain": "time", "crate_name": "chrono", "version": "0.4", "default_features": true, "features": ["serde"], "capabilities": ["datetime_utc"] }],
            "packages": [{ "name": "chrono", "version": "0.4", "capabilities": ["datetime_utc"], "domain": "time" }],
            "required_capabilities": ["datetime_utc"],
            "unresolved_capabilities": [],
        }));

        assert!(compacted.get("registry_path").is_none());
        assert_eq!(compacted["providers"][0]["crate_name"], "chrono");
        assert_eq!(compacted["packages"][0]["domain"], "time");
    }

    #[test]
    fn file_reads_are_cached() {
        let root = std::env::temp_dir().join(format!("reen_run_context_{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("mkdir");
        let path = root.join("test.md");
        std::fs::write(&path, "# Test\n").expect("write");

        let cache = RunContextCache::default();
        let first = cache.read_file_with_hash(&path).expect("first read");
        let second = cache.read_file_with_hash(&path).expect("second read");
        assert_eq!(first.content, second.content);
        assert_eq!(first.sha256, second.sha256);

        std::fs::remove_dir_all(root).ok();
    }
}
