use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use reen::build_tracker::BuildTracker;
use reen::codegen::{ScaffoldOptions, clear_generated_outputs, scaffold_workspace};
use reen::manifest::{CapabilityProviderInput, add_capability_provider, add_types_prefix};
use reen::prepare::{
    PrepareOptions, RefineOptions, clear_prepared_outputs, prepare_workspace, refine_workspace,
};
use reen::workspace::{CommandConfig, RefineConfig, ReenConfig, Selection, Workspace};
use std::path::PathBuf;
use std::process::{Command, ExitStatus};

#[derive(Parser)]
#[command(name = "reen")]
#[command(about = "Prepare DCI-English drafts and deterministically build Rust code")]
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
        about = "Review raw drafts for completeness/concreteness/ambiguity, then prepared artifacts when available"
    )]
    Refine(RefineArgs),

    #[command(about = "Generate Rust source files from prepared artifacts")]
    Scaffold(ScaffoldArgs),

    #[command(about = "Implement method bodies using an LLM agent")]
    Build(BuildArgs),

    #[command(about = "Run cargo build in the current workspace")]
    Compile,

    #[command(about = "Run cargo run in the current workspace")]
    Run {
        #[arg(
            help = "Arguments passed to the generated binary",
            trailing_var_arg = true
        )]
        args: Vec<String>,
    },

    #[command(about = "Run cargo test in the current workspace")]
    Test,

    #[command(subcommand, about = "Edit Reen manifest files under drafts/")]
    Manifest(ManifestCommand),

    #[command(
        subcommand,
        about = "Clear prepared artifacts, generated outputs, or both"
    )]
    Clear(ClearCommand),
}

#[derive(Args, Default, Clone)]
struct ConfigArgs {
    #[arg(long, help = "Persist `fix: true` at the root of reen.yml")]
    fix: bool,

    #[arg(long, help = "Persist `contexts: true` at the root of reen.yml")]
    contexts: bool,

    #[arg(long, help = "Persist `projections: true` at the root of reen.yml")]
    projections: bool,

    #[arg(long, help = "Persist `data: true` at the root of reen.yml")]
    data: bool,

    #[arg(long, help = "Persist `app: true` at the root of reen.yml")]
    app: bool,

    #[arg(long, help = "Persist `profile: ...` at the root of reen.yml")]
    profile: Option<String>,

    #[arg(
        long,
        value_name = "N",
        help = "Persist `min-severity: N` at the root of reen.yml"
    )]
    min_severity: Option<u8>,

    #[arg(
        long,
        help = "Persist `skip-llm-review: true` at the root of reen.yml"
    )]
    skip_llm_review: bool,

    #[arg(
        long,
        help = "Persist `require-llm-review: true` at the root of reen.yml"
    )]
    require_llm_review: bool,

    #[command(subcommand)]
    command: Option<ConfigCommand>,
}

/// Per-command persistence targets. Every variant writes only to its matching section in
/// `reen.yml` — none of them execute the wrapped action.
#[derive(Subcommand, Clone)]
enum ConfigCommand {
    #[command(about = "Persist defaults under `prepare:` in reen.yml")]
    Prepare(PrepareConfigArgs),
    #[command(about = "Persist defaults under `refine:` in reen.yml")]
    Refine(RefineConfigArgs),
    #[command(about = "Persist defaults under `scaffold:` in reen.yml")]
    Scaffold(ScaffoldConfigArgs),
    #[command(about = "Persist defaults under `build:` in reen.yml")]
    Build(BuildConfigArgs),
    #[command(about = "Persist global flags under `compile:` in reen.yml")]
    Compile,
    #[command(about = "Persist global flags under `run:` in reen.yml")]
    Run,
    #[command(about = "Persist global flags under `test:` in reen.yml")]
    Test,
    #[command(about = "Persist global flags under `clear:` in reen.yml")]
    Clear,
    #[command(about = "Persist global flags under `manifest:` in reen.yml")]
    Manifest,
}

#[derive(Args, Clone, Default)]
struct PrepareConfigArgs {
    #[arg(long)]
    fix: bool,
    #[arg(long)]
    contexts: bool,
    #[arg(long)]
    projections: bool,
    #[arg(long)]
    data: bool,
    #[arg(long)]
    app: bool,
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
    app: bool,
    #[arg(long)]
    profile: Option<String>,
    #[arg(long, conflicts_with = "prepared_only")]
    drafts_only: bool,
    #[arg(long, conflicts_with = "drafts_only")]
    prepared_only: bool,
    #[arg(long, value_name = "N")]
    min_severity: Option<u8>,
    #[arg(long)]
    skip_llm_review: bool,
    #[arg(long)]
    require_llm_review: bool,
}

#[derive(Args, Clone, Default)]
struct ScaffoldConfigArgs {
    #[arg(long)]
    fix: bool,
    #[arg(long)]
    contexts: bool,
    #[arg(long)]
    projections: bool,
    #[arg(long)]
    data: bool,
    #[arg(long)]
    app: bool,
}

#[derive(Args, Clone, Default)]
struct BuildConfigArgs {
    #[arg(long)]
    fix: bool,
    #[arg(long)]
    contexts: bool,
    #[arg(long)]
    projections: bool,
    #[arg(long)]
    data: bool,
    #[arg(long)]
    app: bool,
}

#[derive(Args, Clone)]
struct PrepareArgs {
    #[arg(long, help = "Only process drafts from drafts/contexts/")]
    contexts: bool,

    #[arg(long, help = "Only process drafts from drafts/projections/")]
    projections: bool,

    #[arg(long, help = "Only process drafts from drafts/data/")]
    data: bool,

    #[arg(long, help = "Only process drafts/app.md")]
    app: bool,

    #[arg(long, help = "Reserved for future prepare-agent profiles")]
    profile: Option<String>,

    #[arg(
        long,
        help = "Call an LLM to resolve blocking ambiguities in prepared artifacts"
    )]
    fix: bool,

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

    #[arg(long, help = "Only process the app draft/artifact")]
    app: bool,

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

    #[arg(
        long,
        value_name = "N",
        help = "Minimum severity (0-100) for behavioral-ambiguity questions; default 40"
    )]
    min_severity: Option<u8>,

    #[arg(long, help = "Skip the LLM-backed behavioral ambiguity review")]
    skip_llm_review: bool,

    #[arg(
        long,
        help = "Fail if the LLM review cannot run (no key, network error); default: warn and continue"
    )]
    require_llm_review: bool,

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

    #[arg(long, help = "Only process the prepared app artifact")]
    app: bool,

    #[arg(long, help = "Fix compilation errors in generated scaffold code")]
    fix: bool,

    #[arg(help = "Optional list of prepared artifact names without file extension")]
    names: Vec<String>,
}

#[derive(Args, Clone)]
struct BuildArgs {
    #[arg(long, help = "Only process prepared context artifacts")]
    contexts: bool,

    #[arg(long, help = "Only process prepared projection artifacts")]
    projections: bool,

    #[arg(long, help = "Only process prepared data artifacts")]
    data: bool,

    #[arg(long, help = "Only process the prepared app artifact")]
    app: bool,

    #[arg(
        long,
        help = "After implementation, fix compilation errors (deterministic repair, then LLM)"
    )]
    fix: bool,

    #[arg(help = "Optional list of prepared artifact names without file extension")]
    names: Vec<String>,
}

#[derive(Subcommand, Clone)]
enum ClearCommand {
    #[command(about = "Remove drafts/prepare and prepare-stage tracker state")]
    Prepared,

    #[command(about = "Remove generated Rust outputs tracked under .reen/generated_files.json")]
    Generated,

    #[command(
        about = "Clear build-stage tracker state so methods are re-implemented on next build"
    )]
    Built,

    #[command(about = "Remove prepared, generated, and build-stage outputs")]
    All,
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
        Commands::Build(args) => run_build(args, globals, &workspace, &config)?,
        Commands::Compile => run_cargo_command("build", &[])?,
        Commands::Run { args } => run_cargo_command("run", &args)?,
        Commands::Test => run_cargo_command("test", &[])?,
        Commands::Manifest(cmd) => run_manifest(cmd, globals, &workspace)?,
        Commands::Clear(cmd) => run_clear(cmd, globals, &workspace, &config)?,
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
    let selection = selection_from_effective_flags(
        bool_setting(args.contexts, section.contexts, config.contexts),
        bool_setting(args.projections, section.projections, config.projections),
        bool_setting(args.data, section.data, config.data),
        bool_setting(args.app, section.app, config.app),
        args.names,
    );
    let options = PrepareOptions {
        selection,
        profile: option_setting(
            args.profile,
            section.profile.clone(),
            config.profile.clone(),
        ),
        fix: bool_setting(args.fix, section.fix, config.fix),
        verbose: bool_setting(globals.verbose, section.verbose, config.verbose),
        debug: bool_setting(globals.debug, section.debug, config.debug),
        dry_run: bool_setting(globals.dry_run, section.dry_run, config.dry_run),
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
    let selection = selection_from_effective_flags(
        bool_setting(args.contexts, refine_cfg.contexts, config.contexts),
        bool_setting(args.projections, refine_cfg.projections, config.projections),
        bool_setting(args.data, refine_cfg.data, config.data),
        bool_setting(args.app, refine_cfg.app, config.app),
        args.names,
    );
    let options = RefineOptions {
        selection,
        verbose: bool_setting(globals.verbose, refine_cfg.verbose, config.verbose),
        drafts_only: bool_setting(args.drafts_only, refine_cfg.drafts_only, None),
        prepared_only: bool_setting(args.prepared_only, refine_cfg.prepared_only, None),
        min_behavioral_severity: args
            .min_severity
            .or(refine_cfg.min_severity)
            .or(config.min_severity)
            .unwrap_or(reen::draft_refine_llm::DEFAULT_MIN_SEVERITY),
        skip_llm_review: bool_setting(
            args.skip_llm_review,
            refine_cfg.skip_llm_review,
            config.skip_llm_review,
        ),
        require_llm_review: bool_setting(
            args.require_llm_review,
            refine_cfg.require_llm_review,
            config.require_llm_review,
        ),
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
    let selection = selection_from_effective_flags(
        bool_setting(args.contexts, section.contexts, config.contexts),
        bool_setting(args.projections, section.projections, config.projections),
        bool_setting(args.data, section.data, config.data),
        bool_setting(args.app, section.app, config.app),
        args.names,
    );
    let options = ScaffoldOptions {
        selection,
        fix: bool_setting(args.fix, section.fix, config.fix),
        verbose: bool_setting(globals.verbose, section.verbose, config.verbose),
        debug: bool_setting(globals.debug, section.debug, config.debug),
        dry_run: bool_setting(globals.dry_run, section.dry_run, config.dry_run),
    };
    scaffold_workspace(workspace, &options)
}

fn run_build(
    args: BuildArgs,
    globals: GlobalCliOptions,
    workspace: &Workspace,
    config: &ReenConfig,
) -> Result<()> {
    let section = &config.build;
    let selection = selection_from_effective_flags(
        bool_setting(args.contexts, section.contexts, config.contexts),
        bool_setting(args.projections, section.projections, config.projections),
        bool_setting(args.data, section.data, config.data),
        bool_setting(args.app, section.app, config.app),
        args.names,
    );
    let options = reen::build_agent::BuildOptions {
        selection,
        fix: bool_setting(args.fix, section.fix, config.fix),
        verbose: bool_setting(globals.verbose, section.verbose, config.verbose),
        debug: bool_setting(globals.debug, section.debug, config.debug),
        dry_run: bool_setting(globals.dry_run, section.dry_run, config.dry_run),
    };
    reen::build_agent::build_workspace(workspace, &options)
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
        ClearCommand::Built => clear_build_tracker(workspace, dry_run),
        ClearCommand::All => {
            clear_prepared_outputs(workspace, dry_run)?;
            clear_generated_outputs(workspace, dry_run)?;
            clear_build_tracker(workspace, dry_run)
        }
    }
}

fn persist_config_settings(
    config: &mut ReenConfig,
    args: &ConfigArgs,
    globals: GlobalCliOptions,
) {
    let Some(command) = &args.command else {
        // Root-level persistence: both the locally-declared flags and the global flags go
        // at the top of `reen.yml` when no subcommand is provided.
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
        ConfigCommand::Build(cmd_args) => {
            apply_build_config(&mut config.build, cmd_args);
            apply_section_globals(&mut config.build, globals);
        }
        ConfigCommand::Compile => apply_section_globals(&mut config.compile, globals),
        ConfigCommand::Run => apply_section_globals(&mut config.run, globals),
        ConfigCommand::Test => apply_section_globals(&mut config.test, globals),
        ConfigCommand::Clear => apply_section_globals(&mut config.clear, globals),
        ConfigCommand::Manifest => apply_section_globals(&mut config.manifest, globals),
    }
}

fn apply_root_config_flags(config: &mut ReenConfig, args: &ConfigArgs) {
    set_bool_if_requested(&mut config.fix, args.fix);
    set_bool_if_requested(&mut config.contexts, args.contexts);
    set_bool_if_requested(&mut config.projections, args.projections);
    set_bool_if_requested(&mut config.data, args.data);
    set_bool_if_requested(&mut config.app, args.app);
    if let Some(profile) = &args.profile {
        config.profile = Some(profile.clone());
    }
    if let Some(value) = args.min_severity {
        config.min_severity = Some(value);
    }
    set_bool_if_requested(&mut config.skip_llm_review, args.skip_llm_review);
    set_bool_if_requested(&mut config.require_llm_review, args.require_llm_review);
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
    set_bool_if_requested(&mut section.fix, args.fix);
    set_bool_if_requested(&mut section.contexts, args.contexts);
    set_bool_if_requested(&mut section.projections, args.projections);
    set_bool_if_requested(&mut section.data, args.data);
    set_bool_if_requested(&mut section.app, args.app);
    if let Some(profile) = &args.profile {
        section.profile = Some(profile.clone());
    }
}

fn apply_scaffold_config(section: &mut CommandConfig, args: &ScaffoldConfigArgs) {
    set_bool_if_requested(&mut section.fix, args.fix);
    set_bool_if_requested(&mut section.contexts, args.contexts);
    set_bool_if_requested(&mut section.projections, args.projections);
    set_bool_if_requested(&mut section.data, args.data);
    set_bool_if_requested(&mut section.app, args.app);
}

fn apply_build_config(section: &mut CommandConfig, args: &BuildConfigArgs) {
    set_bool_if_requested(&mut section.fix, args.fix);
    set_bool_if_requested(&mut section.contexts, args.contexts);
    set_bool_if_requested(&mut section.projections, args.projections);
    set_bool_if_requested(&mut section.data, args.data);
    set_bool_if_requested(&mut section.app, args.app);
}

fn apply_refine_config(section: &mut RefineConfig, args: &RefineConfigArgs) {
    set_bool_if_requested(&mut section.contexts, args.contexts);
    set_bool_if_requested(&mut section.projections, args.projections);
    set_bool_if_requested(&mut section.data, args.data);
    set_bool_if_requested(&mut section.app, args.app);
    if let Some(profile) = &args.profile {
        section.profile = Some(profile.clone());
    }
    // `drafts_only` / `prepared_only` are mutually exclusive; set one, clear the other so a
    // later `reen config refine --prepared-only` overrides an earlier `--drafts-only`.
    if args.drafts_only {
        section.drafts_only = Some(true);
        section.prepared_only = None;
    }
    if args.prepared_only {
        section.prepared_only = Some(true);
        section.drafts_only = None;
    }
    if let Some(value) = args.min_severity {
        section.min_severity = Some(value);
    }
    set_bool_if_requested(&mut section.skip_llm_review, args.skip_llm_review);
    set_bool_if_requested(&mut section.require_llm_review, args.require_llm_review);
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

fn selection_from_effective_flags(
    contexts: bool,
    projections: bool,
    data: bool,
    app: bool,
    names: Vec<String>,
) -> Selection {
    Selection::new(contexts, projections, data, app, names)
}

fn clear_build_tracker(workspace: &Workspace, dry_run: bool) -> Result<()> {
    let mut tracker = BuildTracker::load(&workspace.root)?;
    if dry_run {
        println!("[dry-run] would clear build-stage tracker entries");
        return Ok(());
    }
    tracker.clear_stage("build");
    tracker.save(&workspace.root)
}

fn run_cargo_command(command: &str, args: &[String]) -> Result<()> {
    let mut cmd = Command::new("cargo");
    cmd.arg(command);
    cmd.args(args);
    let status = cmd.status()?;
    ensure_success(status, PathBuf::from("cargo"), command)
}

fn ensure_success(status: ExitStatus, executable: PathBuf, action: &str) -> Result<()> {
    if status.success() {
        return Ok(());
    }

    let code = status
        .code()
        .map(|value| value.to_string())
        .unwrap_or_else(|| "terminated by signal".to_string());
    anyhow::bail!(
        "{} {} failed with exit status {}",
        executable.display(),
        action,
        code
    );
}
