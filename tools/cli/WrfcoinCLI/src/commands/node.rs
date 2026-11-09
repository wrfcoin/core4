use anyhow::Result;
use clap::Subcommand;
use serde_json::{Value, json};
use wrfcoin_cli::{ApiClient, ApiConfig};

use super::watch::{WatchConfig, watch_loop};
use crate::cli::commands::ExecutionContext;

/// Commands for node information and management
#[derive(Subcommand)]
pub enum NodeCommands {
    /// Assemble, authorize and submit archived bonded service claims.
    Receipt {
        #[clap(subcommand)]
        action: super::node_receipt::ReceiptCommands,
    },
    /// Bonded service lifecycle and on-chain inspection (default OFF).
    Bond {
        #[clap(subcommand)]
        action: super::node_bond::BondCommands,
    },
    /// Report provable Node-service fraud (operator route default OFF).
    Fraud {
        #[clap(subcommand)]
        action: super::node_fraud::FraudCommands,
    },
    /// Show node information
    Info,

    /// Show node status
    Status,

    /// List connected peers
    Peers,

    /// Show mining statistics
    MiningStats,

    /// Show sync status
    SyncStatus,

    /// Show resource usage
    Resources,

    /// Stream node status continuously
    Watch {
        /// Refresh interval in seconds
        #[clap(long, default_value = "5")]
        interval: u64,

        /// Fetch once and exit (for automation)
        #[clap(long)]
        once: bool,
    },
}

pub async fn handle_node_command(command: &NodeCommands, ctx: &ExecutionContext) -> Result<Value> {
    // This explicit-URL builder must not initialize the legacy API client or its retry policy.
    if let NodeCommands::Bond { action: action @ super::node_bond::BondCommands::GrantRequest { .. } } = command {
        return super::node_bond::handle(action).await;
    }
    let client = ApiClient::new(ApiConfig::default())?;

    match command {
        NodeCommands::Receipt { action } => super::node_receipt::handle(action).await,
        NodeCommands::Bond { action } => super::node_bond::handle(action).await,
        NodeCommands::Fraud { action } => super::node_fraud::handle(action).await,
        NodeCommands::Info => {
            let network: Value = client.get("/network/info", None).await?;
            Ok(json!({
                "summary": "Node info retrieved",
                "network": network
            }))
        },
        NodeCommands::Status => {
            let network: Value = client.get("/network/info", None).await?;
            let sync: Value = client.get("/node/sync/status", None).await?;
            Ok(json!({
                "summary": "Node status retrieved",
                "network": network,
                "sync": sync
            }))
        },
        NodeCommands::Peers => {
            let peers: Value = client.get("/network/peers", None).await?;
            Ok(json!({
                "summary": "Connected peers listed",
                "peers": peers
            }))
        },
        NodeCommands::MiningStats => {
            let stats: Value = client.get("/mining/stats", None).await?;
            Ok(json!({
                "summary": "Mining stats retrieved",
                "mining": stats
            }))
        },
        NodeCommands::SyncStatus => {
            let resp: Value = client.get("/node/sync/status", None).await?;
            Ok(json!({
                "summary": "Sync status retrieved",
                "sync_status": resp
            }))
        },
        NodeCommands::Resources => {
            let resp: Value = client.get("/node/resources", None).await?;
            Ok(json!({
                "summary": "Node resources retrieved",
                "resources": resp
            }))
        },
        NodeCommands::Watch { interval, once } => {
            let cfg = WatchConfig {
                interval: *interval,
                once: *once,
            };
            watch_loop(cfg, ctx, "node", || async {
                let c = ApiClient::new(ApiConfig::default())?;
                let network: Value = c.get("/network/info", None).await?;
                let sync: Value = c.get("/node/sync/status", None).await?;
                let peers: Value = c
                    .get("/network/peers", None)
                    .await
                    .unwrap_or_else(|_| json!({"connected_peers": 0, "note": "P2P not enabled"}));
                let resources: Value = c.get("/node/resources", None).await?;
                Ok(json!({
                    "summary": "Node status",
                    "network": network,
                    "sync": sync,
                    "peers": peers,
                    "resources": resources,
                }))
            })
            .await
        },
    }
}
