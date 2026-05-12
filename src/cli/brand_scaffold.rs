use anyhow::{Context, Result};
use regex::Regex;
use serde_json::json;
use std::collections::{HashMap, HashSet};
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
.reen/
/style
Leptos.toml
/public"#;
const BRAND_LIB_RS_MINIMUM_SHAPE: &str = r#"pub mod app;
pub use app::App;"#;
const BRAND_APP_RS_MINIMUM_SHAPE: &str = r#"use leptos::*;
use leptos_router::*;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <style>
            {include_str!("../style/app.css")}
        </style>
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ComponentSpecContract {
    name: String,
    variant_values: Vec<String>,
    enum_name: String,
    rust_variants: Vec<String>,
    default_variant: Option<String>,
    props: Vec<ComponentContractField>,
    object_contracts: Vec<ComponentObjectContract>,
    collection_contracts: Vec<ComponentCollectionContract>,
    interaction_contracts: Vec<ComponentInteractionContract>,
    composition_contracts: Vec<ComponentCompositionContract>,
    brand_constraints: Vec<ComponentBrandConstraint>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ComponentPropSpec {
    name: String,
    ty: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ComponentContractField {
    name: String,
    required: bool,
    shape: String,
    ty: String,
    item_contract: Option<String>,
    object_contract: Option<String>,
    allowed: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ComponentObjectContract {
    name: String,
    fields: Vec<ComponentContractField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ComponentCollectionContract {
    name: String,
    item_contract: String,
    behavior: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ComponentInteractionContract {
    target: String,
    kind: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ComponentCompositionContract {
    name: String,
    usage: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ComponentBrandConstraint {
    topic: String,
    guidance: String,
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
Keep generated reusable components in `src/app.rs` for this scaffold version; do not split them into `src/components/` yet.\n\
When a component specification explicitly enumerates `variant` values, implement that prop with a typed `{{ComponentName}}Variant` enum in `src/app.rs` and use enum values at call sites instead of raw string literals.\n\
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
        let mut normalized_files = generated_files.to_vec();
        for required in REQUIRED_BRAND_SCAFFOLD_PATHS {
            let required = Path::new(required);
            if !normalized_files.iter().any(|file| file.path == required) {
                anyhow::bail!(
                    "Generated brand implementation for '{}' is missing required scaffold file '{}'",
                    context_name,
                    required.display()
                );
            }
        }

        if !normalized_files
            .iter()
            .any(|file| file.path.starts_with("public") && file.path.file_name().is_some())
        {
            anyhow::bail!(
                "Generated brand implementation for '{}' must include at least one file under public/",
                context_name
            );
        }

        let cargo_toml = find_file(&normalized_files, "Cargo.toml")?;
        validate_cargo_toml(context_name, &cargo_toml.content)?;

        let leptos_toml = find_file(&normalized_files, "Leptos.toml")?;
        validate_leptos_toml(context_name, &leptos_toml.content)?;
        validate_matching_leptos_config(context_name, &cargo_toml.content, &leptos_toml.content)?;

        let gitignore = find_file(&normalized_files, ".gitignore")?;
        validate_gitignore(context_name, &gitignore.content)?;

        let main_rs = find_file(&normalized_files, "src/main.rs")?;
        validate_main_rs(context_name, &main_rs.content)?;

        let lib_rs = find_file(&normalized_files, "src/lib.rs")?;
        // Clone to avoid holding an immutable borrow across later in-place CSS repair.
        let lib_rs_content = lib_rs.content.clone();
        validate_lib_rs(context_name, &lib_rs_content)?;

        let app_rs = find_file(&normalized_files, "src/app.rs")?;
        validate_app_rs(context_name, &app_rs.content)?;
        validate_requested_component_implementations(context_name, &app_rs.content)?;

        // Repair brand CSS to keep scaffolds runnable even when drafts/specs are imprecise
        // (e.g. named-only colors, or missing CSS variable definitions).
        let app_css_idx = normalized_files
            .iter()
            .position(|file| file.path == Path::new("style/app.css"))
            .ok_or_else(|| {
                anyhow::anyhow!("Generated brand implementation is missing style/app.css")
            })?;
        let repaired_css = repair_brand_app_css(context_name, &normalized_files[app_css_idx].content)?;
        normalized_files[app_css_idx].content = repaired_css;

        let app_css = find_file(&normalized_files, "style/app.css")?;
        validate_app_css(context_name, &app_css.content)?;
        validate_brand_css_variables(context_name, &app_css.content)?;

        validate_generated_brand_rust_patterns(context_name, &normalized_files)?;
        validate_component_module_wiring(context_name, &normalized_files, &lib_rs_content)?;

        let combined = normalized_files
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
            generated_files: normalized_files,
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

#[derive(Clone, Debug, Eq, PartialEq)]
struct ComponentModuleSpec {
    path: PathBuf,
    module_name: String,
    component_name: String,
}

pub(crate) fn finalize_brand_implementation_output(
    context_file: &Path,
    context_name: &str,
    config: &Config,
    impl_result: String,
) -> Result<()> {
    let component_specs = load_component_spec_contracts()?;
    let generated_files = normalize_generated_brand_files(
        BrandEnvelopeParser::parse(&impl_result)?,
        &component_specs,
    );
    let report = BrandScaffoldValidator::validate(context_file, context_name, &generated_files)?;
    BrandScaffoldWriter::write(context_file, context_name, config, &report.generated_files)
}

fn normalize_generated_brand_files(
    mut generated_files: Vec<GeneratedOutputFile>,
    component_specs: &[ComponentSpecContract],
) -> Vec<GeneratedOutputFile> {
    let mut normalized = generated_files
        .drain(..)
        .map(|mut file| {
            if file.path == Path::new("src/app.rs") {
                file.content = normalize_generated_app_rs(&file.content, component_specs);
            }
            file
        })
        .collect::<Vec<_>>();

    if !normalized
        .iter()
        .any(|file| file.path == Path::new(".gitignore"))
    {
        normalized.push(GeneratedOutputFile {
            path: PathBuf::from(".gitignore"),
            content: BRAND_GITIGNORE_MINIMUM_SHAPE.to_string(),
        });
    }

    normalized
}

fn normalize_generated_app_rs(content: &str, component_specs: &[ComponentSpecContract]) -> String {
    let mut updated =
        Regex::new(r#"view=move \|(?P<param>[A-Za-z_][A-Za-z0-9_]*(?::[^|]+)?)\| \{"#)
            .expect("valid regex")
            .replace_all(content, "children=move |$param| {")
            .to_string();

    updated = dedupe_consecutive_derive_clone(&updated);

    updated = Regex::new(r#"Box<dyn Fn\(\)>"#)
        .expect("valid regex")
        .replace_all(&updated, "fn(MouseEvent)")
        .to_string();
    updated = Regex::new(r#"Box::new\(\|\| \{\}\)"#)
        .expect("valid regex")
        .replace_all(&updated, "|_| {}")
        .to_string();
    updated = Regex::new(r#"on:click=(?P<handler>[A-Za-z_][A-Za-z0-9_\.]*)\.clone\(\)"#)
        .expect("valid regex")
        .replace_all(&updated, "on:click=$handler")
        .to_string();

    if updated.contains("fn(MouseEvent)") && !updated.contains("use leptos::ev::MouseEvent;") {
        updated = format!("use leptos::ev::MouseEvent;\n{}", updated);
    }

    updated = normalize_leptos_view_erasure(&updated);
    updated = normalize_string_prop_defaults(&updated);
    updated = normalize_string_signal_option_updates(&updated);
    updated = normalize_moved_for_item_string_closures(&updated);
    updated = normalize_if_else_view_branches(&updated);
    updated = normalize_spec_defined_variants(&updated, component_specs);
    updated = normalize_component_props_helper_names(&updated);
    updated = normalize_component_data_literals(&updated);
    updated = normalize_forwarded_option_props(&updated);
    updated = expand_component_spread_props(&updated);

    updated.replace("Â©", "©")
}

fn dedupe_consecutive_derive_clone(content: &str) -> String {
    let mut output = Vec::new();
    let mut previous_was_derive_clone = false;

    for line in content.lines() {
        let is_derive_clone = line.trim() == "#[derive(Clone)]";
        if is_derive_clone && previous_was_derive_clone {
            continue;
        }
        output.push(line);
        previous_was_derive_clone = is_derive_clone;
    }

    output.join("\n")
}

fn normalize_leptos_view_erasure(content: &str) -> String {
    content
        .replace("AnyView", "View")
        .replace(".into_any()", ".into_view()")
}

fn normalize_string_prop_defaults(content: &str) -> String {
    let default_string_re = Regex::new(
        r#"#\[prop\((?P<args>[^\]]*?\bdefault\s*=\s*"(?P<value>[^"]+)"[^\]]*)\)\](?P<tail>\s*[A-Za-z_][A-Za-z0-9_]*\s*:\s*String)"#,
    )
    .expect("valid regex");
    let default_literal_re =
        Regex::new(r#"\bdefault\s*=\s*"(?P<value>[^"]+)""#).expect("valid regex");

    default_string_re
        .replace_all(content, |caps: &regex::Captures| {
            let args = caps.name("args").map(|m| m.as_str()).unwrap_or_default();
            let tail = caps.name("tail").map(|m| m.as_str()).unwrap_or_default();
            let rewritten_args = default_literal_re
                .replace(args, r#"default = String::from("$value")"#)
                .to_string();
            let rewritten_args = if prop_args_contain_token(&rewritten_args, "into") {
                rewritten_args
            } else {
                format!("{}, into", rewritten_args.trim())
            };

            format!("#[prop({rewritten_args})]{tail}")
        })
        .to_string()
}

fn prop_args_contain_token(args: &str, token: &str) -> bool {
    args.split(',')
        .map(str::trim)
        .any(|part| part == token || part.starts_with(&format!("{token} ")))
}

fn normalize_string_signal_option_updates(content: &str) -> String {
    if !Regex::new(r#"let\s+[A-Za-z_][A-Za-z0-9_]*\s*=\s*vec!\[\s*""#)
        .expect("valid regex")
        .is_match(content)
    {
        return content.to_string();
    }

    Regex::new(
        r#"(?P<prefix>\bset_[A-Za-z_][A-Za-z0-9_]*\.set\()\s*(?P<value>[A-Za-z_][A-Za-z0-9_]*)\.clone\(\)\s*\)"#,
    )
    .expect("valid regex")
    .replace_all(content, "$prefix$value.to_string())")
    .to_string()
}

fn normalize_moved_for_item_string_closures(content: &str) -> String {
    let is_selected_re = Regex::new(
        r#"(?m)^(?P<indent>\s*)let\s+is_selected\s*=\s*move\s*\|\|\s*(?P<left>[^;\n]+?)\s*==\s*(?P<value>[A-Za-z_][A-Za-z0-9_]*)\s*;"#,
    )
    .expect("valid regex");

    let mut updated = is_selected_re
        .replace_all(content, |caps: &regex::Captures| {
            let indent = caps.name("indent").map(|m| m.as_str()).unwrap_or_default();
            let left = caps.name("left").map(|m| m.as_str()).unwrap_or_default();
            let value = caps.name("value").map(|m| m.as_str()).unwrap_or_default();
            format!(
                "{indent}let selected_value = {value}.clone();\n{indent}let click_value = {value}.clone();\n{indent}let is_selected = move || {left} == selected_value;"
            )
        })
        .to_string();

    if updated.contains("let click_value =") {
        let set_option_re = Regex::new(
            r#"(?P<prefix>\bset_[A-Za-z_][A-Za-z0-9_]*\.set\()\s*option\.(?:clone\(\)|to_string\(\))\s*\)"#,
        )
        .expect("valid regex");
        updated = set_option_re
            .replace_all(&updated, "${prefix}click_value.clone())")
            .to_string();
    }

    updated
}

fn normalize_if_else_view_branches(content: &str) -> String {
    let mut output = String::with_capacity(content.len());
    let mut cursor = 0;

    while let Some(relative_start) = content[cursor..].find("{if ") {
        let start = cursor + relative_start;
        let Some(normalized) = normalize_if_else_view_expression(content, start) else {
            output.push_str(&content[cursor..=start]);
            cursor = start + 1;
            continue;
        };

        output.push_str(&content[cursor..start]);
        output.push_str(&normalized.replacement);
        cursor = normalized.end;
    }

    output.push_str(&content[cursor..]);
    output
}

struct NormalizedIfElseView {
    end: usize,
    replacement: String,
}

fn normalize_if_else_view_expression(content: &str, start: usize) -> Option<NormalizedIfElseView> {
    let then_open = content[start + 1..].find('{')? + start + 1;
    let then_close = find_matching_brace(content, then_open)?;
    let after_then = &content[then_close + 1..];
    let else_offset = after_then.find("else")?;
    if !after_then[..else_offset].trim().is_empty() {
        return None;
    }
    let else_start = then_close + 1 + else_offset;
    let else_open = content[else_start..].find('{')? + else_start;
    if !content[else_start + "else".len()..else_open]
        .trim()
        .is_empty()
    {
        return None;
    }
    let else_close = find_matching_brace(content, else_open)?;
    let after_else = &content[else_close + 1..];
    let outer_close_offset = after_else.find('}')?;
    if !after_else[..outer_close_offset].trim().is_empty() {
        return None;
    }
    let outer_close = else_close + 1 + outer_close_offset;

    let then_body = &content[then_open + 1..then_close];
    let else_body = &content[else_open + 1..else_close];
    let normalized_then = normalize_view_branch_to_into_view(then_body)?;
    let normalized_else = normalize_view_branch_to_into_view(else_body)?;

    let mut replacement = String::new();
    replacement.push_str(&content[start..then_open + 1]);
    replacement.push_str(&normalized_then);
    replacement.push('}');
    replacement.push_str(&content[then_close + 1..else_open + 1]);
    replacement.push_str(&normalized_else);
    replacement.push('}');
    replacement.push_str(&content[else_close + 1..outer_close + 1]);

    Some(NormalizedIfElseView {
        end: outer_close + 1,
        replacement,
    })
}

fn normalize_view_branch_to_into_view(branch: &str) -> Option<String> {
    let leading_len = branch.len() - branch.trim_start().len();
    let trailing_len = branch.len() - branch.trim_end().len();
    let leading = &branch[..leading_len];
    let trailing_start = branch.len() - trailing_len;
    let trailing = &branch[trailing_start..];
    let trimmed = branch.trim();

    if !trimmed.starts_with("view!") {
        return None;
    }
    if trimmed.ends_with(".into_view()") {
        return Some(branch.to_string());
    }

    Some(format!("{leading}{trimmed}.into_view(){trailing}"))
}

fn find_matching_brace(content: &str, open: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    if bytes.get(open) != Some(&b'{') {
        return None;
    }

    let mut depth = 0usize;
    for (index, byte) in bytes.iter().enumerate().skip(open) {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }

    None
}

fn normalize_spec_defined_variants(
    content: &str,
    component_specs: &[ComponentSpecContract],
) -> String {
    let mut updated = content.to_string();

    for spec in component_specs
        .iter()
        .filter(|spec| !spec.variant_values.is_empty())
    {
        updated = ensure_variant_enum_definition(&updated, spec);
        updated = rewrite_component_variant_signature(&updated, spec);
        updated = rewrite_helper_variant_types(&updated, spec);
        updated = rewrite_component_variant_callsites(&updated, spec);
        updated = rewrite_helper_variant_literals(&updated, spec);
        updated = rewrite_component_variant_logic(&updated, spec);
    }

    updated
}

fn ensure_variant_enum_definition(content: &str, spec: &ComponentSpecContract) -> String {
    if content.contains(&format!("enum {}", spec.enum_name)) {
        return content.to_string();
    }

    let variants = spec
        .rust_variants
        .iter()
        .map(|variant| format!("    {},", variant))
        .collect::<Vec<_>>()
        .join("\n");
    let enum_block = format!(
        "#[derive(Clone, Copy)]\npub enum {} {{\n{}\n}}\n\n",
        spec.enum_name, variants
    );

    let component_marker = format!("#[component]\npub fn {}(", spec.name);
    let alt_component_marker = format!("#[component]\nfn {}(", spec.name);
    let fn_marker = format!("pub fn {}(", spec.name);
    let alt_fn_marker = format!("fn {}(", spec.name);
    if let Some(index) = content
        .find(&component_marker)
        .or_else(|| content.find(&alt_component_marker))
        .or_else(|| content.find(&fn_marker))
        .or_else(|| content.find(&alt_fn_marker))
    {
        let mut output = String::with_capacity(content.len() + enum_block.len());
        output.push_str(&content[..index]);
        output.push_str(&enum_block);
        output.push_str(&content[index..]);
        output
    } else if let Some(index) = content.find("\n#[component]") {
        let mut output = String::with_capacity(content.len() + enum_block.len());
        output.push_str(&content[..index + 1]);
        output.push_str(&enum_block);
        output.push_str(&content[index + 1..]);
        output
    } else {
        format!("{}\n\n{}", enum_block.trim_end(), content)
    }
}

fn rewrite_component_variant_signature(content: &str, spec: &ComponentSpecContract) -> String {
    let fn_re = Regex::new(&format!(
        r"(?s)((?:#\[component\]\s*)?(?:pub\s+)?fn\s+{}\s*\()(?P<props>.*?)(\)\s*->)",
        regex::escape(&spec.name)
    ))
    .expect("valid regex");

    fn_re
        .replace_all(content, |caps: &regex::Captures| {
            let prefix = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
            let suffix = caps.get(3).map(|m| m.as_str()).unwrap_or_default();
            let props = caps.name("props").map(|m| m.as_str()).unwrap_or_default();
            let rewritten = rewrite_variant_props_block(props, spec);
            format!("{prefix}{rewritten}{suffix}")
        })
        .to_string()
}

fn rewrite_variant_props_block(props: &str, spec: &ComponentSpecContract) -> String {
    let lines = props.lines().collect::<Vec<_>>();
    let mut rewritten = Vec::new();
    let mut pending_attr: Option<String> = None;

    for line in lines {
        let trimmed = line.trim();
        if trimmed.starts_with("#[prop(") && !trimmed.contains(':') {
            pending_attr = Some(line.to_string());
            continue;
        }

        if trimmed.contains("variant") && trimmed.contains("String") {
            let indent = line
                .chars()
                .take_while(|c| c.is_whitespace())
                .collect::<String>();
            let inline_attr = if line.contains("#[prop(") {
                Some(line.to_string())
            } else {
                None
            };
            if let Some(attr) = inline_attr.or_else(|| pending_attr.take()) {
                if let Some(default_variant) = extract_string_default_variant(&attr, spec) {
                    rewritten.push(format!(
                        "{}#[prop(default = {}::{})]",
                        indent, spec.enum_name, default_variant
                    ));
                }
            }
            rewritten.push(format!("{}variant: {},", indent, spec.enum_name));
            continue;
        }

        if let Some(attr) = pending_attr.take() {
            rewritten.push(attr);
        }
        rewritten.push(line.to_string());
    }

    if let Some(attr) = pending_attr {
        rewritten.push(attr);
    }

    if rewritten.is_empty() {
        props.to_string()
    } else {
        let joined = rewritten.join("\n");
        if props.ends_with('\n') && !joined.ends_with('\n') {
            format!("{joined}\n")
        } else {
            joined
        }
    }
}

fn extract_string_default_variant(attr_line: &str, spec: &ComponentSpecContract) -> Option<String> {
    for (raw, rust) in spec.variant_values.iter().zip(spec.rust_variants.iter()) {
        let direct = format!("\"{}\"", raw);
        let string_from = format!("String::from(\"{}\")", raw);
        let to_string = format!("\"{}\".to_string()", raw);
        if attr_line.contains(&direct)
            || attr_line.contains(&string_from)
            || attr_line.contains(&to_string)
        {
            return Some(rust.clone());
        }
    }
    spec.default_variant.clone()
}

fn rewrite_helper_variant_types(content: &str, spec: &ComponentSpecContract) -> String {
    let helper_names = helper_struct_names_for_component(content, &spec.name);
    let mut updated = content.to_string();

    for helper_name in helper_names {
        let struct_re = Regex::new(&format!(
            r"(?s)((?:pub\s+)?struct\s+{}\s*\{{)(?P<body>.*?)(\n\}})",
            regex::escape(&helper_name)
        ))
        .expect("valid regex");
        updated = struct_re
            .replace_all(&updated, |caps: &regex::Captures| {
                let prefix = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
                let suffix = caps.get(3).map(|m| m.as_str()).unwrap_or_default();
                let body = caps.name("body").map(|m| m.as_str()).unwrap_or_default();
                let rewritten = Regex::new(r"(?m)^(\s*variant\s*:\s*)String(\s*,\s*)$")
                    .expect("valid regex")
                    .replace_all(body, format!("$1{}$2", spec.enum_name))
                    .to_string();
                format!("{prefix}{rewritten}{suffix}")
            })
            .to_string();
    }

    updated
}

fn helper_struct_names_for_component(content: &str, component_name: &str) -> Vec<String> {
    let helper_re = Regex::new(&format!(
        r"(?m)^\s*(?:pub\s+)?struct\s+({}(?:Data|Model|Item|Helper))\b",
        regex::escape(component_name)
    ))
    .expect("valid regex");

    helper_re
        .captures_iter(content)
        .filter_map(|caps| caps.get(1).map(|m| m.as_str().to_string()))
        .collect()
}

fn rewrite_component_variant_callsites(content: &str, spec: &ComponentSpecContract) -> String {
    let mut updated = content.to_string();

    for (raw, rust) in spec.variant_values.iter().zip(spec.rust_variants.iter()) {
        let pattern = Regex::new(&format!(
            r#"(<{}\b[^>]*\bvariant\s*=\s*)"{}""#,
            regex::escape(&spec.name),
            regex::escape(raw)
        ))
        .expect("valid regex");
        updated = pattern
            .replace_all(&updated, |caps: &regex::Captures| {
                let prefix = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
                format!("{}{}::{}", prefix, spec.enum_name, rust)
            })
            .to_string();
    }

    updated
}

fn rewrite_helper_variant_literals(content: &str, spec: &ComponentSpecContract) -> String {
    let mut updated = content.to_string();

    for helper_name in helper_struct_names_for_component(content, &spec.name) {
        for pattern in [
            format!(
                r"(?s)(=\s*{}\s*\{{)(?P<body>.*?)(\n\s*\}})",
                regex::escape(&helper_name)
            ),
            format!(
                r"(?s)(Some\(\s*{}\s*\{{)(?P<body>.*?)(\n\s*\}}\s*\))",
                regex::escape(&helper_name)
            ),
            format!(
                r"(?s)(vec!\[\s*{}\s*\{{)(?P<body>.*?)(\n\s*\}})",
                regex::escape(&helper_name)
            ),
        ] {
            let literal_re = Regex::new(&pattern).expect("valid regex");
            updated = literal_re
                .replace_all(&updated, |caps: &regex::Captures| {
                    let prefix = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
                    let suffix = caps.get(3).map(|m| m.as_str()).unwrap_or_default();
                    let body = caps.name("body").map(|m| m.as_str()).unwrap_or_default();
                    let rewritten = rewrite_variant_literal_values(body, spec);
                    format!("{prefix}{rewritten}{suffix}")
                })
                .to_string();
        }
    }

    updated
}

fn rewrite_variant_literal_values(body: &str, spec: &ComponentSpecContract) -> String {
    let mut updated = body.to_string();

    for (raw, rust) in spec.variant_values.iter().zip(spec.rust_variants.iter()) {
        let replacements = [
            format!(r#"variant:\s*"{}"\.to_string\(\)"#, regex::escape(raw)),
            format!(r#"variant:\s*String::from\("{}"\)"#, regex::escape(raw)),
            format!(r#"variant:\s*"{}""#, regex::escape(raw)),
        ];

        for pattern in replacements {
            updated = Regex::new(&pattern)
                .expect("valid regex")
                .replace_all(&updated, format!("variant: {}::{}", spec.enum_name, rust))
                .to_string();
        }
    }

    updated
}

fn rewrite_component_variant_logic(content: &str, spec: &ComponentSpecContract) -> String {
    let fn_re = Regex::new(&format!(
        r"(?s)((?:#\[component\]\s*)?(?:pub\s+)?fn\s+{}\s*\(.*?\)\s*->\s*impl\s+IntoView\s*\{{)(?P<body>.*?)(\n\}})",
        regex::escape(&spec.name)
    ))
    .expect("valid regex");

    fn_re
        .replace_all(content, |caps: &regex::Captures| {
            let prefix = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
            let suffix = caps.get(3).map(|m| m.as_str()).unwrap_or_default();
            let body = caps.name("body").map(|m| m.as_str()).unwrap_or_default();
            let mut rewritten = body.replace("match variant.as_str()", "match variant");
            rewritten = rewritten.replace("match variant.as_ref()", "match variant");
            for (raw, rust) in spec.variant_values.iter().zip(spec.rust_variants.iter()) {
                rewritten = Regex::new(&format!(r#""{}"\s*=>"#, regex::escape(raw)))
                    .expect("valid regex")
                    .replace_all(&rewritten, format!("{}::{} =>", spec.enum_name, rust))
                    .to_string();
                rewritten = Regex::new(&format!(r#"variant\s*==\s*"{}""#, regex::escape(raw)))
                    .expect("valid regex")
                    .replace_all(
                        &rewritten,
                        format!("variant == {}::{}", spec.enum_name, rust),
                    )
                    .to_string();
            }
            format!("{prefix}{rewritten}{suffix}")
        })
        .to_string()
}

fn expand_component_spread_props(content: &str) -> String {
    let component_props = parse_component_prop_specs(content);
    let spread_re = Regex::new(
        r#"<(?P<component>[A-Z][A-Za-z0-9_]*)\s+\.\.(?P<value>[A-Za-z_][A-Za-z0-9_]*)\s*/>"#,
    )
    .expect("valid regex");

    spread_re
        .replace_all(content, |caps: &regex::Captures| {
            let component = caps
                .name("component")
                .map(|m| m.as_str())
                .unwrap_or_default();
            let value = caps.name("value").map(|m| m.as_str()).unwrap_or_default();
            let Some(props) = component_props.get(component) else {
                return caps
                    .get(0)
                    .map(|m| m.as_str())
                    .unwrap_or_default()
                    .to_string();
            };
            if props.is_empty() {
                return caps
                    .get(0)
                    .map(|m| m.as_str())
                    .unwrap_or_default()
                    .to_string();
            }

            let mapped = props
                .iter()
                .map(|prop| {
                    if prop_type_needs_clone(&prop.ty) {
                        format!("{}={}.{}.clone()", prop.name, value, prop.name)
                    } else {
                        format!("{}={}.{}", prop.name, value, prop.name)
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            format!("<{} {} />", component, mapped)
        })
        .to_string()
}

fn normalize_component_props_helper_names(content: &str) -> String {
    let component_names = extract_component_names(content);
    let mut normalized = content.to_string();

    for component_name in component_names {
        let props_name = format!("{}Props", component_name);
        let struct_re = Regex::new(&format!(
            r"(?m)^\s*(?:pub\s+)?struct\s+{}\b",
            regex::escape(&props_name)
        ))
        .expect("valid regex");
        if !struct_re.is_match(&normalized) {
            continue;
        }

        let replacement_name =
            choose_component_helper_name(&normalized, &component_name, &props_name);
        normalized = Regex::new(&format!(r"\b{}\b", regex::escape(&props_name)))
            .expect("valid regex")
            .replace_all(&normalized, replacement_name.as_str())
            .to_string();
    }

    normalized
}

fn choose_component_helper_name(content: &str, component_name: &str, current_name: &str) -> String {
    for suffix in ["Data", "Model", "Item"] {
        let candidate = format!("{}{}", component_name, suffix);
        if candidate == current_name {
            return candidate;
        }
        let candidate_re =
            Regex::new(&format!(r"\b{}\b", regex::escape(&candidate))).expect("valid regex");
        if !candidate_re.is_match(content) {
            return candidate;
        }
    }

    format!("{}Helper", component_name)
}

fn normalize_component_data_literals(content: &str) -> String {
    let component_props = parse_component_prop_specs(content);
    let mut normalized = content.to_string();
    let mut helper_blocks = Vec::new();

    for (component, props) in component_props {
        if props.is_empty() {
            continue;
        }

        let helper_name = format!("{}Data", component);
        let literal_marker = format!("{} {{", component);
        let type_patterns = [
            format!("Option<{}>", component),
            format!("Vec<{}>", component),
            format!("Option<Vec<{}>>", component),
            format!(": {}>", component),
            format!(": {},", component),
            format!(": {})", component),
        ];

        let needs_helper = normalized.contains(&literal_marker)
            || type_patterns
                .iter()
                .any(|pattern| normalized.contains(pattern));
        if !needs_helper {
            continue;
        }

        normalized = Regex::new(&format!(r"\b{}\s*\{{", regex::escape(&component)))
            .expect("valid regex")
            .replace_all(&normalized, format!("{} {{", helper_name))
            .to_string();
        normalized = normalized.replace(
            &format!("Option<Vec<{}>>", component),
            &format!("Option<Vec<{}>>", helper_name),
        );
        normalized = normalized.replace(
            &format!("Option<{}>", component),
            &format!("Option<{}>", helper_name),
        );
        normalized = normalized.replace(
            &format!("Vec<{}>", component),
            &format!("Vec<{}>", helper_name),
        );
        normalized = Regex::new(&format!(r"(:\s+){}\b", regex::escape(&component)))
            .expect("valid regex")
            .replace_all(&normalized, format!("$1{}", helper_name))
            .to_string();

        helper_blocks.push(render_component_data_helper(
            &component,
            &helper_name,
            &props,
        ));
    }

    if helper_blocks.is_empty() {
        return normalized;
    }

    let insertion_anchor = "\n#[component]\npub fn App()";
    if let Some(index) = normalized.find(insertion_anchor) {
        let mut output = String::with_capacity(normalized.len() + helper_blocks.len() * 256);
        output.push_str(&normalized[..index]);
        output.push_str(&helper_blocks.join("\n\n"));
        output.push_str("\n\n");
        output.push_str(&normalized[index..]);
        output
    } else {
        format!("{}\n\n{}", normalized, helper_blocks.join("\n\n"))
    }
}

fn render_component_data_helper(
    component_name: &str,
    helper_name: &str,
    props: &[ComponentPropSpec],
) -> String {
    let fields = props
        .iter()
        .map(|prop| format!("    pub {}: {},", prop.name, prop.ty))
        .collect::<Vec<_>>()
        .join("\n");
    let prop_mappings = props
        .iter()
        .map(|prop| format!("{}=self.{}", prop.name, prop.name))
        .collect::<Vec<_>>()
        .join(" ");

    format!(
        "#[derive(Clone)]\npub struct {helper_name} {{\n{fields}\n}}\n\nimpl IntoView for {helper_name} {{\n    fn into_view(self) -> View {{\n        view! {{ <{component_name} {prop_mappings} /> }}.into_view()\n    }}\n}}",
    )
}

pub(crate) fn render_brand_variant_contract(
    component_specs: &[ComponentSpecContract],
) -> Option<String> {
    let relevant = component_specs
        .iter()
        .filter(|spec| !spec.variant_values.is_empty())
        .collect::<Vec<_>>();
    if relevant.is_empty() {
        return None;
    }

    let mut sections = Vec::new();
    for spec in relevant {
        let default_line = spec
            .default_variant
            .as_ref()
            .map(|value| format!("- default enum variant: `{}::{}`", spec.enum_name, value))
            .unwrap_or_else(|| {
                "- default enum variant: none; require explicit variant selection at call sites"
                    .to_string()
            });
        let members = spec
            .variant_values
            .iter()
            .zip(spec.rust_variants.iter())
            .map(|(raw, rust)| format!("  - `{}` -> `{}::{}`", raw, spec.enum_name, rust))
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!(
            "Component `{}` variant contract:\n- prop type: `{}`\n- do not use `variant: String`\n- enum members:\n{}\n{}\n- component call sites must use enum values like `variant={}::{}`\n- helper data that forwards this variant must also store `{}` values, not `String`",
            spec.name,
            spec.enum_name,
            members,
            default_line,
            spec.enum_name,
            spec.default_variant.as_deref().unwrap_or_else(|| spec.rust_variants.first().map(|s| s.as_str()).unwrap_or("Default")),
            spec.enum_name,
        ));
    }

    Some(format!(
        "Treat the following variant contracts as authoritative for this run.\nDo not infer alternate enum names, string-based variant props, or raw string call sites.\n\n{}",
        sections.join("\n\n")
    ))
}

pub(crate) fn render_component_implementation_contract(
    component_specs: &[ComponentSpecContract],
) -> Option<String> {
    if component_specs.is_empty() {
        return None;
    }

    let rendered = component_specs
        .iter()
        .map(|spec| {
            let payload = json!({
                "component": spec.name,
                "props": spec.props.iter().map(|prop| {
                    json!({
                        "name": prop.name,
                        "required": prop.required,
                        "shape": prop.shape,
                        "type": prop.ty,
                        "item_contract": prop.item_contract,
                        "object_contract": prop.object_contract,
                        "allowed": prop.allowed,
                    })
                }).collect::<Vec<_>>(),
                "object_contracts": spec.object_contracts.iter().map(|contract| {
                    json!({
                        "name": contract.name,
                        "fields": contract.fields.iter().map(|field| {
                            json!({
                                "name": field.name,
                                "required": field.required,
                                "shape": field.shape,
                                "type": field.ty,
                                "item_contract": field.item_contract,
                                "object_contract": field.object_contract,
                                "allowed": field.allowed,
                            })
                        }).collect::<Vec<_>>(),
                    })
                }).collect::<Vec<_>>(),
                "collection_contracts": spec.collection_contracts.iter().map(|contract| {
                    json!({
                        "name": contract.name,
                        "item_contract": contract.item_contract,
                        "behavior": contract.behavior,
                    })
                }).collect::<Vec<_>>(),
                "interaction_contracts": spec.interaction_contracts.iter().map(|contract| {
                    json!({
                        "target": contract.target,
                        "kind": contract.kind,
                    })
                }).collect::<Vec<_>>(),
                "composition_contracts": spec.composition_contracts.iter().map(|contract| {
                    json!({
                        "name": contract.name,
                        "usage": contract.usage,
                    })
                }).collect::<Vec<_>>(),
                "brand_constraints": spec.brand_constraints.iter().map(|constraint| {
                    json!({
                        "topic": constraint.topic,
                        "guidance": constraint.guidance,
                    })
                }).collect::<Vec<_>>(),
            });
            format!(
                "Component `{}` implementation contract:\n```json\n{}\n```",
                spec.name,
                serde_json::to_string_pretty(&payload).expect("component contract json")
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    Some(format!(
        "Treat the following CLI-extracted component implementation contracts as authoritative for this run.\nUse them for exact prop shapes, nested object fields, repeated-item schemas, interaction kinds, composition usage, and concrete brand-informed implementation constraints.\n\n{}",
        rendered
    ))
}

fn normalize_forwarded_option_props(content: &str) -> String {
    let inline_prop_attr_re = Regex::new(r"#\[prop\([^\]]+\)\]\s*").expect("valid regex");
    let optional_attr_line_re =
        Regex::new(r#"^#\[prop\((?:optional|optional,\s*into|into,\s*optional)\)\]$"#)
            .expect("valid regex");
    let optional_attr_inline_re =
        Regex::new(r#"#\[prop\((?:optional|optional,\s*into|into,\s*optional)\)\]\s*"#)
            .expect("valid regex");

    let mut output = Vec::new();
    let mut pending_optional_attr_line: Option<String> = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if optional_attr_line_re.is_match(trimmed) {
            pending_optional_attr_line = Some(line.to_string());
            continue;
        }

        let parsed = parse_option_prop_candidate(trimmed, &inline_prop_attr_re);
        let should_strip = parsed
            .as_ref()
            .map(|(name, _)| prop_receives_explicit_option_value(content, name))
            .unwrap_or(false);

        if should_strip {
            if optional_attr_inline_re.is_match(line) {
                output.push(inline_prop_attr_re.replace_all(line, "").to_string());
                pending_optional_attr_line = None;
                continue;
            }

            if pending_optional_attr_line.take().is_some() {
                output.push(line.to_string());
                continue;
            }
        }

        if let Some(attr_line) = pending_optional_attr_line.take() {
            output.push(attr_line);
        }
        output.push(line.to_string());
    }

    if let Some(attr_line) = pending_optional_attr_line {
        output.push(attr_line);
    }

    output.join("\n")
}

fn prop_receives_explicit_option_value(content: &str, prop_name: &str) -> bool {
    if prop_name.is_empty() {
        return false;
    }

    let escaped = regex::escape(prop_name);
    let patterns = [
        format!(r"\b{}\s*=\s*None\b", escaped),
        format!(r"\b{}\s*=\s*Some\(", escaped),
        format!(
            r"\b{}\s*=\s*[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)+\s*(?:\.clone\(\))?",
            escaped
        ),
    ];

    patterns
        .into_iter()
        .any(|pattern| Regex::new(&pattern).expect("valid regex").is_match(content))
}

fn extract_prop_name(candidate: &str) -> &str {
    candidate
        .split_once(':')
        .map(|(name, _)| name.trim())
        .unwrap_or_default()
}

fn parse_option_prop_candidate(
    trimmed: &str,
    inline_prop_attr_re: &Regex,
) -> Option<(String, String)> {
    let sanitized = inline_prop_attr_re.replace_all(trimmed, "");
    let mut candidate = sanitized.as_ref().trim();
    if candidate.is_empty() {
        return None;
    }

    if candidate.starts_with("pub fn ") || candidate.starts_with("fn ") {
        if let Some((_, rhs)) = candidate.split_once('(') {
            candidate = rhs.trim();
        }
    }
    if let Some((lhs, _)) = candidate.split_once(')') {
        candidate = lhs.trim();
    }

    let (name, ty) = candidate.split_once(':')?;
    let name = name.trim();
    let ty = ty.trim().trim_end_matches(',').trim();
    if name.is_empty() || !ty.starts_with("Option<") {
        return None;
    }

    Some((name.to_string(), ty.to_string()))
}

fn parse_component_prop_specs(content: &str) -> HashMap<String, Vec<ComponentPropSpec>> {
    let mut components = HashMap::new();
    let mut previous_was_component_attr = false;
    let mut in_signature = false;
    let mut current_component = String::new();
    let mut current_props = Vec::new();
    let inline_prop_attr_re = Regex::new(r"#\[prop\([^\]]+\)\]\s*").expect("valid regex");

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "#[component]" {
            previous_was_component_attr = true;
            continue;
        }

        if previous_was_component_attr {
            if trimmed.starts_with("pub fn ") || trimmed.starts_with("fn ") {
                previous_was_component_attr = false;
                in_signature = true;
                current_props = Vec::new();
                current_component = trimmed
                    .strip_prefix("pub fn ")
                    .or_else(|| trimmed.strip_prefix("fn "))
                    .and_then(|rest| {
                        rest.split_once('(')
                            .map(|(name, _)| name.trim().to_string())
                    })
                    .unwrap_or_default();
            } else if !trimmed.is_empty() {
                previous_was_component_attr = false;
            }
        }

        if !in_signature {
            continue;
        }

        let sanitized = inline_prop_attr_re.replace_all(trimmed, "");
        let mut candidate = sanitized.as_ref().trim();
        if trimmed.starts_with("pub fn ") || trimmed.starts_with("fn ") {
            if let Some((_, rhs)) = candidate.split_once('(') {
                candidate = rhs.trim();
            }
        }
        if let Some((lhs, _)) = candidate.split_once(')') {
            candidate = lhs.trim();
        }
        if let Some((name, ty)) = candidate.split_once(':') {
            let name = name.trim();
            let ty = ty.trim().trim_end_matches(',').trim();
            if !name.is_empty() && !ty.is_empty() {
                current_props.push(ComponentPropSpec {
                    name: name.to_string(),
                    ty: ty.to_string(),
                });
            }
        }

        if sanitized.contains(')') {
            in_signature = false;
            if !current_component.is_empty() {
                components.insert(current_component.clone(), current_props.clone());
            }
            current_component.clear();
            current_props.clear();
        }
    }

    components
}

fn prop_type_needs_clone(ty: &str) -> bool {
    !matches!(
        ty,
        "bool"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "usize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "isize"
            | "f32"
            | "f64"
            | "&str"
    )
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

fn component_module_specs(generated_files: &[GeneratedOutputFile]) -> Vec<ComponentModuleSpec> {
    let mut specs = generated_files
        .iter()
        .filter_map(|file| component_module_spec(&file.path))
        .collect::<Vec<_>>();
    specs.sort_by(|left, right| left.path.cmp(&right.path));
    specs
}

fn component_module_spec(path: &Path) -> Option<ComponentModuleSpec> {
    let parent = path.parent()?;
    if parent != Path::new("src/components") {
        return None;
    }
    if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
        return None;
    }

    let module_name = path.file_stem()?.to_str()?;
    if module_name == "mod" {
        return None;
    }

    Some(ComponentModuleSpec {
        path: path.to_path_buf(),
        module_name: module_name.to_string(),
        component_name: pascal_case_identifier(module_name),
    })
}

fn pascal_case_identifier(value: &str) -> String {
    let mut output = String::new();
    for segment in value.split(|ch: char| !ch.is_ascii_alphanumeric()) {
        if segment.is_empty() {
            continue;
        }

        let mut chars = segment.chars();
        if let Some(first) = chars.next() {
            output.extend(first.to_uppercase());
            output.push_str(chars.as_str());
        }
    }
    output
}

fn validate_component_module_wiring(
    context_name: &str,
    generated_files: &[GeneratedOutputFile],
    lib_rs: &str,
) -> Result<()> {
    let components = component_module_specs(generated_files);
    if components.is_empty() {
        return Ok(());
    }

    if !lib_rs.contains("mod components;") {
        anyhow::bail!(
            "Generated brand implementation for '{}' emits src/components/*.rs files but src/lib.rs does not declare 'pub mod components;'",
            context_name
        );
    }

    let components_mod = find_file(generated_files, "src/components/mod.rs").map_err(|_| {
        anyhow::anyhow!(
            "Generated brand implementation for '{}' emits src/components/*.rs files but is missing src/components/mod.rs",
            context_name
        )
    })?;

    for component in components {
        if !has_module_declaration(&components_mod.content, &component.module_name) {
            anyhow::bail!(
                "Generated brand implementation for '{}' is missing module declaration 'mod {};' in src/components/mod.rs for {}",
                context_name,
                component.module_name,
                component.path.display()
            );
        }

        if !has_component_reexport(
            &components_mod.content,
            &component.module_name,
            &component.component_name,
        ) {
            anyhow::bail!(
                "Generated brand implementation for '{}' is missing re-export 'pub use {}::{};' in src/components/mod.rs for {}",
                context_name,
                component.module_name,
                component.component_name,
                component.path.display()
            );
        }
    }

    Ok(())
}

fn has_module_declaration(content: &str, module_name: &str) -> bool {
    let private_decl = format!("mod {};", module_name);
    let public_decl = format!("pub mod {};", module_name);
    content
        .lines()
        .map(str::trim)
        .any(|line| line == private_decl || line == public_decl)
}

fn has_component_reexport(content: &str, module_name: &str, component_name: &str) -> bool {
    let direct_reexport = format!("pub use {}::{};", module_name, component_name);
    let self_reexport = format!("pub use self::{}::{};", module_name, component_name);
    content
        .lines()
        .map(str::trim)
        .any(|line| line == direct_reexport || line == self_reexport)
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
    let component_specs = load_component_spec_contracts()?;
    validate_app_rs_with_component_specs(context_name, content, &component_specs)
}

fn validate_app_rs_with_component_specs(
    context_name: &str,
    content: &str,
    component_specs: &[ComponentSpecContract],
) -> Result<()> {
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
    if !content.contains("include_str!(\"../style/app.css\")") {
        anyhow::bail!(
            "Generated brand implementation for '{}' does not permanently reference style/app.css from src/app.rs",
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

    let forbidden_markers = [
        (
            "<use ",
            "raw SVG <use> tag; use <use_ ... /> in Leptos view!",
        ),
        (
            "..Default::default()",
            "Rust struct update syntax in generated UI data; prefer explicit field mapping",
        ),
    ];
    for (marker, description) in forbidden_markers {
        if content.contains(marker) {
            anyhow::bail!(
                "Generated brand implementation for '{}' uses invalid Leptos syntax in src/app.rs: {}",
                context_name,
                description
            );
        }
    }

    validate_for_component_syntax(context_name, content)?;
    validate_forbidden_leptos_class_syntax(context_name, content, "src/app.rs")?;
    validate_component_spread_syntax(context_name, content)?;
    validate_component_name_collisions(context_name, content)?;
    validate_component_struct_literal_usage(context_name, content)?;
    validate_non_cloneable_callback_patterns(context_name, content)?;
    validate_leptos_view_erasure_patterns(context_name, content)?;
    validate_component_prop_annotations(context_name, content)?;
    validate_string_prop_default_literals(context_name, content)?;
    validate_string_signal_option_updates(context_name, content)?;
    validate_optional_option_forwarding_patterns(context_name, content)?;
    validate_spec_defined_variant_contracts(context_name, content, component_specs)?;
    validate_generated_text_encoding(context_name, content)?;

    Ok(())
}

fn validate_for_component_syntax(context_name: &str, content: &str) -> Result<()> {
    let invalid_for_re = Regex::new(r#"(?s)<For\b[^>]*\bview\s*=\s*move \|"#).unwrap();
    if invalid_for_re.is_match(content) {
        anyhow::bail!(
            "Generated brand implementation for '{}' uses Leptos-incompatible <For /> syntax in src/app.rs: for the scaffold's Leptos 0.6 target, use children=move |item| view! {{ ... }} instead of view=move |item| {{ ... }}",
            context_name
        );
    }
    Ok(())
}

fn validate_forbidden_leptos_class_syntax(
    context_name: &str,
    content: &str,
    file_name: &str,
) -> Result<()> {
    let forbidden_markers = [
        ("class=[", "`class=[...]`"),
        ("class:", "`class:...`"),
        ("classes!(", "`classes!(...)`"),
        ("class=(", "`class=(...)`"),
    ];

    for (marker, label) in forbidden_markers {
        if content.contains(marker) {
            anyhow::bail!(
                "Generated brand implementation for '{}' uses forbidden Leptos class syntax in {}: {}. For the brand scaffold's Leptos 0.6 target, precompute class strings in Rust locals and bind them with plain `class=...`.",
                context_name,
                file_name,
                label
            );
        }
    }

    Ok(())
}

fn validate_leptos_view_erasure_patterns(context_name: &str, content: &str) -> Result<()> {
    if content.contains("AnyView") {
        anyhow::bail!(
            "Generated brand implementation for '{}' uses unsupported `AnyView` in src/app.rs; for the scaffold's Leptos 0.6 target, use `View` and `.into_view()`",
            context_name
        );
    }

    if content.contains(".into_any()") {
        anyhow::bail!(
            "Generated brand implementation for '{}' uses unsupported `.into_any()` in src/app.rs; for the scaffold's Leptos 0.6 target, use `.into_view()`",
            context_name
        );
    }

    Ok(())
}

fn validate_component_name_collisions(context_name: &str, content: &str) -> Result<()> {
    let component_names = extract_component_names(content);
    for name in component_names {
        let struct_re = Regex::new(&format!(
            r"(?m)^\s*(?:pub\s+)?struct\s+{}\b",
            regex::escape(&name)
        ))
        .unwrap();
        if struct_re.is_match(content) {
            anyhow::bail!(
                "Generated brand implementation for '{}' reuses '{}' as both a Leptos component and a Rust struct in src/app.rs; use a distinct helper type name such as '{}Data'",
                context_name,
                name,
                name
            );
        }

        let props_re = Regex::new(&format!(
            r"(?m)^\s*(?:pub\s+)?struct\s+{}Props\b",
            regex::escape(&name)
        ))
        .unwrap();
        if props_re.is_match(content) {
            anyhow::bail!(
                "Generated brand implementation for '{}' manually defines '{}Props' in src/app.rs, but the #[component] macro already generates that props type; use a helper name like '{}Data' instead",
                context_name,
                name,
                name
            );
        }
    }
    Ok(())
}

fn validate_component_spread_syntax(context_name: &str, content: &str) -> Result<()> {
    let spread_re = Regex::new(r"<[A-Z][A-Za-z0-9_]*\s+\.\.[A-Za-z_][A-Za-z0-9_]*").unwrap();
    if spread_re.is_match(content) {
        anyhow::bail!(
            "Generated brand implementation for '{}' uses JSX-style component spread props in src/app.rs; Leptos view! requires explicit prop mapping instead of <Component ..props />",
            context_name
        );
    }
    Ok(())
}

fn validate_component_struct_literal_usage(context_name: &str, content: &str) -> Result<()> {
    let component_names = extract_component_names(content);
    for name in component_names {
        let literal_marker = format!("{} {{", name);
        if content.contains(&literal_marker) {
            anyhow::bail!(
                "Generated brand implementation for '{}' treats Leptos component '{}' like a Rust struct literal in src/app.rs; instantiate a distinct helper data type such as '{}Data' and map fields into <{} ... /> explicitly",
                context_name,
                name,
                name,
                name
            );
        }
    }
    Ok(())
}

fn validate_non_cloneable_callback_patterns(context_name: &str, content: &str) -> Result<()> {
    let forbidden_patterns = [
        (
            "Box<dyn Fn()>",
            "boxed dyn Fn() callback fields are not Clone and tend to break repeated-item rendering; use static actions, enums, or a cloneable callback strategy",
        ),
        (
            "on:click=action.action.clone()",
            "cloning a boxed callback into on:click is not valid here; avoid storing non-cloneable callbacks in helper data",
        ),
    ];

    for (marker, description) in forbidden_patterns {
        if content.contains(marker) {
            anyhow::bail!(
                "Generated brand implementation for '{}' uses invalid callback pattern in src/app.rs: {}",
                context_name,
                description
            );
        }
    }

    let zero_arg_field_re =
        Regex::new(r"(?m)^\s*(?:pub\s+)?(?P<field>[A-Za-z_][A-Za-z0-9_]*)\s*:\s*fn\(\)\s*,\s*$")
            .unwrap();
    for captures in zero_arg_field_re.captures_iter(content) {
        let field = captures
            .name("field")
            .map(|m| m.as_str())
            .unwrap_or_default();
        if field.is_empty() {
            continue;
        }

        let usage_re = Regex::new(&format!(
            r"on:click\s*=\s*[A-Za-z_][A-Za-z0-9_\.]*\b{}\b",
            regex::escape(field)
        ))
        .unwrap();
        if usage_re.is_match(content) {
            anyhow::bail!(
                "Generated brand implementation for '{}' uses invalid callback pattern in src/app.rs: zero-argument stored callbacks such as '{}' cannot be wired directly into on:click; use an inline `move |_| ...` closure or `fn(MouseEvent)`",
                context_name,
                field
            );
        }
    }

    Ok(())
}

fn validate_requested_component_implementations(context_name: &str, content: &str) -> Result<()> {
    let requested_components = requested_component_names()?;
    for component_name in requested_components {
        let fn_marker = format!("fn {}(", component_name);
        if !content.contains(&fn_marker) {
            anyhow::bail!(
                "Generated brand implementation for '{}' does not define requested component '{}' in src/app.rs",
                context_name,
                component_name
            );
        }
    }
    Ok(())
}

fn requested_component_names() -> Result<Vec<String>> {
    Ok(load_component_spec_contracts()?
        .into_iter()
        .map(|spec| spec.name)
        .collect())
}

fn load_component_spec_contracts() -> Result<Vec<ComponentSpecContract>> {
    let components_dir = Path::new(SPECIFICATIONS_DIR).join("components");
    if !components_dir.exists() {
        return Ok(Vec::new());
    }

    let mut paths = fs::read_dir(&components_dir)
        .with_context(|| {
            format!(
                "Failed to read component specifications {}",
                components_dir.display()
            )
        })?
        .filter_map(|entry| entry.ok().map(|item| item.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("md"))
        .collect::<Vec<_>>();
    paths.sort();

    load_component_spec_contracts_from_paths(&paths)
}

pub(crate) fn load_component_spec_contracts_from_paths(
    paths: &[PathBuf],
) -> Result<Vec<ComponentSpecContract>> {
    let mut contracts = Vec::new();
    let mut seen = HashSet::new();

    for path in paths {
        let content = fs::read_to_string(&path).with_context(|| {
            format!("Failed to read component specification {}", path.display())
        })?;
        let Some(name) = extract_component_name_from_spec(&content)
            .or_else(|| {
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(to_pascal_case)
            })
            .filter(|name| !name.is_empty())
        else {
            continue;
        };

        let variant_values = extract_variant_values_from_spec(&content);
        let implementation_contract =
            parse_component_implementation_contract(&name, &content, &variant_values)?;
        if seen.insert(name.clone()) {
            contracts.push(build_component_spec_contract(
                name,
                variant_values,
                implementation_contract,
            ));
        }
    }

    contracts.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(contracts)
}

fn build_component_spec_contract(
    name: String,
    variant_values: Vec<String>,
    implementation_contract: ParsedImplementationContract,
) -> ComponentSpecContract {
    let enum_name = format!("{}Variant", name);
    let rust_variants = variant_values
        .iter()
        .map(|value| to_pascal_case(value))
        .collect::<Vec<_>>();
    let default_variant = variant_values
        .iter()
        .position(|value| value == "default")
        .and_then(|index| rust_variants.get(index).cloned());

    ComponentSpecContract {
        name,
        variant_values,
        enum_name,
        rust_variants,
        default_variant,
        props: implementation_contract.props,
        object_contracts: implementation_contract.object_contracts,
        collection_contracts: implementation_contract.collection_contracts,
        interaction_contracts: implementation_contract.interaction_contracts,
        composition_contracts: implementation_contract.composition_contracts,
        brand_constraints: implementation_contract.brand_constraints,
    }
}

fn extract_component_name_from_spec(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix("- **Name**:") {
            let name = name.trim();
            if !name.is_empty() {
                return Some(normalize_component_name(name));
            }
        }
        if let Some(name) = trimmed.strip_prefix("# ") {
            let name = name.split(" - ").next().unwrap_or(name).trim();
            if !name.is_empty() {
                return Some(normalize_component_name(name));
            }
        }
    }
    None
}

fn normalize_component_name(raw: &str) -> String {
    let trimmed = raw.trim();
    if is_preserved_component_identifier(trimmed) {
        return trimmed.to_string();
    }
    to_pascal_case(trimmed)
}

fn is_preserved_component_identifier(raw: &str) -> bool {
    let mut chars = raw.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_uppercase()
        && chars.clone().all(|c| c.is_ascii_alphanumeric())
        && chars.any(|c| c.is_ascii_uppercase())
}

fn to_pascal_case(raw: &str) -> String {
    let mut out = String::new();
    for token in raw.split(|c: char| !c.is_ascii_alphanumeric()) {
        if token.is_empty() {
            continue;
        }
        let mut chars = token.chars();
        if let Some(first) = chars.next() {
            out.push_str(&first.to_uppercase().collect::<String>());
            out.push_str(&chars.as_str().to_ascii_lowercase());
        }
    }
    out
}

fn extract_variant_values_from_spec(content: &str) -> Vec<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if !(trimmed.starts_with("- `variant`:") || trimmed.starts_with("- **variant**:")) {
            continue;
        }
        let Some((_, raw_values)) = trimmed.split_once(':') else {
            continue;
        };
        let values = extract_backtick_values(raw_values);
        if !values.is_empty() {
            return values;
        }
    }
    Vec::new()
}

#[derive(Debug, Default)]
struct ParsedImplementationContract {
    props: Vec<ComponentContractField>,
    object_contracts: Vec<ComponentObjectContract>,
    collection_contracts: Vec<ComponentCollectionContract>,
    interaction_contracts: Vec<ComponentInteractionContract>,
    composition_contracts: Vec<ComponentCompositionContract>,
    brand_constraints: Vec<ComponentBrandConstraint>,
}

fn parse_component_implementation_contract(
    component_name: &str,
    content: &str,
    variant_values: &[String],
) -> Result<ParsedImplementationContract> {
    let section =
        extract_markdown_section(content, "Implementation Contract").ok_or_else(|| {
            anyhow::anyhow!(
            "Component specification '{}' is missing required '## Implementation Contract' section",
            component_name
        )
        })?;

    let subsections = split_contract_subsections(&section);
    let props_lines = subsections.get("Props").ok_or_else(|| {
        anyhow::anyhow!(
            "Component specification '{}' is missing required '### Props' section under '## Implementation Contract'",
            component_name
        )
    })?;

    let props = props_lines
        .iter()
        .filter(|line| line.starts_with("- "))
        .map(|line| parse_contract_field_line(line, component_name, "Props"))
        .collect::<Result<Vec<_>>>()?;
    if props.is_empty() {
        anyhow::bail!(
            "Component specification '{}' must declare at least one prop in '## Implementation Contract > ### Props'",
            component_name
        );
    }

    let object_contracts = parse_object_contracts(
        component_name,
        subsections
            .get("Object Contracts")
            .cloned()
            .unwrap_or_default(),
    )?;
    let collection_contracts = parse_collection_contracts(
        component_name,
        subsections
            .get("Collection Contracts")
            .cloned()
            .unwrap_or_default(),
    )?;
    let interaction_contracts = parse_interaction_contracts(
        component_name,
        subsections
            .get("Interaction Contracts")
            .cloned()
            .unwrap_or_default(),
    )?;
    let composition_contracts = parse_composition_contracts(
        component_name,
        subsections
            .get("Composition Contracts")
            .cloned()
            .unwrap_or_default(),
    )?;
    let brand_constraints = parse_brand_constraints(
        component_name,
        subsections
            .get("Brand Constraints")
            .cloned()
            .unwrap_or_default(),
    )?;

    validate_component_implementation_contract(
        component_name,
        variant_values,
        &props,
        &object_contracts,
        &collection_contracts,
        &interaction_contracts,
        &brand_constraints,
    )?;

    Ok(ParsedImplementationContract {
        props,
        object_contracts,
        collection_contracts,
        interaction_contracts,
        composition_contracts,
        brand_constraints,
    })
}

fn extract_markdown_section(content: &str, section_name: &str) -> Option<String> {
    let target = format!("## {}", section_name);
    let mut in_section = false;
    let mut lines = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            if in_section {
                break;
            }
            if trimmed == target {
                in_section = true;
                continue;
            }
        }
        if in_section {
            lines.push(line.to_string());
        }
    }

    if in_section {
        Some(lines.join("\n"))
    } else {
        None
    }
}

fn split_contract_subsections(section: &str) -> HashMap<String, Vec<String>> {
    let mut sections = HashMap::new();
    let mut current: Option<String> = None;

    for line in section.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix("### ") {
            let key = name.trim().to_string();
            sections.entry(key.clone()).or_insert_with(Vec::new);
            current = Some(key);
            continue;
        }

        if let Some(name) = current.as_ref() {
            if !trimmed.is_empty() {
                sections
                    .entry(name.clone())
                    .or_insert_with(Vec::new)
                    .push(trimmed.to_string());
            }
        }
    }

    sections
}

fn parse_contract_field_line(
    line: &str,
    component_name: &str,
    section_name: &str,
) -> Result<ComponentContractField> {
    let (name, attrs) = parse_named_contract_line(line, component_name, section_name)?;
    let required = parse_bool_contract_attr(
        attrs.get("required"),
        component_name,
        section_name,
        &name,
        "required",
    )?;
    let shape = require_contract_attr(
        attrs.get("shape"),
        component_name,
        section_name,
        &name,
        "shape",
    )?;
    let ty = require_contract_attr(
        attrs.get("type"),
        component_name,
        section_name,
        &name,
        "type",
    )?;
    let allowed = attrs
        .get("allowed")
        .map(|raw| {
            raw.split('|')
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(ComponentContractField {
        name,
        required,
        shape,
        ty,
        item_contract: attrs.get("item_contract").cloned(),
        object_contract: attrs.get("object_contract").cloned(),
        allowed,
    })
}

fn parse_object_contracts(
    component_name: &str,
    lines: Vec<String>,
) -> Result<Vec<ComponentObjectContract>> {
    let mut contracts = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_fields = Vec::new();

    for line in lines {
        if let Some(name) = parse_h4_backtick_name(&line) {
            if let Some(previous) = current_name.replace(name) {
                if current_fields.is_empty() {
                    anyhow::bail!(
                        "Component specification '{}' declares object contract '{}' without any fields",
                        component_name,
                        previous
                    );
                }
                contracts.push(ComponentObjectContract {
                    name: previous,
                    fields: std::mem::take(&mut current_fields),
                });
            }
            continue;
        }

        if line.starts_with("- ") {
            current_fields.push(parse_contract_field_line(
                &line,
                component_name,
                "Object Contracts",
            )?);
        }
    }

    if let Some(previous) = current_name {
        if current_fields.is_empty() {
            anyhow::bail!(
                "Component specification '{}' declares object contract '{}' without any fields",
                component_name,
                previous
            );
        }
        contracts.push(ComponentObjectContract {
            name: previous,
            fields: current_fields,
        });
    }

    Ok(contracts)
}

fn parse_collection_contracts(
    component_name: &str,
    lines: Vec<String>,
) -> Result<Vec<ComponentCollectionContract>> {
    lines
        .iter()
        .filter(|line| line.starts_with("- "))
        .map(|line| {
            let (name, attrs) =
                parse_named_contract_line(line, component_name, "Collection Contracts")?;
            Ok(ComponentCollectionContract {
                name: name.clone(),
                item_contract: require_contract_attr(
                    attrs.get("item_contract"),
                    component_name,
                    "Collection Contracts",
                    &name,
                    "item_contract",
                )?,
                behavior: require_contract_attr(
                    attrs.get("behavior"),
                    component_name,
                    "Collection Contracts",
                    &name,
                    "behavior",
                )?,
            })
        })
        .collect()
}

fn parse_interaction_contracts(
    component_name: &str,
    lines: Vec<String>,
) -> Result<Vec<ComponentInteractionContract>> {
    lines
        .iter()
        .filter(|line| line.starts_with("- "))
        .map(|line| {
            let (target, attrs) =
                parse_named_contract_line(line, component_name, "Interaction Contracts")?;
            Ok(ComponentInteractionContract {
                target: target.clone(),
                kind: require_contract_attr(
                    attrs.get("kind"),
                    component_name,
                    "Interaction Contracts",
                    &target,
                    "kind",
                )?,
            })
        })
        .collect()
}

fn parse_composition_contracts(
    component_name: &str,
    lines: Vec<String>,
) -> Result<Vec<ComponentCompositionContract>> {
    lines
        .iter()
        .filter(|line| line.starts_with("- "))
        .map(|line| {
            let (name, attrs) =
                parse_named_contract_line(line, component_name, "Composition Contracts")?;
            Ok(ComponentCompositionContract {
                name: name.clone(),
                usage: require_contract_attr(
                    attrs.get("usage"),
                    component_name,
                    "Composition Contracts",
                    &name,
                    "usage",
                )?,
            })
        })
        .collect()
}

fn parse_brand_constraints(
    component_name: &str,
    lines: Vec<String>,
) -> Result<Vec<ComponentBrandConstraint>> {
    lines
        .iter()
        .filter(|line| line.starts_with("- "))
        .map(|line| {
            let (topic, guidance) =
                parse_named_text_line(line, component_name, "Brand Constraints")?;
            Ok(ComponentBrandConstraint { topic, guidance })
        })
        .collect()
}

fn parse_named_contract_line(
    line: &str,
    component_name: &str,
    section_name: &str,
) -> Result<(String, HashMap<String, String>)> {
    let re = Regex::new(r#"^- `(?P<name>[^`]+)`: (?P<attrs>.+)$"#).expect("valid regex");
    let caps = re.captures(line).ok_or_else(|| {
        anyhow::anyhow!(
            "Component specification '{}' contains malformed '{}' line: '{}'",
            component_name,
            section_name,
            line
        )
    })?;
    Ok((
        caps["name"].trim().to_string(),
        parse_backtick_attributes(caps.name("attrs").map(|m| m.as_str()).unwrap_or_default()),
    ))
}

fn parse_named_text_line(
    line: &str,
    component_name: &str,
    section_name: &str,
) -> Result<(String, String)> {
    let re = Regex::new(r#"^- `(?P<name>[^`]+)`: (?P<text>.+)$"#).expect("valid regex");
    let caps = re.captures(line).ok_or_else(|| {
        anyhow::anyhow!(
            "Component specification '{}' contains malformed '{}' line: '{}'",
            component_name,
            section_name,
            line
        )
    })?;
    let guidance = caps["text"].trim().to_string();
    if guidance.is_empty() {
        anyhow::bail!(
            "Component specification '{}' contains empty '{}' entry for '{}'",
            component_name,
            section_name,
            caps["name"].trim()
        );
    }
    Ok((caps["name"].trim().to_string(), guidance))
}

fn parse_backtick_attributes(raw: &str) -> HashMap<String, String> {
    let attr_re = Regex::new(r#"([a-z_]+)=`([^`]*)`"#).expect("valid regex");
    let mut attrs = HashMap::new();
    for caps in attr_re.captures_iter(raw) {
        attrs.insert(caps[1].to_string(), caps[2].trim().to_string());
    }
    attrs
}

fn require_contract_attr(
    value: Option<&String>,
    component_name: &str,
    section_name: &str,
    field_name: &str,
    attr_name: &str,
) -> Result<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Component specification '{}' is missing '{}' for '{}' in '{}'",
                component_name,
                attr_name,
                field_name,
                section_name
            )
        })
}

fn parse_bool_contract_attr(
    value: Option<&String>,
    component_name: &str,
    section_name: &str,
    field_name: &str,
    attr_name: &str,
) -> Result<bool> {
    match require_contract_attr(value, component_name, section_name, field_name, attr_name)?
        .as_str()
    {
        "true" => Ok(true),
        "false" => Ok(false),
        other => anyhow::bail!(
            "Component specification '{}' has invalid boolean '{}' for '{}' in '{}': expected `true` or `false`, got '{}'",
            component_name,
            attr_name,
            field_name,
            section_name,
            other
        ),
    }
}

fn parse_h4_backtick_name(line: &str) -> Option<String> {
    let re = Regex::new(r#"^#### `([^`]+)`$"#).expect("valid regex");
    re.captures(line)
        .map(|caps| caps[1].trim().to_string())
        .filter(|value| !value.is_empty())
}

fn validate_component_implementation_contract(
    component_name: &str,
    variant_values: &[String],
    props: &[ComponentContractField],
    object_contracts: &[ComponentObjectContract],
    collection_contracts: &[ComponentCollectionContract],
    interaction_contracts: &[ComponentInteractionContract],
    brand_constraints: &[ComponentBrandConstraint],
) -> Result<()> {
    let object_contract_names = object_contracts
        .iter()
        .map(|contract| contract.name.as_str())
        .collect::<HashSet<_>>();
    let collection_contract_names = collection_contracts
        .iter()
        .map(|contract| contract.name.as_str())
        .collect::<HashSet<_>>();

    for prop in props {
        match prop.shape.as_str() {
            "scalar" | "enum" | "object" | "list" => {}
            other => anyhow::bail!(
                "Component specification '{}' uses unsupported prop shape '{}' for '{}'",
                component_name,
                other,
                prop.name
            ),
        }

        if prop.shape == "enum" && prop.allowed.is_empty() {
            anyhow::bail!(
                "Component specification '{}' defines enum-shaped prop '{}' without any allowed values",
                component_name,
                prop.name
            );
        }
        if prop.shape == "object" {
            let object_contract = prop.object_contract.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "Component specification '{}' defines object-shaped prop '{}' without object_contract",
                    component_name,
                    prop.name
                )
            })?;
            if !object_contract_names.contains(object_contract.as_str()) {
                anyhow::bail!(
                    "Component specification '{}' references unknown object contract '{}' for prop '{}'",
                    component_name,
                    object_contract,
                    prop.name
                );
            }
        }
        if prop.shape == "list" {
            if !collection_contract_names.contains(prop.name.as_str()) {
                anyhow::bail!(
                    "Component specification '{}' defines list-shaped prop '{}' without a matching collection contract",
                    component_name,
                    prop.name
                );
            }
            if requires_structured_item_contract(&prop.ty)
                && prop
                    .item_contract
                    .as_ref()
                    .is_none_or(|value| value.trim().is_empty())
            {
                anyhow::bail!(
                    "Component specification '{}' defines structured list prop '{}' without item_contract",
                    component_name,
                    prop.name
                );
            }
        }
    }

    for collection in collection_contracts {
        let prop = props
            .iter()
            .find(|prop| prop.name == collection.name)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Component specification '{}' defines collection contract '{}' without a matching prop",
                    component_name,
                    collection.name
                )
            })?;
        if prop.shape != "list" {
            anyhow::bail!(
                "Component specification '{}' defines collection contract '{}' for non-list prop '{}'",
                component_name,
                collection.name,
                prop.name
            );
        }
    }

    if !variant_values.is_empty() {
        let variant_prop = props.iter().find(|prop| prop.name == "variant").ok_or_else(|| {
            anyhow::anyhow!(
                "Component specification '{}' enumerates variants but does not define a `variant` prop in '## Implementation Contract'",
                component_name
            )
        })?;
        if variant_prop.shape != "enum" {
            anyhow::bail!(
                "Component specification '{}' enumerates variants but its `variant` prop is not marked as shape=`enum`",
                component_name
            );
        }
        if variant_prop.allowed != variant_values {
            anyhow::bail!(
                "Component specification '{}' has mismatched variant values between `## Variants` and `## Implementation Contract`: expected {:?}, found {:?}",
                component_name,
                variant_values,
                variant_prop.allowed
            );
        }
    }

    if contract_requires_interactions(props, object_contracts) && interaction_contracts.is_empty() {
        anyhow::bail!(
            "Component specification '{}' exposes implementation-relevant interactions but does not define any '### Interaction Contracts'",
            component_name
        );
    }

    for constraint in brand_constraints {
        if is_vague_brand_guidance(&constraint.guidance) {
            anyhow::bail!(
                "Component specification '{}' contains vague brand guidance for topic '{}'; brand constraints in '## Implementation Contract' must be concrete enough to influence implementation shape",
                component_name,
                constraint.topic
            );
        }
    }

    Ok(())
}

fn requires_structured_item_contract(ty: &str) -> bool {
    !matches!(
        ty,
        "String" | "Boolean" | "Integer" | "Number" | "Float" | "View"
    )
}

fn contract_requires_interactions(
    props: &[ComponentContractField],
    object_contracts: &[ComponentObjectContract],
) -> bool {
    props.iter().any(prop_name_implies_interaction)
        || object_contracts
            .iter()
            .flat_map(|contract| contract.fields.iter())
            .any(prop_name_implies_interaction)
}

fn prop_name_implies_interaction(prop: &ComponentContractField) -> bool {
    let normalized = prop.name.to_ascii_lowercase();
    normalized.contains("href")
        || normalized.contains("action")
        || normalized.starts_with("on_")
        || normalized.starts_with("on")
        || normalized.contains("toggle")
        || normalized.contains("link")
}

fn is_vague_brand_guidance(guidance: &str) -> bool {
    let normalized = guidance.to_ascii_lowercase();
    let implementation_keywords = [
        "spacing",
        "typography",
        "motion",
        "icon",
        "token",
        "color",
        "contrast",
        "surface",
        "shadow",
        "border",
        "radius",
        "hierarchy",
        "scale",
    ];
    let vague_only_terms = [
        "beautiful",
        "premium",
        "nice",
        "polished",
        "delightful",
        "modern",
    ];

    !implementation_keywords
        .iter()
        .any(|keyword| normalized.contains(keyword))
        || vague_only_terms
            .iter()
            .any(|term| normalized == *term || normalized.starts_with(&format!("{} ", term)))
}

fn extract_backtick_values(raw: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut rest = raw;

    while let Some(start) = rest.find('`') {
        let after_start = &rest[start + 1..];
        let Some(end) = after_start.find('`') else {
            break;
        };
        let value = after_start[..end].trim();
        if !value.is_empty() {
            values.push(value.to_string());
        }
        rest = &after_start[end + 1..];
    }

    values
}

fn extract_component_names(content: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut previous_was_component_attr = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "#[component]" {
            previous_was_component_attr = true;
            continue;
        }

        if previous_was_component_attr {
            previous_was_component_attr = false;
            let rest = trimmed
                .strip_prefix("pub fn ")
                .or_else(|| trimmed.strip_prefix("fn "));
            if let Some(rest) = rest {
                if let Some((name, _)) = rest.split_once('(') {
                    let name = name.trim();
                    if !name.is_empty() {
                        names.push(name.to_string());
                    }
                }
            }
        }
    }

    names
}

fn validate_component_prop_annotations(context_name: &str, content: &str) -> Result<()> {
    let lines = content.lines().collect::<Vec<_>>();
    let mut previous_was_component_attr = false;
    let mut in_signature = false;
    let mut previous_was_prop_attr = false;
    let inline_prop_attr_re = Regex::new(r"#\[prop\([^\]]+\)\]\s*").unwrap();

    for line in &lines {
        let trimmed = line.trim();
        if trimmed == "#[component]" {
            previous_was_component_attr = true;
            in_signature = false;
            previous_was_prop_attr = false;
            continue;
        }

        if previous_was_component_attr {
            if trimmed.starts_with("pub fn ") || trimmed.starts_with("fn ") {
                in_signature = true;
                previous_was_component_attr = false;
            } else if trimmed.is_empty() {
                continue;
            } else {
                previous_was_component_attr = false;
            }
        }

        if !in_signature {
            continue;
        }

        if trimmed.starts_with("#[prop(") {
            previous_was_prop_attr = true;
            continue;
        }

        let has_prop_attr = previous_was_prop_attr || trimmed.contains("#[prop(");
        let sanitized = inline_prop_attr_re.replace_all(trimmed, "");

        let mut candidate = sanitized.as_ref().trim();
        if trimmed.starts_with("pub fn ") || trimmed.starts_with("fn ") {
            if let Some((_, rhs)) = candidate.split_once('(') {
                candidate = rhs.trim();
            }
        }
        if let Some((lhs, _)) = candidate.split_once(')') {
            candidate = lhs.trim();
        }
        if candidate.is_empty() {
            if sanitized.contains(')') {
                in_signature = false;
                previous_was_prop_attr = false;
            }
            continue;
        }

        if let Some((_, ty)) = candidate.split_once(':') {
            let ty = ty.trim().trim_end_matches(',');

            if ty == "String" && !has_prop_attr {
                anyhow::bail!(
                    "Generated brand implementation for '{}' defines a String component prop in src/app.rs without #[prop(into)]; string literal call sites would not compile",
                    context_name
                );
            }

            if ty.starts_with("Option<")
                && !has_prop_attr
                && !prop_receives_explicit_option_value(content, extract_prop_name(candidate))
            {
                anyhow::bail!(
                    "Generated brand implementation for '{}' defines an Option<T> component prop in src/app.rs without #[prop(optional)] or #[prop(default = ...)]",
                    context_name
                );
            }
        }

        previous_was_prop_attr = false;
        if sanitized.contains(')') {
            in_signature = false;
        }
    }

    Ok(())
}

fn validate_string_prop_default_literals(context_name: &str, content: &str) -> Result<()> {
    let bare_string_default_re = Regex::new(
        r#"#\[prop\([^\]]*\bdefault\s*=\s*"[^"]+"[^\]]*\)\]\s*[A-Za-z_][A-Za-z0-9_]*\s*:\s*String"#,
    )
    .unwrap();
    if bare_string_default_re.is_match(content) {
        anyhow::bail!(
            "Generated brand implementation for '{}' defines a String component prop with a bare string-literal default in src/app.rs; use `String::from(...)` and `#[prop(into)]`",
            context_name
        );
    }

    Ok(())
}

fn validate_string_signal_option_updates(context_name: &str, content: &str) -> Result<()> {
    let string_option_vec_re =
        Regex::new(r#"let\s+[A-Za-z_][A-Za-z0-9_]*\s*=\s*vec!\[\s*""#).unwrap();
    let set_cloned_option_re = Regex::new(
        r#"\bset_[A-Za-z_][A-Za-z0-9_]*\.set\(\s*[A-Za-z_][A-Za-z0-9_]*\.clone\(\)\s*\)"#,
    )
    .unwrap();

    if string_option_vec_re.is_match(content) && set_cloned_option_re.is_match(content) {
        anyhow::bail!(
            "Generated brand implementation for '{}' updates a String signal from a cloned string-literal option in src/app.rs; use `.to_string()` or store owned String options",
            context_name
        );
    }

    Ok(())
}

fn validate_optional_option_forwarding_patterns(context_name: &str, content: &str) -> Result<()> {
    let inline_prop_attr_re = Regex::new(r"#\[prop\([^\]]+\)\]\s*").unwrap();
    let optional_attr_line_re =
        Regex::new(r#"^#\[prop\((?:optional|optional,\s*into|into,\s*optional)\)\]$"#).unwrap();
    let optional_attr_inline_re =
        Regex::new(r#"#\[prop\((?:optional|optional,\s*into|into,\s*optional)\)\]\s*"#).unwrap();
    let mut pending_optional_attr = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if optional_attr_line_re.is_match(trimmed) {
            pending_optional_attr = true;
            continue;
        }

        let has_optional_attr = pending_optional_attr || optional_attr_inline_re.is_match(trimmed);
        pending_optional_attr = false;
        if !has_optional_attr {
            continue;
        }

        let Some((name, _)) = parse_option_prop_candidate(trimmed, &inline_prop_attr_re) else {
            continue;
        };

        if Regex::new(&format!(r"\b{}\s*=\s*None\b", regex::escape(&name)))
            .unwrap()
            .is_match(content)
        {
            anyhow::bail!(
                "Generated brand implementation for '{}' forwards `None` into optional-builder prop '{}' in src/app.rs; keep '{}' as a plain `Option<T>` parameter when call sites pass `None` explicitly",
                context_name,
                name,
                name
            );
        }

        if Regex::new(&format!(r"\b{}\s*=\s*Some\(", regex::escape(&name)))
            .unwrap()
            .is_match(content)
        {
            anyhow::bail!(
                "Generated brand implementation for '{}' forwards `Some(...)` into optional-builder prop '{}' in src/app.rs; keep '{}' as a plain `Option<T>` parameter when call sites pass `Option<T>` values directly",
                context_name,
                name,
                name
            );
        }

        if prop_receives_explicit_option_value(content, &name) {
            anyhow::bail!(
                "Generated brand implementation for '{}' forwards `Option<T>` values directly into optional-builder prop '{}' in src/app.rs; keep '{}' as a plain `Option<T>` parameter instead of using `#[prop(optional)]` or `#[prop(optional, into)]`",
                context_name,
                name,
                name
            );
        }
    }

    Ok(())
}

fn validate_spec_defined_variant_contracts(
    context_name: &str,
    content: &str,
    component_specs: &[ComponentSpecContract],
) -> Result<()> {
    let inline_prop_attr_re = Regex::new(r"#\[prop\([^\]]+\)\]\s*").unwrap();
    let sanitized = inline_prop_attr_re.replace_all(content, "");

    for spec in component_specs {
        if spec.variant_values.is_empty() {
            continue;
        }
        if !sanitized.contains(&format!("fn {}(", spec.name)) {
            continue;
        }

        if !sanitized.contains(&format!("enum {}", spec.enum_name)) {
            anyhow::bail!(
                "Generated brand implementation for '{}' does not define the expected typed variant enum '{}' in src/app.rs",
                context_name,
                spec.enum_name
            );
        }

        let string_variant_re = Regex::new(&format!(
            r"(?s)fn\s+{}\s*\(.*?\bvariant\s*:\s*String",
            regex::escape(&spec.name)
        ))
        .unwrap();
        if string_variant_re.is_match(&sanitized) {
            anyhow::bail!(
                "Generated brand implementation for '{}' keeps '{}' variant as String in src/app.rs even though the component specification enumerates allowed variants",
                context_name,
                spec.name
            );
        }

        let typed_variant_re = Regex::new(&format!(
            r"(?s)fn\s+{}\s*\(.*?\bvariant\s*:\s*{}",
            regex::escape(&spec.name),
            regex::escape(&spec.enum_name)
        ))
        .unwrap();
        if !typed_variant_re.is_match(&sanitized) {
            anyhow::bail!(
                "Generated brand implementation for '{}' does not type '{}' variant as '{}' in src/app.rs",
                context_name,
                spec.name,
                spec.enum_name
            );
        }

        let raw_callsite_re = Regex::new(&format!(
            r#"<{}\b[^>]*\bvariant\s*=\s*"[^"]+""#,
            regex::escape(&spec.name)
        ))
        .unwrap();
        if raw_callsite_re.is_match(content) {
            anyhow::bail!(
                "Generated brand implementation for '{}' uses raw string literals for enum-backed '{}' variants in src/app.rs",
                context_name,
                spec.name
            );
        }

        for (value, rust_variant) in spec.variant_values.iter().zip(spec.rust_variants.iter()) {
            let enum_variant_re =
                Regex::new(&format!(r"\b{}\b", regex::escape(&rust_variant))).unwrap();
            if !enum_variant_re.is_match(content) {
                anyhow::bail!(
                    "Generated brand implementation for '{}' does not map spec variant '{}' to the expected Rust enum variant '{}' for component '{}'",
                    context_name,
                    value,
                    rust_variant,
                    spec.name
                );
            }
        }
    }

    Ok(())
}

fn validate_generated_text_encoding(context_name: &str, content: &str) -> Result<()> {
    let suspicious_sequences = ["Â©", "â€œ", "â€", "â€™", "â€“", "â€”"];
    for sequence in suspicious_sequences {
        if content.contains(sequence) {
            anyhow::bail!(
                "Generated brand implementation for '{}' contains suspicious mojibake sequence '{}' in src/app.rs; preserve valid Unicode from the specification",
                context_name,
                sequence
            );
        }
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

fn validate_generated_brand_rust_patterns(
    context_name: &str,
    generated_files: &[GeneratedOutputFile],
) -> Result<()> {
    let component_tag_prop =
        Regex::new(r"=\s*<[A-Z]").expect("component tag prop regex should compile");

    for file in generated_files
        .iter()
        .filter(|file| file.path.extension().and_then(|ext| ext.to_str()) == Some("rs"))
    {
        validate_forbidden_leptos_class_syntax(
            context_name,
            &file.content,
            &file.path.display().to_string(),
        )?;

        if component_tag_prop.is_match(&file.content) {
            anyhow::bail!(
                "Generated brand implementation for '{}' contains a component tag or view fragment as a prop value in {}; use plain data props or nested children instead",
                context_name,
                file.path.display()
            );
        }

        if file.content.contains("AnyView") {
            anyhow::bail!(
                "Generated brand implementation for '{}' uses unsupported AnyView props in {}; use plain data props or an explicitly supported Children/ChildrenFn slot pattern instead",
                context_name,
                file.path.display()
            );
        }
    }

    Ok(())
}

fn validate_brand_css_variables(context_name: &str, content: &str) -> Result<()> {
    let definition_pattern =
        Regex::new(r"(?m)(--brand-[A-Za-z0-9_-]+)\s*:").expect("brand css definition regex");
    let reference_pattern =
        Regex::new(r"var\(\s*(--brand-[A-Za-z0-9_-]+)").expect("brand css reference regex");

    let definitions = definition_pattern
        .captures_iter(content)
        .filter_map(|captures| captures.get(1).map(|m| m.as_str().to_string()))
        .collect::<HashSet<_>>();

    let unresolved = reference_pattern
        .captures_iter(content)
        .filter_map(|captures| captures.get(1).map(|m| m.as_str().to_string()))
        .filter(|reference| !definitions.contains(reference))
        .collect::<HashSet<_>>();

    if !unresolved.is_empty() {
        let mut unresolved = unresolved.into_iter().collect::<Vec<_>>();
        unresolved.sort();
        anyhow::bail!(
            "Generated brand implementation for '{}' references undefined CSS brand variable(s) in style/app.css: {}",
            context_name,
            unresolved.join(", ")
        );
    }

    Ok(())
}

fn repair_brand_app_css(context_name: &str, content: &str) -> Result<String> {
    // 1) Normalize named-color definitions to deterministic concrete values.
    // 2) Ensure referenced `--brand-*` variables are defined when we can infer a fallback.
    let definition_line_pattern = Regex::new(
        r#"(?m)^(?P<indent>\s*)(?P<var>--brand-[A-Za-z0-9_-]+)\s*:\s*(?P<value>[^;]+)\s*;\s*$"#,
    )
    .expect("brand css definition line regex");

    let normalized = definition_line_pattern
        .replace_all(content, |caps: &regex::Captures| {
            let indent = caps.name("indent").map(|m| m.as_str()).unwrap_or("");
            let var = caps.name("var").map(|m| m.as_str()).unwrap_or("");
            let value = caps.name("value").map(|m| m.as_str()).unwrap_or("").trim();

            let normalized_value = normalize_color_value(value)
                .unwrap_or_else(|| value.to_string());

            format!("{indent}{var}: {normalized_value};")
        })
        .to_string();

    let definition_pattern =
        Regex::new(r"(?m)(--brand-[A-Za-z0-9_-]+)\s*:").expect("brand css definition regex");
    let reference_pattern =
        Regex::new(r"var\(\s*(--brand-[A-Za-z0-9_-]+)").expect("brand css reference regex");

    let definitions = definition_pattern
        .captures_iter(&normalized)
        .filter_map(|captures| captures.get(1).map(|m| m.as_str().to_string()))
        .collect::<HashSet<_>>();

    let referenced = reference_pattern
        .captures_iter(&normalized)
        .filter_map(|captures| captures.get(1).map(|m| m.as_str().to_string()))
        .collect::<HashSet<_>>();

    let mut unresolved = referenced
        .into_iter()
        .filter(|reference| !definitions.contains(reference))
        .collect::<Vec<_>>();
    unresolved.sort();

    let mut injectable = Vec::new();
    for var in &unresolved {
        if let Some(value) = brand_var_fallback(var) {
            injectable.push((var.clone(), value.to_string()));
        }
    }

    let repaired = if injectable.is_empty() {
        normalized
    } else {
        inject_root_definitions(&normalized, &injectable)
    };

    // If unknown unresolved vars remain, keep the strict behavior (the subsequent validator will bail).
    // But attach a hint so failures are easier to diagnose.
    if unresolved.iter().any(|var| brand_var_fallback(var).is_none()) {
        // Don't bail here; keep behavior centralized in `validate_brand_css_variables`.
        // Just ensure the output remains deterministic.
        return Ok(repaired);
    }

    // Additional safety: make sure we didn't introduce mojibake or invalid output.
    if repaired.trim().is_empty() {
        anyhow::bail!(
            "Generated brand implementation for '{}' produced empty style/app.css after repair",
            context_name
        );
    }

    Ok(repaired)
}

fn normalize_color_value(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if is_explicit_color_value(trimmed) {
        return None;
    }
    // Accept simple named colors (single token, possibly quoted).
    let unquoted = trimmed
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_ascii_lowercase();
    if !unquoted
        .chars()
        .all(|ch| ch.is_ascii_alphabetic() || ch == '-')
    {
        return None;
    }
    named_color_fallback(&unquoted).map(|fallback| fallback.to_string())
}

fn is_explicit_color_value(value: &str) -> bool {
    let v = value.trim().to_ascii_lowercase();
    v.starts_with('#')
        || v.starts_with("rgb(")
        || v.starts_with("rgba(")
        || v.starts_with("hsl(")
        || v.starts_with("hsla(")
        || v.starts_with("color(")
        || v.starts_with("oklch(")
        || v.starts_with("oklab(")
}

fn named_color_fallback(name: &str) -> Option<&'static str> {
    match name.trim().to_ascii_lowercase().as_str() {
        "black" => Some("#000000"),
        "white" => Some("#ffffff"),
        "red" => Some("#ff0000"),
        "blue" => Some("#0000ff"),
        "green" => Some("#008000"),
        "yellow" => Some("#ffff00"),
        "orange" => Some("#ffa500"),
        "purple" => Some("#800080"),
        "gray" | "grey" => Some("#808080"),
        // A few common “designer” names that appear in drafts.
        "lavender" => Some("#e6e6fa"),
        "beige" => Some("#f5f5dc"),
        _ => None,
    }
}

fn brand_var_fallback(var_name: &str) -> Option<&'static str> {
    let normalized = var_name.trim();
    match normalized {
        "--brand-colors-primary-black" => Some("#000000"), // black
        "--brand-colors-primary-white" => Some("#ffffff"), // white
        "--brand-colors-text-primary" => Some("#111111"), // near-black text
        "--brand-colors-text-secondary" => Some("#444444"), // secondary text gray
        "--brand-colors-surface-default" => Some("#ffffff"), // surface white
        "--brand-colors-surface-raised" => Some("#f7f7f7"), // raised surface (light gray)
        "--brand-colors-surface-muted" => Some("#f2f2f2"), // muted surface (light gray)
        "--brand-colors-border-default" => Some("rgba(0, 0, 0, 0.12)"), // subtle black border
        "--brand-colors-semantic-success" => Some("#00aa55"), // success green
        "--brand-colors-semantic-warning" => Some("#f59e0b"), // warning amber
        "--brand-colors-semantic-destructive" => Some("#dc2626"), // destructive red
        "--brand-colors-semantic-error" => Some("#dc2626"), // error red
        _ => {
            // Heuristics: if the variable name ends with a known named color, use that.
            // Example: `--brand-colors-primary-red` -> fallback for `red`.
            let suffix = normalized
                .rsplit('-')
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            if let Some(color) = named_color_fallback(&suffix) {
                return Some(color);
            }

            // Semantic heuristics.
            if normalized.contains("semantic") {
                if normalized.contains("success") {
                    return Some("#00aa55");
                }
                if normalized.contains("warning") {
                    return Some("#f59e0b");
                }
                if normalized.contains("destructive") || normalized.contains("error") {
                    return Some("#dc2626");
                }
            }

            None
        }
    }
}

fn inject_root_definitions(content: &str, definitions: &[(String, String)]) -> String {
    if definitions.is_empty() {
        return content.to_string();
    }
    let mut lines = Vec::new();
    for (var, value) in definitions {
        lines.push(format!("    {var}: {value};"));
    }
    let injection = lines.join("\n");

    if let Some(root_pos) = content.find(":root") {
        // Find the first '{' following :root
        if let Some(open_brace_rel) = content[root_pos..].find('{') {
            let open_brace = root_pos + open_brace_rel;
            if let Some(close_brace_rel) = content[open_brace..].find('}') {
                let close_brace = open_brace + close_brace_rel;
                let mut out = String::new();
                out.push_str(&content[..close_brace]);
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str(&injection);
                out.push('\n');
                out.push_str(&content[close_brace..]);
                return out;
            }
        }
    }

    // No `:root {}` block found; prepend one.
    let mut out = String::new();
    out.push_str(":root {\n");
    out.push_str(&injection);
    out.push_str("\n}\n\n");
    out.push_str(content);
    out
}

fn validate_gitignore(context_name: &str, content: &str) -> Result<()> {
    let required_entries = ["target/", ".cargo-leptos/", ".leptos/", ".reen/", "/style", "Leptos.toml", "/public"];
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
        build_component_spec_contract, extract_component_name_from_spec, extract_dep_feature_refs,
        component_module_specs, extract_dependency_spec, extract_toml_value,
        normalize_generated_app_rs, normalize_generated_brand_files, pascal_case_identifier,
        render_brand_scaffold_contract, render_brand_variant_contract,
        render_component_implementation_contract, repair_brand_app_css, validate_app_rs,
        validate_app_rs_with_component_specs, validate_brand_css_variables,
        validate_dependency_render_feature_mode, validate_generated_brand_rust_patterns,
        validate_lib_target_name, validate_matching_leptos_config,
        validate_optional_dep_feature_wiring, GeneratedOutputFile, ParsedImplementationContract,
    };
    use std::path::PathBuf;

    fn test_component_spec(name: &str, variant_values: &[&str]) -> super::ComponentSpecContract {
        build_component_spec_contract(
            name.to_string(),
            variant_values
                .iter()
                .map(|value| value.to_string())
                .collect(),
            ParsedImplementationContract {
                props: vec![super::ComponentContractField {
                    name: "variant".to_string(),
                    required: false,
                    shape: "enum".to_string(),
                    ty: format!("{}Variant", name),
                    item_contract: None,
                    object_contract: None,
                    allowed: variant_values
                        .iter()
                        .map(|value| value.to_string())
                        .collect(),
                }],
                ..Default::default()
            },
        )
    }

    fn implementation_contract_spec(name: &str) -> String {
        format!(
            "# {name}\n\n## Component Metadata\n- **Name**: {name}\n\n## Variants\n- `default`\n- `minimal`\n\n## Properties\n- `variant`: `default` | `minimal`\n- `items`: list\n\n## Implementation Contract\n### Props\n- `variant`: required=`false`; shape=`enum`; type=`{name}Variant`; allowed=`default|minimal`\n- `title`: required=`true`; shape=`scalar`; type=`String`\n- `items`: required=`true`; shape=`list`; type=`NavItem`; item_contract=`NavItem`\n- `theme_toggle`: required=`false`; shape=`object`; type=`ThemeToggle`; object_contract=`ThemeToggle`\n\n### Object Contracts\n#### `NavItem`\n- `label`: required=`true`; shape=`scalar`; type=`String`\n- `href`: required=`true`; shape=`scalar`; type=`String`\n\n#### `ThemeToggle`\n- `selected`: required=`true`; shape=`scalar`; type=`String`\n- `options`: required=`true`; shape=`list`; type=`String`\n\n### Collection Contracts\n- `items`: item_contract=`NavItem`; behavior=`repeated-item`\n\n### Interaction Contracts\n- `items[*]`: kind=`navigational`\n- `theme_toggle`: kind=`stateful`\n\n### Composition Contracts\n- `Link`: usage=`reused-subcomponent`\n- `ThemeToggle`: usage=`optional`\n\n### Brand Constraints\n- `spacing`: use active brand spacing rhythm and hierarchy tokens when available\n",
        )
    }

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
        assert!(rendered.contains("include_str!(\"../style/app.css\")"));
        assert!(rendered.contains("target/"));
        assert!(rendered.contains("cargo leptos watch"));
        assert!(
            rendered.contains("Do not redefine the route tree in `src/main.rs` or `src/lib.rs`.")
        );
        assert!(rendered.contains("Keep generated reusable components in `src/app.rs`"));
        assert!(rendered.contains("typed `{ComponentName}Variant` enum"));
        assert!(rendered.contains(".with_state(leptos_options)"));
    }

    #[test]
    fn rendered_variant_contract_includes_expected_enum_shapes() {
        let rendered = render_brand_variant_contract(&[
            test_component_spec("Badge", &["neutral", "success"]),
            test_component_spec("AccountCard", &["default", "positive-balance"]),
        ])
        .expect("expected rendered variant contract");

        assert!(rendered.contains("Component `Badge` variant contract"));
        assert!(rendered.contains("`BadgeVariant`"));
        assert!(rendered.contains("`neutral` -> `BadgeVariant::Neutral`"));
        assert!(rendered.contains("`positive-balance` -> `AccountCardVariant::PositiveBalance`"));
    }

    #[test]
    fn rendered_component_implementation_contract_includes_structured_shapes() {
        let contract = super::parse_component_implementation_contract(
            "TopNav",
            &implementation_contract_spec("TopNav"),
            &["default".to_string(), "minimal".to_string()],
        )
        .expect("parse implementation contract");
        let rendered = render_component_implementation_contract(&[build_component_spec_contract(
            "TopNav".to_string(),
            vec!["default".to_string(), "minimal".to_string()],
            contract,
        )])
        .expect("rendered component implementation contract");

        assert!(rendered.contains("Component `TopNav` implementation contract"));
        assert!(rendered.contains("\"shape\": \"list\""));
        assert!(rendered.contains("\"item_contract\": \"NavItem\""));
        assert!(rendered.contains("\"kind\": \"navigational\""));
        assert!(rendered.contains("\"topic\": \"spacing\""));
    }

    #[test]
    fn component_contract_parser_rejects_missing_collection_item_shape() {
        let spec = "# NavBar\n\n## Component Metadata\n- **Name**: NavBar\n\n## Properties\n- `items`: list\n\n## Implementation Contract\n### Props\n- `items`: required=`true`; shape=`list`; type=`NavItem`\n";
        let err = super::parse_component_implementation_contract("NavBar", spec, &[])
            .expect_err("expected missing collection contract failure");
        assert!(err
            .to_string()
            .contains("without a matching collection contract"));
    }

    #[test]
    fn component_contract_parser_rejects_object_prop_without_fields() {
        let spec = "# Dashboard\n\n## Component Metadata\n- **Name**: Dashboard\n\n## Properties\n- `summary`: object\n\n## Implementation Contract\n### Props\n- `summary`: required=`false`; shape=`object`; type=`SummaryBlock`; object_contract=`SummaryBlock`\n\n### Object Contracts\n#### `SummaryBlock`\n";
        let err = super::parse_component_implementation_contract("Dashboard", spec, &[])
            .expect_err("expected missing object field failure");
        assert!(err.to_string().contains("without any fields"));
    }

    #[test]
    fn component_contract_parser_rejects_variant_mismatch() {
        let spec = "# Badge\n\n## Component Metadata\n- **Name**: Badge\n\n## Variants\n- `default`\n- `success`\n\n## Properties\n- `variant`: `default` | `success`\n\n## Implementation Contract\n### Props\n- `variant`: required=`false`; shape=`enum`; type=`BadgeVariant`; allowed=`default|neutral`\n";
        let err = super::parse_component_implementation_contract(
            "Badge",
            spec,
            &["default".to_string(), "success".to_string()],
        )
        .expect_err("expected variant mismatch");
        assert!(err.to_string().contains(
            "mismatched variant values between `## Variants` and `## Implementation Contract`"
        ));
    }

    #[test]
    fn component_contract_parser_rejects_missing_interaction_contracts_for_navigation_shapes() {
        let spec = "# Footer\n\n## Component Metadata\n- **Name**: Footer\n\n## Properties\n- `legal_links`: list\n\n## Implementation Contract\n### Props\n- `legal_links`: required=`false`; shape=`list`; type=`LegalLink`; item_contract=`LegalLink`\n\n### Object Contracts\n#### `LegalLink`\n- `label`: required=`true`; shape=`scalar`; type=`String`\n- `href`: required=`true`; shape=`scalar`; type=`String`\n\n### Collection Contracts\n- `legal_links`: item_contract=`LegalLink`; behavior=`repeated-item`\n";
        let err = super::parse_component_implementation_contract("Footer", spec, &[])
            .expect_err("expected interaction contract failure");
        assert!(err
            .to_string()
            .contains("does not define any '### Interaction Contracts'"));
    }

    #[test]
    fn component_contract_parser_rejects_vague_brand_constraints() {
        let spec = "# Button\n\n## Component Metadata\n- **Name**: Button\n\n## Properties\n- `label`: text\n\n## Implementation Contract\n### Props\n- `label`: required=`true`; shape=`scalar`; type=`String`\n\n### Brand Constraints\n- `tone`: premium\n";
        let err = super::parse_component_implementation_contract("Button", spec, &[])
            .expect_err("expected vague brand failure");
        assert!(err
            .to_string()
            .contains("brand constraints in '## Implementation Contract' must be concrete enough"));
    }

    #[test]
    fn component_name_extraction_preserves_authored_pascal_case() {
        let account_card = "# AccountCard\n\n## Component Metadata\n- **Name**: AccountCard\n";
        let balance_summary =
            "# BalanceSummary - Component Specification\n\n## Component Metadata\n";
        let lowercase_badge = "# badge\n\n## Component Metadata\n- **Name**: badge\n";

        assert_eq!(
            extract_component_name_from_spec(account_card),
            Some("AccountCard".to_string())
        );
        assert_eq!(
            extract_component_name_from_spec(balance_summary),
            Some("BalanceSummary".to_string())
        );
        assert_eq!(
            extract_component_name_from_spec(lowercase_badge),
            Some("Badge".to_string())
        );
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
    fn component_module_detection_excludes_components_mod_rs() {
        let files = vec![
            GeneratedOutputFile {
                path: PathBuf::from("src/components/button.rs"),
                content: String::new(),
            },
            GeneratedOutputFile {
                path: PathBuf::from("src/components/mod.rs"),
                content: String::new(),
            },
            GeneratedOutputFile {
                path: PathBuf::from("src/app.rs"),
                content: String::new(),
            },
        ];

        let specs = component_module_specs(&files);

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].module_name, "button");
        assert_eq!(specs[0].component_name, "Button");
    }

    #[test]
    fn generated_rust_validation_rejects_component_tag_prop_values() {
        let files = vec![GeneratedOutputFile {
            path: PathBuf::from("src/app.rs"),
            content: r#"
view! {
    <Card media=<Image src="/placeholder.jpg" alt="Placeholder" variant="thumbnail"/> />
}
"#
            .to_string(),
        }];

        let err = validate_generated_brand_rust_patterns("demo", &files)
            .expect_err("expected component tag prop rejection");

        assert!(err
            .to_string()
            .contains("component tag or view fragment as a prop value"));
    }

    #[test]
    fn generated_rust_validation_rejects_any_view_props() {
        let files = vec![GeneratedOutputFile {
            path: PathBuf::from("src/components/card.rs"),
            content: "pub fn Card(#[prop(optional)] media: Option<AnyView>) -> impl IntoView { view! { <article/> } }"
                .to_string(),
        }];

        let err = validate_generated_brand_rust_patterns("demo", &files)
            .expect_err("expected AnyView rejection");

        assert!(err.to_string().contains("unsupported AnyView props"));
    }

    #[test]
    fn generated_rust_validation_accepts_plain_data_composition_props() {
        let files = vec![GeneratedOutputFile {
            path: PathBuf::from("src/components/card.rs"),
            content: r#"
use leptos::*;
use crate::components::{Badge, Button, Image};

#[component]
pub fn Card(
    #[prop(into, optional)] media_src: Option<String>,
    #[prop(into, optional)] badge_label: Option<String>,
    #[prop(into, optional)] action_label: Option<String>,
) -> impl IntoView {
    view! {
        <article>
            {media_src.map(|src| view! { <Image src=src alt="Preview" variant="thumbnail"/> })}
            {badge_label.map(|label| view! { <Badge label=label variant="neutral"/> })}
            {action_label.map(|label| view! { <Button label=label variant="primary"/> })}
        </article>
    }
}
"#
            .to_string(),
        }];

        validate_generated_brand_rust_patterns("demo", &files)
            .expect("plain data composition should be accepted");
    }

    #[test]
    fn brand_css_variable_validation_accepts_defined_references() {
        let css = r#"
:root {
    --brand-colors-primary-red: #ff0000;
    --brand-colors-primary-white: #ffffff;
}

body {
    color: var(--brand-colors-primary-red);
    background: var(--brand-colors-primary-white);
}
"#;

        validate_brand_css_variables("demo", css).expect("defined vars should pass");
    }

    #[test]
    fn brand_css_variable_validation_rejects_undefined_references() {
        let css = r#"
:root {
    --brand-colors-primary-red: #ff0000;
}

body {
    color: var(--brand-colors-primary-black);
}
"#;

        let err = validate_brand_css_variables("demo", css)
            .expect_err("expected undefined variable rejection");

        assert!(err.to_string().contains("--brand-colors-primary-black"));
    }

    #[test]
    fn brand_app_css_repair_normalizes_named_color_values() {
        let css = r#"
:root {
    --brand-colors-primary-red: red;
    --brand-colors-primary-white: #ffffff;
}

body {
    color: var(--brand-colors-primary-red);
    background: var(--brand-colors-primary-white);
}
"#;

        let repaired = repair_brand_app_css("demo", css).expect("repair should succeed");
        assert!(repaired.contains("--brand-colors-primary-red: #ff0000;"));
        validate_brand_css_variables("demo", &repaired).expect("repaired vars should validate");
    }

    #[test]
    fn brand_app_css_repair_injects_missing_definitions_when_fallback_known() {
        let css = r#"
:root {
    --brand-colors-primary-red: #ff0000;
}

body {
    color: var(--brand-colors-primary-black);
}
"#;

        let repaired = repair_brand_app_css("demo", css).expect("repair should succeed");
        assert!(repaired.contains("--brand-colors-primary-black: #000000;"));
        validate_brand_css_variables("demo", &repaired).expect("injected vars should validate");
    }

    #[test]
    fn brand_app_css_repair_does_not_mask_unknown_variables() {
        let css = r#"
:root {
    --brand-colors-primary-red: #ff0000;
}

body {
    color: var(--brand-colors-primary-unknowncustom);
}
"#;

        let repaired = repair_brand_app_css("demo", css).expect("repair should not fail");
        let err = validate_brand_css_variables("demo", &repaired)
            .expect_err("unknown variable should still fail strict validation");
        assert!(err.to_string().contains("--brand-colors-primary-unknowncustom"));
    }

    #[test]
    fn pascal_case_identifier_handles_component_file_stems() {
        assert_eq!(pascal_case_identifier("button"), "Button");
        assert_eq!(pascal_case_identifier("button_minimal"), "ButtonMinimal");
        assert_eq!(pascal_case_identifier("badge"), "Badge");
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

    #[test]
    fn app_rs_rejects_jsx_style_spread_props_and_raw_svg_use() {
        let app = r##"use leptos::*;
use leptos_router::*;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <style>{include_str!("../style/app.css")}</style>
        <Router>
            <Routes>
                <Route path="/" view=HomePage/>
            </Routes>
        </Router>
    }
}

#[component]
pub fn HomePage() -> impl IntoView {
    view! { <AccountCard ..account /> }
}

#[component]
pub fn AccountCard() -> impl IntoView {
    view! { <svg><use href="#icon" /></svg> }
}
"##;

        let err = validate_app_rs_with_component_specs("demo", app, &[])
            .expect_err("expected invalid Leptos syntax");
        assert!(err.to_string().contains("invalid Leptos syntax"));
    }

    #[test]
    fn app_rs_rejects_component_and_struct_name_collisions() {
        let app = r#"use leptos::*;
use leptos_router::*;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <style>{include_str!("../style/app.css")}</style>
        <Router>
            <Routes>
                <Route path="/" view=HomePage/>
            </Routes>
        </Router>
    }
}

#[component]
pub fn HomePage() -> impl IntoView {
    view! { <Badge label="ok" /> }
}

pub struct Badge {
    pub label: String,
}

#[component]
pub fn Badge(#[prop(into)] label: String) -> impl IntoView {
    view! { <span>{label}</span> }
}
"#;

        let err = validate_app_rs_with_component_specs("demo", app, &[])
            .expect_err("expected name collision failure");
        assert!(err
            .to_string()
            .contains("both a Leptos component and a Rust struct"));
    }

    #[test]
    fn app_rs_rejects_leptos_0_6_incompatible_for_view_syntax() {
        let app = r#"use leptos::*;
use leptos_router::*;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <style>{include_str!("../style/app.css")}</style>
        <Router>
            <Routes>
                <Route path="/" view=HomePage/>
            </Routes>
        </Router>
    }
}

#[component]
pub fn HomePage() -> impl IntoView {
    view! {
        <For
            each=move || vec!["a".to_string()]
            key=|item| item.clone()
            view=move |item| {
                view! { <div>{item}</div> }
            }
        />
    }
}
"#;

        let err = validate_app_rs_with_component_specs("demo", app, &[])
            .expect_err("expected invalid For syntax");
        assert!(err
            .to_string()
            .contains("Leptos-incompatible <For /> syntax"));
        assert!(err.to_string().contains("children=move |item| view!"));
    }

    #[test]
    fn app_rs_rejects_manual_component_props_structs() {
        let app = r#"use leptos::*;
use leptos_router::*;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <style>{include_str!("../style/app.css")}</style>
        <Router>
            <Routes>
                <Route path="/" view=HomePage/>
            </Routes>
        </Router>
    }
}

#[component]
pub fn HomePage() -> impl IntoView {
    view! { <ThemeToggle variant="default" /> }
}

pub struct ThemeToggleProps {
    pub variant: String,
}

#[component]
pub fn ThemeToggle(#[prop(into)] variant: String) -> impl IntoView {
    view! { <div>{variant}</div> }
}
"#;

        let err = validate_app_rs_with_component_specs("demo", app, &[])
            .expect_err("expected props struct failure");
        assert!(err
            .to_string()
            .contains("already generates that props type"));
    }

    #[test]
    fn app_rs_rejects_generic_component_spread_props() {
        let app = r#"use leptos::*;
use leptos_router::*;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <style>{include_str!("../style/app.css")}</style>
        <Router>
            <Routes>
                <Route path="/" view=HomePage/>
            </Routes>
        </Router>
    }
}

#[component]
pub fn HomePage() -> impl IntoView {
    view! { <ThemeToggle ..props /> }
}

#[component]
pub fn ThemeToggle() -> impl IntoView {
    view! { <div/> }
}
"#;

        let err = validate_app_rs_with_component_specs("demo", app, &[])
            .expect_err("expected spread props failure");
        assert!(err.to_string().contains("component spread props"));
    }

    #[test]
    fn app_rs_rejects_component_struct_literal_usage() {
        let app = r#"use leptos::*;
use leptos_router::*;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <style>{include_str!("../style/app.css")}</style>
        <Router>
            <Routes>
                <Route path="/" view=HomePage/>
            </Routes>
        </Router>
    }
}

#[component]
pub fn HomePage() -> impl IntoView {
    let _items = vec![AccountCard { title: "x".to_string() }];
    view! { <div/> }
}

#[component]
pub fn AccountCard(#[prop(into)] title: String) -> impl IntoView {
    view! { <div>{title}</div> }
}
"#;

        let err = validate_app_rs_with_component_specs("demo", app, &[])
            .expect_err("expected struct literal failure");
        assert!(err.to_string().contains("like a Rust struct literal"));
    }

    #[test]
    fn app_rs_rejects_non_cloneable_boxed_callback_patterns() {
        let app = r#"use leptos::*;
use leptos_router::*;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <style>{include_str!("../style/app.css")}</style>
        <Router>
            <Routes>
                <Route path="/" view=HomePage/>
            </Routes>
        </Router>
    }
}

pub struct UtilityAction {
    pub action: Box<dyn Fn()>,
}

#[component]
pub fn HomePage() -> impl IntoView {
    let action = UtilityAction { action: Box::new(|| {}) };
    view! { <button on:click=action.action.clone()></button> }
}
"#;

        let err = validate_app_rs_with_component_specs("demo", app, &[])
            .expect_err("expected callback pattern failure");
        assert!(err.to_string().contains("invalid callback pattern"));
    }

    #[test]
    fn app_rs_rejects_non_pub_component_props_struct_collisions() {
        let app = r#"use leptos::*;
use leptos_router::*;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <style>{include_str!("../style/app.css")}</style>
        <Router>
            <Routes>
                <Route path="/" view=HomePage/>
            </Routes>
        </Router>
    }
}

#[component]
fn HomePage() -> impl IntoView {
    view! { <div/> }
}

struct AccountCardProps {
    title: String,
}

#[component]
fn AccountCard(title: String) -> impl IntoView {
    view! { <div>{title}</div> }
}
"#;

        let err = validate_app_rs_with_component_specs("demo", app, &[])
            .expect_err("expected non-pub props collision failure");
        assert!(err
            .to_string()
            .contains("already generates that props type"));
    }

    #[test]
    fn app_rs_rejects_raw_string_variant_callsites_for_spec_defined_variants() {
        let app = r#"use leptos::*;
use leptos_router::*;

#[derive(Clone, Copy)]
enum ThemeToggleVariant {
    Default,
    Compact,
}

#[component]
pub fn App() -> impl IntoView {
    view! {
        <style>{include_str!("../style/app.css")}</style>
        <Router>
            <Routes>
                <Route path="/" view=HomePage/>
            </Routes>
        </Router>
    }
}

#[component]
pub fn HomePage() -> impl IntoView {
    view! { <ThemeToggle variant="default" /> }
}

#[component]
pub fn ThemeToggle(variant: ThemeToggleVariant) -> impl IntoView {
    view! { <div>{match variant { ThemeToggleVariant::Default => "a", ThemeToggleVariant::Compact => "b" }}</div> }
}
"#;

        let specs = vec![test_component_spec("ThemeToggle", &["default", "compact"])];

        let err = validate_app_rs_with_component_specs("demo", app, &specs)
            .expect_err("expected raw string variant failure");
        assert!(err
            .to_string()
            .contains("raw string literals for enum-backed 'ThemeToggle' variants"));
    }

    #[test]
    fn app_rs_rejects_string_variant_props_for_spec_defined_variants() {
        let app = r#"use leptos::*;
use leptos_router::*;

#[derive(Clone, Copy)]
enum AccountCardVariant {
    Default,
    PositiveBalance,
}

#[component]
pub fn App() -> impl IntoView {
    view! {
        <style>{include_str!("../style/app.css")}</style>
        <Router>
            <Routes>
                <Route path="/" view=HomePage/>
            </Routes>
        </Router>
    }
}

#[component]
pub fn HomePage() -> impl IntoView {
    view! { <AccountCard variant=AccountCardVariant::Default /> }
}

#[component]
pub fn AccountCard(#[prop(into)] variant: String) -> impl IntoView {
    view! { <div>{variant}</div> }
}
"#;

        let specs = vec![test_component_spec(
            "AccountCard",
            &["default", "positive-balance"],
        )];

        let err = validate_app_rs_with_component_specs("demo", app, &specs)
            .expect_err("expected string variant prop failure");
        assert!(err
            .to_string()
            .contains("keeps 'AccountCard' variant as String"));
    }

    #[test]
    fn app_rs_accepts_enum_backed_variants_with_dashed_spec_values() {
        let app = r#"use leptos::*;
use leptos_router::*;

#[derive(Clone, Copy)]
enum AccountCardVariant {
    Default,
    PositiveBalance,
}

#[component]
pub fn App() -> impl IntoView {
    view! {
        <style>{include_str!("../style/app.css")}</style>
        <Router>
            <Routes>
                <Route path="/" view=HomePage/>
            </Routes>
        </Router>
    }
}

#[component]
pub fn HomePage() -> impl IntoView {
    view! { <AccountCard variant=AccountCardVariant::PositiveBalance /> }
}

#[component]
pub fn AccountCard(variant: AccountCardVariant) -> impl IntoView {
    view! {
        <div>
            {match variant {
                AccountCardVariant::Default => "default",
                AccountCardVariant::PositiveBalance => "positive",
            }}
        </div>
    }
}
"#;

        let specs = vec![test_component_spec(
            "AccountCard",
            &["default", "positive-balance"],
        )];

        validate_app_rs_with_component_specs("demo", app, &specs)
            .expect("enum-backed dashed variants should validate");
    }

    #[test]
    fn app_rs_rejects_zero_arg_callbacks_wired_to_on_click() {
        let app = r#"use leptos::*;
use leptos_router::*;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <style>{include_str!("../style/app.css")}</style>
        <Router>
            <Routes>
                <Route path="/" view=HomePage/>
            </Routes>
        </Router>
    }
}

struct UtilityAction {
    action: fn(),
}

#[component]
pub fn HomePage() -> impl IntoView {
    let action = UtilityAction { action: noop };
    view! { <button on:click=action.action></button> }
}

fn noop() {}
"#;

        let err = validate_app_rs("demo", app).expect_err("expected zero-arg callback failure");
        assert!(err
            .to_string()
            .contains("zero-argument stored callbacks such as 'action'"));
    }

    #[test]
    fn app_rs_rejects_mojibake_sequences() {
        let app = r#"use leptos::*;
use leptos_router::*;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <style>{include_str!("../style/app.css")}</style>
        <Router>
            <Routes>
                <Route path="/" view=HomePage/>
            </Routes>
        </Router>
    }
}

#[component]
pub fn HomePage() -> impl IntoView {
    view! { <div>"Â© 2023 TestCompany"</div> }
}
"#;

        let err = validate_app_rs("demo", app).expect_err("expected mojibake failure");
        assert!(err.to_string().contains("suspicious mojibake sequence"));
    }

    #[test]
    fn app_rs_rejects_optional_builder_props_when_callsites_forward_option_values() {
        let app = r#"use leptos::*;
use leptos_router::*;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <style>{include_str!("../style/app.css")}</style>
        <Router>
            <Routes>
                <Route path="/" view=HomePage/>
            </Routes>
        </Router>
    }
}

#[derive(Clone)]
struct Account {
    icon: Option<String>,
}

#[component]
pub fn HomePage() -> impl IntoView {
    let account = Account { icon: None };
    view! { <AccountCard icon=account.icon.clone() /> }
}

#[component]
pub fn AccountCard(#[prop(optional)] icon: Option<String>) -> impl IntoView {
    view! { <div/> }
}
"#;

        let err = validate_app_rs("demo", app).expect_err("expected optional forwarding failure");
        assert!(err
            .to_string()
            .contains("forwards `Option<T>` values directly into optional-builder prop 'icon'"));
    }

    #[test]
    fn app_rs_rejects_optional_builder_props_for_component_data_options() {
        let app = r#"use leptos::*;
use leptos_router::*;

#[derive(Clone)]
struct BadgeData {
    label: String,
}

#[derive(Clone)]
struct AccountCardData {
    badge: Option<BadgeData>,
}

#[component]
pub fn App() -> impl IntoView {
    view! {
        <style>{include_str!("../style/app.css")}</style>
        <Router>
            <Routes>
                <Route path="/" view=HomePage/>
            </Routes>
        </Router>
    }
}

#[component]
pub fn HomePage() -> impl IntoView {
    let account = AccountCardData { badge: Some(BadgeData { label: "Primary".to_string() }) };
    view! { <AccountCard badge=account.badge.clone() /> }
}

#[component]
pub fn AccountCard(#[prop(optional)] badge: Option<BadgeData>) -> impl IntoView {
    view! { <div/> }
}
"#;

        let err = validate_app_rs("demo", app)
            .expect_err("expected optional forwarding failure for component data");
        assert!(err
            .to_string()
            .contains("forwards `Option<T>` values directly into optional-builder prop 'badge'"));
    }

    #[test]
    fn app_rs_accepts_plain_option_props_when_callsites_forward_option_values() {
        let app = r#"use leptos::*;
use leptos_router::*;

#[derive(Clone)]
struct BadgeData {
    label: String,
}

#[derive(Clone)]
struct AccountCardData {
    badge: Option<BadgeData>,
}

#[component]
pub fn App() -> impl IntoView {
    view! {
        <style>{include_str!("../style/app.css")}</style>
        <Router>
            <Routes>
                <Route path="/" view=HomePage/>
            </Routes>
        </Router>
    }
}

#[component]
pub fn HomePage() -> impl IntoView {
    let account = AccountCardData { badge: Some(BadgeData { label: "Primary".to_string() }) };
    view! { <AccountCard badge=account.badge.clone() /> }
}

#[component]
pub fn AccountCard(badge: Option<BadgeData>) -> impl IntoView {
    view! { <div/> }
}
"#;

        validate_app_rs("demo", app).expect("plain forwarded Option<T> props should be accepted");
    }

    #[test]
    fn app_rs_rejects_optional_builder_props_for_nested_option_types() {
        let app = r#"use leptos::*;
use leptos_router::*;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <style>{include_str!("../style/app.css")}</style>
        <Router>
            <Routes>
                <Route path="/" view=HomePage/>
            </Routes>
        </Router>
    }
}

#[component]
pub fn HomePage() -> impl IntoView {
    view! { <Footer legal_links=Some(vec![("Privacy".to_string(), "/privacy".to_string())]) /> }
}

#[component]
pub fn Footer(#[prop(optional)] legal_links: Option<Vec<(String, String)>>) -> impl IntoView {
    view! { <div/> }
}
"#;

        let err = validate_app_rs("demo", app)
            .expect_err("expected optional forwarding failure for nested option type");
        assert!(err
            .to_string()
            .contains("forwards `Some(...)` into optional-builder prop 'legal_links'"));
    }

    #[test]
    fn normalize_generated_app_rs_rewrites_for_view_prop_to_children() {
        let app = r#"view! {
    <For
        each=move || vec!["a".to_string()]
        key=|item| item.clone()
        view=move |item| {
            view! { <div>{item}</div> }
        }
    />
}"#;

        let normalized = normalize_generated_app_rs(app, &[]);
        assert!(normalized.contains("children=move |item| {"));
        assert!(!normalized.contains("view=move |item| {"));
    }

    #[test]
    fn normalize_generated_app_rs_rewrites_known_optional_callback_and_mojibake_patterns() {
        let app = r#"use leptos::*;

#[derive(Clone)]
#[derive(Clone)]
struct UtilityAction {
    action: Box<dyn Fn()>,
}

#[component]
fn AccountCard(
    #[prop(optional)] icon: Option<String>,
    #[prop(optional, into)] badge: Option<String>,
    #[prop(optional)] trust_badge_icon: Option<String>,
) -> impl IntoView {
    let action = UtilityAction { action: Box::new(|| {}) };
    view! {
        <div>
            <button on:click=action.action.clone()>"Â©"</button>
        </div>
    }
}"#;

        let normalized = normalize_generated_app_rs(app, &[]);
        assert!(normalized.contains("action: fn(MouseEvent),"));
        assert!(normalized.contains("action: |_| {}"));
        assert!(normalized.contains("on:click=action.action"));
        assert!(normalized.contains("\"©\""));
        assert!(!normalized.contains("Box<dyn Fn()>"));
        assert!(!normalized.contains("Â©"));
        assert_eq!(normalized.matches("#[derive(Clone)]").count(), 1);
    }

    #[test]
    fn normalize_generated_app_rs_rewrites_leptos_06_compile_traps() {
        let app = r#"use leptos::*;

#[component]
fn ThemeToggle(
    #[prop(default = "light")] selected: String,
) -> impl IntoView {
    let (theme, set_theme) = create_signal(selected);
    let options = vec!["light", "dark", "system"];
    view! {
        <div>
            <For
                each=move || options.clone()
                key=|option| option.clone()
                children=move |option| {
                    let is_selected = move || theme.get() == option;
                    view! {
                        <button on:click=move |_| set_theme.set(option.clone())>
                            {option}
                        </button>
                    }
                }
            />
        </div>
    }
}

#[component]
fn AccountCard(badge: Option<AnyView>) -> impl IntoView {
    view! { <div>{badge}</div> }
}

#[component]
fn HomePage() -> impl IntoView {
    view! { <AccountCard badge=Some(view! { <span>"Badge"</span> }.into_any()) /> }
}
"#;

        let normalized = normalize_generated_app_rs(app, &[]);
        assert!(normalized
            .contains(r#"#[prop(default = String::from("light"), into)] selected: String"#));
        assert!(normalized.contains("set_theme.set(click_value.clone())"));
        assert!(normalized.contains("badge: Option<View>"));
        assert!(normalized.contains(".into_view()"));
        assert!(!normalized.contains("AnyView"));
        assert!(!normalized.contains(".into_any()"));
    }

    #[test]
    fn app_rs_rejects_leptos_06_compile_traps_after_normalization_boundary() {
        let app = r#"use leptos::*;
use leptos_router::*;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <style>{include_str!("../style/app.css")}</style>
        <Router>
            <Routes>
                <Route path="/" view=HomePage/>
            </Routes>
        </Router>
    }
}

#[component]
fn HomePage() -> impl IntoView {
    view! { <Card badge=Some(view! { <span>"Badge"</span> }.into_any()) /> }
}

#[component]
fn Card(badge: Option<AnyView>) -> impl IntoView {
    view! { <div>{badge}</div> }
}
"#;

        let err = validate_app_rs("demo", app).expect_err("expected AnyView failure");
        assert!(err.to_string().contains("unsupported `AnyView`"));

        let app = r#"use leptos::*;
use leptos_router::*;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <style>{include_str!("../style/app.css")}</style>
        <Router>
            <Routes>
                <Route path="/" view=HomePage/>
            </Routes>
        </Router>
    }
}

#[component]
fn HomePage() -> impl IntoView {
    view! { <ThemeToggle/> }
}

#[component]
fn ThemeToggle(#[prop(default = "light")] selected: String) -> impl IntoView {
    let (_theme, set_theme) = create_signal(selected);
    let options = vec!["light"];
    view! { <button on:click=move |_| set_theme.set(option.clone())></button> }
}
"#;

        let err = validate_app_rs("demo", app).expect_err("expected string default failure");
        assert!(err.to_string().contains("bare string-literal default"));
    }

    #[test]
    fn app_rs_rejects_forbidden_leptos_class_array_syntax() {
        let app = r#"use leptos::*;
use leptos_router::*;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <style>{include_str!("../style/app.css")}</style>
        <Router>
            <Routes>
                <Route path="/" view=HomePage/>
            </Routes>
        </Router>
    }
}

#[component]
fn HomePage() -> impl IntoView {
    let variant_class = "badge--neutral";
    view! { <span class=["badge", variant_class]>"Neutral"</span> }
}
"#;

        let err = validate_app_rs("demo", app).expect_err("expected class array failure");
        assert!(err.to_string().contains("forbidden Leptos class syntax"));
        assert!(err.to_string().contains("`class=[...]`"));
    }

    #[test]
    fn app_rs_rejects_forbidden_leptos_class_directive_syntax() {
        let app = r#"use leptos::*;
use leptos_router::*;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <style>{include_str!("../style/app.css")}</style>
        <Router>
            <Routes>
                <Route path="/" view=HomePage/>
            </Routes>
        </Router>
    }
}

#[component]
fn HomePage() -> impl IntoView {
    let is_active = true;
    view! { <button class:active=is_active>"Active"</button> }
}
"#;

        let err = validate_app_rs("demo", app).expect_err("expected class directive failure");
        assert!(err.to_string().contains("`class:...`"));
    }

    #[test]
    fn generated_brand_rust_patterns_reject_forbidden_classes_macro_syntax() {
        let files = vec![GeneratedOutputFile {
            path: PathBuf::from("src/components/button.rs"),
            content: r#"use leptos::*;

#[component]
fn Button() -> impl IntoView {
    view! { <button class=classes!("button", ("button--full-width", true))></button> }
}
"#
            .to_string(),
        }];

        let err = validate_generated_brand_rust_patterns("demo", &files)
            .expect_err("expected classes macro failure");
        assert!(err.to_string().contains("src/components/button.rs"));
        assert!(err.to_string().contains("`classes!(...)`"));
    }

    #[test]
    fn app_rs_accepts_precomputed_class_string_bindings() {
        let app = r#"use leptos::*;
use leptos_router::*;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <style>{include_str!("../style/app.css")}</style>
        <Router>
            <Routes>
                <Route path="/" view=HomePage/>
            </Routes>
        </Router>
    }
}

#[component]
fn HomePage() -> impl IntoView {
    let variant_class = "button--primary";
    let mut button_class = format!("button {}", variant_class);
    let full_width = true;
    if full_width {
        button_class.push_str(" button--full-width");
    }
    view! { <button class=button_class>"Primary"</button> }
}
"#;

        validate_app_rs("demo", app).expect("precomputed class strings should be accepted");
    }

    #[test]
    fn normalize_generated_app_rs_rewrites_moved_for_item_and_if_else_views() {
        let app = r#"use leptos::*;

#[component]
fn ThemeToggle() -> impl IntoView {
    let (current_theme, set_current_theme) = create_signal("light".to_string());
    let options = vec!["light".to_string(), "dark".to_string()];
    let disabled = false;
    view! {
        <For each=move || options.clone() key=|option| option.clone() children=move |option| {
            let is_selected = move || current_theme.get() == option;
            let option_class = move || if is_selected() { "selected" } else { "" };
            view! {
                <button class=option_class disabled=disabled on:click=move |_| set_current_theme.set(option.clone())>
                    {option}
                </button>
            }
        } />
    }
}

#[component]
fn AccountsDashboard(accounts: Vec<String>, loading: bool) -> impl IntoView {
    view! {
        <section>
            {if accounts.is_empty() && !loading {
                view! { <div class="accounts-dashboard__empty">No accounts available</div> }
            } else {
                view! {
                    <For each=move || accounts.clone() key=|account| account.clone() children=move |account| {
                        view! { <div>{account}</div> }
                    } />
                }
            }}
        </section>
    }
}
"#;

        let normalized = normalize_generated_app_rs(app, &[]);
        assert!(normalized.contains("let selected_value = option.clone();"));
        assert!(normalized.contains("let click_value = option.clone();"));
        assert!(
            normalized.contains("let is_selected = move || current_theme.get() == selected_value;")
        );
        assert!(normalized.contains("set_current_theme.set(click_value.clone())"));
        assert!(normalized.contains(
            r#"view! { <div class="accounts-dashboard__empty">No accounts available</div> }.into_view()"#
        ));
        assert!(normalized.contains("} />\n                }.into_view()"));
    }

    #[test]
    fn normalize_generated_brand_files_adds_missing_gitignore() {
        let files = vec![GeneratedOutputFile {
            path: PathBuf::from("src/app.rs"),
            content: "pub fn app() {}".to_string(),
        }];

        let normalized = normalize_generated_brand_files(files, &[]);
        let gitignore = normalized
            .iter()
            .find(|file| file.path == PathBuf::from(".gitignore"))
            .expect("gitignore should be synthesized");

        assert!(gitignore.content.contains("target/"));
        assert!(gitignore.content.contains(".cargo-leptos/"));
    }

    #[test]
    fn normalize_generated_app_rs_renames_manual_component_props_helpers() {
        let app = r#"use leptos::*;

#[derive(Clone)]
struct AccountCardProps {
    title: String,
}

#[component]
fn AccountCard(#[prop(into)] title: String) -> impl IntoView {
    view! { <div>{title}</div> }
}

#[component]
fn HomePage() -> impl IntoView {
    let account = AccountCardProps { title: "Primary".to_string() };
    view! { <AccountCard title=account.title.clone() /> }
}"#;

        let normalized = normalize_generated_app_rs(app, &[]);
        assert!(normalized.contains("struct AccountCardData"));
        assert!(normalized.contains("let account = AccountCardData"));
        assert!(!normalized.contains("AccountCardProps"));
    }

    #[test]
    fn normalize_generated_app_rs_renames_manual_component_props_helpers_without_colliding() {
        let app = r#"use leptos::*;

#[derive(Clone)]
struct AccountCardData {
    title: String,
}

#[derive(Clone)]
struct AccountCardProps {
    title: String,
}

#[component]
fn AccountCard(#[prop(into)] title: String) -> impl IntoView {
    view! { <div>{title}</div> }
}

#[component]
fn HomePage() -> impl IntoView {
    let account = AccountCardProps { title: "Primary".to_string() };
    view! { <AccountCard title=account.title.clone() /> }
}"#;

        let normalized =
            normalize_generated_app_rs(app, &[test_component_spec("Badge", &["neutral"])]);
        assert!(normalized.contains("struct AccountCardData"));
        assert!(normalized.contains("struct AccountCardModel"));
        assert!(normalized.contains("let account = AccountCardModel"));
        assert!(!normalized.contains("AccountCardProps"));
    }

    #[test]
    fn normalize_generated_app_rs_expands_component_spread_props() {
        let app = r#"use leptos::*;

#[derive(Clone)]
struct AccountCardData {
    title: String,
    interactive: bool,
}

#[component]
fn AccountCard(
    #[prop(into)] title: String,
    #[prop(default = false)] interactive: bool,
) -> impl IntoView {
    view! { <div>{title}</div> }
}

#[component]
fn HomePage() -> impl IntoView {
    let account = AccountCardData {
        title: "Primary".to_string(),
        interactive: true,
    };
    view! { <AccountCard ..account /> }
}"#;

        let normalized = normalize_generated_app_rs(app, &[]);
        assert!(
            normalized.contains(
                "<AccountCard title=account.title.clone() interactive=account.interactive />"
            ),
            "{}",
            normalized
        );
        assert!(!normalized.contains("<AccountCard ..account />"));
    }

    #[test]
    fn normalize_generated_app_rs_synthesizes_component_data_helpers() {
        let app = r#"use leptos::*;

#[derive(Clone, Copy)]
pub enum BadgeVariant {
    Neutral,
}

#[component]
fn Badge(
    #[prop(into)] label: String,
    #[prop(default = BadgeVariant::Neutral)] variant: BadgeVariant,
) -> impl IntoView {
    view! { <span>{label}</span> }
}

#[component]
fn AccountCard(
    #[prop(into)] title: String,
    #[prop(optional)] badge: Option<Badge>,
) -> impl IntoView {
    view! { <div>{badge}</div> }
}

#[component]
fn App() -> impl IntoView {
    let accounts = vec![AccountCard {
        title: "Primary".to_string(),
        badge: Some(Badge {
            label: "Primary".to_string(),
            variant: BadgeVariant::Neutral,
        }),
    }];
    view! { <div>{accounts}</div> }
}"#;

        let normalized = normalize_generated_app_rs(app, &[]);
        assert!(normalized.contains("pub struct BadgeData"));
        assert!(normalized.contains("pub struct AccountCardData"));
        assert!(normalized.contains("impl IntoView for BadgeData"));
        assert!(normalized.contains("impl IntoView for AccountCardData"));
        assert!(normalized.contains("badge: Option<BadgeData>,"));
        assert!(normalized.contains("let accounts = vec![AccountCardData {"));
        assert!(normalized.contains("badge: Some(BadgeData {"));
        assert!(!normalized.contains("Some(Badge {"));
    }

    #[test]
    fn normalize_generated_app_rs_rewrites_forwarded_option_props_generically() {
        let app = r#"use leptos::*;

#[derive(Clone)]
struct BadgeData {
    label: String,
}

#[derive(Clone)]
struct AccountCardData {
    badge: Option<BadgeData>,
}

#[component]
fn HomePage() -> impl IntoView {
    let account = AccountCardData { badge: Some(BadgeData { label: "Primary".to_string() }) };
    view! { <AccountCard badge=account.badge.clone() label=None helper=Some("ok".to_string()) /> }
}

#[component]
fn AccountCard(
    #[prop(optional)] badge: Option<BadgeData>,
    #[prop(optional, into)] label: Option<String>,
    #[prop(optional, into)] helper: Option<String>,
    #[prop(optional)] omitted: Option<bool>,
) -> impl IntoView {
    view! { <div/> }
}"#;

        let normalized = normalize_generated_app_rs(app, &[]);
        assert!(normalized.contains("badge: Option<BadgeData>,"));
        assert!(normalized.contains("label: Option<String>,"));
        assert!(normalized.contains("helper: Option<String>,"));
        assert!(normalized.contains("#[prop(optional)] omitted: Option<bool>,"));
        assert!(!normalized.contains("#[prop(optional)] badge: Option<BadgeData>,"));
        assert!(!normalized.contains("#[prop(optional, into)] label: Option<String>,"));
        assert!(!normalized.contains("#[prop(optional, into)] helper: Option<String>,"));
    }

    #[test]
    fn normalize_generated_app_rs_rewrites_nested_option_types_generically() {
        let app = r#"use leptos::*;

#[component]
fn HomePage() -> impl IntoView {
    view! { <Footer legal_links=Some(vec![("Privacy".to_string(), "/privacy".to_string())]) /> }
}

#[component]
fn Footer(
    #[prop(optional)] legal_links: Option<Vec<(String, String)>>,
    #[prop(optional)] omitted: Option<bool>,
) -> impl IntoView {
    view! { <div/> }
}"#;

        let normalized = normalize_generated_app_rs(app, &[]);
        assert!(normalized.contains("legal_links: Option<Vec<(String, String)>>,"));
        assert!(normalized.contains("#[prop(optional)] omitted: Option<bool>,"));
        assert!(
            !normalized.contains("#[prop(optional)] legal_links: Option<Vec<(String, String)>>,")
        );
    }

    #[test]
    fn normalize_generated_app_rs_upgrades_string_variants_to_deterministic_enums() {
        let app = r#"use leptos::*;

#[component]
fn Badge(
    #[prop(into)] label: String,
    #[prop(into, default = "neutral".to_string())] variant: String,
) -> impl IntoView {
    let variant_class = match variant.as_str() {
        "neutral" => "badge--neutral",
        "success" => "badge--success",
        _ => "badge--neutral",
    };

    view! { <span class=variant_class>{label}</span> }
}

#[derive(Clone)]
struct BadgeData {
    label: String,
    variant: String,
}

#[component]
fn HomePage() -> impl IntoView {
    let badge = BadgeData {
        label: "Primary".to_string(),
        variant: "success".to_string(),
    };
    view! { <Badge label="Hi" variant="neutral" /> {badge} }
}"#;

        let normalized = normalize_generated_app_rs(
            app,
            &[test_component_spec("Badge", &["neutral", "success"])],
        );
        assert!(normalized.contains("pub enum BadgeVariant"));
        assert!(
            normalized.contains("#[prop(default = BadgeVariant::Neutral)]"),
            "{}",
            normalized
        );
        assert!(normalized.contains("variant: BadgeVariant,"));
        assert!(normalized.contains("match variant {"));
        assert!(normalized.contains("BadgeVariant::Neutral =>"));
        assert!(normalized.contains("BadgeVariant::Success =>"));
        assert!(normalized.contains(r#"variant=BadgeVariant::Neutral"#));
        assert!(normalized.contains("variant: BadgeVariant::Success,"));
    }
}
