use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use reen::codegen::{ScaffoldOptions, clear_generated_outputs, scaffold_workspace};
use reen::manifest::{CapabilityProviderInput, add_capability_provider, add_types_prefix};
use reen::prepare::{
    PrepareOptions, RefineOptions, clear_prepared_outputs, prepare_workspace, refine_workspace,
};
use reen::verify;
use reen::workspace::{CommandConfig, RefineConfig, ReenConfig, Selection, Workspace};

#[derive(Parser)]
#[command(name = "reen")]
#[command(about = "DCI-native spec compiler and verifier")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(long, global = true, help = "Enable verbose progress output")]
    verbose: bool,

    #[arg(
        long,
        global = true,
        help = "Write optional debug artifacts under .reen/debug"
    )]
    debug: bool,

    #[arg(long, global = true, help = "Show actions without writing files")]
    dry_run: bool,
}

#[derive(Debug, Clone, Copy)]
struct GlobalCliOptions {
    verbose: bool,
    debug: bool,
    dry_run: bool,
}

#[derive(Subcommand)]
enum Commands {
    #[command(
        about = "Persist defaults into reen.yml (root or per-command). Never executes the wrapped command."
    )]
    Config(ConfigArgs),

    #[command(about = "Prepare per-draft YAML artifacts under drafts/prepare")]
    Prepare(PrepareArgs),

    #[command(
        about = "Review raw drafts for completeness/concreteness/ambiguity"
    )]
    Refine(RefineArgs),

    #[command(about = "Generate Rust source files from prepared artifacts")]
    Scaffold(ScaffoldArgs),

    #[command(about = "Verify Rust source against prepared specification")]
    Verify(VerifyArgs),

    #[command(subcommand, about = "Edit Reen manifest files under drafts/")]
    Manifest(ManifestCommand),

    #[command(
        subcommand,
        about = "Clear prepared artifacts, generated outputs, or both"
    )]
    Clear(ClearCommand),

    #[command(subcommand, about = "External API ingestion and management")]
    Apis(ApisCommand),

    #[command(about = "Remove tracker, prepared YAML, and all of src/")]
    Clean(CleanArgs),

    #[command(about = "Install or upgrade the Reen bundle in a project")]
    Init(InitArgs),

    #[command(subcommand, about = "Audit-loop persistence: record findings and flip tracker entries")]
    Audit(AuditCommand),
}

#[derive(Subcommand, Clone)]
enum AuditCommand {
    #[command(
        about = "Record audit findings: write .reen/audit.json, render AUDIT.md, flip tracker entries above the severity threshold"
    )]
    Record(AuditRecordArgs),
}

#[derive(Args, Clone)]
struct AuditRecordArgs {
    #[arg(
        long,
        help = "Read findings JSON from this file. Pass `-` (or omit) to read from stdin."
    )]
    from: Option<String>,
    #[arg(
        long,
        default_value = "medium",
        help = "Tracker-flip severity threshold (low, medium, high). Findings below this severity are persisted but do not flip tracker entries."
    )]
    severity_threshold: String,
}

#[derive(Args, Clone)]
struct CleanArgs {
    #[arg(long, help = "Skip the confirmation prompt")]
    force: bool,
}

#[derive(Args, Default, Clone)]
struct ConfigArgs {
    #[arg(long, help = "Persist `contexts: true` at the root of reen.yml")]
    contexts: bool,

    #[arg(long, help = "Persist `projections: true` at the root of reen.yml")]
    projections: bool,

    #[arg(long, help = "Persist `data: true` at the root of reen.yml")]
    data: bool,

    #[arg(long, help = "Persist `profile: ...` at the root of reen.yml")]
    profile: Option<String>,

    #[command(subcommand)]
    command: Option<ConfigCommand>,
}

/// Per-command persistence targets.
#[derive(Subcommand, Clone)]
enum ConfigCommand {
    #[command(about = "Persist defaults under `prepare:` in reen.yml")]
    Prepare(PrepareConfigArgs),
    #[command(about = "Persist defaults under `refine:` in reen.yml")]
    Refine(RefineConfigArgs),
    #[command(about = "Persist defaults under `scaffold:` in reen.yml")]
    Scaffold(ScaffoldConfigArgs),
    #[command(about = "Persist global flags under `clear:` in reen.yml")]
    Clear,
    #[command(about = "Persist global flags under `manifest:` in reen.yml")]
    Manifest,
}

#[derive(Args, Clone, Default)]
struct PrepareConfigArgs {
    #[arg(long)]
    contexts: bool,
    #[arg(long)]
    projections: bool,
    #[arg(long)]
    data: bool,
    #[arg(long)]
    profile: Option<String>,
}

#[derive(Args, Clone, Default)]
struct RefineConfigArgs {
    #[arg(long)]
    contexts: bool,
    #[arg(long)]
    projections: bool,
    #[arg(long)]
    data: bool,
    #[arg(long)]
    profile: Option<String>,
    #[arg(long, conflicts_with = "prepared_only")]
    drafts_only: bool,
    #[arg(long, conflicts_with = "drafts_only")]
    prepared_only: bool,
}

#[derive(Args, Clone, Default)]
struct ScaffoldConfigArgs {
    #[arg(long)]
    contexts: bool,
    #[arg(long)]
    projections: bool,
    #[arg(long)]
    data: bool,
}

#[derive(Args, Clone)]
struct PrepareArgs {
    #[arg(long, help = "Only process drafts from drafts/contexts/")]
    contexts: bool,

    #[arg(long, help = "Only process drafts from drafts/projections/")]
    projections: bool,

    #[arg(long, help = "Only process drafts from drafts/data/")]
    data: bool,

    #[arg(long, help = "Reserved for future prepare profiles")]
    profile: Option<String>,

    #[arg(
        long,
        help = "Downgrade code-token lint from blocking error to visible warning"
    )]
    allow_code_tokens: bool,

    #[arg(help = "Optional list of draft names without file extension")]
    names: Vec<String>,
}

#[derive(Args, Clone)]
struct RefineArgs {
    #[arg(long, help = "Only process context drafts/artifacts")]
    contexts: bool,

    #[arg(long, help = "Only process projection drafts/artifacts")]
    projections: bool,

    #[arg(long, help = "Only process data drafts/artifacts")]
    data: bool,

    #[arg(
        long,
        conflicts_with = "prepared_only",
        help = "Run only the raw-draft review phase"
    )]
    drafts_only: bool,

    #[arg(
        long,
        conflicts_with = "drafts_only",
        help = "Run only the prepared-artifact review phase"
    )]
    prepared_only: bool,

    #[arg(help = "Optional list of draft/artifact names without file extension")]
    names: Vec<String>,
}

#[derive(Args, Clone)]
struct ScaffoldArgs {
    #[arg(long, help = "Only process prepared context artifacts")]
    contexts: bool,

    #[arg(long, help = "Only process prepared projection artifacts")]
    projections: bool,

    #[arg(long, help = "Only process prepared data artifacts")]
    data: bool,

    #[arg(long, help = "Skip client WASM crate for server-only web projects")]
    no_client: bool,

    #[arg(help = "Optional list of prepared artifact names without file extension")]
    names: Vec<String>,
}

#[derive(Args, Clone)]
struct VerifyArgs {
    #[arg(help = "Path to the Rust source file to verify")]
    path: String,

    #[arg(long, help = "Path to the prepared YAML artifact (auto-detected if omitted)")]
    prepared: Option<String>,
}

#[derive(Subcommand, Clone)]
enum ClearCommand {
    #[command(about = "Remove drafts/prepare and prepare-stage tracker state")]
    Prepared,

    #[command(about = "Remove generated Rust outputs tracked under .reen/generated_files.json")]
    Generated,

    #[command(about = "Remove prepared and generated outputs")]
    All,
}

#[derive(Args, Clone)]
struct InitArgs {
    #[arg(long, help = "Install into this directory instead of the current directory")]
    path: Option<String>,

    #[arg(long, help = "Refresh managed files to the current bundle version")]
    upgrade: bool,

    #[arg(long, help = "Dry-run report of what --upgrade would do")]
    check: bool,

    #[arg(long, help = "Overwrite user-edited managed files on --upgrade")]
    force: bool,
}

#[derive(Subcommand, Clone)]
enum ApisCommand {
    #[command(about = "Ingest OpenAPI snapshots into generated data drafts and operation specs")]
    Ingest {
        #[arg(help = "API name (matches drafts/apis/<name>.md); omit to ingest all")]
        api_name: Option<String>,
    },
}

#[derive(Subcommand, Clone)]
enum ManifestCommand {
    #[command(subcommand)]
    Types(TypesManifestCommand),

    #[command(subcommand)]
    Capabilities(CapabilitiesManifestCommand),
}

#[derive(Subcommand, Clone)]
enum TypesManifestCommand {
    #[command(about = "Add an allowed external path prefix to drafts/types-manifest.yml")]
    AddPrefix {
        #[arg(help = "External Rust path prefix, for example `rand::`")]
        prefix: String,
    },
}

#[derive(Subcommand, Clone)]
enum CapabilitiesManifestCommand {
    #[command(about = "Add a capability domain and allow its crate namespace under drafts/")]
    Add {
        #[arg(help = "Capability domain, for example `randomness`")]
        domain: String,

        #[arg(help = "Rust crate name, for example `rand`")]
        crate_name: String,

        #[arg(long, help = "Disable default crate features")]
        no_default_features: bool,

        #[arg(long, help = "Additional crate feature to enable", action = clap::ArgAction::Append)]
        feature: Vec<String>,

        #[arg(
            long,
            help = "Allowed external Rust path prefix. Defaults to `<crate_name>::` if omitted",
            action = clap::ArgAction::Append
        )]
        prefix: Vec<String>,

        #[arg(
            long,
            help = "Additional capability id. Defaults to the domain itself",
            action = clap::ArgAction::Append
        )]
        capability: Vec<String>,
    },
}

fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();
    let globals = GlobalCliOptions {
        verbose: cli.verbose,
        debug: cli.debug,
        dry_run: cli.dry_run,
    };

    // Handle `init` before workspace discovery — it may run in a fresh directory.
    if let Commands::Init(args) = cli.command {
        let exit_code = run_init_cmd(args)?;
        if exit_code != 0 {
            std::process::exit(exit_code);
        }
        return Ok(());
    }

    let workspace = Workspace::discover(std::env::current_dir()?)?;
    let mut config = workspace.load_config()?;

    match cli.command {
        Commands::Config(args) => {
            persist_config_settings(&mut config, &args, globals);
            workspace.save_config(&config)?;
        }
        Commands::Prepare(args) => run_prepare(args, globals, &workspace, &config)?,
        Commands::Refine(args) => run_refine(args, globals, &workspace, &config)?,
        Commands::Scaffold(args) => run_scaffold(args, globals, &workspace, &config)?,
        Commands::Verify(args) => run_verify(args, &workspace)?,
        Commands::Manifest(cmd) => run_manifest(cmd, globals, &workspace)?,
        Commands::Clear(cmd) => run_clear(cmd, globals, &workspace, &config)?,
        Commands::Apis(cmd) => run_apis(cmd, globals, &workspace)?,
        Commands::Clean(args) => run_clean_cmd(args, &workspace)?,
        Commands::Init(_) => unreachable!("handled above"),
        Commands::Audit(cmd) => run_audit_cmd(cmd, &workspace)?,
    }

    Ok(())
}

fn run_audit_cmd(cmd: AuditCommand, workspace: &Workspace) -> Result<()> {
    use anyhow::Context as _;
    use std::io::Read;
    match cmd {
        AuditCommand::Record(args) => {
            let threshold = reen::audit_persist::Severity::parse(&args.severity_threshold)?;
            let raw = match args.from.as_deref() {
                Some("-") | None => {
                    let mut buf = String::new();
                    std::io::stdin().read_to_string(&mut buf)
                        .context("failed to read findings JSON from stdin")?;
                    buf
                }
                Some(path) => std::fs::read_to_string(path)
                    .with_context(|| format!("failed to read findings JSON from {}", path))?,
            };
            let report = reen::audit_persist::AuditReport::from_json(&raw)?;
            let outcome = reen::audit_persist::record(
                &report, &workspace.root, &workspace.state_dir, threshold,
            )?;
            println!(
                "audit: recorded {} finding(s), {} open spec question(s); {} tracker flip(s), {} idempotent skip(s), {} unmatched.",
                outcome.findings_count,
                outcome.open_questions_count,
                outcome.flipped_count,
                outcome.idempotent_skipped_count,
                outcome.unmatched_count,
            );
            println!("  written: {}", outcome.audit_json_path);
            println!("  written: {}", outcome.audit_md_path);
        }
    }
    Ok(())
}

fn run_prepare(
    args: PrepareArgs,
    globals: GlobalCliOptions,
    workspace: &Workspace,
    config: &ReenConfig,
) -> Result<()> {
    let section = &config.prepare;
    let selection = selection_from_flags(
        bool_setting(args.contexts, section.contexts, config.contexts),
        bool_setting(args.projections, section.projections, config.projections),
        bool_setting(args.data, section.data, config.data),
        args.names,
    );
    let options = PrepareOptions {
        selection,
        profile: option_setting(
            args.profile,
            section.profile.clone(),
            config.profile.clone(),
        ),
        verbose: bool_setting(globals.verbose, section.verbose, config.verbose),
        debug: bool_setting(globals.debug, section.debug, config.debug),
        dry_run: bool_setting(globals.dry_run, section.dry_run, config.dry_run),
        allow_code_tokens: args.allow_code_tokens,
    };
    prepare_workspace(workspace, &options)
}

fn run_refine(
    args: RefineArgs,
    globals: GlobalCliOptions,
    workspace: &Workspace,
    config: &ReenConfig,
) -> Result<()> {
    let refine_cfg = &config.refine;
    let selection = selection_from_flags(
        bool_setting(args.contexts, refine_cfg.contexts, config.contexts),
        bool_setting(args.projections, refine_cfg.projections, config.projections),
        bool_setting(args.data, refine_cfg.data, config.data),
        args.names,
    );
    let options = RefineOptions {
        selection,
        verbose: bool_setting(globals.verbose, refine_cfg.verbose, config.verbose),
        drafts_only: bool_setting(args.drafts_only, refine_cfg.drafts_only, None),
        prepared_only: bool_setting(args.prepared_only, refine_cfg.prepared_only, None),
    };
    refine_workspace(workspace, &options)
}

fn run_scaffold(
    args: ScaffoldArgs,
    globals: GlobalCliOptions,
    workspace: &Workspace,
    config: &ReenConfig,
) -> Result<()> {
    let section = &config.scaffold;
    let selection = selection_from_flags(
        bool_setting(args.contexts, section.contexts, config.contexts),
        bool_setting(args.projections, section.projections, config.projections),
        bool_setting(args.data, section.data, config.data),
        args.names,
    );
    let options = ScaffoldOptions {
        selection,
        verbose: bool_setting(globals.verbose, section.verbose, config.verbose),
        debug: bool_setting(globals.debug, section.debug, config.debug),
        dry_run: bool_setting(globals.dry_run, section.dry_run, config.dry_run),
        no_client: args.no_client,
    };
    scaffold_workspace(workspace, &options)?;

    // Also scaffold API operation clients if any exist
    reen::apis_codegen::scaffold_api_operations(
        workspace,
        options.verbose,
        options.dry_run,
    )
}

fn run_verify(args: VerifyArgs, workspace: &Workspace) -> Result<()> {
    use reen::prepared::PreparedArtifact;
    use anyhow::Context;
    use std::path::Path;
    use std::process;

    let rs_path = Path::new(&args.path);
    if !rs_path.is_file() {
        eprintln!("error: file not found: {}", rs_path.display());
        process::exit(2);
    }

    let relative = rs_path
        .strip_prefix(&workspace.root)
        .unwrap_or(rs_path);
    let relative_str = relative
        .to_string_lossy()
        .replace('\\', "/");

    // Check if this is main.rs (verify against App artifact if one exists)
    if relative_str == "src/main.rs" {
        let app_prepared = workspace.prepared_dir.join("app.yml");
        if app_prepared.is_file() {
            let raw = std::fs::read_to_string(&app_prepared)
                .with_context(|| format!("Failed to read {}", app_prepared.display()))?;
            let artifact: PreparedArtifact = serde_yaml::from_str(&raw)
                .with_context(|| format!("Failed to parse {}", app_prepared.display()))?;
            let result = verify::verify_app(&workspace.root, &artifact);
            if result.is_ok() {
                println!("{}", result.render());
            } else {
                eprint!("{}", result.render());
                process::exit(1);
            }
            return Ok(());
        }
    }

    // Check if this is an operation client (under src/apis/)
    let is_operation = relative_str.starts_with("src/apis/");

    if is_operation {
        // Verify against an operation spec
        let result = verify_operation_file(workspace, rs_path)?;
        if result.is_ok() {
            println!("{}", result.render());
        } else {
            eprint!("{}", result.render());
            process::exit(1);
        }
        return Ok(());
    }

    // Find the prepared YAML: explicit --prepared or auto-detect from the .rs path
    let prepared_path = if let Some(p) = &args.prepared {
        std::path::PathBuf::from(p)
    } else {
        auto_detect_prepared(workspace, rs_path)?
    };

    if !prepared_path.is_file() {
        eprintln!(
            "error: prepared artifact not found: {}",
            prepared_path.display()
        );
        process::exit(2);
    }

    let raw = std::fs::read_to_string(&prepared_path)
        .with_context(|| format!("Failed to read {}", prepared_path.display()))?;
    let artifact: PreparedArtifact = serde_yaml::from_str(&raw)
        .with_context(|| format!("Failed to parse {}", prepared_path.display()))?;

    let prepared_root = workspace.root.join("drafts").join("prepare");
    let kind_index = reen::verify_kind::KindIndex::from_prepared_dir(&prepared_root)
        .unwrap_or_default();
    let result = verify::verify_file_with_kind(rs_path, &artifact, &kind_index)?;

    if result.is_ok() {
        println!("{}", result.render());
        // Flip stale → fresh in the build tracker for verified methods
        if let Err(e) = verify::flip_verified_to_fresh(
            rs_path, &artifact, &workspace.root, &workspace.state_dir,
        ) {
            eprintln!("warning: could not update build tracker: {}", e);
        }
    } else {
        eprint!("{}", result.render());
        process::exit(1);
    }

    Ok(())
}

/// Verify an operation client file against its operation spec.
fn verify_operation_file(
    workspace: &Workspace,
    rs_path: &std::path::Path,
) -> Result<verify::VerifyResult> {
    use anyhow::Context;
    use reen::prepared::ApiOperationSpec;

    let relative = rs_path
        .strip_prefix(&workspace.root)
        .unwrap_or(rs_path)
        .to_string_lossy()
        .replace('\\', "/");

    // Extract api name and operation name from path: src/apis/<api>/<op>.rs
    let parts: Vec<&str> = relative.split('/').collect();
    if parts.len() < 4 {
        anyhow::bail!("Cannot determine API name from path: {}", relative);
    }
    let api_name = parts[2];
    let op_stem = std::path::Path::new(parts[3])
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();

    // Find the operation spec under drafts/.apis/operations/<api>/
    let ops_dir = workspace.drafts_dir.join(".apis/operations").join(api_name);
    if !ops_dir.is_dir() {
        anyhow::bail!(
            "No operation specs found for api `{}` at {}",
            api_name,
            ops_dir.display()
        );
    }

    // Normalize: try both the stem as-is and PascalCase variants
    let mut spec_path = None;
    if let Ok(entries) = std::fs::read_dir(&ops_dir) {
        let target_norm = normalize_stem(op_stem);
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("yml") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if normalize_stem(stem) == target_norm {
                        spec_path = Some(path);
                        break;
                    }
                }
            }
        }
    }

    let spec_path = spec_path.ok_or_else(|| {
        anyhow::anyhow!(
            "No operation spec found for `{}` in {}",
            op_stem,
            ops_dir.display()
        )
    })?;

    let raw = std::fs::read_to_string(&spec_path)
        .with_context(|| format!("Failed to read {}", spec_path.display()))?;
    let spec: ApiOperationSpec = serde_yaml::from_str(&raw)
        .with_context(|| format!("Failed to parse {}", spec_path.display()))?;

    let source = std::fs::read_to_string(rs_path)
        .with_context(|| format!("Failed to read {}", rs_path.display()))?;

    verify::verify_operation_source(&source, &spec)
}

/// Auto-detect the prepared YAML for a Rust source file.
/// e.g. src/contexts/money_transfer.rs → drafts/prepare/contexts/money_transfer.yml
fn auto_detect_prepared(
    workspace: &Workspace,
    rs_path: &std::path::Path,
) -> Result<std::path::PathBuf> {
    let relative = rs_path
        .strip_prefix(&workspace.root)
        .unwrap_or(rs_path);
    // Strip "src/" prefix and change extension to .yml under drafts/prepare/
    let under_src = relative
        .strip_prefix("src")
        .unwrap_or(relative);
    let candidate = workspace.root.join("drafts/prepare").join(under_src).with_extension("yml");
    if candidate.is_file() {
        return Ok(candidate);
    }
    // Fall back: the Rust file is snake_case but the YAML may be PascalCase.
    // Compare by stripping non-alphanumeric characters and lowercasing.
    if let Some(parent) = candidate.parent() {
        if parent.is_dir() {
            let target_norm = normalize_stem(
                candidate.file_stem().and_then(|s| s.to_str()).unwrap_or_default(),
            );
            if let Ok(entries) = std::fs::read_dir(parent) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("yml") {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            if normalize_stem(stem) == target_norm {
                                return Ok(path);
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(candidate)
}

/// Normalize a file stem for case-insensitive matching across naming conventions.
/// `player_state` and `PlayerState` both normalize to `playerstate`.
fn normalize_stem(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn run_manifest(
    cmd: ManifestCommand,
    globals: GlobalCliOptions,
    workspace: &Workspace,
) -> Result<()> {
    match cmd {
        ManifestCommand::Types(TypesManifestCommand::AddPrefix { prefix }) => {
            add_types_prefix(workspace, &prefix, globals.dry_run)
        }
        ManifestCommand::Capabilities(CapabilitiesManifestCommand::Add {
            domain,
            crate_name,
            no_default_features,
            feature,
            prefix,
            capability,
        }) => add_capability_provider(
            workspace,
            &CapabilityProviderInput {
                domain,
                crate_name,
                capabilities: capability,
                features: feature,
                default_features: !no_default_features,
                external_path_prefixes: prefix,
            },
            globals.dry_run,
        ),
    }
}

fn run_apis(
    cmd: ApisCommand,
    globals: GlobalCliOptions,
    workspace: &Workspace,
) -> Result<()> {
    match cmd {
        ApisCommand::Ingest { api_name } => {
            reen::apis_ingest::ingest_apis(
                workspace,
                api_name.as_deref(),
                globals.verbose,
            )
        }
    }
}

fn run_clear(
    cmd: ClearCommand,
    globals: GlobalCliOptions,
    workspace: &Workspace,
    config: &ReenConfig,
) -> Result<()> {
    let dry_run = bool_setting(globals.dry_run, config.clear.dry_run, config.dry_run);
    match cmd {
        ClearCommand::Prepared => clear_prepared_outputs(workspace, dry_run),
        ClearCommand::Generated => clear_generated_outputs(workspace, dry_run),
        ClearCommand::All => {
            clear_prepared_outputs(workspace, dry_run)?;
            clear_generated_outputs(workspace, dry_run)
        }
    }
}

fn run_clean_cmd(args: CleanArgs, workspace: &Workspace) -> Result<()> {
    use std::io::{self, Write};
    use reen::workspace::ProjectLayout;

    let build_json = workspace.state_dir.join("build.json");
    let prepared_dir = &workspace.prepared_dir;
    let src_dir = workspace.root.join("src");
    let crates_dir = match &workspace.layout {
        ProjectLayout::MultiCrate { crates_dir } => Some(crates_dir.clone()),
        ProjectLayout::SingleCrate { .. } => None,
    };

    let has_tracker = build_json.is_file();
    let has_prepared = prepared_dir.is_dir();
    let has_src = src_dir.is_dir();
    let has_crates = crates_dir.as_ref().map(|d| d.is_dir()).unwrap_or(false);

    if !has_tracker && !has_prepared && !has_src && !has_crates {
        println!("Nothing to clean.");
        return Ok(());
    }

    // Show what will be removed
    if has_tracker {
        println!("  .reen/build.json");
    }
    if has_prepared {
        println!("  drafts/prepare/");
    }
    if has_src {
        println!("  src/");
    }
    if has_crates {
        println!("  crates/");
    }

    // Confirm unless --force
    if !args.force {
        print!("\nRemove these files? [y/N] ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Aborted.");
            return Ok(());
        }
    }

    // Remove
    if has_tracker {
        std::fs::remove_file(&build_json)?;
        println!("Removed .reen/build.json");
    }
    if has_prepared {
        std::fs::remove_dir_all(prepared_dir)?;
        println!("Removed drafts/prepare/");
    }
    if has_src {
        std::fs::remove_dir_all(&src_dir)?;
        println!("Removed src/");
    }
    if let Some(crates_dir) = &crates_dir {
        if crates_dir.is_dir() {
            std::fs::remove_dir_all(crates_dir)?;
            println!("Removed crates/");
        }
    }

    // Also remove the generated files manifest
    let manifest_path = workspace.root.join(reen::workspace::GENERATED_MANIFEST);
    if manifest_path.is_file() {
        std::fs::remove_file(&manifest_path)?;
    }

    println!("Clean complete.");
    Ok(())
}

fn persist_config_settings(
    config: &mut ReenConfig,
    args: &ConfigArgs,
    globals: GlobalCliOptions,
) {
    let Some(command) = &args.command else {
        apply_root_config_flags(config, args);
        apply_root_globals(config, globals);
        return;
    };

    match command {
        ConfigCommand::Prepare(cmd_args) => {
            apply_prepare_config(&mut config.prepare, cmd_args);
            apply_section_globals(&mut config.prepare, globals);
        }
        ConfigCommand::Refine(cmd_args) => {
            apply_refine_config(&mut config.refine, cmd_args);
            apply_refine_globals(&mut config.refine, globals);
        }
        ConfigCommand::Scaffold(cmd_args) => {
            apply_scaffold_config(&mut config.scaffold, cmd_args);
            apply_section_globals(&mut config.scaffold, globals);
        }
        ConfigCommand::Clear => apply_section_globals(&mut config.clear, globals),
        ConfigCommand::Manifest => apply_section_globals(&mut config.manifest, globals),
    }
}

fn apply_root_config_flags(config: &mut ReenConfig, args: &ConfigArgs) {
    set_bool_if_requested(&mut config.contexts, args.contexts);
    set_bool_if_requested(&mut config.projections, args.projections);
    set_bool_if_requested(&mut config.data, args.data);
    if let Some(profile) = &args.profile {
        config.profile = Some(profile.clone());
    }
}

fn apply_root_globals(config: &mut ReenConfig, globals: GlobalCliOptions) {
    set_bool_if_requested(&mut config.verbose, globals.verbose);
    set_bool_if_requested(&mut config.debug, globals.debug);
    set_bool_if_requested(&mut config.dry_run, globals.dry_run);
}

fn apply_section_globals(section: &mut CommandConfig, globals: GlobalCliOptions) {
    set_bool_if_requested(&mut section.verbose, globals.verbose);
    set_bool_if_requested(&mut section.debug, globals.debug);
    set_bool_if_requested(&mut section.dry_run, globals.dry_run);
}

fn apply_refine_globals(section: &mut RefineConfig, globals: GlobalCliOptions) {
    set_bool_if_requested(&mut section.verbose, globals.verbose);
    set_bool_if_requested(&mut section.debug, globals.debug);
    set_bool_if_requested(&mut section.dry_run, globals.dry_run);
}

fn apply_prepare_config(section: &mut CommandConfig, args: &PrepareConfigArgs) {
    set_bool_if_requested(&mut section.contexts, args.contexts);
    set_bool_if_requested(&mut section.projections, args.projections);
    set_bool_if_requested(&mut section.data, args.data);
    if let Some(profile) = &args.profile {
        section.profile = Some(profile.clone());
    }
}

fn apply_scaffold_config(section: &mut CommandConfig, args: &ScaffoldConfigArgs) {
    set_bool_if_requested(&mut section.contexts, args.contexts);
    set_bool_if_requested(&mut section.projections, args.projections);
    set_bool_if_requested(&mut section.data, args.data);
}

fn apply_refine_config(section: &mut RefineConfig, args: &RefineConfigArgs) {
    set_bool_if_requested(&mut section.contexts, args.contexts);
    set_bool_if_requested(&mut section.projections, args.projections);
    set_bool_if_requested(&mut section.data, args.data);
    if let Some(profile) = &args.profile {
        section.profile = Some(profile.clone());
    }
    if args.drafts_only {
        section.drafts_only = Some(true);
        section.prepared_only = None;
    }
    if args.prepared_only {
        section.prepared_only = Some(true);
        section.drafts_only = None;
    }
}

fn set_bool_if_requested(slot: &mut Option<bool>, requested: bool) {
    if requested {
        *slot = Some(true);
    }
}

fn bool_setting(cli_flag: bool, command_config: Option<bool>, root_config: Option<bool>) -> bool {
    if cli_flag {
        return true;
    }
    command_config.or(root_config).unwrap_or(false)
}

fn option_setting<T>(
    cli_value: Option<T>,
    command_config: Option<T>,
    root_config: Option<T>,
) -> Option<T> {
    cli_value.or(command_config).or(root_config)
}

fn run_init_cmd(args: InitArgs) -> Result<i32> {
    let target_dir = if let Some(p) = &args.path {
        std::path::PathBuf::from(p)
    } else {
        std::env::current_dir()?
    };
    reen::init::run_init(&reen::init::InitOptions {
        target_dir,
        upgrade: args.upgrade,
        check: args.check,
        force: args.force,
    })
}

fn selection_from_flags(
    contexts: bool,
    projections: bool,
    data: bool,
    names: Vec<String>,
) -> Selection {
    Selection::new(contexts, projections, data, false, names)
}
