//! File and GitHub artifact backends. Several types and helpers are unused in the current binary
//! but kept as the public surface for tooling and future commands.
#![allow(dead_code)]

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use super::CategoryFilter;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BackendSelection {
    File,
    GitHub { owner: String, repo: String },
}

impl BackendSelection {
    pub fn from_repo_spec(repo_spec: Option<&str>) -> Result<Self> {
        match repo_spec {
            None => Ok(Self::File),
            Some(spec) => {
                let (owner, repo) = parse_repo_spec(spec)?;
                Ok(Self::GitHub { owner, repo })
            }
        }
    }

    pub fn cache_namespace(&self) -> String {
        match self {
            Self::File => "file".to_string(),
            Self::GitHub { owner, repo } => format!("github:{owner}/{repo}"),
        }
    }
}

fn parse_repo_spec(repo_spec: &str) -> Result<(String, String)> {
    let trimmed = repo_spec.trim();
    let mut parts = trimmed.split('/');
    let owner = parts.next().unwrap_or_default().trim();
    let repo = parts.next().unwrap_or_default().trim();
    if owner.is_empty() || repo.is_empty() || parts.next().is_some() {
        bail!(
            "invalid GitHub repository '{trimmed}', expected the form <owner>/<repo>"
        );
    }
    Ok((owner.to_string(), repo.to_string()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Draft,
    Specification,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactCategory {
    Root,
    Data,
    Context,
    Api,
}

impl ArtifactCategory {
    pub fn label(self) -> Option<&'static str> {
        match self {
            Self::Root => Some("app"),
            Self::Data => Some("data"),
            Self::Context => Some("context"),
            Self::Api => Some("api"),
        }
    }

    pub fn projected_subdir(self, kind: ArtifactKind) -> &'static str {
        match (kind, self) {
            (ArtifactKind::Draft, ArtifactCategory::Root) => "",
            (ArtifactKind::Draft, ArtifactCategory::Data) => "data",
            (ArtifactKind::Draft, ArtifactCategory::Context) => "contexts",
            (ArtifactKind::Draft, ArtifactCategory::Api) => "apis",
            (ArtifactKind::Specification, ArtifactCategory::Root) => "",
            (ArtifactKind::Specification, ArtifactCategory::Data) => "data",
            (ArtifactKind::Specification, ArtifactCategory::Context) => "contexts",
            (ArtifactKind::Specification, ArtifactCategory::Api) => "contexts/external",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub id: String,
    pub name: String,
    pub kind: ArtifactKind,
    pub category: ArtifactCategory,
    pub path: PathBuf,
    pub source_draft_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ArtifactWriteResult {
    pub artifact: ArtifactRef,
    pub output_path: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitHubSyncScope {
    DraftsOnly,
    SpecificationsOnly,
    All,
}

impl GitHubSyncScope {
    fn includes(self, kind: ArtifactKind) -> bool {
        match self {
            Self::DraftsOnly => kind == ArtifactKind::Draft,
            Self::SpecificationsOnly => kind == ArtifactKind::Specification,
            Self::All => true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GitHubProjectionEntry {
    pub issue_number: u64,
    pub issue_title: String,
    pub artifact_id: String,
    pub kind: ArtifactKind,
    pub category: ArtifactCategory,
    pub path: PathBuf,
    pub source_draft_id: Option<String>,
    pub dependency_numbers: Vec<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GitHubProjectionManifest {
    pub owner: String,
    pub repo: String,
    pub root: PathBuf,
    pub drafts_root: PathBuf,
    pub specifications_root: PathBuf,
    pub entries: Vec<GitHubProjectionEntry>,
}

impl GitHubProjectionManifest {
    pub fn issue_summaries_for_kind(&self, kind: ArtifactKind) -> Vec<GitHubProjectionEntry> {
        let mut entries = self
            .entries
            .iter()
            .filter(|entry| entry.kind == kind)
            .cloned()
            .collect::<Vec<_>>();
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        entries
    }

    pub fn projected_file_count(&self) -> usize {
        self.entries.len()
    }

    pub fn dependency_edge_count(&self) -> usize {
        self.entries
            .iter()
            .map(|entry| entry.dependency_numbers.len())
            .sum()
    }

    pub fn entry_for_path(&self, path: &Path) -> Option<&GitHubProjectionEntry> {
        self.entries.iter().find(|entry| entry.path == path)
    }

    pub fn draft_entry_for_spec_path(
        &self,
        spec_path: &Path,
        store: &dyn ArtifactStore,
    ) -> Result<Option<&GitHubProjectionEntry>> {
        let Some(spec_artifact) = store.artifact_for_path(spec_path) else {
            return Ok(None);
        };
        let Some(draft_artifact) = store.find_draft_for_specification(&spec_artifact)? else {
            return Ok(None);
        };
        Ok(self.entry_for_path(&draft_artifact.path))
    }

    pub fn matching_spec_entry(
        &self,
        spec_artifact: &ArtifactRef,
        source_draft_id: Option<&str>,
    ) -> Option<&GitHubProjectionEntry> {
        self.entries.iter().find(|entry| {
            entry.kind == ArtifactKind::Specification
                && entry.category == spec_artifact.category
                && entry.source_draft_id.as_deref() == source_draft_id
                && entry.path.file_name() == spec_artifact.path.file_name()
        })
    }
}

pub trait ArtifactStore: Send + Sync {
    fn backend(&self) -> BackendSelection;
    fn drafts_root(&self) -> &Path;
    fn specifications_root(&self) -> &Path;
    /// Directory that contains the active `drafts/` and `specifications/` trees: repository root
    /// for the file backend (default `./drafts`, `./specifications`), or `.reen/github/<owner>__<repo>`
    /// for GitHub-backed runs. Callers should not mix this with a second root.
    fn artifact_workspace_root(&self) -> PathBuf;
    fn resolve_inputs(
        &self,
        kind: ArtifactKind,
        names: Vec<String>,
        filter: &CategoryFilter,
    ) -> Result<Vec<ArtifactRef>>;
    fn select_dependency_roots(
        &self,
        selected_inputs: Vec<ArtifactRef>,
        names_provided: bool,
        filter: &CategoryFilter,
    ) -> Result<Vec<ArtifactRef>>;
    fn artifact_by_id(&self, id: &str) -> Result<ArtifactRef>;
    fn artifact_for_path(&self, path: &Path) -> Option<ArtifactRef>;
    fn read_content(&self, artifact: &ArtifactRef) -> Result<String>;
    fn find_specification_for_draft(&self, draft: &ArtifactRef) -> Result<Option<ArtifactRef>>;
    fn find_draft_for_specification(&self, spec: &ArtifactRef) -> Result<Option<ArtifactRef>>;
    fn write_specification(
        &self,
        draft: &ArtifactRef,
        display_name: &str,
        category: ArtifactCategory,
        content: String,
    ) -> Result<ArtifactWriteResult>;
    fn clear_specifications(&self, names: &[String], dry_run: bool) -> Result<usize>;
    fn explicit_dependencies(&self, _artifact: &ArtifactRef) -> Result<Option<Vec<ArtifactRef>>> {
        Ok(None)
    }
    fn sync_specification_dependencies(
        &self,
        _spec: &ArtifactRef,
        _direct_dependencies: &[ArtifactRef],
    ) -> Result<()> {
        Ok(())
    }
}

pub fn build_artifact_store(selection: &BackendSelection) -> Result<Arc<dyn ArtifactStore>> {
    match selection {
        BackendSelection::File => Ok(Arc::new(FileArtifactStore::new())),
        BackendSelection::GitHub { owner, repo } => Ok(Arc::new(GitHubArtifactStore::new(
            owner.clone(),
            repo.clone(),
        )?)),
    }
}

pub struct FileArtifactStore {
    drafts_root: PathBuf,
    specs_root: PathBuf,
    id_namespace: String,
}

impl FileArtifactStore {
    pub fn new() -> Self {
        Self::with_roots_and_namespace(
            PathBuf::from("drafts"),
            PathBuf::from("specifications"),
            "file".to_string(),
        )
    }

    pub fn with_roots(drafts_root: PathBuf, specs_root: PathBuf) -> Self {
        Self::with_roots_and_namespace(drafts_root, specs_root, "file".to_string())
    }

    pub fn with_roots_and_namespace(
        drafts_root: PathBuf,
        specs_root: PathBuf,
        id_namespace: String,
    ) -> Self {
        Self {
            drafts_root,
            specs_root,
            id_namespace,
        }
    }
}

impl ArtifactStore for FileArtifactStore {
    fn backend(&self) -> BackendSelection {
        BackendSelection::File
    }

    fn drafts_root(&self) -> &Path {
        &self.drafts_root
    }

    fn specifications_root(&self) -> &Path {
        &self.specs_root
    }

    fn artifact_workspace_root(&self) -> PathBuf {
        match (self.drafts_root.parent(), self.specs_root.parent()) {
            (Some(pd), Some(ps)) if pd == ps => pd.to_path_buf(),
            _ => PathBuf::from("."),
        }
    }

    fn resolve_inputs(
        &self,
        kind: ArtifactKind,
        names: Vec<String>,
        filter: &CategoryFilter,
    ) -> Result<Vec<ArtifactRef>> {
        let root = match kind {
            ArtifactKind::Draft => self.drafts_root(),
            ArtifactKind::Specification => self.specifications_root(),
        };
        resolve_file_inputs(root, kind, names, filter, self.drafts_root(), self.specifications_root(), &self.id_namespace)
    }

    fn select_dependency_roots(
        &self,
        selected_inputs: Vec<ArtifactRef>,
        names_provided: bool,
        filter: &CategoryFilter,
    ) -> Result<Vec<ArtifactRef>> {
        if names_provided {
            return Ok(selected_inputs);
        }

        if let Some(app) = selected_inputs
            .iter()
            .find(|artifact| artifact.category == ArtifactCategory::Root)
        {
            let base_dir = match app.kind {
                ArtifactKind::Draft => self.drafts_root().to_string_lossy().into_owned(),
                ArtifactKind::Specification => {
                    self.specifications_root().to_string_lossy().into_owned()
                }
            };
            if !filter.is_active()
                || filter.matches_path(&app.path, &base_dir)
            {
                return Ok(vec![app.clone()]);
            }
        }
        Ok(selected_inputs)
    }

    fn artifact_by_id(&self, id: &str) -> Result<ArtifactRef> {
        file_artifact_from_id(id, self.drafts_root(), self.specifications_root(), &self.id_namespace)
    }

    fn artifact_for_path(&self, path: &Path) -> Option<ArtifactRef> {
        file_artifact_from_path(
            path,
            self.drafts_root(),
            self.specifications_root(),
            &self.id_namespace,
        )
        .ok()
    }

    fn read_content(&self, artifact: &ArtifactRef) -> Result<String> {
        fs::read_to_string(&artifact.path)
            .with_context(|| format!("failed reading artifact: {}", artifact.path.display()))
    }

    fn find_specification_for_draft(&self, draft: &ArtifactRef) -> Result<Option<ArtifactRef>> {
        let output_path = determine_file_specification_path(
            draft,
            &draft.name,
            draft.category,
            self.drafts_root(),
            self.specifications_root(),
        )?;
        if output_path.exists() {
            return Ok(Some(file_artifact_from_path(
                &output_path,
                self.drafts_root(),
                self.specifications_root(),
                &self.id_namespace,
            )?));
        }
        Ok(None)
    }

    fn find_draft_for_specification(&self, spec: &ArtifactRef) -> Result<Option<ArtifactRef>> {
        let draft_path = determine_file_draft_path(spec.path.as_path(), self.drafts_root(), self.specifications_root());
        if draft_path.exists() {
            return Ok(Some(file_artifact_from_path(
                &draft_path,
                self.drafts_root(),
                self.specifications_root(),
                &self.id_namespace,
            )?));
        }
        Ok(None)
    }

    fn write_specification(
        &self,
        draft: &ArtifactRef,
        display_name: &str,
        category: ArtifactCategory,
        content: String,
    ) -> Result<ArtifactWriteResult> {
        let output_path = determine_file_specification_path(
            draft,
            display_name,
            category,
            self.drafts_root(),
            self.specifications_root(),
        )?;
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).context("Failed to create specification output directory")?;
        }
        fs::write(&output_path, &content).context("Failed to write specification file")?;
        let artifact = file_artifact_from_path(
            &output_path,
            self.drafts_root(),
            self.specifications_root(),
            &self.id_namespace,
        )?;
        Ok(ArtifactWriteResult {
            artifact,
            output_path,
        })
    }

    fn clear_specifications(&self, names: &[String], dry_run: bool) -> Result<usize> {
        if names.is_empty() {
            if !self.specifications_root().exists() {
                return Ok(0);
            }
            if dry_run {
                return Ok(count_markdown_files(self.specifications_root())?);
            }
            fs::remove_dir_all(self.specifications_root()).with_context(|| {
                format!("Failed to remove {}", self.specifications_root().display())
            })?;
            return Ok(1);
        }

        let spec_files = resolve_file_inputs(
            self.specifications_root(),
            ArtifactKind::Specification,
            names.to_vec(),
            &CategoryFilter::all(),
            self.drafts_root(),
            self.specifications_root(),
            &self.id_namespace,
        )?;
        let mut removed = 0usize;
        for spec in spec_files {
            if spec.path.exists() {
                if !dry_run {
                    fs::remove_file(&spec.path)
                        .with_context(|| format!("Failed to remove {}", spec.path.display()))?;
                }
                removed += 1;
            }
        }
        Ok(removed)
    }
}

struct GitHubArtifactStore {
    owner: String,
    repo: String,
    /// `.reen/github/<owner>__<repo>` — parent of `drafts_root` / `specs_root`.
    projection_root: PathBuf,
    drafts_root: PathBuf,
    specs_root: PathBuf,
    state: Mutex<GitHubState>,
}

#[derive(Default)]
struct GitHubState {
    by_id: HashMap<String, GitHubIssueRecord>,
    by_path: HashMap<PathBuf, String>,
    draft_to_specs: HashMap<String, Vec<String>>,
    issue_number_to_id: HashMap<u64, String>,
}

#[derive(Clone, Debug)]
struct GitHubIssueRecord {
    artifact: ArtifactRef,
    issue_number: u64,
    issue_id: u64,
    node_id: String,
    title: String,
    body: String,
    labels: HashSet<String>,
    dependency_numbers: Vec<u64>,
}

impl GitHubArtifactStore {
    fn new(owner: String, repo: String) -> Result<Self> {
        let projection_root = PathBuf::from(".reen")
            .join("github")
            .join(format!("{}__{}", owner, repo));
        let store = Self {
            owner,
            repo,
            drafts_root: projection_root.join("drafts"),
            specs_root: projection_root.join("specifications"),
            projection_root,
            state: Mutex::new(GitHubState::default()),
        };
        store.refresh_projection()?;
        Ok(store)
    }

    fn refresh_projection(&self) -> Result<()> {
        let (records, issue_number_to_id) = load_github_issue_records(
            &self.owner,
            &self.repo,
            &self.drafts_root,
            &self.specs_root,
            GitHubSyncScope::All,
        )?;

        let mut by_id = HashMap::new();
        let mut by_path = HashMap::new();
        let mut draft_to_specs: HashMap<String, Vec<String>> = HashMap::new();

        if self.drafts_root.exists() {
            fs::remove_dir_all(&self.drafts_root)
                .with_context(|| format!("Failed to clear {}", self.drafts_root.display()))?;
        }
        if self.specs_root.exists() {
            fs::remove_dir_all(&self.specs_root)
                .with_context(|| format!("Failed to clear {}", self.specs_root.display()))?;
        }

        for record in records {
            if let Some(parent) = record.artifact.path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create {}", parent.display()))?;
            }
            fs::write(&record.artifact.path, &record.body).with_context(|| {
                format!("Failed to write projected issue artifact {}", record.artifact.path.display())
            })?;
            by_path.insert(record.artifact.path.clone(), record.artifact.id.clone());
            if let Some(draft_id) = &record.artifact.source_draft_id {
                draft_to_specs
                    .entry(draft_id.clone())
                    .or_default()
                    .push(record.artifact.id.clone());
            }
            by_id.insert(record.artifact.id.clone(), record);
        }

        let mut state = self.state.lock().expect("github state");
        state.by_id = by_id;
        state.by_path = by_path;
        state.draft_to_specs = draft_to_specs;
        state.issue_number_to_id = issue_number_to_id
            .into_iter()
            .map(|(number, _id)| (number, github_issue_artifact_id(&self.owner, &self.repo, number)))
            .collect();
        Ok(())
    }

    fn list_repo_issues(&self) -> Result<Vec<GitHubIssuePayload>> {
        list_repo_issues_for_repo(&self.owner, &self.repo)
    }

    fn list_sub_issue_numbers(&self, issue_number: u64) -> Result<Vec<u64>> {
        list_sub_issue_numbers_for_repo(&self.owner, &self.repo, issue_number)
    }

    fn upsert_issue(
        &self,
        issue_number: Option<u64>,
        title: &str,
        body: &str,
        labels: &[&str],
    ) -> Result<GitHubIssuePayload> {
        upsert_repo_issue(&self.owner, &self.repo, issue_number, title, body, labels)
    }

    fn sync_sub_issue_numbers(&self, parent: &GitHubIssueRecord, desired_numbers: &[u64]) -> Result<()> {
        let current: HashSet<u64> = parent.dependency_numbers.iter().copied().collect();
        let desired: HashSet<u64> = desired_numbers.iter().copied().collect();
        let desired_node_ids = self.lookup_node_ids(&desired)?;
        let current_node_ids = self.lookup_node_ids(&current)?;

        for (number, node_id) in desired_node_ids {
            if !current.contains(&number) {
                self.add_sub_issue(&parent.node_id, &node_id)?;
            }
        }
        for (number, node_id) in current_node_ids {
            if !desired.contains(&number) {
                self.remove_sub_issue(&parent.node_id, &node_id)?;
            }
        }
        Ok(())
    }

    fn lookup_node_ids(&self, issue_numbers: &HashSet<u64>) -> Result<Vec<(u64, String)>> {
        let state = self.state.lock().expect("github state");
        let mut result = Vec::new();
        for record in state.by_id.values() {
            if issue_numbers.contains(&record.issue_number) {
                result.push((record.issue_number, record.node_id.clone()));
            }
        }
        Ok(result)
    }

    fn add_sub_issue(&self, parent_node_id: &str, sub_issue_node_id: &str) -> Result<()> {
        let query = r#"mutation($issueId:ID!, $subIssueId:ID!) {
  addSubIssue(input:{issueId:$issueId, subIssueId:$subIssueId}) {
    issue {
      id
    }
  }
}"#;
        let variables = serde_json::json!({
            "issueId": parent_node_id,
            "subIssueId": sub_issue_node_id,
        });
        run_gh_graphql(query, variables).map(|_| ())
    }

    fn remove_sub_issue(&self, parent_node_id: &str, sub_issue_node_id: &str) -> Result<()> {
        let query = r#"mutation($issueId:ID!, $subIssueId:ID!) {
  removeSubIssue(input:{issueId:$issueId, subIssueId:$subIssueId}) {
    issue {
      id
    }
  }
}"#;
        let variables = serde_json::json!({
            "issueId": parent_node_id,
            "subIssueId": sub_issue_node_id,
        });
        run_gh_graphql(query, variables).map(|_| ())
    }
}

pub fn build_file_artifact_store_with_roots(
    drafts_root: PathBuf,
    specs_root: PathBuf,
    id_namespace: String,
) -> Arc<dyn ArtifactStore> {
    Arc::new(FileArtifactStore::with_roots_and_namespace(
        drafts_root,
        specs_root,
        id_namespace,
    ))
}

pub fn sync_github_projection(
    owner: &str,
    repo: &str,
    scope: GitHubSyncScope,
) -> Result<GitHubProjectionManifest> {
    let root = PathBuf::from(".reen")
        .join("github")
        .join(format!("{}__{}", owner, repo));
    let drafts_root = root.join("drafts");
    let specs_root = root.join("specifications");
    let manifest_path = root.join("projection_manifest.json");

    if drafts_root.exists() {
        fs::remove_dir_all(&drafts_root)
            .with_context(|| format!("Failed to clear {}", drafts_root.display()))?;
    }
    if specs_root.exists() {
        fs::remove_dir_all(&specs_root)
            .with_context(|| format!("Failed to clear {}", specs_root.display()))?;
    }
    fs::create_dir_all(&root).with_context(|| format!("Failed to create {}", root.display()))?;

    let (records, _) = load_github_issue_records(owner, repo, &drafts_root, &specs_root, scope)?;
    let mut entries = Vec::new();
    for record in records {
        if let Some(parent) = record.artifact.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        fs::write(&record.artifact.path, &record.body).with_context(|| {
            format!(
                "Failed to write projected issue artifact {}",
                record.artifact.path.display()
            )
        })?;
        entries.push(GitHubProjectionEntry {
            issue_number: record.issue_number,
            issue_title: record.title,
            artifact_id: record.artifact.id.clone(),
            kind: record.artifact.kind,
            category: record.artifact.category,
            path: record.artifact.path,
            source_draft_id: record.artifact.source_draft_id,
            dependency_numbers: record.dependency_numbers,
        });
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    let manifest = GitHubProjectionManifest {
        owner: owner.to_string(),
        repo: repo.to_string(),
        root,
        drafts_root,
        specifications_root: specs_root,
        entries,
    };
    fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)
        .with_context(|| format!("Failed to write {}", manifest_path.display()))?;
    Ok(manifest)
}

pub fn publish_projected_specifications(
    manifest: &GitHubProjectionManifest,
    dry_run: bool,
) -> Result<usize> {
    let store = build_file_artifact_store_with_roots(
        manifest.drafts_root.clone(),
        manifest.specifications_root.clone(),
        format!("github-sync:{}/{}", manifest.owner, manifest.repo),
    );
    let spec_artifacts =
        store.resolve_inputs(ArtifactKind::Specification, Vec::new(), &CategoryFilter::all())?;
    let mut published = 0usize;
    for spec_artifact in spec_artifacts {
        let Some(draft_entry) = manifest.draft_entry_for_spec_path(&spec_artifact.path, store.as_ref())?
        else {
            continue;
        };
        let content = store.read_content(&spec_artifact)?;
        let draft_ref = format!("{}/{}#{}", manifest.owner, manifest.repo, draft_entry.issue_number);
        let existing = manifest.matching_spec_entry(&spec_artifact, Some(&draft_entry.artifact_id));
        let publish_title = existing
            .map(|entry| entry.issue_title.clone())
            .or_else(|| {
                if spec_artifact.category == ArtifactCategory::Root {
                    Some(draft_entry.issue_title.clone())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| spec_artifact.name.clone());
        let labels = [
            "specification",
            spec_artifact.category.label().unwrap_or("context"),
        ];
        if dry_run {
            published += 1;
            continue;
        }
        upsert_repo_issue(
            &manifest.owner,
            &manifest.repo,
            existing.map(|entry| entry.issue_number),
            &publish_title,
            &upsert_issue_metadata(&content, Some(&draft_ref)),
            &labels,
        )?;
        published += 1;
    }
    Ok(published)
}

impl ArtifactStore for GitHubArtifactStore {
    fn backend(&self) -> BackendSelection {
        BackendSelection::GitHub {
            owner: self.owner.clone(),
            repo: self.repo.clone(),
        }
    }

    fn drafts_root(&self) -> &Path {
        &self.drafts_root
    }

    fn specifications_root(&self) -> &Path {
        &self.specs_root
    }

    fn artifact_workspace_root(&self) -> PathBuf {
        self.projection_root.clone()
    }

    fn resolve_inputs(
        &self,
        kind: ArtifactKind,
        names: Vec<String>,
        filter: &CategoryFilter,
    ) -> Result<Vec<ArtifactRef>> {
        self.refresh_projection()?;
        let state = self.state.lock().expect("github state");
        let mut artifacts = state
            .by_id
            .values()
            .filter(|record| record.artifact.kind == kind)
            .map(|record| record.artifact.clone())
            .collect::<Vec<_>>();
        artifacts.sort_by(|a, b| a.path.cmp(&b.path));
        if names.is_empty() {
            let root = match kind {
                ArtifactKind::Draft => self.drafts_root(),
                ArtifactKind::Specification => self.specifications_root(),
            };
            return Ok(filter_artifacts(artifacts, filter, root));
        }

        let lowered = names
            .iter()
            .map(|name| name.to_ascii_lowercase())
            .collect::<HashSet<_>>();
        let mut matches = artifacts
            .into_iter()
            .filter(|artifact| lowered.contains(&artifact.name.to_ascii_lowercase()))
            .collect::<Vec<_>>();
        matches.sort_by(|a, b| a.path.cmp(&b.path));
        let root = match kind {
            ArtifactKind::Draft => self.drafts_root(),
            ArtifactKind::Specification => self.specifications_root(),
        };
        Ok(filter_artifacts(matches, filter, root))
    }

    fn select_dependency_roots(
        &self,
        selected_inputs: Vec<ArtifactRef>,
        names_provided: bool,
        filter: &CategoryFilter,
    ) -> Result<Vec<ArtifactRef>> {
        if names_provided {
            return Ok(selected_inputs);
        }
        if let Some(app) = selected_inputs
            .iter()
            .find(|artifact| artifact.category == ArtifactCategory::Root)
        {
            let base_dir = match app.kind {
                ArtifactKind::Draft => self.drafts_root().to_string_lossy().into_owned(),
                ArtifactKind::Specification => {
                    self.specifications_root().to_string_lossy().into_owned()
                }
            };
            if !filter.is_active()
                || filter.matches_path(&app.path, &base_dir)
            {
                return Ok(vec![app.clone()]);
            }
        }
        Ok(selected_inputs)
    }

    fn artifact_by_id(&self, id: &str) -> Result<ArtifactRef> {
        self.refresh_projection()?;
        let state = self.state.lock().expect("github state");
        state
            .by_id
            .get(id)
            .map(|record| record.artifact.clone())
            .with_context(|| format!("unknown GitHub artifact id: {id}"))
    }

    fn artifact_for_path(&self, path: &Path) -> Option<ArtifactRef> {
        self.refresh_projection().ok()?;
        let state = self.state.lock().expect("github state");
        let id = state.by_path.get(path)?;
        state.by_id.get(id).map(|record| record.artifact.clone())
    }

    fn read_content(&self, artifact: &ArtifactRef) -> Result<String> {
        self.refresh_projection()?;
        let state = self.state.lock().expect("github state");
        state
            .by_id
            .get(&artifact.id)
            .map(|record| record.body.clone())
            .with_context(|| format!("missing projected GitHub artifact {}", artifact.id))
    }

    fn find_specification_for_draft(&self, draft: &ArtifactRef) -> Result<Option<ArtifactRef>> {
        self.refresh_projection()?;
        let state = self.state.lock().expect("github state");
        let Some(ids) = state.draft_to_specs.get(&draft.id) else {
            return Ok(None);
        };
        Ok(ids
            .iter()
            .filter_map(|id| state.by_id.get(id))
            .map(|record| record.artifact.clone())
            .min_by(|a, b| a.path.cmp(&b.path)))
    }

    fn find_draft_for_specification(&self, spec: &ArtifactRef) -> Result<Option<ArtifactRef>> {
        self.refresh_projection()?;
        let state = self.state.lock().expect("github state");
        let Some(draft_id) = &spec.source_draft_id else {
            return Ok(None);
        };
        Ok(state.by_id.get(draft_id).map(|record| record.artifact.clone()))
    }

    fn write_specification(
        &self,
        draft: &ArtifactRef,
        display_name: &str,
        category: ArtifactCategory,
        content: String,
    ) -> Result<ArtifactWriteResult> {
        self.refresh_projection()?;
        let labels = ["specification", category.label().unwrap_or("context")];
        let source_ref = match &self.backend() {
            BackendSelection::GitHub { owner, repo } => {
                let issue_number = github_issue_number_from_artifact_id(&draft.id)?;
                format!("{owner}/{repo}#{issue_number}")
            }
            BackendSelection::File => String::new(),
        };
        let body = upsert_issue_metadata(&content, Some(&source_ref));
        let existing = {
            let state = self.state.lock().expect("github state");
            state
                .draft_to_specs
                .get(&draft.id)
                .into_iter()
                .flatten()
                .filter_map(|id| state.by_id.get(id))
                .find(|record| {
                    record.artifact.name == display_name
                        && record.artifact.category == category
                        && record.artifact.kind == ArtifactKind::Specification
                })
                .map(|record| record.issue_number)
        };
        self.upsert_issue(existing, display_name, &body, &labels)?;
        self.refresh_projection()?;
        let state = self.state.lock().expect("github state");
        let artifact = state
            .draft_to_specs
            .get(&draft.id)
            .into_iter()
            .flatten()
            .filter_map(|id| state.by_id.get(id))
            .find(|record| {
                record.artifact.name == display_name
                    && record.artifact.category == category
                    && record.artifact.kind == ArtifactKind::Specification
            })
            .map(|record| record.artifact.clone())
            .with_context(|| format!("failed to resolve written GitHub specification '{display_name}'"))?;
        Ok(ArtifactWriteResult {
            output_path: artifact.path.clone(),
            artifact,
        })
    }

    fn clear_specifications(&self, _names: &[String], _dry_run: bool) -> Result<usize> {
        bail!("clearing GitHub-backed specification issues is not yet supported")
    }

    fn explicit_dependencies(&self, artifact: &ArtifactRef) -> Result<Option<Vec<ArtifactRef>>> {
        self.refresh_projection()?;
        let state = self.state.lock().expect("github state");
        let Some(record) = state.by_id.get(&artifact.id) else {
            return Ok(Some(Vec::new()));
        };
        let mut refs = Vec::new();
        for number in &record.dependency_numbers {
            let dep_id = github_issue_artifact_id(&self.owner, &self.repo, *number);
            if let Some(dep) = state.by_id.get(&dep_id) {
                refs.push(dep.artifact.clone());
            }
        }
        refs.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Some(refs))
    }

    fn sync_specification_dependencies(
        &self,
        spec: &ArtifactRef,
        direct_dependencies: &[ArtifactRef],
    ) -> Result<()> {
        self.refresh_projection()?;
        let desired_numbers = direct_dependencies
            .iter()
            .filter_map(|dep| {
                if dep.kind == ArtifactKind::Specification {
                    github_issue_number_from_artifact_id(&dep.id).ok()
                } else {
                    self.find_specification_for_draft(dep)
                        .ok()
                        .and_then(|maybe| maybe)
                        .and_then(|spec_dep| github_issue_number_from_artifact_id(&spec_dep.id).ok())
                        .or_else(|| github_issue_number_from_artifact_id(&dep.id).ok())
                }
            })
            .collect::<Vec<_>>();
        let record = {
            let state = self.state.lock().expect("github state");
            state
                .by_id
                .get(&spec.id)
                .cloned()
                .with_context(|| format!("missing GitHub spec record {}", spec.id))?
        };
        self.sync_sub_issue_numbers(&record, &desired_numbers)?;
        self.refresh_projection()?;
        Ok(())
    }
}

fn filter_artifacts(artifacts: Vec<ArtifactRef>, filter: &CategoryFilter, root: &Path) -> Vec<ArtifactRef> {
    if !filter.is_active() {
        return artifacts;
    }
    let root = root.to_string_lossy().into_owned();
    artifacts
        .into_iter()
        .filter(|artifact| filter.matches_path(&artifact.path, &root))
        .collect()
}

fn count_markdown_files(dir: &Path) -> Result<usize> {
    if !dir.exists() {
        return Ok(0);
    }
    let mut count = 0usize;
    for entry in fs::read_dir(dir).with_context(|| format!("Failed to read {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            count += count_markdown_files(&path)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            count += 1;
        }
    }
    Ok(count)
}

fn resolve_file_inputs(
    root: &Path,
    kind: ArtifactKind,
    names: Vec<String>,
    filter: &CategoryFilter,
    drafts_root: &Path,
    specs_root: &Path,
    id_namespace: &str,
) -> Result<Vec<ArtifactRef>> {
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut paths = if names.is_empty() {
        collect_filtered_markdown_paths(root, filter)?
    } else {
        let mut found = Vec::new();
        for name in names {
            found.extend(resolve_named_artifacts(root, &name, filter)?);
        }
        found
    };
    paths.sort();
    paths.dedup();
    paths
        .into_iter()
        .map(|path| file_artifact_from_path(&path, drafts_root, specs_root, id_namespace))
        .map(|result| result.map(|artifact| artifact_for_kind_root(artifact, kind)))
        .collect()
}

fn artifact_for_kind_root(artifact: ArtifactRef, _kind: ArtifactKind) -> ArtifactRef {
    artifact
}

fn collect_filtered_markdown_paths(root: &Path, filter: &CategoryFilter) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if filter.include_data() {
        files.extend(collect_md_files_recursive(&root.join("data"))?);
    }
    if filter.include_contexts() {
        files.extend(collect_md_files_recursive(&root.join("contexts"))?);
        files.extend(collect_md_files_recursive(&root.join("external_apis"))?);
        files.extend(collect_md_files_recursive(&root.join("apis"))?);
    }
    if filter.include_root() {
        for entry in fs::read_dir(root).with_context(|| format!("Failed to read {}", root.display()))? {
            let path = entry?.path();
            if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("md") {
                files.push(path);
            }
        }
    }
    Ok(files)
}

fn collect_md_files_recursive(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("Failed to read {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            files.extend(collect_md_files_recursive(&path)?);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            files.push(path);
        }
    }
    Ok(files)
}

fn resolve_named_artifacts(
    root: &Path,
    name: &str,
    filter: &CategoryFilter,
) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if filter.include_data() {
        files.extend(resolve_named_in_category(&root.join("data"), name)?);
    }
    if filter.include_contexts() {
        files.extend(resolve_named_in_category(&root.join("contexts"), name)?);
        files.extend(resolve_named_in_category(&root.join("external_apis"), name)?);
        files.extend(resolve_named_in_category(&root.join("apis"), name)?);
    }
    if filter.include_root() {
        let root_path = root.join(format!("{name}.md"));
        if root_path.exists() {
            files.push(root_path);
        }
    }
    Ok(files)
}

fn resolve_named_in_category(root: &Path, name: &str) -> Result<Vec<PathBuf>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut matches = Vec::new();
    for entry in collect_md_files_recursive(root)? {
        let stem = entry.file_stem().and_then(|value| value.to_str()).unwrap_or_default();
        if stem == name {
            matches.push(entry);
        }
    }
    Ok(matches)
}

fn determine_file_specification_path(
    draft: &ArtifactRef,
    display_name: &str,
    category: ArtifactCategory,
    drafts_root: &Path,
    specs_root: &Path,
) -> Result<PathBuf> {
    if draft.category == ArtifactCategory::Api {
        let file_name = format!("{display_name}.md");
        return Ok(match category {
            ArtifactCategory::Data => specs_root.join("data").join("external").join(file_name),
            ArtifactCategory::Context | ArtifactCategory::Api | ArtifactCategory::Root => {
                specs_root.join("contexts").join("external").join(file_name)
            }
        });
    }
    let relative = draft
        .path
        .strip_prefix(drafts_root)
        .unwrap_or(draft.path.as_path())
        .to_path_buf();
    let file_name = PathBuf::from(relative)
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| format!("{display_name}.md").into());
    let mut path = specs_root.to_path_buf();
    let subdir = category.projected_subdir(ArtifactKind::Specification);
    if !subdir.is_empty() {
        path.push(subdir);
    }
    path.push(file_name);
    Ok(path)
}

fn file_artifact_from_id(
    id: &str,
    drafts_root: &Path,
    specs_root: &Path,
    id_namespace: &str,
) -> Result<ArtifactRef> {
    let mut parts = id.splitn(3, ':');
    let _prefix = parts.next().unwrap_or_default();
    let kind_str = parts.next().unwrap_or_default();
    let relative = parts.next().unwrap_or_default();
    let _ = id_namespace;
    let kind = match kind_str {
        "draft" => ArtifactKind::Draft,
        "specification" => ArtifactKind::Specification,
        other => bail!("unsupported artifact kind '{other}' in id {id}"),
    };
    let root = match kind {
        ArtifactKind::Draft => drafts_root,
        ArtifactKind::Specification => specs_root,
    };
    file_artifact_from_path(&root.join(relative), drafts_root, specs_root, id_namespace)
}

fn file_artifact_from_path(
    path: &Path,
    drafts_root: &Path,
    specs_root: &Path,
    id_namespace: &str,
) -> Result<ArtifactRef> {
    let (kind, root) = if path.starts_with(drafts_root) {
        (ArtifactKind::Draft, drafts_root)
    } else if path.starts_with(specs_root) {
        (ArtifactKind::Specification, specs_root)
    } else {
        (ArtifactKind::Draft, drafts_root)
    };
    let relative = path.strip_prefix(root).unwrap_or(path);
    let first = relative
        .components()
        .next()
        .and_then(|component| component.as_os_str().to_str())
        .unwrap_or_default();
    let second = relative
        .components()
        .nth(1)
        .and_then(|component| component.as_os_str().to_str())
        .unwrap_or_default();
    let category = match first {
        "data" => ArtifactCategory::Data,
        "contexts" if kind == ArtifactKind::Specification && second == "external" => {
            ArtifactCategory::Api
        }
        "contexts" => ArtifactCategory::Context,
        "apis" | "external_apis" => ArtifactCategory::Api,
        _ => ArtifactCategory::Root,
    };
    let name = path
        .file_stem()
        .and_then(|value| value.to_str())
        .map(|value| value.to_string())
        .context("invalid file artifact name")?;
    Ok(ArtifactRef {
        id: format!(
            "{}:{}:{}",
            id_namespace,
            match kind {
                ArtifactKind::Draft => "draft",
                ArtifactKind::Specification => "specification",
            },
            relative.to_string_lossy()
        ),
        name,
        kind,
        category,
        path: path.to_path_buf(),
        source_draft_id: None,
    })
}

#[derive(Clone, Debug)]
struct GitHubIssuePayload {
    id: u64,
    node_id: String,
    number: u64,
    title: String,
    body: String,
    labels: Vec<String>,
}

impl GitHubIssuePayload {
    fn from_value(value: &Value) -> Result<Self> {
        let labels = value
            .get("labels")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("name").and_then(|v| v.as_str()))
                    .map(|label| label.to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Ok(Self {
            id: value
                .get("id")
                .and_then(|v| v.as_u64())
                .context("missing issue id")?,
            node_id: value
                .get("node_id")
                .and_then(|v| v.as_str())
                .map(|v| v.to_string())
                .context("missing issue node id")?,
            number: value
                .get("number")
                .and_then(|v| v.as_u64())
                .context("missing issue number")?,
            title: value
                .get("title")
                .and_then(|v| v.as_str())
                .map(|v| v.to_string())
                .context("missing issue title")?,
            body: value
                .get("body")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            labels,
        })
    }
}

fn parse_issue_artifact(
    owner: &str,
    repo: &str,
    drafts_root: &Path,
    specs_root: &Path,
    issue: &GitHubIssuePayload,
) -> Result<Option<(ArtifactRef, Option<String>)>> {
    let labels = issue.labels.iter().map(|label| label.as_str()).collect::<HashSet<_>>();
    let kind = if labels.contains("draft") {
        ArtifactKind::Draft
    } else if labels.contains("specification") {
        ArtifactKind::Specification
    } else {
        return Ok(None);
    };
    let category = if labels.contains("app") {
        ArtifactCategory::Root
    } else if labels.contains("data") {
        ArtifactCategory::Data
    } else if labels.contains("context") {
        ArtifactCategory::Context
    } else if labels.contains("api") {
        ArtifactCategory::Api
    } else {
        ArtifactCategory::Root
    };
    let root = match kind {
        ArtifactKind::Draft => drafts_root,
        ArtifactKind::Specification => specs_root,
    };
    let mut path = root.to_path_buf();
    let subdir = category.projected_subdir(kind);
    if !subdir.is_empty() {
        path.push(subdir);
    }
    if category == ArtifactCategory::Root {
        path.push("app.md");
    } else {
        path.push(format!("{}.md", sanitize_projected_name(&issue.title)));
    }
    let source_draft_ref = parse_issue_metadata_draft_ref(&issue.body);
    let source_draft_id = source_draft_ref
        .as_deref()
        .and_then(|reference| parse_issue_reference_number(reference).ok())
        .map(|number| github_issue_artifact_id(owner, repo, number));
    Ok(Some((
        ArtifactRef {
            id: github_issue_artifact_id(owner, repo, issue.number),
            name: issue.title.clone(),
            kind,
            category,
            path,
            source_draft_id,
        },
        source_draft_ref
            .and_then(|reference| parse_issue_reference_number(&reference).ok())
            .map(|number| github_issue_artifact_id(owner, repo, number)),
    )))
}

fn determine_file_draft_path(spec_path: &Path, drafts_root: &Path, specs_root: &Path) -> PathBuf {
    let relative = spec_path.strip_prefix(specs_root).unwrap_or(spec_path);
    let mut components = relative.components();
    let first = components.next().and_then(|c| c.as_os_str().to_str());
    let second = components.next().and_then(|c| c.as_os_str().to_str());
    if first == Some("contexts") && second == Some("external") {
        let remainder = PathBuf::from_iter(relative.components().skip(2));
        if remainder.components().count() <= 1 {
            return drafts_root.join("apis").join(remainder);
        }
        let api_name = remainder
            .components()
            .next()
            .and_then(|component| component.as_os_str().to_str())
            .unwrap_or_default();
        return drafts_root.join("apis").join(format!("{api_name}.md"));
    }
    drafts_root.join(relative)
}

fn load_github_issue_records(
    owner: &str,
    repo: &str,
    drafts_root: &Path,
    specs_root: &Path,
    scope: GitHubSyncScope,
) -> Result<(Vec<GitHubIssueRecord>, HashMap<u64, u64>)> {
    let issues = list_repo_issues_for_repo(owner, repo)?;
    let mut records = Vec::new();
    let mut issue_number_to_id = HashMap::new();

    for issue in issues {
        issue_number_to_id.insert(issue.number, issue.id);
        let parsed = parse_issue_artifact(owner, repo, drafts_root, specs_root, &issue)?;
        if let Some((artifact, source_draft_ref)) = parsed {
            if !scope.includes(artifact.kind) {
                continue;
            }
            records.push(GitHubIssueRecord {
                artifact: ArtifactRef {
                    source_draft_id: source_draft_ref,
                    ..artifact
                },
                issue_number: issue.number,
                issue_id: issue.id,
                node_id: issue.node_id,
                title: issue.title,
                body: issue.body,
                labels: issue.labels.into_iter().collect(),
                dependency_numbers: Vec::new(),
            });
        }
    }

    for record in &mut records {
        record.dependency_numbers = list_sub_issue_numbers_for_repo(owner, repo, record.issue_number)?;
    }

    Ok((records, issue_number_to_id))
}

fn github_issue_artifact_id(owner: &str, repo: &str, issue_number: u64) -> String {
    format!("github:{owner}/{repo}#{issue_number}")
}

pub fn github_issue_number_from_artifact_id(id: &str) -> Result<u64> {
    id.rsplit('#')
        .next()
        .context("missing GitHub issue number")?
        .parse::<u64>()
        .with_context(|| format!("invalid GitHub issue artifact id: {id}"))
}

fn parse_issue_reference_number(reference: &str) -> Result<u64> {
    reference
        .rsplit('#')
        .next()
        .context("missing issue reference number")?
        .parse::<u64>()
        .with_context(|| format!("invalid issue reference '{reference}'"))
}

fn parse_issue_metadata_draft_ref(body: &str) -> Option<String> {
    let start = "<!-- reen:metadata";
    let end = "-->";
    let metadata = body.split(start).nth(1)?.split(end).next()?;
    for line in metadata.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("draft:") {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn upsert_issue_metadata(content: &str, draft_ref: Option<&str>) -> String {
    let metadata = format!(
        "<!-- reen:metadata\ndraft: {}\n-->",
        draft_ref.unwrap_or_default()
    );
    let start = "<!-- reen:metadata";
    if let Some(existing_start) = content.find(start) {
        if let Some(_existing_end_rel) = content[existing_start..].find("-->") {
            let prefix = content[..existing_start].trim_end();
            return if prefix.is_empty() {
                metadata
            } else {
                format!("{prefix}\n\n{metadata}")
            };
        }
    }
    if content.trim().is_empty() {
        metadata
    } else {
        format!("{}\n\n{}", content.trim_end(), metadata)
    }
}

fn sanitize_projected_name(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    out
}

fn list_repo_issues_for_repo(owner: &str, repo: &str) -> Result<Vec<GitHubIssuePayload>> {
    let mut issues = Vec::new();
    for page in 1.. {
        let endpoint = format!("repos/{owner}/{repo}/issues?state=open&per_page=100&page={page}");
        let output = run_gh_json(&["api", endpoint.as_str()])?;
        let page_items: Vec<Value> =
            serde_json::from_slice(&output).context("failed to parse GitHub issues response")?;
        if page_items.is_empty() {
            break;
        }
        for item in page_items {
            if item.get("pull_request").is_some() {
                continue;
            }
            issues.push(GitHubIssuePayload::from_value(&item)?);
        }
    }
    Ok(issues)
}

fn list_sub_issue_numbers_for_repo(owner: &str, repo: &str, issue_number: u64) -> Result<Vec<u64>> {
    let query = r#"query($owner:String!, $repo:String!, $issueNumber:Int!) {
  repository(owner:$owner, name:$repo) {
    issue(number:$issueNumber) {
      subIssues(first:100) {
        nodes {
          number
        }
      }
    }
  }
}"#;
    let variables = serde_json::json!({
        "owner": owner,
        "repo": repo,
        "issueNumber": issue_number as i64,
    });
    let output = run_gh_graphql(query, variables)?;
    let value: Value = serde_json::from_slice(&output).context("failed parsing sub-issue query")?;
    Ok(value
        .get("data")
        .and_then(|v| v.get("repository"))
        .and_then(|v| v.get("issue"))
        .and_then(|v| v.get("subIssues"))
        .and_then(|v| v.get("nodes"))
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("number").and_then(|v| v.as_u64()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default())
}

fn upsert_repo_issue(
    owner: &str,
    repo: &str,
    issue_number: Option<u64>,
    title: &str,
    body: &str,
    labels: &[&str],
) -> Result<GitHubIssuePayload> {
    let payload = serde_json::json!({
        "title": title,
        "body": body,
        "labels": labels,
    });
    let endpoint = if let Some(number) = issue_number {
        format!("repos/{owner}/{repo}/issues/{number}")
    } else {
        format!("repos/{owner}/{repo}/issues")
    };
    let mut args = vec!["api".to_string(), endpoint];
    args.push("--method".to_string());
    args.push(if issue_number.is_some() {
        "PATCH".to_string()
    } else {
        "POST".to_string()
    });
    args.push("--input".to_string());
    args.push("-".to_string());
    let output = run_gh_json_stdin(
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
        payload.to_string().as_bytes(),
    )?;
    let value: Value =
        serde_json::from_slice(&output).context("failed parsing upserted GitHub issue")?;
    GitHubIssuePayload::from_value(&value)
}

fn run_gh_json(args: &[&str]) -> Result<Vec<u8>> {
    run_gh_json_stdin(args, &[])
}

fn run_gh_json_stdin(args: &[&str], stdin_bytes: &[u8]) -> Result<Vec<u8>> {
    let mut command = Command::new("gh");
    command.args(args);
    if !stdin_bytes.is_empty() {
        command.stdin(Stdio::piped());
    }
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    let mut child = command.spawn().context("failed to spawn gh command")?;
    if !stdin_bytes.is_empty() {
        use std::io::Write;
        let stdin = child.stdin.as_mut().context("gh stdin unavailable")?;
        stdin
            .write_all(stdin_bytes)
            .context("failed to write gh stdin payload")?;
    }
    let output = child
        .wait_with_output()
        .context("failed waiting for gh command")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("gh command failed: {}", stderr.trim()));
    }
    Ok(output.stdout)
}

fn run_gh_graphql(query: &str, variables: Value) -> Result<Vec<u8>> {
    let payload = serde_json::json!({
        "query": query,
        "variables": variables,
    });
    run_gh_json_stdin(
        &["api", "graphql", "--input", "-"],
        payload.to_string().as_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time ok")
            .as_nanos();
        std::env::temp_dir().join(format!("reen_artifact_backend_{}_{}", prefix, nanos))
    }

    #[test]
    fn parse_issue_artifact_maps_app_label_to_app_path() {
        let drafts_root = PathBuf::from(".reen/github/demo/drafts");
        let specs_root = PathBuf::from(".reen/github/demo/specifications");
        let issue = GitHubIssuePayload {
            id: 1,
            node_id: "node".to_string(),
            number: 12,
            title: "Snake".to_string(),
            body: "# Snake draft".to_string(),
            labels: vec!["draft".to_string(), "app".to_string()],
        };

        let (artifact, _) = parse_issue_artifact("demo", "snake", &drafts_root, &specs_root, &issue)
            .expect("parse issue")
            .expect("artifact");

        assert_eq!(artifact.kind, ArtifactKind::Draft);
        assert_eq!(artifact.category, ArtifactCategory::Root);
        assert_eq!(artifact.path, drafts_root.join("app.md"));
        assert_eq!(artifact.name, "Snake");
    }

    #[test]
    fn file_store_supports_custom_roots_and_namespaces() {
        let root = temp_root("custom_roots");
        let drafts_root = root.join("drafts");
        let specs_root = root.join("specifications");
        fs::create_dir_all(drafts_root.join("data")).expect("mkdir drafts data");
        fs::write(drafts_root.join("data/User.md"), "# User").expect("write draft");

        let store = FileArtifactStore::with_roots_and_namespace(
            drafts_root.clone(),
            specs_root,
            "github-sync:demo/snake".to_string(),
        );
        let artifacts = store
            .resolve_inputs(ArtifactKind::Draft, Vec::new(), &CategoryFilter::all())
            .expect("resolve inputs");

        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].path, drafts_root.join("data/User.md"));
        assert_eq!(artifacts[0].id, "github-sync:demo/snake:draft:data/User.md");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn publish_projected_specifications_counts_specs_in_dry_run() {
        let root = temp_root("publish_dry_run");
        let drafts_root = root.join("drafts");
        let specs_root = root.join("specifications");
        fs::create_dir_all(drafts_root.join("data")).expect("mkdir drafts data");
        fs::create_dir_all(specs_root.join("data")).expect("mkdir specs data");
        fs::write(drafts_root.join("data/User.md"), "# User draft").expect("write draft");
        fs::write(specs_root.join("data/User.md"), "# User spec").expect("write spec");

        let manifest = GitHubProjectionManifest {
            owner: "demo".to_string(),
            repo: "snake".to_string(),
            root: root.clone(),
            drafts_root: drafts_root.clone(),
            specifications_root: specs_root.clone(),
            entries: vec![
                GitHubProjectionEntry {
                    issue_number: 1,
                    issue_title: "User".to_string(),
                    artifact_id: "github:demo/snake#1".to_string(),
                    kind: ArtifactKind::Draft,
                    category: ArtifactCategory::Data,
                    path: drafts_root.join("data/User.md"),
                    source_draft_id: None,
                    dependency_numbers: Vec::new(),
                },
                GitHubProjectionEntry {
                    issue_number: 2,
                    issue_title: "User".to_string(),
                    artifact_id: "github:demo/snake#2".to_string(),
                    kind: ArtifactKind::Specification,
                    category: ArtifactCategory::Data,
                    path: specs_root.join("data/User.md"),
                    source_draft_id: Some("github:demo/snake#1".to_string()),
                    dependency_numbers: Vec::new(),
                },
            ],
        };

        let published = publish_projected_specifications(&manifest, true).expect("dry run publish");
        assert_eq!(published, 1);

        let _ = fs::remove_dir_all(root);
    }
}
