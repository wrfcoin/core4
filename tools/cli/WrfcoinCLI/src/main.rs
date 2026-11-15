use anyhow::Result;
use clap::{Parser, Subcommand};
use env_logger::Env;
use log::{error, info};
use std::path::PathBuf;
use wrfcoin_cli::config::{ApiKeySource, ConfigManager, OutputPreference};
use wrfcoin_cli::errors::{CliError, CliErrorCode};
use wrfcoin_cli::output::{OutputFormat, resolve_output_format};

// Re-enable wallet and essential command modules
mod cli;
mod commands;
mod completion;
mod crypto;
mod tui;

// Re-enable essential command imports
use cli::commands::{ExecutionContext, Wallet, handle_wallet_command};
use commands::emit::{emit_error, emit_success};
use commands::watch::{WATCH_SENTINEL, WatchConfig, watch_loop};
use commands::{
    AiMlCommands, BatchCommands, BridgeCommands, CeremonyCommands, ChainCommands, ComputeCommands,
    ContributeCommands, DataCommands, DevCommands, ForecastCommands, GovernanceCommands,
    InfraCommands, IoTCommands, MonitorCommands, NftCommands, NodeCommands, OperatorCommands,
    OracleCommands, SetupCommands, SnapshotCommands, StationCommands, StorageCommands, WalletCommands,
    WeatherCommands,
};
use completion::{
    CompletionDomain, CompletionShell, clear_completion_cache, fetch_completion_values,
    generate_completion_script, load_completion_cache, refresh_completion_cache,
};
use std::sync::Arc;
use tokio::sync::RwLock;

// Default API port
const DEFAULT_PORT: u16 = 8545;

#[derive(Parser, Debug, Clone)]
struct GlobalOptions {
    /// Emit structured JSON for automation
    #[clap(long)]
    json: bool,

    /// Emit only raw JSON data
    #[clap(long)]
    quiet: bool,

    /// Explain command decisions (routing, retries, defaults)
    #[clap(long)]
    why: bool,

    /// Output format (human, json, csv, table, quiet)
    #[clap(long, value_enum)]
    output: Option<OutputFormat>,

    /// Skip interactive prompts (answer yes to all)
    #[clap(long, short = 'y')]
    yes: bool,
}

#[derive(Subcommand)]
enum ProfileCommands {
    /// List configured CLI profiles
    List,
    /// Switch the active CLI profile
    Use {
        /// Profile name to activate
        name: String,
    },
    /// Create a new profile from the active configuration
    Create {
        /// Profile name
        name: String,
    },
    /// Show profile settings
    Show {
        /// Profile name (defaults to active profile)
        name: Option<String>,
    },
    /// Update active profile settings
    Set {
        /// Override API URL for active profile
        #[clap(long)]
        api_url: Option<String>,
        /// Preferred output mode for active profile
        #[clap(long, value_enum)]
        output_mode: Option<ProfileOutputMode>,
        /// API key lookup source
        #[clap(long, value_enum)]
        api_key_source: Option<ProfileApiKeySource>,
        /// Default wallet address
        #[clap(long)]
        default_address: Option<String>,
    },
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum ProfileOutputMode {
    Human,
    Json,
    Quiet,
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum ProfileApiKeySource {
    Env,
    Config,
}

#[derive(Subcommand)]
enum CompletionCommands {
    /// Print completion values for a specific dynamic domain
    Values {
        /// Domain to fetch values for
        #[clap(value_enum)]
        domain: CompletionDomain,
        /// Force refresh cache before returning values
        #[clap(long)]
        refresh: bool,
    },
    /// Refresh completion cache for all dynamic domains
    RefreshCache,
    /// Show the current cached completion snapshot
    ShowCache,
    /// Clear cached completion data
    ClearCache,
}

#[derive(Parser)]
// #2721: `version` was the literal "0.1.0" — identical for every commit in
// the repository's history, so `wrfcoin --version` could not distinguish a
// binary built from the genesis cut commit from one built from anything
// else. The semver now comes from the manifest via CARGO_PKG_VERSION and
// carries the commit, the tree state and the build timestamp beside it.
// `long_version` is what `--version` prints; `version` keeps the one-line
// form used in `--help` and in error headers. Neither carries the program
// name: clap prepends it, and a string that carried its own printed
// `wrfcoin wrfcoin 0.1.0 (...)`.
#[clap(
    name = "wrfcoin",
    about = "Wrfcoin Blockchain CLI",
    version = wrfcoin_build_provenance::version_suffix_static(env!("CARGO_PKG_VERSION")),
    long_version = wrfcoin_build_provenance::version_detail_body_static(env!("CARGO_PKG_VERSION"))
)]
struct Cli {
    /// Optional config file path
    #[clap(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    #[clap(flatten)]
    global: GlobalOptions,

    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start a Wrfcoin node
    Start {
        /// Run the node with API server
        #[clap(short, long)]
        api: bool,

        /// Mining mode (if enabled, the node will start mining blocks)
        #[clap(short, long)]
        mine: bool,

        /// API server port
        #[clap(short = 'p', long, default_value_t = DEFAULT_PORT)]
        port: u16,

        /// Enable metrics collection
        #[clap(short = 'M', long)]
        metrics: bool,

        /// Enable checkpoints
        #[clap(short, long)]
        checkpoints: bool,

        /// Run in specific network mode
        #[clap(short = 'n', long, default_value = "testnet")]
        network: String,
    },

    /// Wallet management commands
    Wallet {
        #[clap(subcommand)]
        action: WalletCommands,
    },
    /// Profile and workspace helpers
    Profile {
        #[clap(subcommand)]
        action: ProfileCommands,
    },
    /// Show effective CLI configuration (including active profile)
    Config,
    /// Generate shell completion scripts (supports dynamic API-backed values)
    Completions {
        #[clap(value_enum)]
        shell: CompletionShell,
        /// Refresh dynamic completion cache before generating script
        #[clap(long)]
        refresh_cache: bool,
    },
    /// Internal completion data endpoints used by generated shell scripts
    #[clap(hide = true)]
    Completion {
        #[clap(subcommand)]
        action: CompletionCommands,
    },
    /// Blockchain management and information commands
    Chain {
        #[clap(subcommand)]
        action: ChainCommands,
    },

    /// NFT management commands
    Nft {
        #[clap(subcommand)]
        action: NftCommands,
    },

    /// Node information and management
    Node {
        #[clap(subcommand)]
        action: NodeCommands,
    },

    /// Snapshot join: fetch a verified state snapshot into the local node
    Snapshot {
        #[clap(subcommand)]
        action: SnapshotCommands,
    },

    /// Cross-chain bridge operations
    Bridge {
        #[clap(subcommand)]
        action: BridgeCommands,
    },

    /// Computational integration commands for distributed compute tasks
    Compute {
        #[clap(subcommand)]
        action: ComputeCommands,
    },

    /// Weather data commands
    Weather {
        #[clap(subcommand)]
        action: WeatherCommands,
    },

    /// Forecast commands
    Forecast {
        #[clap(subcommand)]
        action: ForecastCommands,
    },

    /// Environment setup and configuration commands
    Setup {
        #[clap(subcommand)]
        action: SetupCommands,
    },

    /// Network and blockchain monitoring commands
    Monitor {
        #[clap(subcommand)]
        action: MonitorCommands,
    },

    /// Genesis oracle registration ceremony commands
    Oracle {
        #[clap(subcommand)]
        action: OracleCommands,
    },

    /// Remote ceremony kit (#3096): sign a possession proof on your own box,
    /// assemble and verify proofs as coordinator, check key custody
    Ceremony {
        #[clap(subcommand)]
        action: CeremonyCommands,
    },

    /// Weather-station registration artifact commands (#2537)
    Station {
        #[clap(subcommand)]
        action: StationCommands,
    },

    /// Session (device) keys: keygen, register / revoke a scoped key, and sign
    /// a transfer with it, so everyday sends never touch the primary key.
    SessionKey {
        #[clap(subcommand)]
        action: commands::session_key::SessionKeyCommands,
    },

    /// Chain-enforced send allowlist: enable, add (time-locked), remove,
    /// cancel, disable (time-locked), show
    SendAllowlist {
        #[clap(subcommand)]
        action: commands::send_allowlist::SendAllowlistCommands,
    },

    /// Governance participation commands
    Governance {
        #[clap(subcommand)]
        action: GovernanceCommands,
    },

    /// Data provider integration commands
    Data {
        #[clap(subcommand)]
        action: DataCommands,
    },

    /// AI/ML network commands
    Aiml {
        #[clap(subcommand)]
        action: AiMlCommands,
    },

    /// Developer utility commands
    Dev {
        #[clap(subcommand)]
        action: DevCommands,
    },

    /// Infrastructure overview and monitoring commands
    Infra {
        #[clap(subcommand)]
        action: InfraCommands,
    },

    /// Decentralized storage commands
    Storage {
        #[clap(subcommand)]
        action: StorageCommands,
    },

    /// IoT sensor network commands
    Iot {
        #[clap(subcommand)]
        action: IoTCommands,
    },

    /// Batch execution of multiple commands from JSON Lines
    Batch {
        #[clap(subcommand)]
        action: BatchCommands,
    },

    /// Weather station contributor workflow
    Contribute {
        #[clap(subcommand)]
        action: ContributeCommands,
    },

    /// Interactive TUI dashboard for monitoring node, weather, network, and wallet
    Tui {
        /// Data refresh interval in seconds
        #[clap(long, default_value_t = 5)]
        interval: u64,

        /// Weather station ID to monitor
        #[clap(long)]
        station_id: Option<String>,

        /// Wallet address to monitor
        #[clap(long)]
        address: Option<String>,
    },

    /// Multi-pane live dashboard combining all monitoring views
    Live {
        /// Data refresh interval in seconds
        #[clap(long, default_value_t = 5)]
        interval: u64,

        /// Weather station ID to monitor
        #[clap(long)]
        station_id: Option<String>,

        /// Wallet address to monitor
        #[clap(long)]
        address: Option<String>,

        /// Layout mode: quad, vertical, horizontal, focus
        #[clap(long, default_value = "quad")]
        layout: String,
    },

    /// Operator incident mode: health checks and remediation
    Operator {
        #[clap(subcommand)]
        action: OperatorCommands,
    },
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        // Command handlers normally emit through `emit_error`, but failures
        // while constructing the CLI context happen before a handler exists.
        // A silent nonzero exit makes operational scripts unable to diagnose a
        // failed genesis ceremony (#2614).
        eprintln!("wrfcoin: {err:#}");
        std::process::exit(commands::emit::classify_exit_code(&err));
    }
}

async fn run() -> Result<()> {
    // Initialize the logger
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    // Parse command-line arguments
    let cli = Cli::parse();
    if cli.global.why {
        std::env::set_var("WRFCOIN_CLI_WHY", "1");
    }

    let manager = if let Some(path) = cli.config.as_ref() {
        ConfigManager::with_path(path.clone())?
    } else {
        ConfigManager::new()?
    };
    let pref = manager.get_config().preferences.output_mode.clone();
    let output_format =
        resolve_output_format(cli.global.output, cli.global.json, cli.global.quiet, &pref);
    let json = matches!(output_format, OutputFormat::Json);
    let quiet = matches!(output_format, OutputFormat::Quiet);
    let context = ExecutionContext::new(json, quiet, cli.global.why, output_format, cli.global.yes);

    // Handle the command
    match cli.command {
        Commands::Start {
            api,
            mine,
            port,
            metrics,
            checkpoints,
            network,
        } => {
            info!("Starting Wrfcoin node");

            let network_mode = network.to_lowercase();
            if network_mode != "testnet" && network_mode != "mainnet" {
                error!(
                    "Invalid network mode: {}. Must be 'testnet' or 'mainnet'",
                    network
                );
                return Err(anyhow::anyhow!("Invalid network mode"));
            }

            info!("Network mode: {}", network_mode);

            if api {
                info!("API server enabled on port {}", port);
            }

            if mine {
                info!("Mining enabled");
            }

            if metrics {
                info!("Metrics collection enabled");
            }

            if checkpoints {
                info!("Checkpoints enabled");
            }

            println!("Wrfcoin node started successfully in {} mode", network_mode);

            // Keep the application running
            // In a real implementation, this would start the node service
            tokio::signal::ctrl_c().await?;
            info!("Shutting down");
        },

        Commands::Wallet { action } => {
            // Handle Watch separately (it runs a loop and emits its own output)
            if let WalletCommands::Watch {
                interval,
                once,
                address,
            } = &action
            {
                let addr = address.clone();
                let cfg = WatchConfig {
                    interval: *interval,
                    once: *once,
                };
                match watch_loop(cfg, &context, "wallet", || {
                    let a = addr.clone();
                    async move {
                        let client =
                            wrfcoin_cli::ApiClient::new(wrfcoin_cli::ApiConfig::default())?;
                        let target = match &a {
                            Some(addr) => addr.clone(),
                            None => {
                                let home = dirs::home_dir()
                                    .ok_or_else(|| anyhow::anyhow!("Home directory not found"))?;
                                let manager = wrfcoin_cli::KeyManager::new(&home.join(".wrfcoin"))?;
                                let pairs = manager.list_key_pairs()?;
                                pairs
                                    .first()
                                    .map(|(addr, _)| addr.clone())
                                    .ok_or_else(|| anyhow::anyhow!("No wallet address found"))?
                            },
                        };
                        let wallet_data: serde_json::Value =
                            client.get(&format!("/wallet/{}", target), None).await?;
                        Ok(serde_json::json!({
                            "summary": format!("Wallet {}", target),
                            "address": target,
                            "wallet": wallet_data,
                        }))
                    }
                })
                .await
                {
                    Ok(_) => {},
                    Err(err) => {
                        emit_error(&context, "wallet", &err)?;
                        return Err(err);
                    },
                }
            } else {
                // Handle interactive send, swap, price, liquidity with early returns
                if let WalletCommands::Send {
                    to,
                    amount,
                    fee,
                    memo,
                    interactive,
                    dry_run,
                    nonce,
                } = &action
                {
                    if let Err(err) = commands::send_interactive::reject_unsupported_send_options(
                        *fee,
                        memo.as_deref(),
                    ) {
                        emit_error(&context, "wallet", &err)?;
                        return Err(err);
                    }
                    if *interactive || *dry_run {
                        match commands::send_interactive::handle_send_interactive(
                            to,
                            *amount,
                            *dry_run,
                            *nonce,
                            &context,
                        )
                        .await
                        {
                            Ok(data) => emit_success(&context, "wallet", &data)?,
                            Err(err) => {
                                emit_error(&context, "wallet", &err)?;
                                return Err(err);
                            },
                        }
                        return Ok(());
                    }
                }

                if let WalletCommands::Swap {
                    from,
                    to,
                    amount,
                    slippage,
                    dry_run,
                } = &action
                {
                    match commands::swap::handle_swap(
                        from, to, *amount, *slippage, *dry_run, &context,
                    )
                    .await
                    {
                        Ok(data) => emit_success(&context, "wallet", &data)?,
                        Err(err) => {
                            emit_error(&context, "wallet", &err)?;
                            return Err(err);
                        },
                    }
                    return Ok(());
                }

                if let WalletCommands::SecurityKey(command) = &action {
                    match commands::security_key::handle_security_key_command(command).await {
                        Ok(data) => emit_success(&context, "wallet", &data)?,
                        Err(err) => {
                            emit_error(&context, "wallet", &err)?;
                            return Err(err);
                        },
                    }
                    return Ok(());
                }

                if let WalletCommands::Price { pair } = &action {
                    match commands::swap::handle_price(pair).await {
                        Ok(data) => emit_success(&context, "wallet", &data)?,
                        Err(err) => {
                            emit_error(&context, "wallet", &err)?;
                            return Err(err);
                        },
                    }
                    return Ok(());
                }

                if let WalletCommands::Liquidity { pair } = &action {
                    match commands::swap::handle_liquidity(pair).await {
                        Ok(data) => emit_success(&context, "wallet", &data)?,
                        Err(err) => {
                            emit_error(&context, "wallet", &err)?;
                            return Err(err);
                        },
                    }
                    return Ok(());
                }

                // Initialize wallet subsystem
                // #2735: propagate wallet-init failure (with the failing file
                // named) instead of continuing on a cwd-rooted fallback wallet.
                let wallet = Arc::new(RwLock::new(Wallet::try_default()?));

                // Convert the commands::WalletCommands to cli::commands::WalletCommands
                let cli_action = match action {
                    WalletCommands::NewAddress => cli::commands::WalletCommands::NewAddress,
                    WalletCommands::Recover { mnemonic } => {
                        cli::commands::WalletCommands::Recover {
                            mnemonic: mnemonic.clone(),
                        }
                    },
                    WalletCommands::ListAddresses => cli::commands::WalletCommands::ListAddresses,
                    WalletCommands::Balance { address } => cli::commands::WalletCommands::Balance {
                        address: address.clone(),
                    },
                    WalletCommands::SetPrimary { address } => {
                        cli::commands::WalletCommands::SetPrimary {
                            address: address.clone(),
                        }
                    },
                    WalletCommands::Send {
                        to, amount, nonce, ..
                    } => cli::commands::WalletCommands::Send {
                        to: to.clone(),
                        amount,
                        nonce,
                    },
                    WalletCommands::History {
                        address: _,
                        limit: _,
                    } => cli::commands::WalletCommands::History,
                    WalletCommands::MigrateAddresses { dry_run, yes } => {
                        cli::commands::WalletCommands::MigrateAddresses { dry_run, yes }
                    },
                    WalletCommands::Watch { .. }
                    | WalletCommands::Swap { .. }
                    | WalletCommands::Price { .. }
                    | WalletCommands::Liquidity { .. }
                    | WalletCommands::SecurityKey(_) => unreachable!(),
                };

                match handle_wallet_command(&cli_action, wallet).await {
                    Ok(data) => emit_success(&context, "wallet", &data)?,
                    Err(err) => {
                        emit_error(&context, "wallet", &err)?;
                        return Err(err);
                    },
                }
            }
        },
        Commands::Profile { action } => {
            handle_profile_command(&action, cli.config.as_ref())?;
        },
        Commands::Config => {
            let manager = if let Some(path) = cli.config.as_ref() {
                ConfigManager::with_path(path.clone())?
            } else {
                ConfigManager::new()?
            };
            let active = manager.active_profile().to_string();
            let config = manager.get_config().clone();
            let payload = serde_json::json!({
                "active_profile": active,
                "config_source": format!("{:?}", manager.config_source()),
                "config_path": manager.get_config_path().display().to_string(),
                "config": config
            });
            emit_success(&context, "config", &payload)?;
        },
        Commands::Completions {
            shell,
            refresh_cache,
        } => {
            let api_url = resolve_api_url(cli.config.as_ref())?;
            if refresh_cache {
                refresh_completion_cache(&api_url, cli.config.as_ref()).await?;
            }

            let script = generate_completion_script(shell);
            println!("{}", script);
        },
        Commands::Completion { action } => {
            let api_url = resolve_api_url(cli.config.as_ref())?;
            match action {
                CompletionCommands::Values { domain, refresh } => {
                    let values =
                        fetch_completion_values(domain, &api_url, cli.config.as_ref(), refresh)
                            .await?;
                    for value in values {
                        println!("{}", value);
                    }
                },
                CompletionCommands::RefreshCache => {
                    let cache = refresh_completion_cache(&api_url, cli.config.as_ref()).await?;
                    println!(
                        "completion cache updated: bridges={}, peers={}, stations={}",
                        cache.bridges.len(),
                        cache.peers.len(),
                        cache.stations.len()
                    );
                },
                CompletionCommands::ShowCache => {
                    let cache = load_completion_cache(cli.config.as_ref())?;
                    println!("{}", serde_json::to_string_pretty(&cache)?);
                },
                CompletionCommands::ClearCache => {
                    clear_completion_cache(cli.config.as_ref())?;
                    println!("completion cache cleared");
                },
            }
        },
        Commands::Chain { action } => match commands::chain::handle_chain_command(&action).await {
            Ok(data) => emit_success(&context, "chain", &data)?,
            Err(err) => {
                emit_error(&context, "chain", &err)?;
                return Err(err);
            },
        },
        Commands::Nft { action } => match commands::nft::handle_nft_command(&action).await {
            Ok(data) => emit_success(&context, "nft", &data)?,
            Err(err) => {
                emit_error(&context, "nft", &err)?;
                return Err(err);
            },
        },
        Commands::Snapshot { action } => {
            match commands::snapshot::handle_snapshot_command(&action, &context).await {
                Ok(data) => emit_success(&context, "snapshot", &data)?,
                Err(err) => {
                    emit_error(&context, "snapshot", &err)?;
                    return Err(err);
                },
            }
        },
        Commands::Node { action } => {
            match commands::node::handle_node_command(&action, &context).await {
                Ok(data) => {
                    if data.get(WATCH_SENTINEL).is_none() {
                        emit_success(&context, "node", &data)?;
                    }
                },
                Err(err) => {
                    emit_error(&context, "node", &err)?;
                    return Err(err);
                },
            }
        },
        Commands::Bridge { action } => {
            match commands::bridge::handle_bridge_command(&action, &context).await {
                Ok(data) => {
                    if data.get(WATCH_SENTINEL).is_none() {
                        emit_success(&context, "bridge", &data)?;
                    }
                },
                Err(err) => {
                    emit_error(&context, "bridge", &err)?;
                    return Err(err);
                },
            }
        },
        Commands::Compute { action } => {
            match commands::compute::handle_compute_command(&action).await {
                Ok(data) => emit_success(&context, "compute", &data)?,
                Err(err) => {
                    emit_error(&context, "compute", &err)?;
                    return Err(err);
                },
            }
        },
        Commands::Weather { action } => {
            match commands::weather::handle_weather_command(&action).await {
                Ok(data) => emit_success(&context, "weather", &data)?,
                Err(err) => {
                    emit_error(&context, "weather", &err)?;
                    return Err(err);
                },
            }
        },
        Commands::Forecast { action } => {
            match commands::forecast::handle_forecast_command(&action, &context).await {
                Ok(data) => {
                    if data.get(WATCH_SENTINEL).is_none() {
                        emit_success(&context, "forecast", &data)?;
                    }
                },
                Err(err) => {
                    emit_error(&context, "forecast", &err)?;
                    return Err(err);
                },
            }
        },
        Commands::Setup { action } => match commands::setup::handle_setup_command(&action).await {
            Ok(data) => emit_success(&context, "setup", &data)?,
            Err(err) => {
                emit_error(&context, "setup", &err)?;
                return Err(err);
            },
        },
        Commands::Monitor { action } => {
            match commands::monitor::handle_monitor_command(&action).await {
                Ok(data) => emit_success(&context, "monitor", &data)?,
                Err(err) => {
                    emit_error(&context, "monitor", &err)?;
                    return Err(err);
                },
            }
        },
        Commands::Oracle { action } => {
            match commands::oracle::handle_oracle_command(&action).await {
                Ok(data) => emit_success(&context, "oracle", &data)?,
                Err(err) => {
                    emit_error(&context, "oracle", &err)?;
                    return Err(err);
                },
            }
        },
        Commands::Ceremony { action } => {
            match commands::ceremony::handle_ceremony_command(&action) {
                Ok(data) => emit_success(&context, "ceremony", &data)?,
                Err(err) => {
                    emit_error(&context, "ceremony", &err)?;
                    return Err(err);
                },
            }
        },
        Commands::SessionKey { action } => {
            match commands::session_key::handle_session_key_command(&action).await {
                Ok(data) => emit_success(&context, "session-key", &data)?,
                Err(err) => {
                    emit_error(&context, "session-key", &err)?;
                    return Err(err);
                },
            }
        },
        Commands::SendAllowlist { action } => {
            match commands::send_allowlist::handle_send_allowlist_command(&action).await {
                Ok(data) => emit_success(&context, "send-allowlist", &data)?,
                Err(err) => {
                    emit_error(&context, "send-allowlist", &err)?;
                    return Err(err);
                },
            }
        },
        Commands::Station { action } => {
            match commands::station::handle_station_command(&action).await {
                Ok(data) => emit_success(&context, "station", &data)?,
                Err(err) => {
                    emit_error(&context, "station", &err)?;
                    return Err(err);
                },
            }
        },
        Commands::Governance { action } => {
            match commands::governance::handle_governance_command(&action).await {
                Ok(data) => emit_success(&context, "governance", &data)?,
                Err(err) => {
                    emit_error(&context, "governance", &err)?;
                    return Err(err);
                },
            }
        },
        Commands::Data { action } => match commands::data::handle_data_command(&action).await {
            Ok(data) => emit_success(&context, "data", &data)?,
            Err(err) => {
                emit_error(&context, "data", &err)?;
                return Err(err);
            },
        },
        Commands::Aiml { action } => match commands::aiml::handle_aiml_command(&action).await {
            Ok(data) => emit_success(&context, "aiml", &data)?,
            Err(err) => {
                emit_error(&context, "aiml", &err)?;
                return Err(err);
            },
        },
        Commands::Dev { action } => match commands::dev::handle_dev_command(&action).await {
            Ok(data) => emit_success(&context, "dev", &data)?,
            Err(err) => {
                emit_error(&context, "dev", &err)?;
                return Err(err);
            },
        },
        Commands::Infra { action } => match commands::infra::handle_infra_command(&action).await {
            Ok(data) => emit_success(&context, "infra", &data)?,
            Err(err) => {
                emit_error(&context, "infra", &err)?;
                return Err(err);
            },
        },
        Commands::Storage { action } => {
            match commands::storage::handle_storage_command(&action).await {
                Ok(data) => emit_success(&context, "storage", &data)?,
                Err(err) => {
                    emit_error(&context, "storage", &err)?;
                    return Err(err);
                },
            }
        },
        Commands::Iot { action } => match commands::iot::handle_iot_command(&action).await {
            Ok(data) => emit_success(&context, "iot", &data)?,
            Err(err) => {
                emit_error(&context, "iot", &err)?;
                return Err(err);
            },
        },
        Commands::Batch { action } => {
            match commands::batch::handle_batch_command(&action, &context).await {
                Ok(data) => emit_success(&context, "batch", &data)?,
                Err(err) => {
                    emit_error(&context, "batch", &err)?;
                    return Err(err);
                },
            }
        },
        Commands::Contribute { action } => {
            match commands::contribute::handle_contribute_command(&action, &context).await {
                Ok(data) => {
                    if data.get(WATCH_SENTINEL).is_none() {
                        emit_success(&context, "contribute", &data)?;
                    }
                },
                Err(err) => {
                    emit_error(&context, "contribute", &err)?;
                    return Err(err);
                },
            }
        },
        Commands::Tui {
            interval,
            station_id,
            address,
        } => {
            tui::run_tui(interval, station_id, address).await?;
        },
        Commands::Live {
            interval,
            station_id,
            address,
            layout,
        } => {
            tui::live::run_live(interval, station_id, address, &layout).await?;
        },
        Commands::Operator { action } => {
            match commands::operator::handle_operator_command(&action, &context).await {
                Ok(data) => {
                    if data.get(WATCH_SENTINEL).is_none() {
                        emit_success(&context, "operator", &data)?;
                    }
                },
                Err(err) => {
                    emit_error(&context, "operator", &err)?;
                    return Err(err);
                },
            }
        },
    }

    Ok(())
}

fn resolve_api_url(config_path: Option<&PathBuf>) -> Result<String> {
    if let Ok(url) = std::env::var("WRFCOIN_API_URL") {
        if !url.trim().is_empty() {
            return Ok(url);
        }
    }

    let manager = if let Some(path) = config_path {
        ConfigManager::with_path(path.clone())?
    } else {
        ConfigManager::new()?
    };
    Ok(manager.get_config().api_url())
}

fn handle_profile_command(action: &ProfileCommands, config_path: Option<&PathBuf>) -> Result<()> {
    let mut manager = if let Some(path) = config_path {
        ConfigManager::with_path(path.clone())?
    } else {
        ConfigManager::new()?
    };

    match action {
        ProfileCommands::List => {
            println!("Configured profiles:");
            for profile in manager.list_profiles() {
                if profile == manager.active_profile() {
                    println!("* {}", profile);
                } else {
                    println!("  {}", profile);
                }
            }
        },
        ProfileCommands::Use { name } => {
            manager.use_profile(name)?;
            println!("Switched active profile to '{}'", name);
        },
        ProfileCommands::Create { name } => {
            let config = manager.get_config().clone();
            manager.add_profile(name.clone(), config)?;
            println!("Created profile '{}'", name);
        },
        ProfileCommands::Show { name } => {
            let target = name
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| manager.active_profile().to_string());
            let profile = manager.get_profile(&target).cloned().ok_or_else(|| {
                CliError::new(
                    CliErrorCode::ConfigError,
                    format!("Profile '{}' does not exist", target),
                )
            })?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "profile": target,
                    "active_profile": manager.active_profile(),
                    "config": profile
                }))?
            );
        },
        ProfileCommands::Set {
            api_url,
            output_mode,
            api_key_source,
            default_address,
        } => {
            let parsed_api = if let Some(url) = api_url.as_ref() {
                let parsed = url::Url::parse(url)
                    .map_err(|e| anyhow::anyhow!("Invalid --api-url '{}': {}", url, e))?;
                Some((
                    parsed.scheme().to_string(),
                    parsed
                        .host_str()
                        .ok_or_else(|| anyhow::anyhow!("Invalid --api-url host"))?
                        .to_string(),
                    parsed
                        .port_or_known_default()
                        .ok_or_else(|| anyhow::anyhow!("Invalid --api-url port"))?,
                ))
            } else {
                None
            };
            manager.update_config(|cfg| {
                if let Some((protocol, host, port)) = parsed_api.as_ref() {
                    cfg.api.protocol = protocol.clone();
                    cfg.api.host = host.clone();
                    cfg.api.port = *port;
                }
                if let Some(mode) = output_mode {
                    cfg.preferences.output_mode = match mode {
                        ProfileOutputMode::Human => OutputPreference::Human,
                        ProfileOutputMode::Json => OutputPreference::Json,
                        ProfileOutputMode::Quiet => OutputPreference::Quiet,
                    };
                }
                if let Some(source) = api_key_source {
                    cfg.preferences.api_key_source = match source {
                        ProfileApiKeySource::Env => ApiKeySource::Env,
                        ProfileApiKeySource::Config => ApiKeySource::Config,
                    };
                }
                if let Some(address) = default_address {
                    cfg.wallet.default_address = Some(address.clone());
                }
            })?;
            println!("Updated active profile '{}'", manager.active_profile());
        },
    }

    Ok(())
}
