//! Scaffold code generation from v2 prepared YAML.
//!
//! Emits `.rs` files with exact field sets and method signatures derived from
//! the prepared artifact. All method bodies are `todo!()`.

use crate::prepared::{ArtifactKind, PreparedArtifact};
use crate::workspace::{GENERATED_MANIFEST, Workspace};
use anyhow::{Context, Result};
use std::fmt::Write as FmtWrite;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ScaffoldOptions {
    pub selection: crate::workspace::Selection,
    pub verbose: bool,
    pub debug: bool,
    pub dry_run: bool,
}

/// Generate Rust stub files from prepared v2 YAML artifacts.
pub fn scaffold_workspace(workspace: &Workspace, options: &ScaffoldOptions) -> Result<()> {
    let prepared_paths = workspace.prepared_paths(&options.selection)?;
    let mut generated_files: Vec<String> = Vec::new();
    let mut wrote = 0usize;

    for path in &prepared_paths {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let artifact: PreparedArtifact = serde_yaml::from_str(&raw)
            .with_context(|| format!("Failed to parse {}", path.display()))?;

        if artifact.blocking_ambiguities().next().is_some() {
            anyhow::bail!(
                "Cannot scaffold {} — it has blocking ambiguities. Run `reen prepare` and resolve them first.",
                path.display()
            );
        }

        let code = generate_rust(&artifact);
        let output_path = scaffold_output_path(workspace, &artifact);

        if options.dry_run {
            if options.verbose {
                println!("[dry-run] would write {}", output_path.display());
            }
        } else {
            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create {}", parent.display()))?;
            }
            fs::write(&output_path, &code)
                .with_context(|| format!("Failed to write {}", output_path.display()))?;
            let relative = output_path
                .strip_prefix(&workspace.root)
                .unwrap_or(&output_path)
                .to_string_lossy()
                .to_string();
            generated_files.push(relative);
            wrote += 1;
        }
    }

    if !options.dry_run && !generated_files.is_empty() {
        let manifest_path = workspace.root.join(GENERATED_MANIFEST);
        if let Some(parent) = manifest_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&generated_files)?;
        fs::write(&manifest_path, json)?;
    }

    if options.verbose {
        println!("Scaffolded {} file(s)", wrote);
    }
    Ok(())
}

/// Remove generated Rust outputs tracked in `.reen/generated_files.json`.
pub fn clear_generated_outputs(workspace: &Workspace, dry_run: bool) -> Result<()> {
    let manifest_path = workspace.root.join(GENERATED_MANIFEST);
    if !manifest_path.is_file() {
        return Ok(());
    }
    let raw = fs::read_to_string(&manifest_path)
        .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
    let files: Vec<String> = serde_json::from_str(&raw)
        .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;
    for file in &files {
        let path = workspace.root.join(file);
        if path.is_file() {
            if dry_run {
                println!("[dry-run] would remove {}", path.display());
            } else {
                fs::remove_file(&path)
                    .with_context(|| format!("Failed to remove {}", path.display()))?;
            }
        }
    }
    if !dry_run {
        fs::remove_file(&manifest_path).ok();
    }
    Ok(())
}

// ── Code generation ───────────────────────────────────────────────

fn scaffold_output_path(workspace: &Workspace, artifact: &PreparedArtifact) -> PathBuf {
    let subdir = match artifact.kind {
        ArtifactKind::Data => "data",
        ArtifactKind::Projection => "projections",
        ArtifactKind::Context => "contexts",
        ArtifactKind::Operation => "apis", // handled separately by apis_codegen
        // Phase 10 will add scaffold paths for web kinds
        ArtifactKind::Component => "components",
        ArtifactKind::Brand => "brand",
        ArtifactKind::RoutingTable => "routes",
        ArtifactKind::App => return workspace.root.join("src").join("main.rs"),
    };
    let filename = to_snake_case(&artifact.export.name);
    workspace
        .root
        .join("src")
        .join(subdir)
        .join(format!("{}.rs", filename))
}

pub fn generate_rust(artifact: &PreparedArtifact) -> String {
    match artifact.kind {
        ArtifactKind::Context => generate_context(artifact),
        ArtifactKind::Data => generate_data(artifact),
        ArtifactKind::Projection => generate_projection(artifact),
        ArtifactKind::Operation => String::new(), // operations use apis_codegen
        // Phase 10 will add codegen for web kinds
        ArtifactKind::Component | ArtifactKind::Brand | ArtifactKind::RoutingTable => {
            String::new()
        }
        // App codegen is handled separately via generate_app_main
        ArtifactKind::App => String::new(),
    }
}

fn generate_context(a: &PreparedArtifact) -> String {
    let name = &a.export.name;
    let mut out = String::new();

    // Struct
    writeln!(out, "pub struct {} {{", name).unwrap();
    for rp in &a.role_players {
        let rust_type = sanitize_rust_type(&rp.data_type);
        if rust_type == "()" {
            writeln!(out, "    {}: (), // {}", to_snake_case(&rp.name), rp.data_type).unwrap();
        } else {
            writeln!(out, "    {}: {},", to_snake_case(&rp.name), rust_type).unwrap();
        }
    }
    for prop in &a.props {
        // Props are scalar config — we don't know the type, so use a placeholder
        writeln!(out, "    {}: (), // prop: {}", to_snake_case(&prop.name), prop.description).unwrap();
    }
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    // Impl
    writeln!(out, "impl {} {{", name).unwrap();

    // Public methods = functionalities (default to &self; implementer upgrades to &mut self)
    for func in &a.functionalities {
        writeln!(out, "    /// {}", func.description.lines().next().unwrap_or("")).unwrap();
        writeln!(out, "    pub fn {}(&self) {{", to_snake_case(&func.name)).unwrap();
        writeln!(out, "        todo!()").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out).unwrap();
    }

    // Private methods = role methods (canonical names)
    for group in &a.role_methods {
        for rm in &group.methods {
            writeln!(out, "    /// Role method ({}.{}): {}", group.role, rm.name, rm.citation).unwrap();
            writeln!(
                out,
                "    fn {}(&self, {}_: &{}) {{",
                rm.canonical_name,
                to_snake_case(&group.role),
                role_player_type(a, &group.role)
            )
            .unwrap();
            writeln!(out, "        todo!()").unwrap();
            writeln!(out, "    }}").unwrap();
            writeln!(out).unwrap();
        }
    }

    writeln!(out, "}}").unwrap();
    out
}

fn generate_data(a: &PreparedArtifact) -> String {
    let name = &a.export.name;
    let mut out = String::new();

    // Struct
    writeln!(out, "pub struct {} {{", name).unwrap();
    for attr in &a.attributes {
        writeln!(out, "    pub {}: (), // {}", to_snake_case(&attr.name), attr.description).unwrap();
    }
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    // Impl with operations
    if !a.operations.is_empty() {
        writeln!(out, "impl {} {{", name).unwrap();
        for op in &a.operations {
            writeln!(out, "    /// {}", op.description.lines().next().unwrap_or("")).unwrap();
            writeln!(out, "    pub fn {}(&self) {{", to_snake_case(&op.name)).unwrap();
            writeln!(out, "        todo!()").unwrap();
            writeln!(out, "    }}").unwrap();
            writeln!(out).unwrap();
        }
        writeln!(out, "}}").unwrap();
    }

    out
}

fn generate_projection(a: &PreparedArtifact) -> String {
    let name = &a.export.name;
    let mut out = String::new();

    // Struct
    writeln!(out, "pub struct {} {{", name).unwrap();
    if let Some(src) = &a.source_data {
        let rust_type = sanitize_rust_type(src);
        if rust_type == "()" {
            writeln!(out, "    source: (), // {}", src).unwrap();
        } else {
            writeln!(out, "    source: {}, // projection over this data", rust_type).unwrap();
        }
    }
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    // Impl with queries
    if !a.queries.is_empty() {
        writeln!(out, "impl {} {{", name).unwrap();
        for query in &a.queries {
            writeln!(out, "    /// {}", query.description.lines().next().unwrap_or("")).unwrap();
            writeln!(out, "    pub fn {}(&self) {{", to_snake_case(&query.name)).unwrap();
            writeln!(out, "        todo!()").unwrap();
            writeln!(out, "    }}").unwrap();
            writeln!(out).unwrap();
        }
        writeln!(out, "}}").unwrap();
    }

    out
}

// ── Helpers ───────────────────────────────────────────────────────

fn role_player_type<'a>(artifact: &'a PreparedArtifact, role_name: &str) -> &'a str {
    artifact
        .role_players
        .iter()
        .find(|rp| rp.name == role_name)
        .map(|rp| sanitize_rust_type(&rp.data_type))
        .unwrap_or("()")
}

/// Return the data_type as-is if it looks like a valid Rust type identifier
/// (PascalCase name or path like `rand::rngs::ThreadRng`), otherwise return `()`.
fn sanitize_rust_type(data_type: &str) -> &str {
    let trimmed = data_type.trim();
    if trimmed.is_empty() {
        return "()";
    }
    // A valid Rust type is either a simple identifier (ascii-alphanumeric + _),
    // a path with `::`, or a generic with `<>`. If it contains spaces,
    // `@` (API refs), or `/` it's not a Rust type.
    if trimmed.contains(' ') || trimmed.contains('@') || trimmed.contains('/') {
        return "()";
    }
    trimmed
}

pub fn to_snake_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    let mut prev_was_lower = false;
    for ch in s.chars() {
        if ch.is_ascii_uppercase() {
            if prev_was_lower {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
            prev_was_lower = false;
        } else if ch == ' ' || ch == '-' {
            out.push('_');
            prev_was_lower = false;
        } else {
            out.push(ch);
            prev_was_lower = ch.is_ascii_lowercase();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prepared::*;

    fn test_context() -> PreparedArtifact {
        let mut a = PreparedArtifact::new(
            ArtifactKind::Context,
            "contexts/money_transfer.md".into(),
            "MoneyTransfer".into(),
            "MoneyTransfer".into(),
        );
        a.role_players = vec![
            RolePlayerSpec { name: "source".into(), data_type: "Account".into(), responsibility: "withdraw from".into() },
            RolePlayerSpec { name: "sink".into(), data_type: "Account".into(), responsibility: "deposit to".into() },
        ];
        a.props = vec![
            PropSpec { name: "amount".into(), description: "transfer amount".into() },
        ];
        a.functionalities = vec![
            FunctionalitySpec { name: "transfer".into(), description: "Execute the transfer.".into(), role_method_refs: vec!["source_withdraw".into(), "sink_deposit".into()] },
        ];
        a.role_methods = vec![
            RoleMethodGroupSpec {
                role: "source".into(),
                methods: vec![RoleMethodSpec {
                    name: "withdraw".into(),
                    canonical_name: "source_withdraw".into(),
                    signature_hint: None,
                    citation: "removes the transfer amount".into(),
                }],
            },
            RoleMethodGroupSpec {
                role: "sink".into(),
                methods: vec![RoleMethodSpec {
                    name: "deposit".into(),
                    canonical_name: "sink_deposit".into(),
                    signature_hint: None,
                    citation: "adds the transfer amount".into(),
                }],
            },
        ];
        a
    }

    #[test]
    fn scaffold_context_has_struct_fields_and_methods() {
        let code = generate_rust(&test_context());
        assert!(code.contains("pub struct MoneyTransfer"), "struct name");
        assert!(code.contains("source: Account"), "role player field");
        assert!(code.contains("sink: Account"), "role player field");
        assert!(code.contains("amount: ()"), "prop field");
        assert!(code.contains("pub fn transfer("), "functionality method");
        assert!(code.contains("fn source_withdraw("), "role method canonical");
        assert!(code.contains("fn sink_deposit("), "role method canonical");
        assert!(code.contains("source_: &Account"), "role player param");
    }

    #[test]
    fn scaffold_data_has_attributes_and_operations() {
        let mut a = PreparedArtifact::new(
            ArtifactKind::Data, "data/Amount.md".into(), "Amount".into(), "Amount".into(),
        );
        a.attributes = vec![AttributeSpec { name: "value".into(), description: "cents".into() }];
        a.operations = vec![OperationSpec { name: "add".into(), description: "Add two amounts.".into() }];

        let code = generate_rust(&a);
        assert!(code.contains("pub struct Amount"));
        assert!(code.contains("pub value: ()"));
        assert!(code.contains("pub fn add("));
    }

    #[test]
    fn scaffold_projection_has_source_and_queries() {
        let mut a = PreparedArtifact::new(
            ArtifactKind::Projection, "projections/balance.md".into(), "AccountBalance".into(), "AccountBalance".into(),
        );
        a.source_data = Some("Account".into());
        a.queries = vec![QuerySpec { name: "current_balance".into(), description: "Current balance.".into() }];

        let code = generate_rust(&a);
        assert!(code.contains("pub struct AccountBalance"));
        assert!(code.contains("source: Account"));
        assert!(code.contains("pub fn current_balance("));
    }

    #[test]
    fn prose_source_data_falls_back_to_unit() {
        let mut a = PreparedArtifact::new(
            ArtifactKind::Projection, "projections/account.md".into(), "Account".into(), "Account".into(),
        );
        a.source_data = Some("Ledger, filtered by account identifier".into());
        a.queries = vec![QuerySpec { name: "balance".into(), description: "Current balance.".into() }];

        let code = generate_rust(&a);
        assert!(
            !code.contains("source: Ledger, filtered"),
            "prose source_data should not appear as Rust struct field type, got:\n{}",
            code
        );
        assert!(
            code.contains("source: ()"),
            "prose source_data should fall back to () for struct field, got:\n{}",
            code
        );
    }

    #[test]
    fn to_snake_case_works() {
        assert_eq!(to_snake_case("MoneyTransfer"), "money_transfer");
        assert_eq!(to_snake_case("Account"), "account");
        assert_eq!(to_snake_case("current_balance"), "current_balance");
    }

    #[test]
    fn functionality_receiver_defaults_to_immutable() {
        let code = generate_rust(&test_context());
        // Functionalities should use &self by default, not &mut self
        assert!(
            code.contains("pub fn transfer(&self)"),
            "functionality should default to &self, got:\n{}",
            code
        );
    }

    #[test]
    fn prose_data_type_falls_back_to_unit() {
        let mut a = PreparedArtifact::new(
            ArtifactKind::Context,
            "contexts/game.md".into(),
            "Game".into(),
            "Game".into(),
        );
        a.role_players = vec![
            RolePlayerSpec {
                name: "rng".into(),
                data_type: "a random number generator".into(),
                responsibility: "picks random positions".into(),
            },
        ];
        a.role_methods = vec![RoleMethodGroupSpec {
            role: "rng".into(),
            methods: vec![RoleMethodSpec {
                name: "pick".into(),
                canonical_name: "rng_pick".into(),
                signature_hint: None,
                citation: "picks a position".into(),
            }],
        }];
        a.functionalities = vec![FunctionalitySpec {
            name: "play".into(),
            description: "Play the game.".into(),
            role_method_refs: vec![],
        }];

        let code = generate_rust(&a);
        // Prose data_type with spaces must not appear as a raw Rust type in
        // struct fields or method parameters. It may appear in comments.
        assert!(
            !code.contains("rng: a random number generator"),
            "prose data_type should not appear as Rust struct field type, got:\n{}",
            code
        );
        assert!(
            code.contains("rng: ()"),
            "prose data_type should fall back to () for struct field, got:\n{}",
            code
        );
        assert!(
            code.contains("rng_: &()"),
            "prose data_type should fall back to &() for role method param, got:\n{}",
            code
        );
    }
}
