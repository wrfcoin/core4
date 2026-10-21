use anyhow::Result;
use clap::Subcommand;
use serde_json::{Value, json};
use wrfcoin_cli::{ApiClient, ApiConfig};

#[derive(Subcommand)]
pub enum MonitorCommands {
    /// Show network status and health
    Status,
    /// Show network metrics snapshot
    Metrics {
        /// Refresh interval in seconds (informational only on main_api_server)
        #[clap(short, long, default_value = "5")]
        interval: u64,
    },
    /// Show blockchain performance metrics
    Blockchain,
    /// Show cross-chain bridge status
    Bridges,
    /// Show IoT sensor network status
    #[clap(name = "iot")]
    IoT,
    /// Show distributed compute network status
    Compute,
    /// Show storage network status
    Storage,
    /// Show AI/ML network status
    AIML,
}

pub async fn handle_monitor_command(action: &MonitorCommands) -> Result<Value> {
    let client = ApiClient::new(ApiConfig::default())?;

    match action {
        MonitorCommands::Status => {
            let network: Value = client.get("/network/info", None).await?;
            // Same backend/launch_api route selection as
            // `ApiClient::get_blockchain_status`: launch_api has no `/stats`,
            // so a hard `?` there errored every `monitor status` against the
            // live node.
            let stats_path = if network.get("chain_height").is_some() {
                "/mining/stats"
            } else {
                "/stats"
            };
            let stats: Value = client.get(stats_path, None).await?;
            Ok(json!({
                "summary": "Monitor status retrieved",
                "network": network,
                "stats": stats
            }))
        },
        MonitorCommands::Metrics { interval } => {
            let network: Value = client.get("/network/info", None).await?;
            let stats_path = if network.get("chain_height").is_some() {
                "/mining/stats"
            } else {
                "/stats"
            };
            let stats: Value = client.get(stats_path, None).await?;
            Ok(json!({
                "summary": "Network metrics retrieved",
                "refresh_interval_seconds": interval,
                "metrics": stats
            }))
        },
        MonitorCommands::Blockchain => {
            let network: Value = client.get("/network/info", None).await?;
            let mining: Value = client.get("/mining/stats", None).await?;
            Ok(json!({
                "summary": "Blockchain monitor data retrieved",
                "network": network,
                "mining": mining
            }))
        },
        MonitorCommands::Bridges => {
            let bridges: Value = client.get("/bridge/safety/status", None).await?;
            Ok(json!({
                "summary": "Bridge monitor data retrieved",
                "bridges": bridges
            }))
        },
        MonitorCommands::IoT => {
            let iot: Value = client.get("/iot/status", None).await?;
            Ok(json!({
                "summary": "IoT monitor data retrieved",
                "iot": iot
            }))
        },
        MonitorCommands::Compute => {
            let compute: Value = client.get("/compute/status", None).await?;
            Ok(json!({
                "summary": "Compute monitor data retrieved",
                "compute": compute
            }))
        },
        MonitorCommands::Storage => {
            let storage: Value = client.get("/storage/status", None).await?;
            Ok(json!({
                "summary": "Storage monitor data retrieved",
                "storage": storage
            }))
        },
        MonitorCommands::AIML => {
            let aiml: Value = client.get("/aiml/status", None).await?;
            Ok(json!({
                "summary": "AI/ML monitor data retrieved",
                "aiml": aiml
            }))
        },
    }
}
