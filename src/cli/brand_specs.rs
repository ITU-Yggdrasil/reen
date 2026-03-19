use anyhow::{anyhow, bail, Context, Result};
use regex::Regex;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BrandSpecValidation {
    pub blocking_ambiguities: Vec<String>,
    pub defined_tokens: BTreeSet<String>,
}

pub fn is_brand_draft_path(path: &Path, drafts_dir: &str) -> bool {
    path.strip_prefix(Path::new(drafts_dir))
        .ok()
        .and_then(|relative| relative.components().next())
        .and_then(|component| component.as_os_str().to_str())
        == Some("brands")
}

pub fn is_brand_spec_path(path: &Path, specifications_dir: &str) -> bool {
    path.strip_prefix(Path::new(specifications_dir))
        .ok()
        .and_then(|relative| relative.components().next())
        .and_then(|component| component.as_os_str().to_str())
        == Some("brands")
}

pub fn validate_brand_spec_content(spec_content: &str) -> Result<BrandSpecValidation> {
    let root: Value =
        serde_json::from_str(spec_content).context("brand specification must be valid JSON")?;
    let obj = root
        .as_object()
        .ok_or_else(|| anyhow!("brand specification root must be a JSON object"))?;

    expect_exact_string(obj.get("schema_version"), "schema_version", "1.0")?;
    expect_non_empty_string(obj.get("brand_id"), "brand_id")?;

    let color_tokens = expect_object(obj.get("color_tokens"), "color_tokens")?;
    if color_tokens.is_empty() {
        bail!("brand specification field 'color_tokens' must not be empty");
    }

    let typography = expect_object(obj.get("typography"), "typography")?;
    expect_object(typography.get("font_families"), "typography.font_families")?;
    expect_object(typography.get("font_sizes"), "typography.font_sizes")?;
    expect_object(typography.get("font_weights"), "typography.font_weights")?;
    expect_object(typography.get("line_heights"), "typography.line_heights")?;
    expect_object(typography.get("text_styles"), "typography.text_styles")?;

    let spacing = expect_object(obj.get("spacing"), "spacing")?;
    expect_number(spacing.get("base_unit"), "spacing.base_unit")?;
    expect_object(spacing.get("scale"), "spacing.scale")?;
    expect_object(spacing.get("layout_margins"), "spacing.layout_margins")?;

    expect_object(obj.get("iconography"), "iconography")?;
    expect_object(obj.get("motion"), "motion")?;

    let layout = expect_object(obj.get("layout"), "layout")?;
    expect_object(layout.get("grid"), "layout.grid")?;
    expect_object(layout.get("breakpoints"), "layout.breakpoints")?;
    expect_object(layout.get("container_widths"), "layout.container_widths")?;

    let blocking_ambiguities = obj
        .get("blocking_ambiguities")
        .ok_or_else(|| anyhow!("brand specification field 'blocking_ambiguities' is required"))?
        .as_array()
        .ok_or_else(|| anyhow!("brand specification field 'blocking_ambiguities' must be an array"))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .ok_or_else(|| {
                    anyhow!(
                        "brand specification field 'blocking_ambiguities' must contain non-empty strings"
                    )
                })
        })
        .collect::<Result<Vec<_>>>()?;

    let mut defined_tokens = BTreeSet::new();
    collect_defined_tokens(
        obj.get("color_tokens").unwrap_or(&Value::Null),
        &mut vec!["brand".to_string(), "colors".to_string()],
        &mut defined_tokens,
    );
    collect_defined_tokens(
        obj.get("typography").unwrap_or(&Value::Null),
        &mut vec!["brand".to_string(), "typography".to_string()],
        &mut defined_tokens,
    );
    collect_defined_tokens(
        obj.get("spacing").unwrap_or(&Value::Null),
        &mut vec!["brand".to_string(), "spacing".to_string()],
        &mut defined_tokens,
    );
    collect_defined_tokens(
        obj.get("iconography").unwrap_or(&Value::Null),
        &mut vec!["brand".to_string(), "iconography".to_string()],
        &mut defined_tokens,
    );
    collect_defined_tokens(
        obj.get("motion").unwrap_or(&Value::Null),
        &mut vec!["brand".to_string(), "motion".to_string()],
        &mut defined_tokens,
    );
    collect_defined_tokens(
        obj.get("layout").unwrap_or(&Value::Null),
        &mut vec!["brand".to_string(), "layout".to_string()],
        &mut defined_tokens,
    );

    Ok(BrandSpecValidation {
        blocking_ambiguities,
        defined_tokens,
    })
}

pub fn collect_brand_token_references(content: &str) -> Vec<String> {
    let pattern = Regex::new(r"\bbrand(?:\.[A-Za-z0-9_-]+)+\b").expect("brand ref regex");
    let mut refs = BTreeSet::new();
    for m in pattern.find_iter(content) {
        refs.insert(m.as_str().to_string());
    }
    refs.into_iter().collect()
}

pub fn unresolved_brand_token_references(
    content: &str,
    specifications_dir: &str,
) -> Result<Vec<String>> {
    let references = collect_brand_token_references(content);
    if references.is_empty() {
        return Ok(Vec::new());
    }

    let defined = collect_defined_brand_tokens(specifications_dir)?;
    Ok(references
        .into_iter()
        .filter(|reference| !defined.contains(reference))
        .collect())
}

fn collect_defined_brand_tokens(specifications_dir: &str) -> Result<BTreeSet<String>> {
    let brand_dir = Path::new(specifications_dir).join("brands");
    if !brand_dir.exists() {
        return Ok(BTreeSet::new());
    }

    let mut files = Vec::new();
    collect_json_files_recursive(&brand_dir, &mut files)?;
    files.sort();

    let mut token_to_file: HashMap<String, PathBuf> = HashMap::new();
    for file in files {
        let content = fs::read_to_string(&file)
            .with_context(|| format!("failed reading brand specification {}", file.display()))?;
        let validation = validate_brand_spec_content(&content)
            .with_context(|| format!("invalid brand specification {}", file.display()))?;
        for token in validation.defined_tokens {
            if let Some(existing) = token_to_file.insert(token.clone(), file.clone()) {
                bail!(
                    "duplicate brand token '{}' defined in both {} and {}",
                    token,
                    existing.display(),
                    file.display()
                );
            }
        }
    }

    Ok(token_to_file.into_keys().collect())
}

fn collect_json_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.exists() || !dir.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            collect_json_files_recursive(&path, files)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            files.push(path);
        }
    }

    Ok(())
}

fn collect_defined_tokens(value: &Value, path: &mut Vec<String>, out: &mut BTreeSet<String>) {
    match value {
        Value::Null => {}
        Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            out.insert(path.join("."));
        }
        Value::Array(items) => {
            out.insert(path.join("."));
            for (index, item) in items.iter().enumerate() {
                path.push(index.to_string());
                collect_defined_tokens(item, path, out);
                path.pop();
            }
        }
        Value::Object(map) => {
            if map.is_empty() {
                out.insert(path.join("."));
                return;
            }
            for (key, child) in map {
                path.push(key.clone());
                collect_defined_tokens(child, path, out);
                path.pop();
            }
        }
    }
}

fn expect_object<'a>(
    value: Option<&'a Value>,
    field: &str,
) -> Result<&'a serde_json::Map<String, Value>> {
    value
        .ok_or_else(|| anyhow!("brand specification field '{}' is required", field))?
        .as_object()
        .ok_or_else(|| anyhow!("brand specification field '{}' must be an object", field))
}

fn expect_non_empty_string(value: Option<&Value>, field: &str) -> Result<()> {
    let text = value
        .ok_or_else(|| anyhow!("brand specification field '{}' is required", field))?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            anyhow!(
                "brand specification field '{}' must be a non-empty string",
                field
            )
        })?;
    if text.is_empty() {
        bail!(
            "brand specification field '{}' must be a non-empty string",
            field
        );
    }
    Ok(())
}

fn expect_exact_string(value: Option<&Value>, field: &str, expected: &str) -> Result<()> {
    let text = value
        .ok_or_else(|| anyhow!("brand specification field '{}' is required", field))?
        .as_str()
        .ok_or_else(|| anyhow!("brand specification field '{}' must be a string", field))?;
    if text != expected {
        bail!(
            "brand specification field '{}' must equal '{}'",
            field,
            expected
        );
    }
    Ok(())
}

fn expect_number(value: Option<&Value>, field: &str) -> Result<()> {
    if value
        .ok_or_else(|| anyhow!("brand specification field '{}' is required", field))?
        .is_number()
    {
        Ok(())
    } else {
        bail!("brand specification field '{}' must be a number", field);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        collect_brand_token_references, unresolved_brand_token_references,
        validate_brand_spec_content,
    };
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time ok")
            .as_nanos();
        std::env::temp_dir().join(format!("reen_brand_specs_{}_{}", prefix, nanos))
    }

    #[test]
    fn validates_brand_spec_and_collects_tokens() {
        let spec = r##"{
          "schema_version": "1.0",
          "brand_id": "acme",
          "color_tokens": { "primary": { "default": "#112233" } },
          "typography": {
            "font_families": { "heading": "Fraunces" },
            "font_sizes": { "md": 16 },
            "font_weights": { "regular": 400 },
            "line_heights": { "md": 24 },
            "text_styles": { "body": { "font_size": "md" } }
          },
          "spacing": {
            "base_unit": 4,
            "scale": { "4": 16 },
            "layout_margins": { "desktop": 64 }
          },
          "iconography": { "stroke_width": 1.5 },
          "motion": { "durations": { "fast": 120 } },
          "layout": {
            "grid": { "columns": 12 },
            "breakpoints": { "md": 768 },
            "container_widths": { "lg": 1200 }
          },
          "blocking_ambiguities": []
        }"##;

        let validation = validate_brand_spec_content(spec).expect("valid brand spec");
        assert!(validation
            .defined_tokens
            .contains("brand.colors.primary.default"));
        assert!(validation
            .defined_tokens
            .contains("brand.typography.font_families.heading"));
    }

    #[test]
    fn collects_references_from_markdown() {
        let refs = collect_brand_token_references(
            "Use `brand.colors.primary.default` and brand.spacing.scale.4 in the layout.",
        );
        assert_eq!(
            refs,
            vec![
                "brand.colors.primary.default".to_string(),
                "brand.spacing.scale.4".to_string()
            ]
        );
    }

    #[test]
    fn unresolved_reference_detection_uses_brand_specs_directory() {
        let root = temp_root("refs");
        let specs = root.join("specifications").join("brands");
        fs::create_dir_all(&specs).expect("mkdir");
        fs::write(
            specs.join("acme.json"),
            r##"{
              "schema_version": "1.0",
              "brand_id": "acme",
              "color_tokens": { "primary": { "default": "#112233" } },
              "typography": {
                "font_families": { "heading": "Fraunces" },
                "font_sizes": { "md": 16 },
                "font_weights": { "regular": 400 },
                "line_heights": { "md": 24 },
                "text_styles": { "body": { "font_size": "md" } }
              },
              "spacing": {
                "base_unit": 4,
                "scale": { "4": 16 },
                "layout_margins": { "desktop": 64 }
              },
              "iconography": {},
              "motion": {},
              "layout": {
                "grid": { "columns": 12 },
                "breakpoints": { "md": 768 },
                "container_widths": { "lg": 1200 }
              },
              "blocking_ambiguities": []
            }"##,
        )
        .expect("write");

        let unresolved = unresolved_brand_token_references(
            "brand.colors.primary.default brand.layout.breakpoints.xl",
            root.join("specifications").to_str().expect("spec path"),
        )
        .expect("resolved refs");

        assert_eq!(unresolved, vec!["brand.layout.breakpoints.xl".to_string()]);

        let _ = fs::remove_dir_all(root);
    }
}
