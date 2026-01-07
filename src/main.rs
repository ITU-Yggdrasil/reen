use clap::{Parser, Subcommand};
use anyhow::Result;

mod cli;

#[derive(Parser)]
#[command(name = "reen")]
#[command(about = "A compiler-like CLI for agent-driven specification and implementation", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(long, global = true, help = "Enable verbose debug output")]
    verbose: bool,

    #[arg(long, global = true, help = "Perform a dry run without executing actions")]
    dry_run: bool,
}

#[derive(Subcommand)]
enum Commands {
    #[command(subcommand)]
    Create(CreateCommands),

    #[command(about = "Compile the generated project using cargo build")]
    Compile,

    #[command(about = "Build and run the application using cargo run")]
    Run {
        #[arg(help = "Arguments to pass to the application", trailing_var_arg = true)]
        args: Vec<String>,
    },

    #[command(about = "Test the project using cargo test")]
    Test,
}

#[derive(Subcommand)]
enum CreateCommands {
    #[command(about = "Create specifications from draft files")]
    Specification {
        #[arg(help = "Optional list of draft names (without .md extension)")]
        names: Vec<String>,
    },

    #[command(about = "Create implementation from context files")]
    Implementation {
        #[arg(help = "Optional list of context names (without .md extension)")]
        names: Vec<String>,
    },

    #[command(about = "Create tests from context files")]
    Tests {
        #[arg(help = "Optional list of context names (without .md extension)")]
        names: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let config = cli::Config {
        verbose: cli.verbose,
        dry_run: cli.dry_run,
    };

    match cli.command {
        Commands::Create(create_cmd) => {
            match create_cmd {
                CreateCommands::Specification { names } => {
                    cli::create_specification(names, &config).await?;
                }
                CreateCommands::Implementation { names } => {
                    cli::create_implementation(names, &config).await?;
                }
                CreateCommands::Tests { names } => {
                    cli::create_tests(names, &config).await?;
                }
            }
        }
        Commands::Compile => {
            cli::compile(&config).await?;
        }
        Commands::Run { args } => {
            cli::run(args, &config).await?;
        }
        Commands::Test => {
            cli::test(&config).await?;
        }
    }

    Ok(())
}
