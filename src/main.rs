use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use reen::build_tracker::BuildTracker;
use reen::codegen::{ScaffoldOptions, clear_generated_outputs, scaffold_workspace};
use reen::manifest::{CapabilityProviderInput, add_capability_provider, add_types_prefix};
use reen::prepare::{PrepareOptions, clear_prepared_outputs, prepare_workspace};
use reen::workspace::{CommandConfig, ReenConfig, Selection, Workspace};
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
    #[command(about = "Persist defaults into reen.yml, then optionally run the wrapped command")]
    Init(InitArgs),

    #[command(about = "Prepare per-draft YAML artifacts under drafts/prepare")]
    Prepare(PrepareArgs),

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
struct InitArgs {
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

    #[command(subcommand)]
    command: Option<InitCommand>,
}

#[derive(Subcommand, Clone)]
enum InitCommand {
    Prepare(PrepareArgs),
    Scaffold(ScaffoldArgs),
    Build(BuildArgs),
    Compile,
    Run { args: Vec<String> },
    Test,
    #[command(subcommand)]
    Clear(ClearCommand),
    #[command(subcommand)]
    Manifest(ManifestCommand),
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
    #[command(
        about = "Add a capability domain and allow its crate namespace under drafts/"
    )]
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

#[derive(Clone)]
enum RunnableCommand {
    Prepare(PrepareArgs),
    Scaffold(ScaffoldArgs),
    Build(BuildArgs),
    Compile,
    Run { args: Vec<String> },
    Test,
    Manifest(ManifestCommand),
    Clear(ClearCommand),
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
        Commands::Init(args) => {
            persist_init_settings(&mut config, &args);
            if globals.dry_run {
                println!(
                    "[dry-run] would write {}",
                    workspace.config_path().display()
                );
            } else {
                workspace.save_config(&config)?;
            }
            if let Some(command) = args.command {
                execute_command(command.into(), globals, &workspace, &config)?;
            }
        }
        Commands::Prepare(args) => {
            execute_command(RunnableCommand::Prepare(args), globals, &workspace, &config)?;
        }
        Commands::Scaffold(args) => {
            execute_command(RunnableCommand::Scaffold(args), globals, &workspace, &config)?;
        }
        Commands::Build(args) => {
            execute_command(RunnableCommand::Build(args), globals, &workspace, &config)?;
        }
        Commands::Compile => {
            execute_command(RunnableCommand::Compile, globals, &workspace, &config)?
        }
        Commands::Run { args } => {
            execute_command(RunnableCommand::Run { args }, globals, &workspace, &config)?
        }
        Commands::Test => execute_command(RunnableCommand::Test, globals, &workspace, &config)?,
        Commands::Manifest(cmd) => {
            execute_command(RunnableCommand::Manifest(cmd), globals, &workspace, &config)?
        }
        Commands::Clear(cmd) => {
            execute_command(RunnableCommand::Clear(cmd), globals, &workspace, &config)?
        }
    }

    Ok(())
}

impl From<InitCommand> for RunnableCommand {
    fn from(value: InitCommand) -> Self {
        match value {
            InitCommand::Prepare(args) => Self::Prepare(args),
            InitCommand::Scaffold(args) => Self::Scaffold(args),
            InitCommand::Build(args) => Self::Build(args),
            InitCommand::Compile => Self::Compile,
            InitCommand::Run { args } => Self::Run { args },
            InitCommand::Test => Self::Test,
            InitCommand::Manifest(cmd) => Self::Manifest(cmd),
            InitCommand::Clear(cmd) => Self::Clear(cmd),
        }
    }
}

fn execute_command(
    command: RunnableCommand,
    globals: GlobalCliOptions,
    workspace: &Workspace,
    config: &ReenConfig,
) -> Result<()> {
    match command {
        RunnableCommand::Prepare(args) => {
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
            prepare_workspace(workspace, &options)?;
        }
        RunnableCommand::Scaffold(args) => {
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
            scaffold_workspace(workspace, &options)?;
        }
        RunnableCommand::Build(args) => {
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
            reen::build_agent::build_workspace(workspace, &options)?;
        }
        RunnableCommand::Compile => {
            run_cargo_command("build", &[])?;
        }
        RunnableCommand::Run { args } => {
            run_cargo_command("run", &args)?;
        }
        RunnableCommand::Test => {
            run_cargo_command("test", &[])?;
        }
        RunnableCommand::Manifest(cmd) => match cmd {
            ManifestCommand::Types(TypesManifestCommand::AddPrefix { prefix }) => {
                add_types_prefix(workspace, &prefix, globals.dry_run)?;
            }
            ManifestCommand::Capabilities(CapabilitiesManifestCommand::Add {
                domain,
                crate_name,
                no_default_features,
                feature,
                prefix,
                capability,
            }) => {
                add_capability_provider(
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
                )?;
            }
        },
        RunnableCommand::Clear(cmd) => match cmd {
            ClearCommand::Prepared => {
                clear_prepared_outputs(
                    workspace,
                    bool_setting(globals.dry_run, config.clear.dry_run, config.dry_run),
                )?
            }
            ClearCommand::Generated => {
                clear_generated_outputs(
                    workspace,
                    bool_setting(globals.dry_run, config.clear.dry_run, config.dry_run),
                )?
            }
            ClearCommand::Built => {
                clear_build_tracker(
                    workspace,
                    bool_setting(globals.dry_run, config.clear.dry_run, config.dry_run),
                )?
            }
            ClearCommand::All => {
                let dry_run =
                    bool_setting(globals.dry_run, config.clear.dry_run, config.dry_run);
                clear_prepared_outputs(workspace, dry_run)?;
                clear_generated_outputs(workspace, dry_run)?;
                clear_build_tracker(workspace, dry_run)?;
            }
        },
    }
    Ok(())
}

fn persist_init_settings(config: &mut ReenConfig, args: &InitArgs) {
    apply_root_init_flags(config, args);
    if let Some(command) = &args.command {
        let section = match command {
            InitCommand::Prepare(_) => &mut config.prepare,
            InitCommand::Scaffold(_) => &mut config.scaffold,
            InitCommand::Build(_) => &mut config.build,
            InitCommand::Compile => &mut config.compile,
            InitCommand::Run { .. } => &mut config.run,
            InitCommand::Test => &mut config.test,
            InitCommand::Manifest(_) => return,
            InitCommand::Clear(_) => &mut config.clear,
        };
        apply_command_init_flags(section, command);
    }
}

fn apply_root_init_flags(config: &mut ReenConfig, args: &InitArgs) {
    set_bool_if_requested(&mut config.fix, args.fix);
    set_bool_if_requested(&mut config.contexts, args.contexts);
    set_bool_if_requested(&mut config.projections, args.projections);
    set_bool_if_requested(&mut config.data, args.data);
    set_bool_if_requested(&mut config.app, args.app);
    if let Some(profile) = &args.profile {
        config.profile = Some(profile.clone());
    }
}

fn apply_command_init_flags(section: &mut CommandConfig, command: &InitCommand) {
    match command {
        InitCommand::Prepare(args) => {
            set_bool_if_requested(&mut section.contexts, args.contexts);
            set_bool_if_requested(&mut section.projections, args.projections);
            set_bool_if_requested(&mut section.data, args.data);
            set_bool_if_requested(&mut section.app, args.app);
            set_bool_if_requested(&mut section.fix, args.fix);
            if let Some(profile) = &args.profile {
                section.profile = Some(profile.clone());
            }
        }
        InitCommand::Scaffold(args) => {
            set_bool_if_requested(&mut section.contexts, args.contexts);
            set_bool_if_requested(&mut section.projections, args.projections);
            set_bool_if_requested(&mut section.data, args.data);
            set_bool_if_requested(&mut section.app, args.app);
            set_bool_if_requested(&mut section.fix, args.fix);
        }
        InitCommand::Build(args) => {
            set_bool_if_requested(&mut section.contexts, args.contexts);
            set_bool_if_requested(&mut section.projections, args.projections);
            set_bool_if_requested(&mut section.data, args.data);
            set_bool_if_requested(&mut section.app, args.app);
            set_bool_if_requested(&mut section.fix, args.fix);
        }
        InitCommand::Compile
        | InitCommand::Run { .. }
        | InitCommand::Test
        | InitCommand::Manifest(_)
        | InitCommand::Clear(_) => {}
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
