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
        help = "Persist debug artifacts under .reen/debug during contract/build flows"
    )]
    debug: bool,

    #[arg(
        long,
        global = true,
        help = "Perform a dry run without executing actions"
    )]
    dry_run: bool,

    #[arg(
        long,
        global = true,
        help = "Use GitHub issues in <owner>/<repo> as the drafts backend"
    )]
    github: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Create contracts, implementations, or tests from drafts")]
    Create(CreateArgs),

    #[command(about = "Check drafts and create implementation in one pass")]
    Build(BuildArgs),

    #[command(
        subcommand,
        about = "Check generated draft contracts and blocking ambiguities"
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

    #[command(
        about = "Clear caches and/or generated sources (run with no subcommand to clear both)"
    )]
    Clear {
        #[command(subcommand)]
        cmd: Option<ClearSubcommand>,
    },
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

        #[arg(
            long,
            help = "Crate version; defaults to the latest stable crates.io release"
        )]
        version: Option<String>,

        #[arg(long = "feature", help = "Cargo feature to enable", action = clap::ArgAction::Append)]
        features: Vec<String>,

        #[arg(long, help = "Disable default features for the selected crate")]
        no_default_features: bool,
    },
}

#[derive(Subcommand)]
enum ClearSubcommand {
    #[command(about = "Clear build-tracker and agent response caches")]
    Cache(ClearFilterArgs),

    #[command(about = "Remove generated implementation files from src/")]
    Implementation(ClearFilterArgs),
}

#[derive(Args)]
struct ClearFilterArgs {
    #[arg(long, help = "Only clear artifacts from the contexts/ folder")]
    contexts: bool,

    #[arg(long, help = "Only clear artifacts from the projections/ folder")]
    projections: bool,

    #[arg(long, help = "Only clear artifacts from the data/ folder")]
    data: bool,

    #[arg(help = "Optional list of artifact names (without .md extension)")]
    names: Vec<String>,
}

#[derive(Subcommand)]
enum CreateCommands {
    #[command(
        about = "Synthesize internal contract bundles from draft files",
        alias = "contracts"
    )]
    Contract {
        #[arg(help = "Optional list of draft names (without .md extension)")]
        names: Vec<String>,

        #[arg(
            long,
            help = "Accepted for backward compatibility; drafts are read-only and are never modified automatically"
        )]
        fix: bool,

        #[arg(
            long,
            help = "Accepted for backward compatibility; drafts are read-only and this setting is ignored"
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
        global = true,
        help = "Clear build-tracker cache for this stage before creating (optionally scoped by provided names)"
    )]
    clear_cache: bool,

    #[arg(
        long,
        global = true,
        help = "Only process drafts from the contexts/ folder"
    )]
    contexts: bool,

    #[arg(
        long,
        global = true,
        help = "Only process drafts from the projections/ folder"
    )]
    projections: bool,

    #[arg(
        long,
        global = true,
        help = "Only process drafts from the data/ folder"
    )]
    data: bool,

    #[arg(
        long,
        global = true,
        help = "Maximum API requests per second (overrides REEN_RATE_LIMIT and registry)"
    )]
    rate_limit: Option<f64>,

    #[arg(
        long,
        global = true,
        help = "Maximum tokens per minute (overrides REEN_TOKEN_LIMIT and registry)"
    )]
    token_limit: Option<f64>,

    #[arg(
        long,
        global = true,
        help = "Maximum items processed concurrently per stage (default: 4, or from reen.yml create.parallel-limit)"
    )]
    parallel_limit: Option<u32>,

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

    #[arg(long, help = "Only process drafts from the projections/ folder")]
    projections: bool,

    #[arg(long, help = "Only process drafts from the data/ folder")]
    data: bool,

    #[arg(
        long,
        help = "Accepted for backward compatibility; build never mutates drafts"
    )]
    fix: bool,

    #[arg(
        long,
        help = "Accepted for backward compatibility; build never mutates drafts and this setting is ignored"
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

    #[arg(
        long,
        help = "Maximum items processed concurrently per stage (default: 4, or from reen.yml create.parallel-limit)"
    )]
    parallel_limit: Option<u32>,
}

#[derive(Subcommand)]
enum CheckCommands {
    #[command(
        about = "Validate drafts and synthesize internal contract bundles",
        alias = "contracts"
    )]
    Drafts {
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
    let debug = cli.debug || reen_config.debug.unwrap_or(false);

    let config = cli::Config {
        verbose,
        debug,
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
            let projections =
                create_args.projections || rc.and_then(|c| c.projections).unwrap_or(false);
            let data = create_args.data || rc.and_then(|c| c.data).unwrap_or(false);

            let category_filter = cli::CategoryFilter {
                contexts,
                projections,
                data,
            };

            // parallel_limit: CLI > reen.yml > built-in default
            let parallel_limit = create_args
                .parallel_limit
                .or_else(|| rc.and_then(|c| c.parallel_limit))
                .map(|v| (v as usize).max(1))
                .unwrap_or(cli::DEFAULT_PARALLEL_LIMIT);

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
                CreateCommands::Contract {
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
                        parallel_limit,
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
                        parallel_limit,
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
                        parallel_limit,
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
            let projections =
                build_args.projections || rc.and_then(|c| c.projections).unwrap_or(false);
            let data = build_args.data || rc.and_then(|c| c.data).unwrap_or(false);
            let category_filter = cli::CategoryFilter {
                contexts,
                projections,
                data,
            };

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

            let parallel_limit = build_args
                .parallel_limit
                .or_else(|| rc.and_then(|c| c.parallel_limit))
                .map(|v| (v as usize).max(1))
                .unwrap_or(cli::DEFAULT_PARALLEL_LIMIT);

            cli::build(
                build_args.names,
                clear_cache,
                &category_filter,
                rate_limit,
                token_limit,
                parallel_limit,
                max_fix_attempts,
                max_compile_fix_attempts,
                &config,
            )
            .await?;
        }
        Commands::Check(check_cmd) => match check_cmd {
            CheckCommands::Drafts { names } => {
                cli::check_drafts(names, &config).await?;
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
        Commands::Clear { cmd } => match cmd {
            None => cli::clear_all_cache_and_src(&config).await?,
            Some(ClearSubcommand::Cache(args)) => {
                let filter = cli::CategoryFilter {
                    contexts: args.contexts,
                    projections: args.projections,
                    data: args.data,
                };
                cli::clear_entire_cache_filtered(args.names, &filter, &config).await?;
            }
            Some(ClearSubcommand::Implementation(args)) => {
                let filter = cli::CategoryFilter {
                    contexts: args.contexts,
                    projections: args.projections,
                    data: args.data,
                };
                cli::clear_implementation_filtered(args.names, &filter, &config).await?;
            }
        },
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        BuildArgs, CapabilityCommands, ClearSubcommand, Cli, Commands, CreateArgs, CreateCommands,
    };
    use clap::Parser;

    #[test]
    fn parses_create_command_with_projection_and_parallel_limit() {
        let cli = Cli::parse_from([
            "reen",
            "create",
            "--projections",
            "--parallel-limit",
            "6",
            "--rate-limit",
            "1.5",
            "implementation",
            "account_summary",
        ]);

        match cli.command {
            Commands::Create(CreateArgs {
                projections,
                contexts,
                data,
                rate_limit,
                parallel_limit,
                ..
            }) => {
                assert!(projections);
                assert!(!contexts);
                assert!(!data);
                assert_eq!(rate_limit, Some(1.5));
                assert_eq!(parallel_limit, Some(6));
            }
            other => panic!(
                "expected create command, got {:?}",
                std::mem::discriminant(&other)
            ),
        }
    }

    #[test]
    fn parses_create_shared_flags_after_subcommand() {
        let cli = Cli::parse_from([
            "reen",
            "create",
            "implementation",
            "--clear-cache",
            "--contexts",
            "--projections",
            "--rate-limit",
            "1.5",
            "--parallel-limit",
            "6",
            "game_loop",
        ]);

        match cli.command {
            Commands::Create(CreateArgs {
                clear_cache,
                projections,
                contexts,
                data,
                rate_limit,
                parallel_limit,
                command: CreateCommands::Implementation { names, .. },
                ..
            }) => {
                assert!(clear_cache);
                assert!(projections);
                assert!(contexts);
                assert!(!data);
                assert_eq!(rate_limit, Some(1.5));
                assert_eq!(parallel_limit, Some(6));
                assert_eq!(names, vec!["game_loop".to_string()]);
            }
            other => panic!(
                "expected create implementation command, got {:?}",
                std::mem::discriminant(&other)
            ),
        }
    }

    #[test]
    fn parses_build_command_with_shared_and_stage_flags() {
        let cli = Cli::parse_from([
            "reen",
            "build",
            "--clear-cache",
            "--contexts",
            "--projections",
            "--rate-limit",
            "2",
            "--token-limit",
            "60000",
            "--parallel-limit",
            "7",
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
                projections,
                data,
                fix,
                max_fix_attempts,
                max_compile_fix_attempts,
                names,
                rate_limit,
                token_limit,
                parallel_limit,
                ..
            }) => {
                assert!(clear_cache);
                assert!(contexts);
                assert!(projections);
                assert!(!data);
                assert!(!fix);
                assert_eq!(max_fix_attempts, Some(4));
                assert_eq!(max_compile_fix_attempts, Some(5));
                assert_eq!(names, vec!["app".to_string(), "game_loop".to_string()]);
                assert_eq!(rate_limit, Some(2.0));
                assert_eq!(token_limit, Some(60000.0));
                assert_eq!(parallel_limit, Some(7));
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
                assert_eq!(version.as_deref(), Some("0.27"));
                assert_eq!(features, vec!["events"]);
                assert!(no_default_features);
            }
            other => panic!("unexpected command: {:?}", std::mem::discriminant(&other)),
        }
    }

    #[test]
    fn parses_capabilities_add_without_version() {
        let cli = Cli::parse_from([
            "reen",
            "capabilities",
            "add",
            "error_handling",
            "anyhow",
            "--domain",
            "errors",
        ]);
        match cli.command {
            Commands::Capabilities(CapabilityCommands::Add {
                capability,
                krate,
                domain,
                version,
                ..
            }) => {
                assert_eq!(capability, "error_handling");
                assert_eq!(krate, "anyhow");
                assert_eq!(domain, "errors");
                assert_eq!(version, None);
            }
            other => panic!("unexpected command: {:?}", std::mem::discriminant(&other)),
        }
    }

    #[test]
    fn parses_clear_with_no_subcommand() {
        let cli = Cli::parse_from(["reen", "clear"]);
        match cli.command {
            Commands::Clear { cmd } => assert!(cmd.is_none()),
            other => panic!("unexpected command: {:?}", std::mem::discriminant(&other)),
        }
    }

    #[test]
    fn parses_clear_cache_subcommand() {
        let cli = Cli::parse_from(["reen", "clear", "cache"]);
        match cli.command {
            Commands::Clear { cmd } => assert!(matches!(cmd, Some(ClearSubcommand::Cache(_)))),
            other => panic!("unexpected command: {:?}", std::mem::discriminant(&other)),
        }
    }

    #[test]
    fn parses_clear_cache_with_filter() {
        let cli = Cli::parse_from(["reen", "clear", "cache", "--data", "--contexts", "Board"]);
        match cli.command {
            Commands::Clear {
                cmd: Some(ClearSubcommand::Cache(args)),
            } => {
                assert!(args.data);
                assert!(args.contexts);
                assert!(!args.projections);
                assert_eq!(args.names, vec!["Board".to_string()]);
            }
            other => panic!("unexpected command: {:?}", std::mem::discriminant(&other)),
        }
    }

    #[test]
    fn parses_clear_implementation_subcommand() {
        let cli = Cli::parse_from(["reen", "clear", "implementation"]);
        match cli.command {
            Commands::Clear { cmd } => {
                assert!(matches!(cmd, Some(ClearSubcommand::Implementation(_))));
            }
            other => panic!("unexpected command: {:?}", std::mem::discriminant(&other)),
        }
    }

    #[test]
    fn parses_clear_implementation_with_filter() {
        let cli = Cli::parse_from([
            "reen",
            "clear",
            "implementation",
            "--contexts",
            "game_loop",
            "terminal_renderer",
        ]);
        match cli.command {
            Commands::Clear {
                cmd: Some(ClearSubcommand::Implementation(args)),
            } => {
                assert!(!args.data);
                assert!(args.contexts);
                assert!(!args.projections);
                assert_eq!(
                    args.names,
                    vec!["game_loop".to_string(), "terminal_renderer".to_string()]
                );
            }
            other => panic!("unexpected command: {:?}", std::mem::discriminant(&other)),
        }
    }
}
