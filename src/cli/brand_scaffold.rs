use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::brand_specs::{collect_brand_token_references, unresolved_brand_token_references};
use super::{extract_implementation_failure_message, Config, SPECIFICATIONS_DIR};

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

impl BrandScaffoldValidator {
    pub(crate) fn validate(
        context_file: &Path,
        context_name: &str,
        generated_files: &[GeneratedOutputFile],
    ) -> Result<BrandValidationReport> {
        let required_paths = [
            Path::new("Cargo.toml"),
            Path::new("Leptos.toml"),
            Path::new("src/main.rs"),
            Path::new("src/lib.rs"),
            Path::new("src/app.rs"),
            Path::new("style/app.css"),
        ];
        for required in required_paths {
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
    for (marker, description) in required_markers {
        if !content.contains(marker) {
            anyhow::bail!(
                "Generated brand implementation for '{}' is missing {} in Cargo.toml",
                context_name,
                description
            );
        }
    }
    Ok(())
}

fn validate_leptos_toml(context_name: &str, content: &str) -> Result<()> {
    let required_markers = ["output-name", "site-root", "site-pkg-dir", "style-file"];
    for marker in required_markers {
        if !content.contains(marker) {
            anyhow::bail!(
                "Generated brand implementation for '{}' is missing '{}' in Leptos.toml",
                context_name,
                marker
            );
        }
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
