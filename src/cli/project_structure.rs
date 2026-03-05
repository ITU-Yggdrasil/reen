use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

/// Information about the project structure extracted from specifications
#[derive(Debug, Default)]
pub struct ProjectInfo {
    /// Module paths (e.g., "data/ledger_entry", "contexts/account")
    pub modules: HashMap<String, Vec<String>>, // folder -> [module names]
    /// Type names extracted from specs (folder/module_name -> TypeName)
    pub type_names: HashMap<String, String>,
    /// Dependencies and their versions
    pub dependencies: HashMap<String, String>,
    /// Package name
    pub package_name: String,
}

/// Analyzes all specifications and extracts project structure information
pub fn analyze_specifications(spec_dir: &Path, draft_dir: Option<&Path>) -> Result<ProjectInfo> {
    let mut project_info = ProjectInfo {
        package_name: spec_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("generated_project")
            .to_string(),
        ..Default::default()
    };

    // Always include tracing
    project_info
        .dependencies
        .insert("tracing".to_string(), "0.1".to_string());

    // Scan all specification files
    scan_directory(spec_dir, spec_dir, draft_dir, &mut project_info)?;

    Ok(project_info)
}

fn scan_directory(
    base_dir: &Path,
    current_dir: &Path,
    draft_dir: Option<&Path>,
    project_info: &mut ProjectInfo,
) -> Result<()> {
    if !current_dir.is_dir() {
        return Ok(());
    }

    let entries = fs::read_dir(current_dir)
        .with_context(|| format!("Failed to read directory: {}", current_dir.display()))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            // Recursively scan subdirectories
            scan_directory(base_dir, &path, draft_dir, project_info)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("md") {
            // Process specification file
            analyze_spec_file(base_dir, &path, draft_dir, project_info)?;
        }
    }

    Ok(())
}

fn analyze_spec_file(
    base_dir: &Path,
    spec_path: &Path,
    draft_dir: Option<&Path>,
    project_info: &mut ProjectInfo,
) -> Result<()> {
    // Read specification content
    let content = fs::read_to_string(spec_path)
        .with_context(|| format!("Failed to read spec file: {}", spec_path.display()))?;

    // Extract module path
    let relative_path = spec_path.strip_prefix(base_dir).unwrap_or(spec_path);

    if let Some(parent) = relative_path.parent() {
        let folder = parent.to_string_lossy().to_string();
        let module_name = spec_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_lowercase(); // Ensure snake_case module names

        project_info
            .modules
            .entry(folder.clone())
            .or_insert_with(Vec::new)
            .push(module_name.clone());

        // Extract type name from corresponding draft first (if available), then from specification.
        let draft_type_name = draft_dir
            .and_then(|draft_root| read_draft_type_name(draft_root, relative_path).ok())
            .flatten();
        let type_name = draft_type_name.or_else(|| extract_type_name(&content));
        if let Some(type_name) = type_name {
            let key = if folder.is_empty() {
                module_name
            } else {
                format!("{}/{}", folder, module_name)
            };
            project_info.type_names.insert(key, type_name);
        }
    }

    // Detect dependencies from content
    detect_dependencies(&content, project_info);

    Ok(())
}

fn read_draft_type_name(draft_root: &Path, relative_spec_path: &Path) -> Result<Option<String>> {
    let draft_path = draft_root.join(relative_spec_path);
    if !draft_path.exists() {
        return Ok(None);
    }
    let draft_content = fs::read_to_string(&draft_path)
        .with_context(|| format!("Failed to read draft file: {}", draft_path.display()))?;
    Ok(extract_type_name(&draft_content))
}

/// Extracts a likely type name from content.
/// Priority:
/// 1. First markdown header (`# ...`)
/// 2. Any CamelCase token in content (`LedgerEntry`)
fn extract_type_name(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") {
            let name = trimmed.trim_start_matches('#').trim();
            if !name.is_empty() {
                if let Some(rust_name) = to_pascal_case_title(name) {
                    return Some(rust_name);
                }
            }
        }
    }

    // Fallback: find a CamelCase token reference in prose/signatures.
    let mut candidate: Option<String> = None;
    for token in content.split(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
        if looks_like_camel_type(token) {
            candidate = Some(token.to_string());
            break;
        }
    }
    candidate
}

fn looks_like_camel_type(token: &str) -> bool {
    let mut chars = token.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_uppercase() || token.len() < 3 {
        return false;
    }
    let has_lower = token.chars().any(|c| c.is_ascii_lowercase());
    let has_upper_after_first = token.chars().skip(1).any(|c| c.is_ascii_uppercase());
    has_lower && has_upper_after_first
}

fn to_pascal_case_title(s: &str) -> Option<String> {
    let mut out = String::new();
    for raw in s.split(|c: char| !c.is_ascii_alphanumeric()) {
        if raw.is_empty() {
            continue;
        }

        // Preserve mixed-case tokens such as LedgerEntry; normalize plain words.
        let has_lower = raw.chars().any(|c| c.is_ascii_lowercase());
        let has_upper = raw.chars().any(|c| c.is_ascii_uppercase());
        let token = if has_lower && has_upper {
            let mut ch = raw.chars();
            match ch.next() {
                Some(first) => first.to_uppercase().collect::<String>() + ch.as_str(),
                None => String::new(),
            }
        } else {
            let lower = raw.to_ascii_lowercase();
            let mut ch = lower.chars();
            match ch.next() {
                Some(first) => first.to_uppercase().collect::<String>() + ch.as_str(),
                None => String::new(),
            }
        };
        out.push_str(&token);
    }

    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn detect_dependencies(content: &str, project_info: &mut ProjectInfo) {
    // Detect serde (from Serialization section or Serialize/Deserialize keywords)
    if content.contains("Serialize") || content.contains("Deserialize") || content.contains("serde")
    {
        project_info.dependencies.insert(
            "serde".to_string(),
            r#"{ version = "1.0", features = ["derive"] }"#.to_string(),
        );
    }

    // Detect chrono (from DateTime types, Utc, Timestamp references)
    if content.contains("DateTime")
        || content.contains("chrono")
        || content.contains("Utc::now")
        || content.contains("Timestamp")
    {
        project_info.dependencies.insert(
            "chrono".to_string(),
            r#"{ version = "0.4", features = ["serde"] }"#.to_string(),
        );
    }

    // Detect anyhow (from Result types or error handling)
    if content.contains("anyhow") {
        project_info
            .dependencies
            .insert("anyhow".to_string(), "1.0".to_string());
    }

    // Detect base64
    if content.contains("base64") || content.contains("Base64") || content.contains("RFC 4648") {
        project_info
            .dependencies
            .insert("base64".to_string(), "0.22".to_string());
    }

    // Detect sha2/SHA256 hashing
    if content.contains("sha2")
        || content.contains("Sha256")
        || content.contains("SHA256")
        || content.contains("sha256")
    {
        project_info
            .dependencies
            .insert("sha2".to_string(), "0.10".to_string());
    }
}

/// Generates Cargo.toml for the project
pub fn generate_cargo_toml(project_info: &ProjectInfo, output_dir: &Path) -> Result<()> {
    let cargo_toml_path = output_dir.join("Cargo.toml");

    let mut content = String::new();
    content.push_str(&format!(
        "[package]\n\
         name = \"{}\"\n\
         version = \"0.1.0\"\n\
         edition = \"2021\"\n\
         \n",
        project_info.package_name
    ));

    // Add [lib] section
    content.push_str(&format!(
        "[lib]\n\
         name = \"{}\"\n\
         path = \"src/lib.rs\"\n\
         \n",
        project_info.package_name
    ));

    // Add dependencies
    content.push_str("[dependencies]\n");
    let mut deps: Vec<_> = project_info.dependencies.iter().collect();
    deps.sort_by_key(|(k, _)| *k);
    for (name, version) in deps {
        if version.starts_with('{') {
            content.push_str(&format!("{} = {}\n", name, version));
        } else {
            content.push_str(&format!("{} = \"{}\"\n", name, version));
        }
    }

    // Add features for context modules. All context features are enabled by default.
    let mut context_features = project_info
        .modules
        .get("contexts")
        .cloned()
        .unwrap_or_default();
    context_features.sort();
    context_features.dedup();

    if !context_features.is_empty() {
        content.push_str("\n[features]\n");
        let default_list = context_features
            .iter()
            .map(|f| format!("\"{}\"", f))
            .collect::<Vec<_>>()
            .join(", ");
        content.push_str(&format!("default = [{}]\n", default_list));
        for feature in &context_features {
            content.push_str(&format!("{} = []\n", feature));
        }
    }

    fs::write(&cargo_toml_path, content).with_context(|| {
        format!(
            "Failed to write Cargo.toml to {}",
            cargo_toml_path.display()
        )
    })?;

    Ok(())
}

/// Generates src/lib.rs with module declarations
pub fn generate_lib_rs(project_info: &ProjectInfo, output_dir: &Path) -> Result<()> {
    let lib_rs_path = output_dir.join("src/lib.rs");

    // Ensure src directory exists
    let src_dir = output_dir.join("src");
    fs::create_dir_all(&src_dir)
        .with_context(|| format!("Failed to create src directory: {}", src_dir.display()))?;

    let mut content = String::new();
    content.push_str("// Auto-generated by reen - do not edit manually\n\n");

    // Collect top-level module folders
    let mut folders: HashSet<String> = HashSet::new();
    for folder in project_info.modules.keys() {
        if !folder.is_empty() {
            if let Some(top_level) = folder.split('/').next() {
                folders.insert(top_level.to_string());
            }
        }
    }

    // Declare modules
    let mut folders_vec: Vec<_> = folders.into_iter().collect();
    folders_vec.sort();

    for folder in &folders_vec {
        content.push_str(&format!("pub mod {};\n", folder));
    }

    content.push_str("\n// Re-export all public items\n");
    for folder in &folders_vec {
        content.push_str(&format!("pub use {}::*;\n", folder));
    }

    fs::write(&lib_rs_path, content)
        .with_context(|| format!("Failed to write lib.rs to {}", lib_rs_path.display()))?;

    Ok(())
}

/// Generates mod.rs files for subdirectories
pub fn generate_mod_files(project_info: &ProjectInfo, output_dir: &Path) -> Result<()> {
    let src_dir = output_dir.join("src");

    for (folder, modules) in &project_info.modules {
        if folder.is_empty() {
            continue;
        }

        let mod_rs_path = src_dir.join(folder).join("mod.rs");

        // Ensure directory exists
        if let Some(parent) = mod_rs_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }

        let mut content = String::new();
        content.push_str("// Auto-generated by reen - do not edit manually\n\n");

        let mut sorted_modules = modules.clone();
        sorted_modules.sort();

        // Declare modules
        for module in &sorted_modules {
            content.push_str(&format!("mod {};\n", module));
        }

        content.push_str("\n// Re-export public items\n");
        for module in &sorted_modules {
            // Get actual type name from specification, or fall back to PascalCase conversion
            let key = format!("{}/{}", folder, module);
            let type_name = project_info
                .type_names
                .get(&key)
                .cloned()
                .unwrap_or_else(|| to_pascal_case(module));

            content.push_str(&format!("pub use {}::{};\n", module, type_name));
        }

        fs::write(&mod_rs_path, content)
            .with_context(|| format!("Failed to write mod.rs to {}", mod_rs_path.display()))?;
    }

    Ok(())
}

/// Converts snake_case to PascalCase
fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_pascal_case() {
        assert_eq!(to_pascal_case("ledger_entry"), "LedgerEntry");
        assert_eq!(to_pascal_case("account"), "Account");
        assert_eq!(to_pascal_case("money_transfer"), "MoneyTransfer");
    }

    #[test]
    fn test_extract_type_name_from_header_title_words() {
        let content = "# Money transfer\n\n## Description\n...";
        assert_eq!(
            extract_type_name(content),
            Some("MoneyTransfer".to_string())
        );
    }

    #[test]
    fn test_extract_type_name_from_camel_case_reference() {
        let content = "Returns anyhow::Result<LedgerEntry>";
        assert_eq!(extract_type_name(content), Some("LedgerEntry".to_string()));
    }
}
