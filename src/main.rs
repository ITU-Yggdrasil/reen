use anyhow::Result;
use clap::{Args, Parser, Subcommand};

mod cli;

#[derive(Parser)]
#[command(name = "reen")]
#[command(about = "A compiler-like CLI for agent-driven specification and implementation", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(
        long,
        global = true,
        help = "Use agents/agent_model_registry.<profile>.yml for model selection"
    )]
    profile: Option<String>,

    #[arg(long, global = true, help = "Enable verbose debug output")]
    verbose: bool,

    #[arg(
        long,
        global = true,
        help = "Perform a dry run without executing actions"
    )]
    dry_run: bool,

    #[arg(
        long,
        global = true,
        help = "Use GitHub issues in <owner>/<repo> as the drafts/specifications backend"
    )]
    github: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Create specifications, implementations, or tests from drafts/contexts")]
    Create(CreateArgs),

    #[command(about = "Create specifications and implementation in one pass")]
    Build(BuildArgs),

    #[command(
        subcommand,
        about = "Check generated specifications for existence and blocking ambiguities"
    )]
    Check(CheckCommands),

    #[command(
        about = "Attempt to automatically fix compilation errors (compile → patch → recompile loop)"
    )]
    Fix {
        #[arg(
            long,
            help = "Ignore cached compilation-fix/planning agent responses for this run"
        )]
        clear_cache: bool,

        #[arg(
            long,
            help = "Maximum automatic compilation-fix attempts (default: 3, or from reen.yml fix.max-compile-fix-attempts)"
        )]
        max_compile_fix_attempts: Option<u32>,

        #[arg(
            long,
            help = "Maximum API requests per second (overrides REEN_RATE_LIMIT and registry)"
        )]
        rate_limit: Option<f64>,

        #[arg(
            long,
            help = "Maximum tokens per minute (overrides REEN_TOKEN_LIMIT and registry)"
        )]
        token_limit: Option<f64>,
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

    #[command(subcommand, about = "Manage capability-to-crate planning")]
    Capabilities(CapabilityCommands),

    #[command(subcommand, about = "Clear cache entries or generated artifacts")]
    Clear(ClearCommands),
}

#[derive(Subcommand)]
enum CapabilityCommands {
    #[command(about = "Scan drafts and create the initial capability registry")]
    Init {
        #[arg(
            long,
            help = "Use an agent to propose mappings for unresolved or ambiguous capabilities"
        )]
        agent: bool,

        #[arg(
            long,
            help = "Regenerate the registry even if drafts/capability_registry.yml already exists"
        )]
        force: bool,
    },

    #[command(about = "Add or extend a capability mapping in the registry")]
    Add {
        #[arg(help = "Capability identifier in snake_case")]
        capability: String,

        #[arg(help = "Crate name to provide this capability", value_name = "crate")]
        krate: String,

        #[arg(long, help = "Capability domain in snake_case")]
        domain: String,

        #[arg(long, help = "Crate version")]
        version: String,

        #[arg(long = "feature", help = "Cargo feature to enable", action = clap::ArgAction::Append)]
        features: Vec<String>,

        #[arg(long, help = "Disable default features for the selected crate")]
        no_default_features: bool,
    },
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

        #[arg(
            long,
            help = "When blocking ambiguities are detected, invoke agent to fix drafts and retry"
        )]
        fix: bool,

        #[arg(
            long,
            help = "Max fix attempts per draft when --fix is used (default: 3, or from reen.yml)"
        )]
        max_fix_attempts: Option<u32>,
    },

    #[command(about = "Create implementation from context files")]
    Implementation {
        #[arg(
            long,
            help = "When compilation fails after code generation, invoke the automatic compilation-fix loop"
        )]
        fix: bool,

        #[arg(
            long,
            help = "Maximum automatic compilation-fix attempts when --fix is used (default: 3, or from reen.yml)"
        )]
        max_compile_fix_attempts: Option<u32>,

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

    #[arg(long, help = "Only process drafts from the contexts/ folder")]
    contexts: bool,

    #[arg(long, help = "Only process drafts from the data/ folder")]
    data: bool,

    #[arg(
        long,
        help = "Maximum API requests per second (overrides REEN_RATE_LIMIT and registry)"
    )]
    rate_limit: Option<f64>,

    #[arg(
        long,
        help = "Maximum tokens per minute (overrides REEN_TOKEN_LIMIT and registry)"
    )]
    token_limit: Option<f64>,

    #[command(subcommand)]
    command: CreateCommands,
}

#[derive(Args)]
struct BuildArgs {
    #[arg(
        long,
        help = "Clear build-tracker cache for both stages before building (optionally scoped by provided names)"
    )]
    clear_cache: bool,

    #[arg(long, help = "Only process drafts from the contexts/ folder")]
    contexts: bool,

    #[arg(long, help = "Only process drafts from the data/ folder")]
    data: bool,

    #[arg(
        long,
        help = "Accepted for parity with create; build always enables draft and compilation repair"
    )]
    fix: bool,

    #[arg(
        long,
        help = "Max draft-fix attempts per draft during the specification stage (default: 3, or from reen.yml)"
    )]
    max_fix_attempts: Option<u32>,

    #[arg(
        long,
        help = "Maximum automatic compilation-fix attempts during the implementation stage (default: 3, or from reen.yml)"
    )]
    max_compile_fix_attempts: Option<u32>,

    #[arg(help = "Optional list of names (without .md extension)")]
    names: Vec<String>,

    #[arg(
        long,
        help = "Maximum API requests per second (overrides REEN_RATE_LIMIT and registry)"
    )]
    rate_limit: Option<f64>,

    #[arg(
        long,
        help = "Maximum tokens per minute (overrides REEN_TOKEN_LIMIT and registry)"
    )]
    token_limit: Option<f64>,
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

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Load reen.yml config; missing file yields an empty config (all fields None).
    let reen_config = cli::yaml_config::load_config().unwrap_or_default();

    // profile: CLI flag > reen.yml > none
    let effective_profile = cli
        .profile
        .as_deref()
        .or_else(|| reen_config.profile.as_deref());
    if let Some(profile) = effective_profile {
        unsafe {
            std::env::set_var("REEN_PROFILE", profile);
        }
    }
    reen::registries::validate_registry_profile(effective_profile).map_err(anyhow::Error::msg)?;

    // verbose: CLI flag > reen.yml > false
    let verbose = cli.verbose || reen_config.verbose.unwrap_or(false);

    let config = cli::Config {
        verbose,
        dry_run: cli.dry_run,
        github_repo: cli::resolve_github_repo(cli.github.as_deref())?,
    };

    match cli.command {
        Commands::Create(create_args) => {
            let rc = reen_config.create.as_ref();
            let root_rate_limit = reen_config.rate_limit;
            let root_token_limit = reen_config.token_limit;

            // Boolean flags: CLI true > reen.yml true > false
            let clear_cache =
                create_args.clear_cache || rc.and_then(|c| c.clear_cache).unwrap_or(false);
            let contexts = create_args.contexts || rc.and_then(|c| c.contexts).unwrap_or(false);
            let data = create_args.data || rc.and_then(|c| c.data).unwrap_or(false);

            let category_filter = cli::CategoryFilter { contexts, data };

            // rate/token limits: CLI > reen.yml > env > registry
            let rate_limit = cli::resolve_rate_limit(
                create_args
                    .rate_limit
                    .or_else(|| rc.and_then(|c| c.rate_limit))
                    .or(root_rate_limit),
            );
            let token_limit = cli::resolve_token_limit(
                create_args
                    .token_limit
                    .or_else(|| rc.and_then(|c| c.token_limit))
                    .or(root_token_limit),
            );

            cli::ensure_create_preconditions(&config)?;

            match create_args.command {
                CreateCommands::Specification {
                    names,
                    fix,
                    max_fix_attempts,
                } => {
                    let spec_cfg = rc.and_then(|c| c.specification.as_ref());
                    let create_fix_enabled = rc.map_or(false, |c| c.fix.is_enabled());
                    let fix =
                        fix || spec_cfg.map_or(false, |s| s.fix.is_enabled()) || create_fix_enabled;
                    let max_fix_attempts = max_fix_attempts
                        .or_else(|| spec_cfg.and_then(|s| s.fix.mapping_u32("max-fix-attempts")))
                        .or_else(|| {
                            spec_cfg.and_then(|s| s.fix.mapping_u32("max-compile-fix-attempts"))
                        })
                        .or_else(|| rc.and_then(|c| c.fix.mapping_u32("max-fix-attempts")))
                        .or_else(|| rc.and_then(|c| c.fix.mapping_u32("max-compile-fix-attempts")))
                        .unwrap_or(3) as usize;
                    cli::create_specification(
                        names,
                        clear_cache,
                        &category_filter,
                        rate_limit,
                        token_limit,
                        fix,
                        max_fix_attempts,
                        &config,
                    )
                    .await?;
                }
                CreateCommands::Implementation {
                    fix,
                    max_compile_fix_attempts,
                    names,
                } => {
                    let impl_cfg = rc.and_then(|c| c.implementation.as_ref());
                    let create_fix_enabled = rc.map_or(false, |c| c.fix.is_enabled());
                    let fix =
                        fix || impl_cfg.map_or(false, |i| i.fix.is_enabled()) || create_fix_enabled;
                    let max_compile_fix_attempts = max_compile_fix_attempts
                        .or_else(|| {
                            impl_cfg.and_then(|i| i.fix.mapping_u32("max-compile-fix-attempts"))
                        })
                        .or_else(|| rc.and_then(|c| c.fix.mapping_u32("max-compile-fix-attempts")))
                        .unwrap_or(3) as usize;
                    cli::create_implementation(
                        names,
                        fix,
                        max_compile_fix_attempts,
                        clear_cache,
                        &category_filter,
                        rate_limit,
                        token_limit,
                        &config,
                    )
                    .await?;
                }
                CreateCommands::Tests { names } => {
                    cli::create_tests(
                        names,
                        clear_cache,
                        &category_filter,
                        rate_limit,
                        token_limit,
                        &config,
                    )
                    .await?;
                }
            }
        }
        Commands::Build(build_args) => {
            let rc = reen_config.create.as_ref();
            let root_rate_limit = reen_config.rate_limit;
            let root_token_limit = reen_config.token_limit;

            let clear_cache =
                build_args.clear_cache || rc.and_then(|c| c.clear_cache).unwrap_or(false);
            let contexts = build_args.contexts || rc.and_then(|c| c.contexts).unwrap_or(false);
            let data = build_args.data || rc.and_then(|c| c.data).unwrap_or(false);
            let category_filter = cli::CategoryFilter { contexts, data };

            let rate_limit = cli::resolve_rate_limit(
                build_args
                    .rate_limit
                    .or_else(|| rc.and_then(|c| c.rate_limit))
                    .or(root_rate_limit),
            );
            let token_limit = cli::resolve_token_limit(
                build_args
                    .token_limit
                    .or_else(|| rc.and_then(|c| c.token_limit))
                    .or(root_token_limit),
            );

            cli::ensure_create_preconditions(&config)?;

            let spec_cfg = rc.and_then(|c| c.specification.as_ref());
            let max_fix_attempts = build_args
                .max_fix_attempts
                .or_else(|| spec_cfg.and_then(|s| s.fix.mapping_u32("max-fix-attempts")))
                .or_else(|| spec_cfg.and_then(|s| s.fix.mapping_u32("max-compile-fix-attempts")))
                .or_else(|| rc.and_then(|c| c.fix.mapping_u32("max-fix-attempts")))
                .or_else(|| rc.and_then(|c| c.fix.mapping_u32("max-compile-fix-attempts")))
                .unwrap_or(3) as usize;

            let impl_cfg = rc.and_then(|c| c.implementation.as_ref());
            let max_compile_fix_attempts = build_args
                .max_compile_fix_attempts
                .or_else(|| impl_cfg.and_then(|i| i.fix.mapping_u32("max-compile-fix-attempts")))
                .or_else(|| rc.and_then(|c| c.fix.mapping_u32("max-compile-fix-attempts")))
                .unwrap_or(3) as usize;

            let _ = build_args.fix;

            cli::build(
                build_args.names,
                clear_cache,
                &category_filter,
                rate_limit,
                token_limit,
                max_fix_attempts,
                max_compile_fix_attempts,
                &config,
            )
            .await?;
        }
        Commands::Check(check_cmd) => match check_cmd {
            CheckCommands::Specification { names } => {
                cli::check_specification(names, &config).await?;
            }
        },
        Commands::Fix {
            clear_cache,
            max_compile_fix_attempts,
            rate_limit,
            token_limit,
        } => {
            let fix_cfg = reen_config.fix.as_ref();
            let root_rate_limit = reen_config.rate_limit;
            let root_token_limit = reen_config.token_limit;
            let clear_cache = clear_cache || fix_cfg.and_then(|f| f.clear_cache).unwrap_or(false);
            let max_compile_fix_attempts = max_compile_fix_attempts
                .or_else(|| fix_cfg.and_then(|f| f.max_compile_fix_attempts))
                .unwrap_or(3) as usize;
            let rate_limit = cli::resolve_rate_limit(
                rate_limit
                    .or_else(|| fix_cfg.and_then(|f| f.rate_limit))
                    .or(root_rate_limit),
            );
            let token_limit = cli::resolve_token_limit(
                token_limit
                    .or_else(|| fix_cfg.and_then(|f| f.token_limit))
                    .or(root_token_limit),
            );
            cli::fix(
                max_compile_fix_attempts,
                clear_cache,
                rate_limit,
                token_limit,
                &config,
            )
            .await?;
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
        Commands::Capabilities(command) => match command {
            CapabilityCommands::Init { agent, force } => {
                cli::capabilities_init(agent, force, &config).await?;
            }
            CapabilityCommands::Add {
                capability,
                krate,
                domain,
                version,
                features,
                no_default_features,
            } => {
                cli::capabilities_add(
                    capability,
                    krate,
                    domain,
                    version,
                    features,
                    !no_default_features,
                    &config,
                )
                .await?;
            }
        },
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
    use super::{BuildArgs, CapabilityCommands, Cli, Commands};
    use clap::Parser;

    #[test]
    fn parses_build_command_with_shared_and_stage_flags() {
        let cli = Cli::parse_from([
            "reen",
            "build",
            "--clear-cache",
            "--contexts",
            "--rate-limit",
            "2",
            "--token-limit",
            "60000",
            "--max-fix-attempts",
            "4",
            "--max-compile-fix-attempts",
            "5",
            "app",
            "game_loop",
        ]);

        match cli.command {
            Commands::Build(BuildArgs {
                clear_cache,
                contexts,
                data,
                fix,
                max_fix_attempts,
                max_compile_fix_attempts,
                names,
                rate_limit,
                token_limit,
            }) => {
                assert!(clear_cache);
                assert!(contexts);
                assert!(!data);
                assert!(!fix);
                assert_eq!(max_fix_attempts, Some(4));
                assert_eq!(max_compile_fix_attempts, Some(5));
                assert_eq!(names, vec!["app".to_string(), "game_loop".to_string()]);
                assert_eq!(rate_limit, Some(2.0));
                assert_eq!(token_limit, Some(60000.0));
            }
            other => panic!(
                "expected build command, got {:?}",
                std::mem::discriminant(&other)
            ),
        }
    }

    #[test]
    fn parses_build_fix_flag_for_parity() {
        let cli = Cli::parse_from(["reen", "build", "--fix"]);
        match cli.command {
            Commands::Build(BuildArgs { fix, .. }) => assert!(fix),
            other => panic!(
                "expected build command, got {:?}",
                std::mem::discriminant(&other)
            ),
        }
    }

    #[test]
    fn parses_capabilities_init_with_agent() {
        let cli = Cli::parse_from(["reen", "capabilities", "init", "--agent"]);
        match cli.command {
            Commands::Capabilities(CapabilityCommands::Init { agent, force }) => {
                assert!(agent);
                assert!(!force);
            }
            other => panic!("unexpected command: {:?}", std::mem::discriminant(&other)),
        }
    }

    #[test]
    fn parses_capabilities_add() {
        let cli = Cli::parse_from([
            "reen",
            "capabilities",
            "add",
            "terminal_raw_input",
            "crossterm",
            "--domain",
            "terminal",
            "--version",
            "0.27",
            "--feature",
            "events",
            "--no-default-features",
        ]);
        match cli.command {
            Commands::Capabilities(CapabilityCommands::Add {
                capability,
                krate,
                domain,
                version,
                features,
                no_default_features,
            }) => {
                assert_eq!(capability, "terminal_raw_input");
                assert_eq!(krate, "crossterm");
                assert_eq!(domain, "terminal");
                assert_eq!(version, "0.27");
                assert_eq!(features, vec!["events"]);
                assert!(no_default_features);
            }
            other => panic!("unexpected command: {:?}", std::mem::discriminant(&other)),
        }
    }
}
