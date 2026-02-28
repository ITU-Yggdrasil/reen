use anyhow::{Context, Result};
use regex::Regex;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencySource {
    Primary,
    Fallback,
}

#[derive(Clone, Debug, Serialize)]
pub struct DependencyArtifact {
    pub name: String,
    pub path: String,
    pub source: DependencySource,
    pub content: String,
    pub sha256: String,
}

#[derive(Clone, Debug)]
pub struct ExecutionNode {
    pub name: String,
    pub input_path: PathBuf,
    direct_dependencies: Vec<DependencyLocator>,
}

impl ExecutionNode {
    pub fn direct_dependency_names(&self) -> Vec<String> {
        self.direct_dependencies
            .iter()
            .map(|d| d.name.clone())
            .collect()
    }

    pub fn resolve_direct_dependencies(&self) -> Result<Vec<DependencyArtifact>> {
        let mut resolved = Vec::new();
        let mut seen = HashSet::new();

        for dep in &self.direct_dependencies {
            let (path, source) = if let Some(primary) = dep.primary_path.as_ref() {
                if primary.exists() {
                    (primary.clone(), DependencySource::Primary)
                } else if let Some(fallback) = dep.fallback_path.as_ref() {
                    if fallback.exists() {
                        (fallback.clone(), DependencySource::Fallback)
                    } else {
                        continue;
                    }
                } else {
                    continue;
                }
            } else if let Some(fallback) = dep.fallback_path.as_ref() {
                if fallback.exists() {
                    (fallback.clone(), DependencySource::Fallback)
                } else {
                    continue;
                }
            } else {
                continue;
            };

            let key = path.to_string_lossy().to_string();
            if !seen.insert(key.clone()) {
                continue;
            }

            let content = fs::read_to_string(&path).with_context(|| {
                format!("failed reading dependency artifact: {}", path.display())
            })?;
            let mut hasher = Sha256::new();
            hasher.update(content.as_bytes());
            let sha256 = hex::encode(hasher.finalize());

            resolved.push(DependencyArtifact {
                name: dep.name.clone(),
                path: key,
                source,
                content,
                sha256,
            });
        }

        Ok(resolved)
    }

    pub fn resolve_dependency_closure(
        &self,
        primary_root: &str,
        fallback_root: Option<&str>,
    ) -> Result<Vec<DependencyArtifact>> {
        let primary_index = build_index(primary_root)?;
        let fallback_index = match fallback_root {
            Some(root) => build_index(root)?,
            None => Vec::new(),
        };
        let primary_by_canonical = index_by_canonical(&primary_index);
        let fallback_by_canonical = index_by_canonical(&fallback_index);

        let mut queue = self.direct_dependencies.clone();
        let mut seen_paths = HashSet::new();
        let mut resolved = Vec::new();

        while let Some(dep) = queue.pop() {
            let (path, source) = match resolve_dependency_locator(&dep) {
                Some(v) => v,
                None => continue,
            };

            if path == self.input_path {
                continue;
            }

            let key = path.to_string_lossy().to_string();
            if !seen_paths.insert(key.clone()) {
                continue;
            }

            let content = fs::read_to_string(&path).with_context(|| {
                format!("failed reading dependency artifact: {}", path.display())
            })?;
            let mut hasher = Sha256::new();
            hasher.update(content.as_bytes());
            let sha256 = hex::encode(hasher.finalize());

            resolved.push(DependencyArtifact {
                name: dep.name.clone(),
                path: key,
                source,
                content: content.clone(),
                sha256,
            });

            let canonicals =
                extract_dependency_canonicals(&content, &primary_index, &fallback_index);
            for canonical in canonicals {
                let primary_candidates = primary_by_canonical
                    .get(&canonical)
                    .cloned()
                    .unwrap_or_default();
                let fallback_candidates = fallback_by_canonical
                    .get(&canonical)
                    .cloned()
                    .unwrap_or_default();

                if !primary_candidates.is_empty() {
                    for candidate in primary_candidates {
                        if candidate.path == path || candidate.path == self.input_path {
                            continue;
                        }

                        queue.push(DependencyLocator {
                            name: candidate.name.clone(),
                            primary_path: Some(candidate.path.clone()),
                            fallback_path: fallback_candidates.first().map(|f| f.path.clone()),
                        });
                    }
                } else {
                    for candidate in fallback_candidates {
                        if candidate.path == path || candidate.path == self.input_path {
                            continue;
                        }
                        queue.push(DependencyLocator {
                            name: candidate.name.clone(),
                            primary_path: None,
                            fallback_path: Some(candidate.path.clone()),
                        });
                    }
                }
            }
        }

        resolved.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(resolved)
    }
}

#[derive(Clone, Debug)]
struct DependencyLocator {
    name: String,
    primary_path: Option<PathBuf>,
    fallback_path: Option<PathBuf>,
}

fn resolve_dependency_locator(dep: &DependencyLocator) -> Option<(PathBuf, DependencySource)> {
    if let Some(primary) = dep.primary_path.as_ref() {
        if primary.exists() {
            return Some((primary.clone(), DependencySource::Primary));
        }
        if let Some(fallback) = dep.fallback_path.as_ref() {
            if fallback.exists() {
                return Some((fallback.clone(), DependencySource::Fallback));
            }
        }
        return None;
    }

    if let Some(fallback) = dep.fallback_path.as_ref() {
        if fallback.exists() {
            return Some((fallback.clone(), DependencySource::Fallback));
        }
    }

    None
}

#[derive(Clone, Debug)]
struct IndexedArtifact {
    name: String,
    canonical: String,
    path: PathBuf,
    token_len: usize,
}

pub fn build_execution_plan(
    selected_inputs: Vec<PathBuf>,
    primary_root: &str,
    fallback_root: Option<&str>,
) -> Result<Vec<Vec<ExecutionNode>>> {
    if selected_inputs.is_empty() {
        return Ok(Vec::new());
    }

    let primary_index = build_index(primary_root)?;
    let fallback_index = match fallback_root {
        Some(root) => build_index(root)?,
        None => Vec::new(),
    };

    let primary_by_canonical = index_by_canonical(&primary_index);
    let fallback_by_canonical = index_by_canonical(&fallback_index);
    let selected_set: HashSet<PathBuf> = selected_inputs.iter().cloned().collect();

    let mut nodes = Vec::new();
    let mut edges: Vec<Vec<usize>> = Vec::new();
    let mut selected_position = HashMap::new();

    for (idx, path) in selected_inputs.iter().enumerate() {
        selected_position.insert(path.clone(), idx);
    }

    for input_path in &selected_inputs {
        let name = input_path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .context("invalid input filename")?;
        let content = fs::read_to_string(input_path)
            .with_context(|| format!("failed reading input file: {}", input_path.display()))?;

        let mut direct_dependencies = Vec::new();
        let mut edge_targets = Vec::new();

        let canonicals = extract_dependency_canonicals(&content, &primary_index, &fallback_index);
        for canonical in canonicals {
            let primary_candidates = primary_by_canonical
                .get(&canonical)
                .cloned()
                .unwrap_or_default();
            let fallback_candidates = fallback_by_canonical
                .get(&canonical)
                .cloned()
                .unwrap_or_default();

            if primary_candidates.is_empty() && fallback_candidates.is_empty() {
                continue;
            }

            if !primary_candidates.is_empty() {
                for candidate in primary_candidates {
                    if candidate.path == *input_path {
                        continue;
                    }

                    direct_dependencies.push(DependencyLocator {
                        name: candidate.name.clone(),
                        primary_path: Some(candidate.path.clone()),
                        fallback_path: fallback_candidates.first().map(|f| f.path.clone()),
                    });

                    if selected_set.contains(&candidate.path) {
                        if let Some(pos) = selected_position.get(&candidate.path) {
                            edge_targets.push(*pos);
                        }
                    }
                }
            } else {
                for candidate in fallback_candidates {
                    if candidate.path == *input_path {
                        continue;
                    }

                    direct_dependencies.push(DependencyLocator {
                        name: candidate.name.clone(),
                        primary_path: None,
                        fallback_path: Some(candidate.path.clone()),
                    });
                }
            }
        }

        nodes.push(ExecutionNode {
            name,
            input_path: input_path.clone(),
            direct_dependencies,
        });
        edge_targets.sort_unstable();
        edge_targets.dedup();
        edges.push(edge_targets);
    }

    let levels = levelize_with_cycles(nodes, edges);
    Ok(levels)
}

fn levelize_with_cycles(
    nodes: Vec<ExecutionNode>,
    edges: Vec<Vec<usize>>,
) -> Vec<Vec<ExecutionNode>> {
    let components = strongly_connected_components(edges.as_slice());
    let component_count = components.len();
    let mut node_component = vec![0usize; nodes.len()];
    for (component_id, component_nodes) in components.iter().enumerate() {
        for &node_idx in component_nodes {
            node_component[node_idx] = component_id;
        }
    }

    let mut component_deps: Vec<HashSet<usize>> = vec![HashSet::new(); component_count];
    for (node_idx, deps) in edges.iter().enumerate() {
        let from_component = node_component[node_idx];
        for &dep_idx in deps {
            let dep_component = node_component[dep_idx];
            if from_component != dep_component {
                component_deps[from_component].insert(dep_component);
            }
        }
    }

    let mut nodes_by_component: Vec<Vec<ExecutionNode>> = vec![Vec::new(); component_count];
    for (idx, node) in nodes.into_iter().enumerate() {
        nodes_by_component[node_component[idx]].push(node);
    }
    for group in &mut nodes_by_component {
        group.sort_by(|a, b| a.input_path.cmp(&b.input_path));
    }

    let mut remaining: HashSet<usize> = (0..component_count).collect();
    let mut levels = Vec::new();

    while !remaining.is_empty() {
        let mut current_components: Vec<usize> = remaining
            .iter()
            .copied()
            .filter(|component| {
                component_deps[*component]
                    .iter()
                    .all(|dep| !remaining.contains(dep))
            })
            .collect();

        if current_components.is_empty() {
            current_components = remaining.iter().copied().collect();
        }

        current_components.sort_by(|a, b| {
            let a_key = nodes_by_component[*a]
                .first()
                .map(|n| n.input_path.to_string_lossy().to_string())
                .unwrap_or_default();
            let b_key = nodes_by_component[*b]
                .first()
                .map(|n| n.input_path.to_string_lossy().to_string())
                .unwrap_or_default();
            a_key.cmp(&b_key)
        });

        let mut level_nodes = Vec::new();
        for component in &current_components {
            remaining.remove(component);
            level_nodes.extend(nodes_by_component[*component].clone());
        }

        level_nodes.sort_by(|a, b| a.input_path.cmp(&b.input_path));
        levels.push(level_nodes);
    }

    levels
}

fn strongly_connected_components(edges: &[Vec<usize>]) -> Vec<Vec<usize>> {
    struct Tarjan<'a> {
        edges: &'a [Vec<usize>],
        index: usize,
        stack: Vec<usize>,
        on_stack: Vec<bool>,
        indices: Vec<Option<usize>>,
        lowlink: Vec<usize>,
        components: Vec<Vec<usize>>,
    }

    impl<'a> Tarjan<'a> {
        fn new(edges: &'a [Vec<usize>]) -> Self {
            let n = edges.len();
            Self {
                edges,
                index: 0,
                stack: Vec::new(),
                on_stack: vec![false; n],
                indices: vec![None; n],
                lowlink: vec![0; n],
                components: Vec::new(),
            }
        }

        fn run(mut self) -> Vec<Vec<usize>> {
            for v in 0..self.edges.len() {
                if self.indices[v].is_none() {
                    self.strong_connect(v);
                }
            }
            self.components
        }

        fn strong_connect(&mut self, v: usize) {
            self.indices[v] = Some(self.index);
            self.lowlink[v] = self.index;
            self.index += 1;
            self.stack.push(v);
            self.on_stack[v] = true;

            for &w in &self.edges[v] {
                if self.indices[w].is_none() {
                    self.strong_connect(w);
                    self.lowlink[v] = self.lowlink[v].min(self.lowlink[w]);
                } else if self.on_stack[w] {
                    let w_index = self.indices[w].unwrap_or(usize::MAX);
                    self.lowlink[v] = self.lowlink[v].min(w_index);
                }
            }

            if self.lowlink[v] == self.indices[v].unwrap_or(usize::MAX) {
                let mut component = Vec::new();
                while let Some(w) = self.stack.pop() {
                    self.on_stack[w] = false;
                    component.push(w);
                    if w == v {
                        break;
                    }
                }
                component.sort_unstable();
                self.components.push(component);
            }
        }
    }

    Tarjan::new(edges).run()
}

fn build_index(root: &str) -> Result<Vec<IndexedArtifact>> {
    let root_path = PathBuf::from(root);
    if !root_path.exists() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    collect_markdown_files(&root_path, &mut files)?;
    let mut artifacts = Vec::new();

    for path in files {
        let name = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => continue,
        };
        let canonical = canonicalize(&name);
        if canonical.is_empty() {
            continue;
        }

        let token_len = token_count(&name).max(1);
        artifacts.push(IndexedArtifact {
            name,
            canonical,
            path,
            token_len,
        });
    }

    Ok(artifacts)
}

fn index_by_canonical(index: &[IndexedArtifact]) -> HashMap<String, Vec<IndexedArtifact>> {
    let mut by_canonical: HashMap<String, Vec<IndexedArtifact>> = HashMap::new();
    for entry in index {
        by_canonical
            .entry(entry.canonical.clone())
            .or_default()
            .push(entry.clone());
    }
    for values in by_canonical.values_mut() {
        values.sort_by(|a, b| a.path.cmp(&b.path));
    }
    by_canonical
}

fn extract_dependency_canonicals(
    content: &str,
    primary_index: &[IndexedArtifact],
    fallback_index: &[IndexedArtifact],
) -> BTreeSet<String> {
    let mut known = HashSet::new();
    let mut max_tokens = 1usize;

    for entry in primary_index.iter().chain(fallback_index.iter()) {
        known.insert(entry.canonical.clone());
        max_tokens = max_tokens.max(entry.token_len);
    }

    let mut discovered = BTreeSet::new();
    if known.is_empty() {
        return discovered;
    }

    let explicit = extract_explicit_dependency_names(content);
    for name in explicit {
        let canonical = canonicalize(&name);
        if known.contains(&canonical) {
            discovered.insert(canonical);
        }
    }

    let token_re = Regex::new(r"[A-Za-z0-9]+").expect("valid token regex");
    let tokens: Vec<String> = token_re
        .find_iter(content)
        .map(|m| m.as_str().to_lowercase())
        .collect();

    for i in 0..tokens.len() {
        let mut merged = String::new();
        for len in 1..=max_tokens {
            if i + len > tokens.len() {
                break;
            }
            merged.push_str(&tokens[i + len - 1]);
            if known.contains(&merged) {
                discovered.insert(merged.clone());
            }
        }
    }

    discovered
}

fn extract_explicit_dependency_names(content: &str) -> Vec<String> {
    let depends_on_re =
        Regex::new(r"(?im)^\s*depends\s+on\s*:\s*(.+)\s*$").expect("valid depends regex");
    let mut names = Vec::new();
    for captures in depends_on_re.captures_iter(content) {
        if let Some(raw) = captures.get(1) {
            for token in raw.as_str().split(',') {
                let trimmed = token.trim();
                if !trimmed.is_empty() {
                    names.push(trimmed.to_string());
                }
            }
        }
    }
    names
}

fn canonicalize(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn token_count(raw: &str) -> usize {
    let token_re = Regex::new(r"[A-Za-z0-9]+").expect("valid token regex");
    token_re.find_iter(raw).count()
}

fn collect_markdown_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    let entries = fs::read_dir(dir)
        .with_context(|| format!("failed to read directory: {}", dir.display()))?;
    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            collect_markdown_files(&path, files)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            files.push(path);
        }
    }
    Ok(())
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
        std::env::temp_dir().join(format!("reen_dep_graph_{}_{}", prefix, nanos))
    }

    #[test]
    fn plan_runs_leaves_before_dependents() {
        let root = temp_root("levels");
        let drafts = root.join("drafts");
        fs::create_dir_all(drafts.join("contexts")).expect("mkdir");

        let a = drafts.join("contexts").join("a.md");
        let b = drafts.join("contexts").join("b.md");
        let c = drafts.join("app.md");
        fs::write(&a, "no deps").expect("write");
        fs::write(&b, "uses a").expect("write");
        fs::write(&c, "depends on: b").expect("write");

        let selected = vec![a.clone(), b.clone(), c.clone()];
        let levels = build_execution_plan(selected, drafts.to_str().unwrap_or("drafts"), None)
            .expect("plan");

        assert_eq!(levels.len(), 3);
        assert_eq!(levels[0].len(), 1);
        assert_eq!(levels[0][0].input_path, a);
        assert_eq!(levels[1][0].input_path, b);
        assert_eq!(levels[2][0].input_path, c);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cycle_is_grouped_and_does_not_fail() {
        let root = temp_root("cycle");
        let drafts = root.join("drafts");
        fs::create_dir_all(&drafts).expect("mkdir");

        let x = drafts.join("x.md");
        let y = drafts.join("y.md");
        fs::write(&x, "depends on: y").expect("write");
        fs::write(&y, "depends on: x").expect("write");

        let levels = build_execution_plan(
            vec![x.clone(), y.clone()],
            drafts.to_str().unwrap_or("drafts"),
            None,
        )
        .expect("plan");

        assert_eq!(levels.len(), 1);
        assert_eq!(levels[0].len(), 2);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn dependency_falls_back_when_primary_is_missing() {
        let root = temp_root("fallback");
        let specs = root.join("specifications");
        let drafts = root.join("drafts");
        fs::create_dir_all(specs.join("contexts")).expect("mkdir");
        fs::create_dir_all(drafts.join("contexts")).expect("mkdir");

        let app_spec = specs.join("app.md");
        let money_transfer_draft = drafts.join("contexts").join("money_transfer.md");
        fs::write(&app_spec, "uses money transfer context").expect("write");
        fs::write(&money_transfer_draft, "draft context").expect("write");

        let levels = build_execution_plan(
            vec![app_spec.clone()],
            specs.to_str().unwrap_or("specifications"),
            Some(drafts.to_str().unwrap_or("drafts")),
        )
        .expect("plan");

        let deps = levels[0][0].resolve_direct_dependencies().expect("deps");
        assert_eq!(deps.len(), 1);
        assert!(matches!(deps[0].source, DependencySource::Fallback));
        assert!(deps[0].path.ends_with("money_transfer.md"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn dependency_closure_includes_transitive_dependencies() {
        let root = temp_root("closure");
        let drafts = root.join("drafts");
        fs::create_dir_all(drafts.join("data")).expect("mkdir");

        let app = drafts.join("app.md");
        let amount = drafts.join("data").join("amount.md");
        let currency = drafts.join("data").join("currency.md");

        fs::write(&app, "Uses Amount and requires DKK to be supported").expect("write");
        fs::write(&amount, "Amount stores a Currency value").expect("write");
        fs::write(&currency, "Currency enum includes DKK").expect("write");

        let levels =
            build_execution_plan(vec![app.clone()], drafts.to_str().unwrap_or("drafts"), None)
                .expect("plan");

        let closure = levels[0][0]
            .resolve_dependency_closure(drafts.to_str().unwrap_or("drafts"), None)
            .expect("closure");
        let paths: Vec<String> = closure.iter().map(|d| d.path.clone()).collect();

        assert!(paths.iter().any(|p| p.ends_with("amount.md")));
        assert!(paths.iter().any(|p| p.ends_with("currency.md")));

        let _ = fs::remove_dir_all(root);
    }
}
