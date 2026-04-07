use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

const CAPABILITY_REGISTRY_SCHEMA: &str = "reen.capability-registry/v1";
const CAPABILITY_REGISTRY_NAME: &str = "capability_registry.yml";
const DEPENDENCY_SCHEMA: &str = "reen.dependencies/v1";
const DEPENDENCY_MANIFEST_NAME: &str = "dependencies.yml";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityRegistry {
    #[serde(default)]
    pub schema: Option<String>,
    #[serde(default)]
    pub providers: Vec<CapabilityProvider>,
    #[serde(default)]
    pub unmapped_capabilities: Vec<UnmappedCapability>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityProvider {
    pub domain: String,
    #[serde(rename = "crate")]
    pub crate_name: String,
    pub version: String,
    #[serde(default = "default_true")]
    pub default_features: bool,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct UnmappedCapability {
    pub capability: String,
    pub domain: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct DetectedCapability {
    pub capability: String,
    pub domain: String,
    pub evidence_paths: Vec<String>,
    pub evidence_snippets: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub struct CapabilityScan {
    pub detected: Vec<DetectedCapability>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ResolvedDependencyPlan {
    pub registry_path: String,
    pub dependency_manifest_path: String,
    pub providers: Vec<ResolvedProvider>,
    pub packages: Vec<ResolvedPackage>,
    pub required_capabilities: Vec<String>,
    pub unresolved_capabilities: Vec<UnmappedCapability>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ResolvedProvider {
    pub domain: String,
    pub crate_name: String,
    pub version: String,
    pub default_features: bool,
    pub features: Vec<String>,
    pub capabilities: Vec<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ResolvedPackage {
    pub name: String,
    pub version: String,
    pub capabilities: Vec<String>,
    pub domain: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct DerivedDependencyManifest {
    #[serde(default)]
    schema: Option<String>,
    #[serde(default)]
    packages: Vec<DerivedDependencyPackage>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct DerivedDependencyPackage {
    name: String,
    version: String,
    #[serde(default)]
    capabilities: Vec<String>,
}

#[derive(Clone, Copy)]
struct CapabilityRule {
    capability: &'static str,
    domain: &'static str,
    patterns: &'static [&'static str],
}

fn default_true() -> bool {
    true
}

pub fn capability_registry_path(drafts_root: &Path) -> PathBuf {
    drafts_root.join(CAPABILITY_REGISTRY_NAME)
}

pub fn dependency_manifest_path(drafts_root: &Path) -> PathBuf {
    drafts_root.join(DEPENDENCY_MANIFEST_NAME)
}

pub fn capability_registry_exists(drafts_root: Option<&Path>) -> bool {
    drafts_root.is_some_and(|root| capability_registry_path(root).exists())
}

pub fn empty_registry() -> CapabilityRegistry {
    CapabilityRegistry {
        schema: Some(CAPABILITY_REGISTRY_SCHEMA.to_string()),
        providers: Vec::new(),
        unmapped_capabilities: Vec::new(),
    }
}

pub fn load_capability_registry(path: &Path) -> Result<Option<CapabilityRegistry>> {
    if !path.exists() {
        return Ok(None);
    }

    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let mut registry: CapabilityRegistry = serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    validate_capability_registry(&mut registry, path)?;
    Ok(Some(registry))
}

pub fn parse_capability_registry_fragment(input: &str) -> Result<CapabilityRegistry> {
    let yaml = extract_yaml_candidate(input).unwrap_or(input);
    let mut registry: CapabilityRegistry =
        serde_yaml::from_str(yaml).context("Failed to parse capability registry fragment")?;
    if registry.schema.is_none() {
        registry.schema = Some(CAPABILITY_REGISTRY_SCHEMA.to_string());
    }
    validate_capability_registry(&mut registry, Path::new("<agent-output>"))?;
    Ok(registry)
}

pub fn write_capability_registry(path: &Path, registry: &CapabilityRegistry) -> Result<()> {
    let mut normalized = registry.clone();
    validate_capability_registry(&mut normalized, path)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let mut content = String::from(
        "# Derived and maintained by reen capability planning. Manual edits are allowed.\n",
    );
    content.push_str(
        &serde_yaml::to_string(&normalized).context("Failed to serialize capability registry")?,
    );
    fs::write(path, content).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

pub fn scan_draft_capabilities(drafts_root: &Path) -> Result<CapabilityScan> {
    let mut detected: BTreeMap<String, DetectedCapability> = BTreeMap::new();
    scan_draft_capabilities_recursive(drafts_root, drafts_root, &mut detected)?;
    Ok(CapabilityScan {
        detected: detected.into_values().collect(),
    })
}

pub fn bootstrap_registry_from_scan(
    existing: Option<&CapabilityRegistry>,
    scan: &CapabilityScan,
) -> CapabilityRegistry {
    let mut registry = existing.cloned().unwrap_or_else(empty_registry);

    for item in &scan.detected {
        if registry_maps_capability(&registry, &item.capability)
            || registry_marks_capability_unmapped(&registry, &item.capability)
        {
            continue;
        }

        if let Some(mut provider) = builtin_provider_for_capability(&item.capability) {
            provider.capabilities = vec![item.capability.clone()];
            merge_provider_with_manual_preference(&mut registry, provider);
        } else {
            registry.unmapped_capabilities.push(UnmappedCapability {
                capability: item.capability.clone(),
                domain: item.domain.clone(),
            });
        }
    }

    normalize_registry(&mut registry);
    registry
}

pub fn merge_registry_proposals(
    base: &mut CapabilityRegistry,
    proposal: &CapabilityRegistry,
) -> Result<()> {
    let mut normalized = proposal.clone();
    validate_capability_registry(&mut normalized, Path::new("<proposal>"))?;

    for mut provider in normalized.providers {
        provider.capabilities.retain(|capability| {
            !registry_maps_capability(base, capability)
                || capability_provider_domain(base, capability).as_deref() == Some(&provider.domain)
        });
        if provider.capabilities.is_empty() {
            continue;
        }
        remove_unmapped_entries(base, &provider.capabilities);
        merge_provider_with_manual_preference(base, provider);
    }

    for unmapped in normalized.unmapped_capabilities {
        if registry_maps_capability(base, &unmapped.capability)
            || registry_marks_capability_unmapped(base, &unmapped.capability)
        {
            continue;
        }
        base.unmapped_capabilities.push(unmapped);
    }

    normalize_registry(base);
    Ok(())
}

pub fn ensure_scan_coverage(registry: &mut CapabilityRegistry, scan: &CapabilityScan) {
    for item in &scan.detected {
        if registry_maps_capability(registry, &item.capability)
            || registry_marks_capability_unmapped(registry, &item.capability)
        {
            continue;
        }
        registry.unmapped_capabilities.push(UnmappedCapability {
            capability: item.capability.clone(),
            domain: item.domain.clone(),
        });
    }
    normalize_registry(registry);
}

pub fn add_capability_mapping_to_registry(
    registry: &mut CapabilityRegistry,
    capability: &str,
    crate_name: &str,
    domain: &str,
    version: &str,
    features: &[String],
    default_features: bool,
) -> Result<()> {
    validate_identifier("capability", capability, Path::new("<command>"))?;
    validate_identifier("domain", domain, Path::new("<command>"))?;

    if crate_name.trim().is_empty() {
        bail!("crate name cannot be empty");
    }
    if version.trim().is_empty() {
        bail!("version cannot be empty");
    }

    if let Some(existing_domain) = capability_provider_domain(registry, capability) {
        if existing_domain != domain {
            bail!(
                "Capability '{}' is already mapped under domain '{}'",
                capability,
                existing_domain
            );
        }
    }

    let normalized_features = normalize_feature_list(features);
    if let Some(existing) = registry
        .providers
        .iter_mut()
        .find(|provider| provider.domain == domain)
    {
        if canonicalize_crate_name(&existing.crate_name) != canonicalize_crate_name(crate_name)
            || existing.version != version.trim()
            || existing.default_features != default_features
            || normalize_feature_list(&existing.features) != normalized_features
        {
            bail!(
                "Domain '{}' is already mapped to crate '{}' with a different specification",
                domain,
                existing.crate_name
            );
        }
        existing.capabilities.push(capability.trim().to_string());
    } else {
        registry.providers.push(CapabilityProvider {
            domain: domain.trim().to_string(),
            crate_name: crate_name.trim().to_string(),
            version: version.trim().to_string(),
            default_features,
            features: normalized_features,
            capabilities: vec![capability.trim().to_string()],
        });
    }

    remove_unmapped_entries(registry, &[capability.trim().to_string()]);
    normalize_registry(registry);
    Ok(())
}

pub fn sync_dependency_manifest_from_capability_registry(
    drafts_root: &Path,
    verbose: bool,
) -> Result<Option<ResolvedDependencyPlan>> {
    let registry_path = capability_registry_path(drafts_root);
    let Some(registry) = load_capability_registry(&registry_path)? else {
        return Ok(None);
    };

    let scan = scan_draft_capabilities(drafts_root)?;
    let plan = resolve_dependency_plan(drafts_root, &registry, &scan)?;
    write_dependency_manifest(&dependency_manifest_path(drafts_root), &plan.packages)?;

    if verbose {
        println!(
            "Synchronized derived dependency manifest from {}",
            registry_path.display()
        );
    }

    Ok(Some(plan))
}

pub fn resolved_dependency_plan_context(drafts_root: &Path) -> Result<Option<Value>> {
    let Some(plan) = sync_dependency_manifest_from_capability_registry(drafts_root, false)? else {
        return Ok(None);
    };
    Ok(Some(
        serde_json::to_value(plan).context("Failed to serialize resolved dependency plan")?,
    ))
}

pub fn allowed_external_crate_roots(drafts_root: &Path) -> Result<Option<BTreeSet<String>>> {
    let Some(plan) = sync_dependency_manifest_from_capability_registry(drafts_root, false)? else {
        return Ok(None);
    };
    Ok(Some(
        plan.packages
            .iter()
            .map(|package| package.name.clone())
            .collect(),
    ))
}

pub fn registry_provider_domains_by_crate(
    drafts_root: &Path,
) -> Result<Option<HashMap<String, String>>> {
    let registry_path = capability_registry_path(drafts_root);
    let Some(registry) = load_capability_registry(&registry_path)? else {
        return Ok(None);
    };

    Ok(Some(
        registry
            .providers
            .into_iter()
            .map(|provider| (provider.crate_name, provider.domain))
            .collect(),
    ))
}

pub fn builtin_provider_catalog_json() -> Value {
    serde_json::to_value(
        builtin_provider_catalog()
            .into_iter()
            .map(|provider| ResolvedProvider {
                domain: provider.domain,
                crate_name: provider.crate_name,
                version: provider.version,
                default_features: provider.default_features,
                features: provider.features,
                capabilities: provider.capabilities,
            })
            .collect::<Vec<_>>(),
    )
    .unwrap_or_else(|_| Value::Array(Vec::new()))
}

fn scan_draft_capabilities_recursive(
    drafts_root: &Path,
    current_dir: &Path,
    detected: &mut BTreeMap<String, DetectedCapability>,
) -> Result<()> {
    if !current_dir.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(current_dir)
        .with_context(|| format!("Failed to read directory: {}", current_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            scan_draft_capabilities_recursive(drafts_root, &path, detected)?;
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) != Some("md") {
            continue;
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let content_lower = content.to_ascii_lowercase();
        let relative = path
            .strip_prefix(drafts_root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        for rule in capability_rules() {
            if !rule
                .patterns
                .iter()
                .any(|pattern| content_lower.contains(pattern))
            {
                continue;
            }

            let entry = detected
                .entry(rule.capability.to_string())
                .or_insert_with(|| DetectedCapability {
                    capability: rule.capability.to_string(),
                    domain: rule.domain.to_string(),
                    evidence_paths: Vec::new(),
                    evidence_snippets: Vec::new(),
                });
            entry.evidence_paths.push(relative.clone());
            entry
                .evidence_snippets
                .extend(collect_evidence_snippets(&content, rule.patterns));
        }
    }

    for entry in detected.values_mut() {
        entry.evidence_paths.sort();
        entry.evidence_paths.dedup();
        entry.evidence_snippets.sort();
        entry.evidence_snippets.dedup();
    }

    Ok(())
}

fn collect_evidence_snippets(content: &str, patterns: &[&str]) -> Vec<String> {
    let patterns = patterns
        .iter()
        .map(|pattern| pattern.to_ascii_lowercase())
        .collect::<Vec<_>>();
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| {
            let lowered = line.to_ascii_lowercase();
            patterns.iter().any(|pattern| lowered.contains(pattern))
        })
        .take(4)
        .map(|line| line.to_string())
        .collect()
}

fn resolve_dependency_plan(
    drafts_root: &Path,
    registry: &CapabilityRegistry,
    scan: &CapabilityScan,
) -> Result<ResolvedDependencyPlan> {
    let mut providers = registry.providers.clone();
    normalize_registry_providers(&mut providers);
    let mapped = providers
        .iter()
        .enumerate()
        .flat_map(|(index, provider)| {
            provider
                .capabilities
                .iter()
                .map(move |capability| (capability.clone(), index))
        })
        .collect::<HashMap<_, _>>();
    let unmapped = registry
        .unmapped_capabilities
        .iter()
        .map(|item| (item.capability.clone(), item.domain.clone()))
        .collect::<HashMap<_, _>>();

    let mut required_capabilities = BTreeSet::new();
    let mut provider_to_capabilities: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
    let mut unresolved = BTreeMap::<String, String>::new();

    for item in &scan.detected {
        required_capabilities.insert(item.capability.clone());
        if let Some(index) = mapped.get(&item.capability) {
            provider_to_capabilities
                .entry(*index)
                .or_default()
                .insert(item.capability.clone());
        } else if let Some(domain) = unmapped.get(&item.capability) {
            unresolved.insert(item.capability.clone(), domain.clone());
        } else {
            bail!(
                "Required capability '{}' is not mapped and is not listed under `unmapped_capabilities` in {}",
                item.capability,
                capability_registry_path(drafts_root).display()
            );
        }
    }

    let resolved_providers = provider_to_capabilities
        .iter()
        .map(|(index, capabilities)| {
            let provider = &providers[*index];
            ResolvedProvider {
                domain: provider.domain.clone(),
                crate_name: provider.crate_name.clone(),
                version: provider.version.clone(),
                default_features: provider.default_features,
                features: provider.features.clone(),
                capabilities: capabilities.iter().cloned().collect(),
            }
        })
        .collect::<Vec<_>>();

    let packages = resolved_providers
        .iter()
        .map(|provider| ResolvedPackage {
            name: provider.crate_name.clone(),
            version: dependency_version_spec(
                &provider.version,
                provider.default_features,
                &provider.features,
            ),
            capabilities: provider.capabilities.clone(),
            domain: provider.domain.clone(),
        })
        .collect::<Vec<_>>();

    Ok(ResolvedDependencyPlan {
        registry_path: capability_registry_path(drafts_root)
            .to_string_lossy()
            .into_owned(),
        dependency_manifest_path: dependency_manifest_path(drafts_root)
            .to_string_lossy()
            .into_owned(),
        providers: resolved_providers,
        packages,
        required_capabilities: required_capabilities.into_iter().collect(),
        unresolved_capabilities: unresolved
            .into_iter()
            .map(|(capability, domain)| UnmappedCapability { capability, domain })
            .collect(),
    })
}

fn write_dependency_manifest(path: &Path, packages: &[ResolvedPackage]) -> Result<()> {
    let manifest = DerivedDependencyManifest {
        schema: Some(DEPENDENCY_SCHEMA.to_string()),
        packages: packages
            .iter()
            .map(|package| DerivedDependencyPackage {
                name: package.name.clone(),
                version: package.version.clone(),
                capabilities: package.capabilities.clone(),
            })
            .collect(),
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let mut content =
        String::from("# Derived from drafts/capability_registry.yml. Do not edit manually.\n");
    content.push_str(
        &serde_yaml::to_string(&manifest).context("Failed to serialize dependency manifest")?,
    );
    fs::write(path, content).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

fn validate_capability_registry(registry: &mut CapabilityRegistry, path: &Path) -> Result<()> {
    match registry.schema.as_deref() {
        Some(CAPABILITY_REGISTRY_SCHEMA) => {}
        Some(other) => bail!(
            "Unsupported capability registry schema '{}' in {}",
            other,
            path.display()
        ),
        None => bail!(
            "Capability registry is missing required schema '{}' in {}",
            CAPABILITY_REGISTRY_SCHEMA,
            path.display()
        ),
    }

    normalize_registry(registry);

    let mut seen_domains = BTreeSet::new();
    let mut seen_capabilities = BTreeMap::<String, String>::new();
    for provider in &registry.providers {
        validate_identifier("domain", &provider.domain, path)?;
        if !seen_domains.insert(provider.domain.clone()) {
            bail!(
                "Duplicate capability provider domain '{}' in {}",
                provider.domain,
                path.display()
            );
        }
        if provider.crate_name.trim().is_empty() {
            bail!(
                "Provider domain '{}' is missing a crate name in {}",
                provider.domain,
                path.display()
            );
        }
        if provider.version.trim().is_empty() {
            bail!(
                "Provider domain '{}' is missing a version in {}",
                provider.domain,
                path.display()
            );
        }
        for capability in &provider.capabilities {
            validate_identifier("capability", capability, path)?;
            if let Some(existing_domain) =
                seen_capabilities.insert(capability.clone(), provider.domain.clone())
            {
                bail!(
                    "Capability '{}' is mapped in both domains '{}' and '{}' in {}",
                    capability,
                    existing_domain,
                    provider.domain,
                    path.display()
                );
            }
        }
    }

    for unmapped in &registry.unmapped_capabilities {
        validate_identifier("capability", &unmapped.capability, path)?;
        validate_identifier("domain", &unmapped.domain, path)?;
        if let Some(existing_domain) = seen_capabilities.get(&unmapped.capability) {
            bail!(
                "Capability '{}' is both mapped under domain '{}' and listed as unmapped in {}",
                unmapped.capability,
                existing_domain,
                path.display()
            );
        }
        if let Some(existing_domain) =
            seen_capabilities.insert(unmapped.capability.clone(), unmapped.domain.clone())
        {
            bail!(
                "Capability '{}' appears more than once (existing domain '{}') in {}",
                unmapped.capability,
                existing_domain,
                path.display()
            );
        }
    }

    Ok(())
}

fn validate_identifier(kind: &str, value: &str, path: &Path) -> Result<()> {
    let re = Regex::new(r"^[a-z][a-z0-9_]*$").unwrap();
    if re.is_match(value.trim()) {
        Ok(())
    } else {
        bail!(
            "{} '{}' must be canonical snake_case in {}",
            kind,
            value,
            path.display()
        )
    }
}

fn normalize_registry(registry: &mut CapabilityRegistry) {
    normalize_registry_providers(&mut registry.providers);
    registry.unmapped_capabilities = registry
        .unmapped_capabilities
        .iter()
        .map(|item| UnmappedCapability {
            capability: item.capability.trim().to_string(),
            domain: item.domain.trim().to_string(),
        })
        .collect();
    registry.unmapped_capabilities.sort_by(|a, b| {
        a.capability
            .cmp(&b.capability)
            .then(a.domain.cmp(&b.domain))
    });
    registry.unmapped_capabilities.dedup();
}

fn normalize_registry_providers(providers: &mut Vec<CapabilityProvider>) {
    for provider in providers.iter_mut() {
        provider.domain = provider.domain.trim().to_string();
        provider.crate_name = provider.crate_name.trim().to_string();
        provider.version = provider.version.trim().to_string();
        provider.features = normalize_feature_list(&provider.features);
        provider.capabilities = provider
            .capabilities
            .iter()
            .map(|capability| capability.trim().to_string())
            .filter(|capability| !capability.is_empty())
            .collect();
        provider.capabilities.sort();
        provider.capabilities.dedup();
    }
    providers.sort_by(|a, b| a.domain.cmp(&b.domain));
}

fn normalize_feature_list(features: &[String]) -> Vec<String> {
    let mut normalized = features
        .iter()
        .map(|feature| feature.trim().to_string())
        .filter(|feature| !feature.is_empty())
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    normalized
}

fn merge_provider_with_manual_preference(
    registry: &mut CapabilityRegistry,
    provider: CapabilityProvider,
) {
    if let Some(existing) = registry
        .providers
        .iter_mut()
        .find(|existing| existing.domain == provider.domain)
    {
        existing.capabilities.extend(provider.capabilities);
        existing.capabilities.sort();
        existing.capabilities.dedup();
        return;
    }

    registry.providers.push(provider);
}

fn remove_unmapped_entries(registry: &mut CapabilityRegistry, capabilities: &[String]) {
    let capability_set = capabilities.iter().cloned().collect::<BTreeSet<_>>();
    registry
        .unmapped_capabilities
        .retain(|entry| !capability_set.contains(&entry.capability));
}

fn registry_maps_capability(registry: &CapabilityRegistry, capability: &str) -> bool {
    registry
        .providers
        .iter()
        .any(|provider| provider.capabilities.iter().any(|item| item == capability))
}

fn registry_marks_capability_unmapped(registry: &CapabilityRegistry, capability: &str) -> bool {
    registry
        .unmapped_capabilities
        .iter()
        .any(|item| item.capability == capability)
}

fn capability_provider_domain(registry: &CapabilityRegistry, capability: &str) -> Option<String> {
    registry.providers.iter().find_map(|provider| {
        provider
            .capabilities
            .iter()
            .any(|item| item == capability)
            .then(|| provider.domain.clone())
    })
}

fn dependency_version_spec(version: &str, default_features: bool, features: &[String]) -> String {
    if default_features && features.is_empty() {
        return version.to_string();
    }

    let mut entries = vec![format!("version = \"{}\"", version)];
    if !default_features {
        entries.push("default-features = false".to_string());
    }
    if !features.is_empty() {
        entries.push(format!(
            "features = [{}]",
            features
                .iter()
                .map(|feature| format!("\"{}\"", feature))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    format!("{{ {} }}", entries.join(", "))
}

fn extract_yaml_candidate(input: &str) -> Option<&str> {
    let trimmed = input.trim();
    if trimmed.starts_with("```") {
        let mut parts = trimmed.splitn(3, "```");
        let _prefix = parts.next()?;
        let middle = parts.next()?;
        let _suffix = parts.next()?;
        let middle = middle
            .trim_start_matches("yaml")
            .trim_start_matches("yml")
            .trim();
        return Some(middle);
    }
    None
}

fn builtin_provider_for_capability(capability: &str) -> Option<CapabilityProvider> {
    builtin_provider_catalog()
        .into_iter()
        .find(|provider| provider.capabilities.iter().any(|item| item == capability))
}

fn builtin_provider_catalog() -> Vec<CapabilityProvider> {
    vec![
        CapabilityProvider {
            domain: "terminal".to_string(),
            crate_name: "crossterm".to_string(),
            version: "0.27".to_string(),
            default_features: true,
            features: Vec::new(),
            capabilities: vec![
                "terminal_raw_input".to_string(),
                "terminal_size".to_string(),
                "terminal_screen_control".to_string(),
            ],
        },
        CapabilityProvider {
            domain: "serialization".to_string(),
            crate_name: "serde".to_string(),
            version: "1.0".to_string(),
            default_features: true,
            features: vec!["derive".to_string()],
            capabilities: vec!["data_serialization".to_string()],
        },
        CapabilityProvider {
            domain: "time".to_string(),
            crate_name: "chrono".to_string(),
            version: "0.4".to_string(),
            default_features: true,
            features: vec!["serde".to_string()],
            capabilities: vec!["datetime_utc".to_string()],
        },
        CapabilityProvider {
            domain: "errors".to_string(),
            crate_name: "anyhow".to_string(),
            version: "1.0".to_string(),
            default_features: true,
            features: Vec::new(),
            capabilities: vec!["error_handling".to_string()],
        },
        CapabilityProvider {
            domain: "encoding".to_string(),
            crate_name: "base64".to_string(),
            version: "0.22".to_string(),
            default_features: true,
            features: Vec::new(),
            capabilities: vec!["base64_encoding".to_string()],
        },
        CapabilityProvider {
            domain: "hashing".to_string(),
            crate_name: "sha2".to_string(),
            version: "0.10".to_string(),
            default_features: true,
            features: Vec::new(),
            capabilities: vec!["sha256_hashing".to_string()],
        },
    ]
}

fn capability_rules() -> &'static [CapabilityRule] {
    &[
        CapabilityRule {
            capability: "terminal_raw_input",
            domain: "terminal",
            patterns: &[
                "raw mode",
                "keypress",
                "key press",
                "key presses",
                "arrow key",
                "arrow keys",
                "disabled echo",
                "non-blocking reads",
            ],
        },
        CapabilityRule {
            capability: "terminal_size",
            domain: "terminal",
            patterns: &[
                "terminal size",
                "window size",
                "screen size",
                "viewport size",
                "terminal width",
                "terminal height",
            ],
        },
        CapabilityRule {
            capability: "terminal_screen_control",
            domain: "terminal",
            patterns: &[
                "clear screen",
                "alternate screen",
                "cursor movement",
                "terminal render",
            ],
        },
        CapabilityRule {
            capability: "data_serialization",
            domain: "serialization",
            patterns: &["serialize", "deserialize", "serde"],
        },
        CapabilityRule {
            capability: "datetime_utc",
            domain: "time",
            patterns: &["datetime", "utc::now", "timestamp", "chrono"],
        },
        CapabilityRule {
            capability: "error_handling",
            domain: "errors",
            patterns: &["anyhow"],
        },
        CapabilityRule {
            capability: "base64_encoding",
            domain: "encoding",
            patterns: &["base64", "rfc 4648"],
        },
        CapabilityRule {
            capability: "sha256_hashing",
            domain: "hashing",
            patterns: &["sha256", "sha-256", "sha2"],
        },
        CapabilityRule {
            capability: "serial_io",
            domain: "serial",
            patterns: &["serial port", "baud", "ttyusb", "ttyacm", "uart"],
        },
        CapabilityRule {
            capability: "websocket_client",
            domain: "websocket",
            patterns: &["websocket", "ws://", "wss://"],
        },
    ]
}

fn canonicalize_crate_name(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        UnmappedCapability, add_capability_mapping_to_registry, bootstrap_registry_from_scan,
        empty_registry, merge_registry_proposals, parse_capability_registry_fragment,
        resolved_dependency_plan_context, scan_draft_capabilities,
        sync_dependency_manifest_from_capability_registry, write_capability_registry,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("reen_capability_registry_{prefix}_{nanos}"))
    }

    #[test]
    fn add_mapping_rejects_conflicting_domain_spec() {
        let mut registry = empty_registry();
        add_capability_mapping_to_registry(
            &mut registry,
            "terminal_raw_input",
            "crossterm",
            "terminal",
            "0.27",
            &[],
            true,
        )
        .expect("first mapping");

        let error = add_capability_mapping_to_registry(
            &mut registry,
            "terminal_size",
            "termion",
            "terminal",
            "1.5",
            &[],
            true,
        )
        .expect_err("expected conflict");
        assert!(error.to_string().contains("different specification"));
    }

    #[test]
    fn bootstrap_registry_marks_unknown_capabilities_unmapped() {
        let root = temp_dir("bootstrap_unknown");
        fs::create_dir_all(root.join("drafts/contexts")).expect("mkdir");
        fs::write(
            root.join("drafts/contexts/serial.md"),
            "# SerialContext\n\nReads from a serial port at a fixed baud rate.\n",
        )
        .expect("write draft");

        let scan = scan_draft_capabilities(&root.join("drafts")).expect("scan");
        let registry = bootstrap_registry_from_scan(None, &scan);
        assert!(
            registry
                .unmapped_capabilities
                .iter()
                .any(|item| item.capability == "serial_io" && item.domain == "serial")
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn sync_materializes_dependencies_for_terminal_project() {
        let root = temp_dir("terminal_sync");
        let drafts = root.join("drafts");
        fs::create_dir_all(drafts.join("contexts")).expect("mkdir");
        fs::write(
            drafts.join("contexts/terminal_renderer.md"),
            "# TerminalRenderer\n\nReads key presses in raw mode and uses terminal size.\n",
        )
        .expect("write draft");

        let scan = scan_draft_capabilities(&drafts).expect("scan");
        let registry = bootstrap_registry_from_scan(None, &scan);
        write_capability_registry(&drafts.join("capability_registry.yml"), &registry)
            .expect("write registry");

        let plan = sync_dependency_manifest_from_capability_registry(&drafts, false)
            .expect("sync")
            .expect("plan");
        assert!(
            plan.packages
                .iter()
                .any(|package| package.name == "crossterm" && package.domain == "terminal")
        );
        let manifest = fs::read_to_string(drafts.join("dependencies.yml")).expect("read manifest");
        assert!(manifest.contains("crossterm"));

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn merge_registry_proposals_keeps_manual_mapping_and_resolves_gaps() {
        let mut base = empty_registry();
        add_capability_mapping_to_registry(
            &mut base,
            "terminal_raw_input",
            "crossterm",
            "terminal",
            "0.27",
            &[],
            true,
        )
        .expect("manual mapping");
        base.unmapped_capabilities.push(UnmappedCapability {
            capability: "serial_io".to_string(),
            domain: "serial".to_string(),
        });

        let proposal = parse_capability_registry_fragment(
            r#"
schema: reen.capability-registry/v1
providers:
  - domain: terminal
    crate: termion
    version: "1.5"
    capabilities: [terminal_size]
  - domain: serial
    crate: serialport
    version: "4.6"
    capabilities: [serial_io]
"#,
        )
        .expect("parse proposal");

        merge_registry_proposals(&mut base, &proposal).expect("merge");
        assert!(base.providers.iter().any(|provider| {
            provider.domain == "terminal"
                && provider.crate_name == "crossterm"
                && provider
                    .capabilities
                    .iter()
                    .any(|capability| capability == "terminal_size")
        }));
        assert!(base.providers.iter().any(|provider| {
            provider.domain == "serial"
                && provider.crate_name == "serialport"
                && provider
                    .capabilities
                    .iter()
                    .any(|capability| capability == "serial_io")
        }));
        assert!(
            !base
                .unmapped_capabilities
                .iter()
                .any(|item| item.capability == "serial_io")
        );
    }

    #[test]
    fn resolved_dependency_plan_context_includes_unresolved_capabilities() {
        let root = temp_dir("context_unresolved");
        let drafts = root.join("drafts");
        fs::create_dir_all(drafts.join("contexts")).expect("mkdir");
        fs::write(
            drafts.join("contexts/serial.md"),
            "# SerialContext\n\nReads from a serial port.\n",
        )
        .expect("write draft");

        let mut registry = empty_registry();
        registry.unmapped_capabilities.push(UnmappedCapability {
            capability: "serial_io".to_string(),
            domain: "serial".to_string(),
        });
        write_capability_registry(&drafts.join("capability_registry.yml"), &registry)
            .expect("write registry");

        let value = resolved_dependency_plan_context(&drafts)
            .expect("plan value")
            .expect("some plan");
        assert!(
            value
                .get("unresolved_capabilities")
                .and_then(|value| value.as_array())
                .is_some_and(|items| !items.is_empty())
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn parse_fragment_accepts_yaml_code_fence() {
        let registry = parse_capability_registry_fragment(
            r#"```yaml
schema: reen.capability-registry/v1
providers:
  - domain: terminal
    crate: crossterm
    version: "0.27"
    capabilities: [terminal_raw_input]
```"#,
        )
        .expect("parse fragment");
        assert_eq!(registry.providers.len(), 1);
        assert_eq!(registry.providers[0].crate_name, "crossterm");
    }
}
