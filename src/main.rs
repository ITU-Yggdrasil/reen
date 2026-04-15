use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use reen::codegen::{ScaffoldOptions, scaffold_workspace, clear_generated_outputs};
use reen::prepare::{PrepareOptions, clear_prepared_outputs, prepare_workspace};
use reen::workspace::{Selection, Workspace};
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

    #[arg(long, global = true, help = "Write optional debug artifacts under .reen/debug")]
    debug: bool,

    #[arg(long, global = true, help = "Show actions without writing files")]
    dry_run: bool,
}

#[derive(Subcommand)]
enum Commands {
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
        #[arg(help = "Arguments passed to the generated binary", trailing_var_arg = true)]
        args: Vec<String>,
    },

    #[command(about = "Run cargo test in the current workspace")]
    Test,

    #[command(subcommand, about = "Clear prepared artifacts, generated outputs, or both")]
    Clear(ClearCommand),
}

#[derive(Args)]
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

    #[arg(long, help = "Call an LLM to resolve blocking ambiguities in prepared artifacts")]
    fix: bool,

    #[arg(help = "Optional list of draft names without file extension")]
    names: Vec<String>,
}

#[derive(Args)]
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

#[derive(Args)]
struct BuildArgs {
    #[arg(long, help = "Only process prepared context artifacts")]
    contexts: bool,

    #[arg(long, help = "Only process prepared projection artifacts")]
    projections: bool,

    #[arg(long, help = "Only process prepared data artifacts")]
    data: bool,

    #[arg(long, help = "Only process the prepared app artifact")]
    app: bool,

    #[arg(help = "Optional list of prepared artifact names without file extension")]
    names: Vec<String>,
}

#[derive(Subcommand)]
enum ClearCommand {
    #[command(about = "Remove drafts/prepare and prepare-stage tracker state")]
    Prepared,

    #[command(about = "Remove generated Rust outputs tracked under .reen/generated_files.json")]
    Generated,

    #[command(about = "Remove both prepared and generated outputs")]
    All,
}

fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();
    match cli.command {
        Commands::Prepare(args) => {
            let workspace = Workspace::discover(std::env::current_dir()?)?;
            let selection = selection_from_flags(
                args.contexts,
                args.projections,
                args.data,
                args.app,
                args.names,
            );
            let options = PrepareOptions {
                selection,
                profile: args.profile,
                fix: args.fix,
                verbose: cli.verbose,
                debug: cli.debug,
                dry_run: cli.dry_run,
            };
            prepare_workspace(&workspace, &options)?;
        }
        Commands::Scaffold(args) => {
            let workspace = Workspace::discover(std::env::current_dir()?)?;
            let selection = selection_from_flags(
                args.contexts,
                args.projections,
                args.data,
                args.app,
                args.names,
            );
            let options = ScaffoldOptions {
                selection,
                fix: args.fix,
                verbose: cli.verbose,
                debug: cli.debug,
                dry_run: cli.dry_run,
            };
            scaffold_workspace(&workspace, &options)?;
        }
        Commands::Build(args) => {
            let workspace = Workspace::discover(std::env::current_dir()?)?;
            let selection = selection_from_flags(
                args.contexts,
                args.projections,
                args.data,
                args.app,
                args.names,
            );
            let options = reen::build_agent::BuildOptions {
                selection,
                verbose: cli.verbose,
                debug: cli.debug,
                dry_run: cli.dry_run,
            };
            reen::build_agent::build_workspace(&workspace, &options)?;
        }
        Commands::Compile => {
            run_cargo_command("build", &[])?;
        }
        Commands::Run { args } => {
            run_cargo_command("run", &args)?;
        }
        Commands::Test => {
            run_cargo_command("test", &[])?;
        }
        Commands::Clear(cmd) => {
            let workspace = Workspace::discover(std::env::current_dir()?)?;
            match cmd {
                ClearCommand::Prepared => clear_prepared_outputs(&workspace, cli.dry_run)?,
                ClearCommand::Generated => clear_generated_outputs(&workspace, cli.dry_run)?,
                ClearCommand::All => {
                    clear_prepared_outputs(&workspace, cli.dry_run)?;
                    clear_generated_outputs(&workspace, cli.dry_run)?;
                }
            }
        }
    }

    Ok(())
}

fn selection_from_flags(
    contexts: bool,
    projections: bool,
    data: bool,
    app: bool,
    names: Vec<String>,
) -> Selection {
    Selection::new(contexts, projections, data, app, names)
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
