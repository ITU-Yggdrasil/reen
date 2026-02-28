use anyhow::Result;
use clap::{Args, Parser, Subcommand};

mod cli;

#[derive(Parser)]
#[command(name = "reen")]
#[command(about = "A compiler-like CLI for agent-driven specification and implementation", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(long, global = true, help = "Enable verbose debug output")]
    verbose: bool,

    #[arg(
        long,
        global = true,
        help = "Perform a dry run without executing actions"
    )]
    dry_run: bool,
}

#[derive(Subcommand)]
enum Commands {
    Create(CreateArgs),

    #[command(subcommand)]
    Check(CheckCommands),

    #[command(subcommand)]
    Review(ReviewCommands),

    #[command(
        about = "Attempt to automatically fix compilation errors (compile → patch → recompile loop)"
    )]
    Fix {
        #[arg(
            long,
            default_value_t = 3,
            help = "Maximum automatic compilation-fix attempts"
        )]
        max_compile_fix_attempts: u32,
    },

    #[command(about = "Compile the generated project using cargo build")]
    Compile,

    #[command(about = "Build and run the application using cargo run")]
    Run {
        #[arg(help = "Arguments to pass to the application", trailing_var_arg = true)]
        args: Vec<String>,
    },

    #[command(about = "Test the project using cargo test")]
    Test,

    #[command(subcommand, about = "Clear cache entries or generated artifacts")]
    Clear(ClearCommands),
}

#[derive(Subcommand)]
enum ClearCommands {
    #[command(subcommand, about = "Clear cached build-tracker entries for a stage")]
    Cache(ClearCacheTargets),

    #[command(subcommand, about = "Remove generated artifacts", alias = "artifact")]
    Artefact(ClearArtifactTargets),
}

#[derive(Subcommand)]
enum ClearCacheTargets {
    #[command(about = "Clear specification cache entries", alias = "specifications")]
    Specification {
        #[arg(help = "Optional list of names to clear (without .md extension)")]
        names: Vec<String>,
    },

    #[command(
        about = "Clear implementation cache entries",
        alias = "implementations"
    )]
    Implementation {
        #[arg(help = "Optional list of names to clear (without .md extension)")]
        names: Vec<String>,
    },

    #[command(about = "Clear test cache entries", alias = "test")]
    Tests {
        #[arg(help = "Optional list of names to clear (without .md extension)")]
        names: Vec<String>,
    },
}

#[derive(Subcommand)]
enum ClearArtifactTargets {
    #[command(about = "Clear specification artifacts", alias = "specifications")]
    Specification {
        #[arg(help = "Optional list of names to clear (without .md extension)")]
        names: Vec<String>,
    },

    #[command(about = "Clear implementation artifacts", alias = "implementations")]
    Implementation {
        #[arg(help = "Optional list of names to clear (without .md extension)")]
        names: Vec<String>,
    },

    #[command(about = "Clear test artifacts", alias = "test")]
    Tests {
        #[arg(help = "Optional list of names to clear (without .md extension)")]
        names: Vec<String>,
    },
}

#[derive(Subcommand)]
enum CreateCommands {
    #[command(
        about = "Create specifications from draft files",
        alias = "specifications"
    )]
    Specification {
        #[arg(help = "Optional list of draft names (without .md extension)")]
        names: Vec<String>,
    },

    #[command(about = "Create implementation from context files")]
    Implementation {
        #[arg(
            long,
            default_value_t = 3,
            help = "Maximum automatic compilation-fix attempts after code generation"
        )]
        max_compile_fix_attempts: u32,

        #[arg(help = "Optional list of context names (without .md extension)")]
        names: Vec<String>,
    },

    #[command(about = "Create tests from context files", alias = "test")]
    Tests {
        #[arg(help = "Optional list of context names (without .md extension)")]
        names: Vec<String>,
    },
}

#[derive(Args)]
struct CreateArgs {
    #[arg(
        long,
        help = "Clear build-tracker cache for this stage before creating (optionally scoped by provided names)"
    )]
    clear_cache: bool,

    #[command(subcommand)]
    command: CreateCommands,
}

#[derive(Subcommand)]
enum CheckCommands {
    #[command(
        about = "Check generated specifications for existence and blocking ambiguities",
        alias = "specifications"
    )]
    Specification {
        #[arg(help = "Optional list of draft names (without .md extension)")]
        names: Vec<String>,
    },
}

#[derive(Subcommand)]
enum ReviewCommands {
    #[command(
        about = "Review draft quality against specification errors",
        alias = "specifications"
    )]
    Specification {
        #[arg(long, help = "Apply suggested corrections directly to draft files")]
        fix: bool,

        #[arg(help = "Optional list of draft names (without .md extension)")]
        names: Vec<String>,
    },

    #[command(
        about = "Review draft quality against implementation errors",
        alias = "implementations"
    )]
    Implementation {
        #[arg(long, help = "Apply suggested corrections directly to draft files")]
        fix: bool,

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
        Commands::Create(create_args) => match create_args.command {
            CreateCommands::Specification { names } => {
                cli::create_specification(names, create_args.clear_cache, &config).await?;
            }
            CreateCommands::Implementation {
                max_compile_fix_attempts,
                names,
            } => {
                cli::create_implementation(
                    names,
                    max_compile_fix_attempts as usize,
                    create_args.clear_cache,
                    &config,
                )
                .await?;
            }
            CreateCommands::Tests { names } => {
                cli::create_tests(names, create_args.clear_cache, &config).await?;
            }
        },
        Commands::Check(check_cmd) => match check_cmd {
            CheckCommands::Specification { names } => {
                cli::check_specification(names, &config).await?;
            }
        },
        Commands::Review(review_cmd) => match review_cmd {
            ReviewCommands::Specification { fix, names } => {
                cli::review_specification(names, fix, &config).await?;
            }
            ReviewCommands::Implementation { fix, names } => {
                cli::review_implementation(names, fix, &config).await?;
            }
        },
        Commands::Fix {
            max_compile_fix_attempts,
        } => {
            cli::fix(max_compile_fix_attempts as usize, &config).await?;
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
        Commands::Clear(clear_cmd) => match clear_cmd {
            ClearCommands::Cache(target) => match target {
                ClearCacheTargets::Specification { names } => {
                    cli::clear_cache("specification", names, &config).await?;
                }
                ClearCacheTargets::Implementation { names } => {
                    cli::clear_cache("implementation", names, &config).await?;
                }
                ClearCacheTargets::Tests { names } => {
                    cli::clear_cache("tests", names, &config).await?;
                }
            },
            ClearCommands::Artefact(target) => match target {
                ClearArtifactTargets::Specification { names } => {
                    cli::clear_artifacts("specification", names, &config).await?;
                }
                ClearArtifactTargets::Implementation { names } => {
                    cli::clear_artifacts("implementation", names, &config).await?;
                }
                ClearArtifactTargets::Tests { names } => {
                    cli::clear_artifacts("tests", names, &config).await?;
                }
            },
        },
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parses_review_specification_with_fix_flag() {
        let cli = Cli::try_parse_from(["reen", "review", "specification", "--fix", "app"])
            .expect("cli parse");
        match cli.command {
            Commands::Review(ReviewCommands::Specification { fix, names }) => {
                assert!(fix);
                assert_eq!(names, vec!["app"]);
            }
            _ => panic!("unexpected command variant"),
        }
    }

    #[test]
    fn parses_review_implementation_without_fix() {
        let cli = Cli::try_parse_from(["reen", "review", "implementation", "game_loop"])
            .expect("cli parse");
        match cli.command {
            Commands::Review(ReviewCommands::Implementation { fix, names }) => {
                assert!(!fix);
                assert_eq!(names, vec!["game_loop"]);
            }
            _ => panic!("unexpected command variant"),
        }
    }
}
