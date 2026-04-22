use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::brand_specs::{collect_brand_token_references, unresolved_brand_token_references};
use super::{extract_implementation_failure_message, Config, SPECIFICATIONS_DIR};

const REQUIRED_BRAND_SCAFFOLD_PATHS: &[&str] = &[
    "Cargo.toml",
    "Leptos.toml",
    ".gitignore",
    "src/main.rs",
    "src/lib.rs",
    "src/app.rs",
    "style/app.css",
];
const CARGO_LEPTOS_REQUIRED_KEYS: &[&str] = &[
    "output-name",
    "site-root",
    "site-pkg-dir",
    "style-file",
    "assets-dir",
    "site-addr",
    "reload-port",
    "bin-features",
    "bin-default-features",
    "lib-features",
    "lib-default-features",
];
const SHARED_LEPTOS_CONFIG_KEYS: &[&str] = &[
    "output-name",
    "site-root",
    "site-pkg-dir",
    "style-file",
    "assets-dir",
    "site-addr",
    "reload-port",
];
const LEPTOS_TOML_PACKAGE_KEYS: &[&str] = &["name", "lib", "bin"];
const BRAND_CARGO_MINIMUM_SHAPE: &str = r#"[package]
name = "app-name"
version = "0.1.0"
edition = "2021"

[package.metadata.leptos]
output-name = "app-name"
site-root = "target/site"
site-pkg-dir = "pkg"
style-file = "style/app.css"
assets-dir = "public"
site-addr = "127.0.0.1:3000"
reload-port = 3001
bin-features = ["ssr"]
bin-default-features = false
lib-features = ["hydrate"]
lib-default-features = false

[lib]
name = "app_name"
path = "src/lib.rs"
crate-type = ["cdylib", "rlib"]

[[bin]]
name = "app-name"
path = "src/main.rs"
required-features = ["ssr"]

[dependencies]
axum = { version = "...", optional = true }
leptos = { version = "...", default-features = false }
leptos_meta = { version = "...", default-features = false }
leptos_axum = { version = "...", optional = true }
leptos_router = { version = "...", default-features = false }
wasm-bindgen = { version = "...", optional = true }
tokio = { version = "...", features = ["full"], optional = true }

[features]
default = []
hydrate = [
    "dep:wasm-bindgen",
    "leptos/hydrate",
    "leptos_meta/hydrate",
    "leptos_router/hydrate",
]
ssr = [
    "dep:axum",
    "dep:leptos_axum",
    "dep:tokio",
    "leptos/ssr",
    "leptos_meta/ssr",
    "leptos_router/ssr",
]"#;

const BRAND_LEPTOS_TOML_MINIMUM_SHAPE: &str = r#"[package]
name = "app-name"
lib = { path = "src/lib.rs" }
bin = { path = "src/main.rs" }

[leptos]
output-name = "app-name"
site-root = "target/site"
site-pkg-dir = "pkg"
style-file = "style/app.css"
assets-dir = "public"
site-addr = "127.0.0.1:3000"
reload-port = 3001"#;
const BRAND_GITIGNORE_MINIMUM_SHAPE: &str = r#"target/
.cargo-leptos/
.leptos/
.reen/"#;
const BRAND_LIB_RS_MINIMUM_SHAPE: &str = r#"pub mod app;
pub use app::App;"#;
const BRAND_APP_RS_MINIMUM_SHAPE: &str = r#"use leptos::*;
use leptos_router::*;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <Router>
            <Routes>
                <Route path="/" view=HomePage/>
            </Routes>
        </Router>
    }
}"#;
const BRAND_MAIN_RS_MINIMUM_SHAPE: &str = r#"use axum::Router;
use leptos::*;
use leptos_axum::{generate_route_list, LeptosRoutes};
use app_name::App;

#[tokio::main]
async fn main() {
    let conf = get_configuration(None).await.unwrap();
    let leptos_options = conf.leptos_options;
    let addr = leptos_options.site_addr;
    let routes = generate_route_list(|| view! { <App/> });

    let app = Router::new()
        .leptos_routes(&leptos_options, routes, || view! { <App/> })
        .with_state(leptos_options);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app.into_make_service()).await.unwrap();
}"#;
const BRAND_APP_CSS_MINIMUM_SHAPE: &str = r#":root {
    --brand-colors-primary-black: #000000;
    --brand-colors-primary-white: #ffffff;
}

body {
    font-family: 'Inter', Arial, Helvetica, sans-serif;
    color: var(--brand-colors-primary-black);
    background-color: var(--brand-colors-primary-white);
}"#;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GeneratedOutputFile {
    pub(crate) path: PathBuf,
    pub(crate) content: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrandValidationReport {
    pub(crate) generated_files: Vec<GeneratedOutputFile>,
}

pub(crate) struct BrandEnvelopeParser;

impl BrandEnvelopeParser {
    pub(crate) fn parse(output: &str) -> Result<Vec<GeneratedOutputFile>> {
        const FILE_PREFIX: &str = "===FILE:";
        const FILE_SUFFIX: &str = "===";
        const END_MARKER: &str = "===END_FILE===";

        let mut files = Vec::new();
        let mut current_path: Option<String> = None;
        let mut current_lines: Vec<String> = Vec::new();
        let mut seen_paths = HashSet::new();

        for line in output.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with(FILE_PREFIX) && trimmed.ends_with(FILE_SUFFIX) {
                if current_path.is_some() {
                    anyhow::bail!(
                        "generated output started a new file block before closing the previous one"
                    );
                }
                let raw_path = trimmed
                    .trim_start_matches(FILE_PREFIX)
                    .trim_end_matches(FILE_SUFFIX)
                    .trim();
                if raw_path.is_empty() {
                    anyhow::bail!("generated output declared an empty file path");
                }
                current_path = Some(raw_path.to_string());
                current_lines.clear();
                continue;
            }

            if trimmed == END_MARKER {
                let raw_path = current_path.take().ok_or_else(|| {
                    anyhow::anyhow!("generated output ended a file block before starting one")
                })?;
                let path = validate_generated_output_path(&raw_path)?;
                let key = path.to_string_lossy().to_string();
                if !seen_paths.insert(key.clone()) {
                    anyhow::bail!("generated output contains duplicate file entry '{}'", key);
                }
                files.push(GeneratedOutputFile {
                    path,
                    content: current_lines.join("\n"),
                });
                current_lines.clear();
                continue;
            }

            if current_path.is_some() {
                current_lines.push(line.to_string());
            } else if !trimmed.is_empty() {
                anyhow::bail!(
                    "generated output contains non-file content outside file blocks: '{}'",
                    trimmed
                );
            }
        }

        if current_path.is_some() {
            anyhow::bail!("generated output ended before closing the last file block");
        }
        if files.is_empty() {
            anyhow::bail!("generated output did not contain any file blocks");
        }

        Ok(files)
    }
}

pub(crate) struct BrandScaffoldValidator;

pub(crate) fn render_brand_scaffold_contract() -> String {
    format!(
        "Use the following scaffold shape as the default baseline unless the specification forces a compatible variation.\n\
The scaffold must remain compatible with a normal single-package Leptos app intended to run with `cargo leptos watch`.\n\n\
`Cargo.toml` minimum shape:\n\n\
{cargo}\n\n\
`Leptos.toml` minimum shape:\n\n\
{leptos_toml}\n\n\
`.gitignore` minimum shape:\n\n\
{gitignore}\n\n\
`src/lib.rs` minimum shape:\n\n\
{lib_rs}\n\n\
`src/app.rs` minimum shape:\n\n\
{app_rs}\n\n\
Do not redefine the route tree in `src/main.rs` or `src/lib.rs`.\n\n\
`src/main.rs` minimum shape:\n\n\
{main_rs}\n\n\
`style/app.css` minimum shape:\n\n\
{app_css}",
        cargo = BRAND_CARGO_MINIMUM_SHAPE,
        leptos_toml = BRAND_LEPTOS_TOML_MINIMUM_SHAPE,
        gitignore = BRAND_GITIGNORE_MINIMUM_SHAPE,
        lib_rs = BRAND_LIB_RS_MINIMUM_SHAPE,
        app_rs = BRAND_APP_RS_MINIMUM_SHAPE,
        main_rs = BRAND_MAIN_RS_MINIMUM_SHAPE,
        app_css = BRAND_APP_CSS_MINIMUM_SHAPE,
    )
}

impl BrandScaffoldValidator {
    pub(crate) fn validate(
        context_file: &Path,
        context_name: &str,
        generated_files: &[GeneratedOutputFile],
    ) -> Result<BrandValidationReport> {
        for required in REQUIRED_BRAND_SCAFFOLD_PATHS {
            let required = Path::new(required);
            if !generated_files.iter().any(|file| file.path == required) {
                anyhow::bail!(
                    "Generated brand implementation for '{}' is missing required scaffold file '{}'",
                    context_name,
                    required.display()
                );
            }
        }

        if !generated_files
            .iter()
            .any(|file| file.path.starts_with("public") && file.path.file_name().is_some())
        {
            anyhow::bail!(
                "Generated brand implementation for '{}' must include at least one file under public/",
                context_name
            );
        }

        let cargo_toml = find_file(generated_files, "Cargo.toml")?;
        validate_cargo_toml(context_name, &cargo_toml.content)?;

        let leptos_toml = find_file(generated_files, "Leptos.toml")?;
        validate_leptos_toml(context_name, &leptos_toml.content)?;
        validate_matching_leptos_config(context_name, &cargo_toml.content, &leptos_toml.content)?;

        let gitignore = find_file(generated_files, ".gitignore")?;
        validate_gitignore(context_name, &gitignore.content)?;

        let main_rs = find_file(generated_files, "src/main.rs")?;
        validate_main_rs(context_name, &main_rs.content)?;

        let lib_rs = find_file(generated_files, "src/lib.rs")?;
        validate_lib_rs(context_name, &lib_rs.content)?;

        let app_rs = find_file(generated_files, "src/app.rs")?;
        validate_app_rs(context_name, &app_rs.content)?;

        let app_css = find_file(generated_files, "style/app.css")?;
        validate_app_css(context_name, &app_css.content)?;

        let combined = generated_files
            .iter()
            .map(|file| file.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let referenced_tokens = collect_brand_token_references(&combined);
        if !referenced_tokens.is_empty() {
            let unresolved = unresolved_brand_token_references(&combined, SPECIFICATIONS_DIR)
                .with_context(|| {
                    format!(
                        "failed to validate generated brand token references for {}",
                        context_file.display()
                    )
                })?;
            if !unresolved.is_empty() {
                anyhow::bail!(
                    "Generated brand implementation for '{}' references undefined brand token(s): {}",
                    context_name,
                    unresolved.join(", ")
                );
            }
        }

        Ok(BrandValidationReport {
            generated_files: generated_files.to_vec(),
        })
    }
}

pub(crate) struct BrandScaffoldWriter;

impl BrandScaffoldWriter {
    pub(crate) fn write(
        context_file: &Path,
        context_name: &str,
        config: &Config,
        generated_files: &[GeneratedOutputFile],
    ) -> Result<()> {
        for file in generated_files {
            if let Some(parent) = file.path.parent() {
                fs::create_dir_all(parent).with_context(|| {
                    format!(
                        "Failed to create brand implementation directory {}",
                        parent.display()
                    )
                })?;
            }
            fs::write(&file.path, &file.content).with_context(|| {
                format!(
                    "Failed to write brand implementation file {}",
                    file.path.display()
                )
            })?;
            if config.verbose {
                println!("Written brand implementation file: {}", file.path.display());
            }
        }

        if let Some((failed_file, message)) = generated_files
            .iter()
            .filter(|file| file.path.extension().and_then(|ext| ext.to_str()) == Some("rs"))
            .find_map(|file| {
                extract_implementation_failure_message(&file.content)
                    .map(|message| (file.path.clone(), message))
            })
        {
            eprintln!("error[impl:compile_error]:");
            eprintln!("\u{001b}[31m{}\u{001b}[0m", context_file.display());
            eprintln!(
                "  Generated brand implementation for '{}' contains an explicit failure marker in {}:",
                context_name,
                failed_file.display()
            );
            eprintln!();
            for line in message.lines() {
                eprintln!("  {}", line);
            }
            eprintln!();
            anyhow::bail!(
                "Generated brand implementation for '{}' contains explicit failure marker",
                context_name
            );
        }

        Ok(())
    }
}

pub(crate) fn finalize_brand_implementation_output(
    context_file: &Path,
    context_name: &str,
    config: &Config,
    impl_result: String,
) -> Result<()> {
    let generated_files = BrandEnvelopeParser::parse(&impl_result)?;
    let report = BrandScaffoldValidator::validate(context_file, context_name, &generated_files)?;
    BrandScaffoldWriter::write(context_file, context_name, config, &report.generated_files)
}

fn validate_generated_output_path(raw_path: &str) -> Result<PathBuf> {
    let path = Path::new(raw_path);
    if path.is_absolute() {
        anyhow::bail!("generated output path '{}' must be relative", raw_path);
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(part) => normalized.push(part),
            _ => anyhow::bail!(
                "generated output path '{}' contains disallowed path traversal or prefix components",
                raw_path
            ),
        }
    }

    if normalized.as_os_str().is_empty() {
        anyhow::bail!(
            "generated output path '{}' is empty after normalization",
            raw_path
        );
    }

    Ok(normalized)
}

fn find_file<'a>(
    generated_files: &'a [GeneratedOutputFile],
    path: &str,
) -> Result<&'a GeneratedOutputFile> {
    generated_files
        .iter()
        .find(|file| file.path == Path::new(path))
        .ok_or_else(|| anyhow::anyhow!("Generated brand implementation is missing {}", path))
}

fn validate_cargo_toml(context_name: &str, content: &str) -> Result<()> {
    let required_markers = [
        ("leptos", "Leptos dependency"),
        ("leptos_router", "Leptos router dependency"),
        ("leptos_axum", "Leptos Axum integration"),
        ("axum", "Axum dependency"),
        ("ssr", "ssr feature"),
        ("hydrate", "hydrate feature"),
    ];
    if content.contains("[workspace]") {
        anyhow::bail!(
            "Generated brand implementation for '{}' must be a single-package project, not a workspace",
            context_name
        );
    }
    require_section(
        context_name,
        "Cargo.toml",
        content,
        "package.metadata.leptos",
    )?;
    for key in CARGO_LEPTOS_REQUIRED_KEYS {
        require_key_in_section(
            context_name,
            "Cargo.toml",
            content,
            "package.metadata.leptos",
            key,
        )?;
    }
    for (marker, description) in required_markers {
        if !content.contains(marker) {
            anyhow::bail!(
                "Generated brand implementation for '{}' is missing {} in Cargo.toml",
                context_name,
                description
            );
        }
    }
    validate_optional_dep_feature_wiring(context_name, content)?;
    validate_dependency_render_feature_mode(context_name, content)?;
    validate_lib_target_name(context_name, content)?;
    Ok(())
}

fn validate_leptos_toml(context_name: &str, content: &str) -> Result<()> {
    require_section(context_name, "Leptos.toml", content, "package")?;
    require_section(context_name, "Leptos.toml", content, "leptos")?;
    for key in LEPTOS_TOML_PACKAGE_KEYS {
        require_key_in_section(context_name, "Leptos.toml", content, "package", key)?;
    }
    for key in SHARED_LEPTOS_CONFIG_KEYS {
        require_key_in_section(context_name, "Leptos.toml", content, "leptos", key)?;
    }
    Ok(())
}

fn validate_main_rs(context_name: &str, content: &str) -> Result<()> {
    let required_markers = [
        "axum",
        "leptos_axum",
        "tokio::main",
        "generate_route_list",
        "leptos_routes",
        "with_state",
        "axum::serve",
        "TcpListener",
        "into_make_service",
    ];
    for marker in required_markers {
        if !content.contains(marker) {
            anyhow::bail!(
                "Generated brand implementation for '{}' does not contain detectable Axum SSR wiring in src/main.rs",
                context_name
            );
        }
    }
    let forbidden_markers = [
        "axum::Server::bind",
        "|cx|",
        "view! { cx,",
        "axum::serve(listener, app).",
    ];
    for marker in forbidden_markers {
        if content.contains(marker) {
            anyhow::bail!(
                "Generated brand implementation for '{}' uses stale Leptos/Axum pattern '{}' in src/main.rs",
                context_name,
                marker
            );
        }
    }
    Ok(())
}

fn validate_lib_rs(context_name: &str, content: &str) -> Result<()> {
    let required_markers = ["mod app", "pub use app::App"];
    if required_markers
        .iter()
        .any(|marker| !content.contains(marker))
    {
        anyhow::bail!(
            "Generated brand implementation for '{}' does not contain detectable hydration/bootstrap wiring in src/lib.rs",
            context_name
        );
    }
    Ok(())
}

fn validate_app_rs(context_name: &str, content: &str) -> Result<()> {
    if !content.contains("App") || !content.contains("Route") {
        anyhow::bail!(
            "Generated brand implementation for '{}' does not define a detectable App router in src/app.rs",
            context_name
        );
    }
    if !content.contains("leptos_router") {
        anyhow::bail!(
            "Generated brand implementation for '{}' does not import router APIs from leptos_router in src/app.rs",
            context_name
        );
    }
    let markers = [
        "path=\"/\"",
        "path = \"/\"",
        "path=path!(\"/\")",
        "path = path!(\"/\")",
        "StaticSegment(\"\")",
    ];
    if !markers.iter().any(|marker| content.contains(marker)) {
        anyhow::bail!(
            "Generated brand implementation for '{}' does not define a detectable root route in src/app.rs",
            context_name
        );
    }
    Ok(())
}

fn validate_app_css(context_name: &str, content: &str) -> Result<()> {
    if !content.contains(":root") || !content.contains("--brand-") {
        anyhow::bail!(
            "Generated brand implementation for '{}' does not emit detectable brand CSS custom properties in style/app.css",
            context_name
        );
    }
    Ok(())
}

fn validate_gitignore(context_name: &str, content: &str) -> Result<()> {
    let required_entries = ["target/", ".cargo-leptos/"];
    for entry in required_entries {
        if !content.lines().any(|line| line.trim() == entry) {
            anyhow::bail!(
                "Generated brand implementation for '{}' is missing '{}' in .gitignore",
                context_name,
                entry
            );
        }
    }
    Ok(())
}

fn require_section(
    context_name: &str,
    file_name: &str,
    content: &str,
    section: &str,
) -> Result<()> {
    let header = format!("[{}]", section);
    if !content.contains(&header) {
        anyhow::bail!(
            "Generated brand implementation for '{}' is missing '[{}]' in {}",
            context_name,
            section,
            file_name
        );
    }
    Ok(())
}

fn require_key_in_section(
    context_name: &str,
    file_name: &str,
    content: &str,
    section: &str,
    key: &str,
) -> Result<String> {
    extract_toml_value(content, section, key).ok_or_else(|| {
        anyhow::anyhow!(
            "Generated brand implementation for '{}' is missing '{}' in [{}] of {}",
            context_name,
            key,
            section,
            file_name
        )
    })
}

fn extract_toml_value(content: &str, section: &str, key: &str) -> Option<String> {
    let target_header = format!("[{}]", section);
    let mut in_section = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_section = trimmed == target_header;
            continue;
        }
        if !in_section {
            continue;
        }

        if let Some((found_key, value)) = trimmed.split_once('=') {
            if found_key.trim() == key {
                return Some(value.trim().trim_matches('"').to_string());
            }
        }
    }

    None
}

fn validate_matching_leptos_config(
    context_name: &str,
    cargo_toml: &str,
    leptos_toml: &str,
) -> Result<()> {
    for key in SHARED_LEPTOS_CONFIG_KEYS {
        let cargo_value = require_key_in_section(
            context_name,
            "Cargo.toml",
            cargo_toml,
            "package.metadata.leptos",
            key,
        )?;
        let leptos_value =
            require_key_in_section(context_name, "Leptos.toml", leptos_toml, "leptos", key)?;
        if cargo_value != leptos_value {
            anyhow::bail!(
                "Generated brand implementation for '{}' has mismatched '{}' between Cargo.toml and Leptos.toml",
                context_name,
                key
            );
        }
    }
    Ok(())
}

fn validate_optional_dep_feature_wiring(context_name: &str, cargo_toml: &str) -> Result<()> {
    for dependency in extract_dep_feature_refs(cargo_toml) {
        let Some(spec) = extract_dependency_spec(cargo_toml, &dependency) else {
            anyhow::bail!(
                "Generated brand implementation for '{}' references optional dependency feature 'dep:{}' without declaring dependency '{}'",
                context_name,
                dependency,
                dependency
            );
        };
        if !spec.contains("optional = true") {
            anyhow::bail!(
                "Generated brand implementation for '{}' uses 'dep:{}' in Cargo.toml features, but dependency '{}' is not declared with optional = true",
                context_name,
                dependency,
                dependency
            );
        }
    }
    Ok(())
}

fn validate_dependency_render_feature_mode(context_name: &str, cargo_toml: &str) -> Result<()> {
    for dependency in ["leptos", "leptos_meta", "leptos_router"] {
        let Some(spec) = extract_dependency_spec(cargo_toml, dependency) else {
            continue;
        };
        let has_ssr = spec.contains("ssr");
        let has_hydrate = spec.contains("hydrate");
        let has_csr = spec.contains("csr");
        if (has_ssr && has_hydrate) || (has_ssr && has_csr) {
            anyhow::bail!(
                "Generated brand implementation for '{}' directly enables conflicting render features for dependency '{}'; gate render modes through Cargo features instead",
                context_name,
                dependency
            );
        }
    }
    Ok(())
}

fn validate_lib_target_name(context_name: &str, cargo_toml: &str) -> Result<()> {
    if let Some(lib_name) = extract_toml_value(cargo_toml, "lib", "name") {
        if lib_name.contains('-') {
            anyhow::bail!(
                "Generated brand implementation for '{}' uses invalid [lib].name '{}'; Rust library target names must not contain hyphens",
                context_name,
                lib_name
            );
        }
    }
    Ok(())
}

fn extract_dep_feature_refs(content: &str) -> Vec<String> {
    let mut refs = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("\"dep:") {
            if let Some(name) = rest.strip_suffix("\",") {
                refs.push(name.to_string());
                continue;
            }
            if let Some(name) = rest.strip_suffix('"') {
                refs.push(name.to_string());
            }
        }
    }
    refs.sort();
    refs.dedup();
    refs
}

fn extract_dependency_spec(content: &str, dependency: &str) -> Option<String> {
    let mut in_dependencies = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_dependencies = trimmed == "[dependencies]";
            continue;
        }
        if !in_dependencies || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let (name, spec) = trimmed.split_once('=')?;
        if name.trim() == dependency {
            return Some(spec.trim().to_string());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{
        extract_dep_feature_refs, extract_dependency_spec, extract_toml_value,
        render_brand_scaffold_contract, validate_dependency_render_feature_mode,
        validate_lib_target_name, validate_matching_leptos_config,
        validate_optional_dep_feature_wiring,
    };

    #[test]
    fn rendered_contract_includes_expected_scaffold_shapes() {
        let rendered = render_brand_scaffold_contract();

        assert!(rendered.contains("`Cargo.toml` minimum shape:"));
        assert!(rendered.contains("`Leptos.toml` minimum shape:"));
        assert!(rendered.contains("`.gitignore` minimum shape:"));
        assert!(rendered.contains("`src/lib.rs` minimum shape:"));
        assert!(rendered.contains("`src/app.rs` minimum shape:"));
        assert!(rendered.contains("`src/main.rs` minimum shape:"));
        assert!(rendered.contains("`style/app.css` minimum shape:"));
        assert!(rendered.contains("[package.metadata.leptos]"));
        assert!(rendered.contains("[lib]\nname = \"app_name\""));
        assert!(rendered.contains("target/"));
        assert!(rendered.contains("cargo leptos watch"));
        assert!(
            rendered.contains("Do not redefine the route tree in `src/main.rs` or `src/lib.rs`.")
        );
        assert!(rendered.contains(".with_state(leptos_options)"));
    }

    #[test]
    fn toml_value_extraction_reads_expected_section_keys() {
        let content = r#"[package.metadata.leptos]
site-root = "target/site"
reload-port = 3001
"#;

        assert_eq!(
            extract_toml_value(content, "package.metadata.leptos", "site-root"),
            Some("target/site".to_string())
        );
        assert_eq!(
            extract_toml_value(content, "package.metadata.leptos", "reload-port"),
            Some("3001".to_string())
        );
        assert_eq!(
            extract_toml_value(content, "package.metadata.leptos", "missing-key"),
            None
        );
    }

    #[test]
    fn matching_leptos_config_requires_equal_shared_values() {
        let cargo = r#"[package.metadata.leptos]
output-name = "demo"
site-root = "target/site"
site-pkg-dir = "pkg"
style-file = "style/app.css"
assets-dir = "public"
site-addr = "127.0.0.1:3000"
reload-port = 3001
bin-features = ["ssr"]
bin-default-features = false
lib-features = ["hydrate"]
lib-default-features = false
"#;
        let leptos = r#"[package]
name = "demo"
lib = { path = "src/lib.rs" }
bin = { path = "src/main.rs" }

[leptos]
output-name = "demo"
site-root = "target/site"
site-pkg-dir = "pkg"
style-file = "style/app.css"
assets-dir = "public"
site-addr = "127.0.0.1:3000"
reload-port = 3001
"#;

        validate_matching_leptos_config("demo", cargo, leptos).expect("matching config");

        let mismatched = leptos.replace("reload-port = 3001", "reload-port = 4001");
        let err = validate_matching_leptos_config("demo", cargo, &mismatched)
            .expect_err("expected mismatch failure");
        assert!(err.to_string().contains("mismatched 'reload-port'"));
    }

    #[test]
    fn dep_feature_refs_require_optional_dependencies() {
        let cargo = r#"[dependencies]
axum = { version = "0.7", optional = true }
tokio = { version = "1", features = ["full"] }

[features]
ssr = [
    "dep:axum",
    "dep:tokio",
]
"#;

        assert_eq!(
            extract_dep_feature_refs(cargo),
            vec!["axum".to_string(), "tokio".to_string()]
        );
        assert_eq!(
            extract_dependency_spec(cargo, "axum"),
            Some(r#"{ version = "0.7", optional = true }"#.to_string())
        );

        let err = validate_optional_dep_feature_wiring("demo", cargo)
            .expect_err("expected non-optional dep failure");
        assert!(err.to_string().contains("dep:tokio"));
        assert!(err.to_string().contains("optional = true"));
    }

    #[test]
    fn direct_render_mode_features_must_not_be_enabled_together_on_dependencies() {
        let cargo = r#"[dependencies]
leptos = { version = "0.6", features = ["ssr", "hydrate"] }
leptos_meta = { version = "0.6", default-features = false }
leptos_router = { version = "0.6", default-features = false }
"#;

        let err = validate_dependency_render_feature_mode("demo", cargo)
            .expect_err("expected conflicting render features");
        assert!(err.to_string().contains("conflicting render features"));
        assert!(err.to_string().contains("leptos"));
    }

    #[test]
    fn lib_target_name_must_not_use_hyphens() {
        let cargo = r#"[lib]
name = "test-company"
path = "src/lib.rs"
"#;

        let err = validate_lib_target_name("demo", cargo).expect_err("expected hyphen failure");
        assert!(err
            .to_string()
            .contains("library target names must not contain hyphens"));
    }
}
