use crate::prepared::PreparedArtifact;
use crate::workspace::{DRAFTS_DIR, PREPARED_DIR, Workspace};
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct ArtifactSpecContext {
    pub prepared_relative: String,
    pub prepared_yaml: String,
    pub draft_relative: Option<String>,
    pub draft_markdown: Option<String>,
}

impl ArtifactSpecContext {
    pub(crate) fn render_prompt_block(&self) -> String {
        self.render_prompt_block_with_heading("##")
    }

    pub(crate) fn render_prompt_block_with_heading(&self, heading: &str) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "{} {}\n```yaml\n{}\n```\n\n",
            heading, self.prepared_relative, self.prepared_yaml
        ));
        if let (Some(relative), Some(markdown)) = (&self.draft_relative, &self.draft_markdown) {
            out.push_str(&format!(
                "{} {}\n```md\n{}\n```\n\n",
                heading, relative, markdown
            ));
        }
        out
    }
}

pub(crate) fn load_artifact_spec_context_for_generated_file(
    workspace: &Workspace,
    generated_file: &Path,
) -> Result<Option<ArtifactSpecContext>> {
    let Some(prepared_relative) = prepared_relative_for_generated_file(workspace, generated_file)
    else {
        return Ok(None);
    };
    let prepared_path = workspace.root.join(&prepared_relative);
    if !prepared_path.is_file() {
        return Ok(None);
    }

    let prepared_yaml = fs::read_to_string(&prepared_path)
        .with_context(|| format!("Failed to read {}", prepared_path.display()))?;
    let artifact: PreparedArtifact = serde_yaml::from_str(&prepared_yaml)
        .with_context(|| format!("Failed to parse {}", prepared_path.display()))?;

    let draft_path = resolve_draft_path(workspace, &artifact.source.path);
    let (draft_relative, draft_markdown) = if draft_path.is_file() {
        let markdown = fs::read_to_string(&draft_path)
            .with_context(|| format!("Failed to read {}", draft_path.display()))?;
        (
            Some(
                draft_path
                    .strip_prefix(&workspace.root)
                    .unwrap_or(&draft_path)
                    .to_string_lossy()
                    .replace('\\', "/"),
            ),
            Some(markdown),
        )
    } else {
        (None, None)
    };

    Ok(Some(ArtifactSpecContext {
        prepared_relative: prepared_relative.to_string_lossy().replace('\\', "/"),
        prepared_yaml,
        draft_relative,
        draft_markdown,
    }))
}

pub(crate) fn prepared_relative_for_generated_file(
    workspace: &Workspace,
    generated_file: &Path,
) -> Option<PathBuf> {
    let relative = generated_file
        .strip_prefix(&workspace.root)
        .unwrap_or(generated_file);
    let normalized = relative.to_string_lossy().replace('\\', "/");
    if normalized == "src/main.rs" {
        return Some(PathBuf::from(PREPARED_DIR).join("app.yml"));
    }

    let parts = normalized.split('/').collect::<Vec<_>>();
    if parts.len() != 3 || parts[0] != "src" {
        return None;
    }

    let section = parts[1];
    if !matches!(section, "contexts" | "projections" | "data") {
        return None;
    }
    let stem = Path::new(parts[2])
        .file_stem()?
        .to_string_lossy()
        .to_string();
    Some(
        PathBuf::from(PREPARED_DIR)
            .join(section)
            .join(format!("{stem}.yml")),
    )
}

fn resolve_draft_path(workspace: &Workspace, source_path: &str) -> PathBuf {
    let source = PathBuf::from(source_path);
    if source.is_absolute() {
        return source;
    }
    if source
        .components()
        .next()
        .and_then(|component| component.as_os_str().to_str())
        == Some(DRAFTS_DIR)
    {
        return workspace.root.join(source);
    }
    workspace.root.join(DRAFTS_DIR).join(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Workspace;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn load_artifact_spec_context_maps_generated_context_to_prepare_and_draft() {
        let root = temp_root("spec_context");
        fs::create_dir_all(root.join("drafts/prepare/contexts")).unwrap();
        fs::create_dir_all(root.join("drafts/contexts")).unwrap();
        fs::create_dir_all(root.join("src/contexts")).unwrap();
        fs::write(
            root.join("drafts/prepare/contexts/game_loop.yml"),
            r#"schema: reen.prepare/v1
source:
  path: contexts/game_loop.md
  kind: context
  title: GameLoop
export:
  name: GameLoopContext
mutable: true
"#,
        )
        .unwrap();
        fs::write(
            root.join("drafts/contexts/game_loop.md"),
            "# GameLoop\n\nBody.\n",
        )
        .unwrap();
        let workspace = Workspace::discover(root.clone()).unwrap();

        let context = load_artifact_spec_context_for_generated_file(
            &workspace,
            &root.join("src/contexts/game_loop.rs"),
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            context.prepared_relative,
            "drafts/prepare/contexts/game_loop.yml"
        );
        assert_eq!(
            context.draft_relative.as_deref(),
            Some("drafts/contexts/game_loop.md")
        );
        assert!(context.prepared_yaml.contains("GameLoopContext"));
        assert!(
            context
                .draft_markdown
                .as_deref()
                .unwrap()
                .contains("# GameLoop")
        );
    }

    fn temp_root(prefix: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("reen_{prefix}_{stamp}"))
    }
}
