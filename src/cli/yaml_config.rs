//! `reen.yml` parsing and helpers used by tests and the CLI.
use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_yaml::{Mapping, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub const CONFIG_FILENAME: &str = "reen.yml";

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct ReenConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verbose: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create: Option<CreateConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<FixCommandConfig>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct CreateConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clear_cache: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_limit: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub specification: Option<CreateSpecificationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub implementation: Option<CreateImplementationConfig>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct CreateSpecificationConfig {
    #[serde(default, skip_serializing_if = "OptionalYamlValue::is_missing")]
    pub fix: OptionalYamlValue,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct CreateImplementationConfig {
    #[serde(default, skip_serializing_if = "OptionalYamlValue::is_missing")]
    pub fix: OptionalYamlValue,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum OptionalYamlValue {
    #[default]
    Missing,
    Present(Value),
}

impl OptionalYamlValue {
    fn is_missing(&self) -> bool {
        matches!(self, Self::Missing)
    }

    /// Returns true if the value is present and not explicitly `false`.
    /// `fix:` (null/mapping) and `fix: true` both count as enabled.
    pub fn is_enabled(&self) -> bool {
        match self {
            Self::Missing => false,
            Self::Present(Value::Bool(b)) => *b,
            Self::Present(Value::Null) | Self::Present(Value::Mapping(_)) => true,
            Self::Present(_) => false,
        }
    }

    /// Returns a `u32` from a named key inside a mapping value, if present.
    /// Used to extract e.g. `max-compile-fix-attempts` from `fix: { ... }`.
    pub fn mapping_u32(&self, key: &str) -> Option<u32> {
        let mapping = match self {
            Self::Present(Value::Mapping(m)) => m,
            _ => return None,
        };
        let value = mapping.get(&Value::String(key.to_string()))?;
        match value {
            Value::Number(n) => n.as_u64().and_then(|v| u32::try_from(v).ok()),
            _ => None,
        }
    }
}

impl<'de> Deserialize<'de> for OptionalYamlValue {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Value::deserialize(deserializer).map(Self::Present)
    }
}

impl Serialize for OptionalYamlValue {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Missing => serializer.serialize_none(),
            Self::Present(value) => value.serialize(serializer),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct FixCommandConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_compile_fix_attempts: Option<u32>,
}

pub fn resolve_config_path() -> PathBuf {
    match std::env::current_dir() {
        Ok(cwd) => resolve_config_path_from(cwd.as_path()),
        Err(_) => PathBuf::from(CONFIG_FILENAME),
    }
}

pub fn resolve_config_path_from(start: &Path) -> PathBuf {
    if let Some(path) = find_upwards(start, CONFIG_FILENAME) {
        return path;
    }
    start.join(CONFIG_FILENAME)
}

pub fn load_config() -> Result<ReenConfig> {
    let path = resolve_config_path();
    let raw = if path.exists() {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file '{}'", path.display()))?;
        if content.trim().is_empty() {
            Value::Mapping(Mapping::new())
        } else {
            serde_yaml::from_str::<Value>(&content)
                .with_context(|| format!("Failed to parse YAML config '{}'", path.display()))?
        }
    } else {
        Value::Mapping(Mapping::new())
    };

    let raw = match raw {
        Value::Null => Value::Mapping(Mapping::new()),
        Value::Mapping(_) => raw,
        _ => anyhow::bail!(
            "Config file '{}' must contain a YAML mapping",
            path.display()
        ),
    };
    serde_yaml::from_value::<ReenConfig>(raw)
        .with_context(|| format!("Failed to decode config '{}'", path.display()))
}

fn find_upwards(start: &Path, name: &str) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        let candidate = current.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
        if !current.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{OptionalYamlValue, ReenConfig, Value, resolve_config_path_from};
    use anyhow::{Context, Result};
    use serde_yaml::Mapping;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn set_path(root: &mut Value, path: &[&str], value: Value) {
        if path.is_empty() {
            *root = value;
            return;
        }

        if !matches!(root, Value::Mapping(_)) {
            *root = Value::Mapping(Mapping::new());
        }

        let mut current = root;
        for segment in &path[..path.len() - 1] {
            let mapping = current
                .as_mapping_mut()
                .expect("config root should be a mapping");
            let entry = mapping
                .entry(Value::String((*segment).to_string()))
                .or_insert_with(|| Value::Mapping(Mapping::new()));
            if !matches!(entry, Value::Mapping(_)) {
                *entry = Value::Mapping(Mapping::new());
            }
            current = entry;
        }

        current
            .as_mapping_mut()
            .expect("config parent should be a mapping")
            .insert(Value::String(path[path.len() - 1].to_string()), value);
    }

    fn get_path<'a>(root: &'a Value, path: &[&str]) -> Option<&'a Value> {
        let mut current = root;
        for segment in path {
            let mapping = current.as_mapping()?;
            current = mapping.get(Value::String((*segment).to_string()))?;
        }
        Some(current)
    }

    fn ensure_switch_enabled(root: &mut Value, path: &[&str]) {
        match get_path(root, path) {
            Some(Value::Null) | Some(Value::Bool(true)) | Some(Value::Mapping(_)) => return,
            _ => {}
        }
        set_path(root, path, Value::Null);
    }

    fn render_yaml(raw: &Value) -> Result<String> {
        let serialized = serde_yaml::to_string(raw).context("Failed to serialize YAML")?;
        Ok(serialized.replace(": null\n", ":\n"))
    }

    fn fix_value_enabled(fix: &OptionalYamlValue) -> bool {
        match fix {
            OptionalYamlValue::Missing => false,
            OptionalYamlValue::Present(Value::Bool(enabled)) => *enabled,
            OptionalYamlValue::Present(Value::Null)
            | OptionalYamlValue::Present(Value::Mapping(_)) => true,
            OptionalYamlValue::Present(_) => false,
        }
    }

    fn fix_mapping_u32(fix: &OptionalYamlValue, key: &str) -> Option<u32> {
        let mapping = match fix {
            OptionalYamlValue::Present(Value::Mapping(m)) => m,
            _ => return None,
        };
        let value = mapping.get(Value::String(key.to_string()))?;
        match value {
            Value::Number(number) => number.as_u64().and_then(|raw| u32::try_from(raw).ok()),
            _ => None,
        }
    }

    #[test]
    fn parses_fix_null_as_enabled() {
        let parsed: ReenConfig = serde_yaml::from_str(
            r#"
create:
  specification:
    fix:
"#,
        )
        .expect("parse config");

        let create = parsed.create.expect("create config");
        let specification = create.specification.expect("spec config");
        assert!(fix_value_enabled(&specification.fix));
        assert_eq!(
            fix_mapping_u32(&specification.fix, "max-fix-attempts"),
            None
        );
    }

    #[test]
    fn parses_fix_mapping_with_related_settings() {
        let parsed: ReenConfig = serde_yaml::from_str(
            r#"
create:
  implementation:
    fix:
      max-compile-fix-attempts: 7
fix:
  max-compile-fix-attempts: 4
"#,
        )
        .expect("parse config");

        let implementation = parsed
            .create
            .and_then(|create| create.implementation)
            .expect("implementation config");
        assert!(fix_value_enabled(&implementation.fix));
        assert_eq!(
            fix_mapping_u32(&implementation.fix, "max-compile-fix-attempts"),
            Some(7)
        );
        assert_eq!(
            parsed.fix.and_then(|fix| fix.max_compile_fix_attempts),
            Some(4)
        );
    }

    #[test]
    fn resolve_config_path_prefers_existing_ancestor() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("current time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("reen-config-test-{unique}"));
        let nested = root.join("tests").join("snake");
        fs::create_dir_all(&nested).expect("create nested dirs");
        let existing = root.join("reen.yml");
        fs::write(&existing, "verbose: true\n").expect("write config file");

        assert_eq!(resolve_config_path_from(&nested), existing);

        fs::remove_dir_all(&root).expect("cleanup temp dir");
    }

    #[test]
    fn ensure_switch_enabled_preserves_existing_mapping() {
        let mut raw = Value::Mapping(Mapping::new());
        set_path(
            &mut raw,
            &["create", "specification", "fix", "max-fix-attempts"],
            Value::Number(serde_yaml::Number::from(5)),
        );

        ensure_switch_enabled(&mut raw, &["create", "specification", "fix"]);

        let fix = get_path(&raw, &["create", "specification", "fix"]).expect("fix path");
        assert_eq!(
            fix,
            &serde_yaml::from_str::<Value>("max-fix-attempts: 5").expect("mapping value")
        );
    }

    #[test]
    fn renders_blank_fix_value_without_literal_null() {
        let mut raw = Value::Mapping(Mapping::new());
        set_path(&mut raw, &["create", "specification", "fix"], Value::Null);

        let rendered = render_yaml(&raw).expect("render yaml");

        assert!(rendered.contains("fix:\n"));
        assert!(!rendered.contains("fix: null"));
    }
}
