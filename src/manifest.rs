use crate::workspace::Workspace;
use anyhow::{Context, Result, anyhow, bail};
use serde_yaml::{Mapping, Sequence, Value};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct CapabilityProviderInput {
    pub domain: String,
    pub crate_name: String,
    pub capabilities: Vec<String>,
    pub features: Vec<String>,
    pub default_features: bool,
    pub external_path_prefixes: Vec<String>,
}

/// Ensure the crate containing `rust_type` is registered in `drafts/dependencies.yml` and that
/// its path prefix is allowlisted in `drafts/types-manifest.yml`.
///
/// The crate-to-prefix mapping is sourced from `drafts/capability_registry.yml`. If no matching
/// provider is registered, this is a no-op (the caller will surface the missing crate error).
///
/// Returns `true` when a manifest change was persisted.
pub fn ensure_external_dependency_for_type(workspace: &Workspace, rust_type: &str) -> Result<bool> {
    let trimmed = rust_type.trim();
    if !trimmed.contains("::") {
        return Ok(false);
    }
    let crate_root = trimmed.split("::").next().unwrap_or_default();
    if crate_root.is_empty()
        || matches!(
            crate_root,
            "std" | "core" | "alloc" | "crate" | "self" | "super"
        )
    {
        return Ok(false);
    }

    let registry_path = workspace.drafts_dir.join("capability_registry.yml");
    if !registry_path.is_file() {
        return Ok(false);
    }
    let registry_raw = fs::read_to_string(&registry_path)
        .with_context(|| format!("Failed to read {}", registry_path.display()))?;
    let registry: Value = serde_yaml::from_str(&registry_raw)
        .with_context(|| format!("Failed to parse {}", registry_path.display()))?;
    let Some(mapping) = registry.as_mapping() else {
        return Ok(false);
    };
    let Some(providers) = mapping
        .get(Value::String("providers".to_string()))
        .and_then(Value::as_sequence)
    else {
        return Ok(false);
    };

    let mut input: Option<CapabilityProviderInput> = None;
    for provider in providers {
        let Some(map) = provider.as_mapping() else {
            continue;
        };
        if string_field(map, "crate") != Some(crate_root) {
            continue;
        }
        let domain = string_field(map, "domain")
            .unwrap_or(crate_root)
            .to_string();
        let features = map
            .get(Value::String("features".to_string()))
            .and_then(Value::as_sequence)
            .map(|seq| {
                seq.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let capabilities = map
            .get(Value::String("capabilities".to_string()))
            .and_then(Value::as_sequence)
            .map(|seq| {
                seq.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let external_path_prefixes = map
            .get(Value::String("external_path_prefixes".to_string()))
            .and_then(Value::as_sequence)
            .map(|seq| {
                seq.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| vec![format!("{}::", crate_root.replace('-', "_"))]);
        let default_features = map
            .get(Value::String("default_features".to_string()))
            .and_then(Value::as_bool)
            .unwrap_or(true);

        input = Some(CapabilityProviderInput {
            domain,
            crate_name: crate_root.to_string(),
            capabilities,
            features,
            default_features,
            external_path_prefixes,
        });
        break;
    }

    let Some(input) = input else {
        return Ok(false);
    };

    // Skip work when the manifests already reflect the registry entry.
    if manifests_already_cover(workspace, &input)? {
        return Ok(false);
    }

    add_capability_provider(workspace, &input, false)?;
    Ok(true)
}

fn manifests_already_cover(workspace: &Workspace, input: &CapabilityProviderInput) -> Result<bool> {
    let deps_path = workspace.drafts_dir.join("dependencies.yml");
    let deps_has_crate = if deps_path.is_file() {
        let raw = fs::read_to_string(&deps_path)
            .with_context(|| format!("Failed to read {}", deps_path.display()))?;
        let value: Value = serde_yaml::from_str(&raw)
            .with_context(|| format!("Failed to parse {}", deps_path.display()))?;
        value
            .as_mapping()
            .and_then(|m| m.get(Value::String("packages".to_string())))
            .and_then(Value::as_sequence)
            .is_some_and(|seq| {
                seq.iter().any(|pkg| {
                    pkg.as_mapping()
                        .is_some_and(|m| string_field(m, "name") == Some(&input.crate_name))
                })
            })
    } else {
        false
    };

    let types_path = workspace.drafts_dir.join("types-manifest.yml");
    let types_has_all_prefixes = if types_path.is_file() {
        let raw = fs::read_to_string(&types_path)
            .with_context(|| format!("Failed to read {}", types_path.display()))?;
        let value: Value = serde_yaml::from_str(&raw)
            .with_context(|| format!("Failed to parse {}", types_path.display()))?;
        let prefixes: Vec<String> = value
            .as_mapping()
            .and_then(|m| m.get(Value::String("external_path_prefixes".to_string())))
            .and_then(Value::as_sequence)
            .map(|seq| {
                seq.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        input
            .external_path_prefixes
            .iter()
            .all(|wanted| prefixes.iter().any(|existing| existing == wanted))
    } else {
        false
    };

    Ok(deps_has_crate && types_has_all_prefixes)
}

pub fn add_types_prefix(workspace: &Workspace, prefix: &str, dry_run: bool) -> Result<()> {
    let normalized = normalize_prefix(prefix)?;
    let path = workspace.drafts_dir.join("types-manifest.yml");
    let mut document = load_yaml_or_empty(&path)?;
    let mapping = expect_mapping_mut(&mut document, &path)?;

    push_unique_string(
        ensure_sequence_field(mapping, "external_path_prefixes", &path)?,
        normalized,
    );

    write_yaml_document(&path, &document, dry_run)
}

pub fn add_capability_provider(
    workspace: &Workspace,
    input: &CapabilityProviderInput,
    dry_run: bool,
) -> Result<()> {
    let normalized = normalize_capability_input(input)?;

    let registry_path = workspace.drafts_dir.join("capability_registry.yml");
    let mut registry = load_yaml_or_empty(&registry_path)?;
    update_capability_registry(&mut registry, &registry_path, &normalized)?;

    let dependencies_path = workspace.drafts_dir.join("dependencies.yml");
    let mut dependencies = load_yaml_or_empty(&dependencies_path)?;
    update_dependencies_manifest(&mut dependencies, &dependencies_path, &normalized)?;

    let types_manifest_path = workspace.drafts_dir.join("types-manifest.yml");
    let mut types_manifest = load_yaml_or_empty(&types_manifest_path)?;
    update_types_manifest(&mut types_manifest, &types_manifest_path, &normalized)?;

    write_yaml_document(&registry_path, &registry, dry_run)?;
    write_yaml_document(&dependencies_path, &dependencies, dry_run)?;
    write_yaml_document(&types_manifest_path, &types_manifest, dry_run)?;
    Ok(())
}

fn normalize_capability_input(input: &CapabilityProviderInput) -> Result<CapabilityProviderInput> {
    let domain = normalize_nonempty("capability domain", &input.domain)?;
    let crate_name = normalize_nonempty("crate name", &input.crate_name)?;
    let mut capabilities = if input.capabilities.is_empty() {
        vec![domain.clone()]
    } else {
        input
            .capabilities
            .iter()
            .map(|value| normalize_nonempty("capability", value))
            .collect::<Result<Vec<_>>>()?
    };
    dedupe_strings(&mut capabilities);

    let mut features = input
        .features
        .iter()
        .map(|value| normalize_nonempty("feature", value))
        .collect::<Result<Vec<_>>>()?;
    dedupe_strings(&mut features);

    let inferred_prefix = format!("{}::", crate_name.replace('-', "_"));
    let mut external_path_prefixes = if input.external_path_prefixes.is_empty() {
        vec![inferred_prefix]
    } else {
        input
            .external_path_prefixes
            .iter()
            .map(|value| normalize_prefix(value).map(str::to_string))
            .collect::<Result<Vec<_>>>()?
    };
    dedupe_strings(&mut external_path_prefixes);

    Ok(CapabilityProviderInput {
        domain,
        crate_name,
        capabilities,
        features,
        default_features: input.default_features,
        external_path_prefixes,
    })
}

fn update_capability_registry(
    document: &mut Value,
    path: &Path,
    input: &CapabilityProviderInput,
) -> Result<()> {
    let mapping = expect_mapping_mut(document, path)?;
    set_string_field(mapping, "schema", "reen.capability-registry/v1");
    ensure_sequence_field(mapping, "unmapped_capabilities", path)?;

    let providers = ensure_sequence_field(mapping, "providers", path)?;
    let provider = find_or_create_provider(providers, path, &input.domain, &input.crate_name)?;
    set_string_field(provider, "domain", &input.domain);
    set_string_field(provider, "crate", &input.crate_name);
    set_string_field(provider, "version", "*");
    provider.insert(
        Value::String("default_features".to_string()),
        Value::Bool(input.default_features),
    );
    merge_string_sequence(provider, "features", &input.features, path)?;
    merge_string_sequence(provider, "capabilities", &input.capabilities, path)?;
    merge_string_sequence(
        provider,
        "external_path_prefixes",
        &input.external_path_prefixes,
        path,
    )?;

    Ok(())
}

fn update_dependencies_manifest(
    document: &mut Value,
    path: &Path,
    input: &CapabilityProviderInput,
) -> Result<()> {
    let mapping = expect_mapping_mut(document, path)?;
    set_string_field(mapping, "schema", "reen.dependencies/v1");
    let packages = ensure_sequence_field(mapping, "packages", path)?;
    let package = find_or_create_package(packages, path, &input.crate_name)?;
    set_string_field(package, "name", &input.crate_name);
    set_string_field(
        package,
        "version",
        &render_dependency_version(input.default_features, &input.features),
    );
    merge_string_sequence(package, "capabilities", &input.capabilities, path)?;
    Ok(())
}

fn update_types_manifest(
    document: &mut Value,
    path: &Path,
    input: &CapabilityProviderInput,
) -> Result<()> {
    let mapping = expect_mapping_mut(document, path)?;
    let prefixes = ensure_sequence_field(mapping, "external_path_prefixes", path)?;
    for prefix in &input.external_path_prefixes {
        push_unique_string(prefixes, prefix);
    }
    Ok(())
}

fn render_dependency_version(default_features: bool, features: &[String]) -> String {
    if default_features && features.is_empty() {
        return "*".to_string();
    }

    let mut parts = vec!["version = \"*\"".to_string()];
    if !default_features {
        parts.push("default-features = false".to_string());
    }
    if !features.is_empty() {
        let features = features
            .iter()
            .map(|feature| format!("\"{feature}\""))
            .collect::<Vec<_>>()
            .join(", ");
        parts.push(format!("features = [{features}]"));
    }
    format!("{{ {} }}", parts.join(", "))
}

fn load_yaml_or_empty(path: &Path) -> Result<Value> {
    if path.is_file() {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        serde_yaml::from_str::<Value>(&raw)
            .with_context(|| format!("Failed to parse {}", path.display()))
    } else {
        Ok(Value::Mapping(Mapping::new()))
    }
}

fn write_yaml_document(path: &Path, document: &Value, dry_run: bool) -> Result<()> {
    let yaml = serde_yaml::to_string(document)?;
    if dry_run {
        println!("[dry-run] would write {}", path.display());
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    fs::write(path, yaml).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

fn expect_mapping_mut<'a>(document: &'a mut Value, path: &Path) -> Result<&'a mut Mapping> {
    document
        .as_mapping_mut()
        .ok_or_else(|| anyhow!("{} must contain a top-level YAML mapping", path.display()))
}

fn ensure_sequence_field<'a>(
    mapping: &'a mut Mapping,
    field: &str,
    path: &Path,
) -> Result<&'a mut Sequence> {
    mapping
        .entry(Value::String(field.to_string()))
        .or_insert_with(|| Value::Sequence(Sequence::new()))
        .as_sequence_mut()
        .ok_or_else(|| anyhow!("{} field `{field}` must be a YAML sequence", path.display()))
}

fn find_or_create_provider<'a>(
    providers: &'a mut Sequence,
    path: &Path,
    domain: &str,
    crate_name: &str,
) -> Result<&'a mut Mapping> {
    let existing = providers.iter().position(|value| {
        let Some(mapping) = value.as_mapping() else {
            return false;
        };
        string_field(mapping, "domain") == Some(domain)
            && string_field(mapping, "crate") == Some(crate_name)
    });
    let index = if let Some(index) = existing {
        index
    } else {
        providers.push(Value::Mapping(Mapping::new()));
        providers.len() - 1
    };
    providers[index].as_mapping_mut().ok_or_else(|| {
        anyhow!(
            "{} field `providers` must contain YAML mappings",
            path.display()
        )
    })
}

fn find_or_create_package<'a>(
    packages: &'a mut Sequence,
    path: &Path,
    name: &str,
) -> Result<&'a mut Mapping> {
    let existing = packages.iter().position(|value| {
        let Some(mapping) = value.as_mapping() else {
            return false;
        };
        string_field(mapping, "name") == Some(name)
    });
    let index = if let Some(index) = existing {
        index
    } else {
        packages.push(Value::Mapping(Mapping::new()));
        packages.len() - 1
    };
    packages[index].as_mapping_mut().ok_or_else(|| {
        anyhow!(
            "{} field `packages` must contain YAML mappings",
            path.display()
        )
    })
}

fn merge_string_sequence(
    mapping: &mut Mapping,
    field: &str,
    values: &[String],
    path: &Path,
) -> Result<()> {
    let sequence = ensure_sequence_field(mapping, field, path)?;
    for value in values {
        push_unique_string(sequence, value);
    }
    Ok(())
}

fn push_unique_string(sequence: &mut Sequence, value: &str) {
    if !sequence
        .iter()
        .filter_map(Value::as_str)
        .any(|existing| existing == value)
    {
        sequence.push(Value::String(value.to_string()));
    }
}

fn set_string_field(mapping: &mut Mapping, field: &str, value: &str) {
    mapping.insert(
        Value::String(field.to_string()),
        Value::String(value.to_string()),
    );
}

fn string_field<'a>(mapping: &'a Mapping, field: &str) -> Option<&'a str> {
    mapping
        .get(Value::String(field.to_string()))
        .and_then(Value::as_str)
}

fn normalize_prefix(prefix: &str) -> Result<&str> {
    let trimmed = prefix.trim();
    if trimmed.is_empty() {
        bail!("manifest type prefix cannot be empty");
    }
    if !trimmed.ends_with("::") {
        bail!("manifest type prefix must end with `::`");
    }
    Ok(trimmed)
}

fn normalize_nonempty(label: &str, value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("{label} cannot be empty");
    }
    Ok(trimmed.to_string())
}

fn dedupe_strings(values: &mut Vec<String>) {
    values.sort();
    values.dedup();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Workspace;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(prefix: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("reen_manifest_{prefix}_{stamp}"))
    }

    #[test]
    fn add_types_prefix_creates_manifest_when_missing() {
        let root = temp_root("create");
        let workspace = Workspace::discover(root.clone()).unwrap();

        add_types_prefix(&workspace, "rand::", false).unwrap();

        let written = fs::read_to_string(root.join("drafts/types-manifest.yml")).unwrap();
        assert!(written.contains("external_path_prefixes:"));
        assert!(written.contains("- 'rand::'") || written.contains("- rand::"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn add_types_prefix_is_idempotent() {
        let root = temp_root("dedupe");
        let workspace = Workspace::discover(root.clone()).unwrap();
        fs::create_dir_all(root.join("drafts")).unwrap();
        fs::write(
            root.join("drafts/types-manifest.yml"),
            "external_path_prefixes:\n  - 'std::'\n",
        )
        .unwrap();

        add_types_prefix(&workspace, "std::", false).unwrap();
        add_types_prefix(&workspace, "rand::", false).unwrap();

        let written = fs::read_to_string(root.join("drafts/types-manifest.yml")).unwrap();
        assert_eq!(written.matches("std::").count(), 1);
        assert_eq!(written.matches("rand::").count(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn add_capability_provider_writes_registry_dependencies_and_types_manifest() {
        let root = temp_root("capability");
        let workspace = Workspace::discover(root.clone()).unwrap();

        add_capability_provider(
            &workspace,
            &CapabilityProviderInput {
                domain: "randomness".to_string(),
                crate_name: "rand".to_string(),
                capabilities: Vec::new(),
                features: vec!["std_rng".to_string()],
                default_features: true,
                external_path_prefixes: Vec::new(),
            },
            false,
        )
        .unwrap();

        let registry = fs::read_to_string(root.join("drafts/capability_registry.yml")).unwrap();
        assert!(registry.contains("reen.capability-registry/v1"));
        assert!(registry.contains("domain: randomness"));
        assert!(registry.contains("crate: rand"));
        assert!(registry.contains("- randomness"));
        assert!(registry.contains("rand::"));
        assert!(registry.contains("version: '*'") || registry.contains("version: \"*\""));

        let dependencies = fs::read_to_string(root.join("drafts/dependencies.yml")).unwrap();
        assert!(dependencies.contains("reen.dependencies/v1"));
        assert!(dependencies.contains("name: rand"));
        assert!(dependencies.contains("- randomness"));
        assert!(dependencies.contains("version = \"*\""));
        assert!(dependencies.contains("features = [\"std_rng\"]"));

        let manifest = fs::read_to_string(root.join("drafts/types-manifest.yml")).unwrap();
        assert!(manifest.contains("external_path_prefixes:"));
        assert!(manifest.contains("rand::"));
        assert!(!manifest.contains("allowlists:"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn add_capability_provider_is_idempotent() {
        let root = temp_root("capability_dedupe");
        let workspace = Workspace::discover(root.clone()).unwrap();

        let input = CapabilityProviderInput {
            domain: "randomness".to_string(),
            crate_name: "rand".to_string(),
            capabilities: Vec::new(),
            features: vec!["std_rng".to_string()],
            default_features: true,
            external_path_prefixes: vec!["rand::".to_string()],
        };

        add_capability_provider(&workspace, &input, false).unwrap();
        add_capability_provider(&workspace, &input, false).unwrap();

        let registry = fs::read_to_string(root.join("drafts/capability_registry.yml")).unwrap();
        assert_eq!(
            registry.matches("domain: randomness").count(),
            1,
            "{registry}"
        );
        assert_eq!(registry.matches("- randomness").count(), 1, "{registry}");

        let dependencies = fs::read_to_string(root.join("drafts/dependencies.yml")).unwrap();
        assert_eq!(
            dependencies.matches("name: rand").count(),
            1,
            "{dependencies}"
        );
        assert_eq!(
            dependencies.matches("- randomness").count(),
            1,
            "{dependencies}"
        );

        let manifest: Value = serde_yaml::from_str(
            &fs::read_to_string(root.join("drafts/types-manifest.yml")).unwrap(),
        )
        .unwrap();
        let mapping = manifest.as_mapping().unwrap();
        let prefixes = mapping
            .get(Value::String("external_path_prefixes".to_string()))
            .and_then(Value::as_sequence)
            .unwrap();
        assert_eq!(prefixes.len(), 1);
        assert_eq!(prefixes[0].as_str(), Some("rand::"));

        let _ = fs::remove_dir_all(root);
    }
}
