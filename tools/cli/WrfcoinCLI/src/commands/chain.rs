use anyhow::{Result, anyhow};
use clap::Subcommand;
use serde_json::{Value, json};
use std::fs;
use wrfcoin_cli::{ApiClient, ApiConfig};

/// Commands for blockchain management
#[derive(Subcommand)]
pub enum ChainCommands {
    /// Display blockchain information
    Info,

    /// Get details about a specific block
    Block {
        /// Block hash
        #[clap(long)]
        hash: Option<String>,

        /// Block height
        #[clap(long)]
        height: Option<u64>,
    },

    /// List blocks
    ListBlocks {
        /// Maximum number of blocks to show
        #[clap(long, default_value = "10")]
        limit: u32,

        /// Start listing from this height (if omitted, starts from the latest)
        #[clap(long)]
        start_height: Option<u64>,
    },

    /// List pending transactions in the mempool
    PendingTransactions,

    /// Export blockchain data
    Export {
        /// Output file
        #[clap(long)]
        output: String,

        /// Export format (json, csv)
        #[clap(long, default_value = "json")]
        format: String,

        /// Start height for export
        #[clap(long)]
        start_height: Option<u64>,

        /// End height for export
        #[clap(long)]
        end_height: Option<u64>,
    },

    /// Import blockchain data
    Import {
        /// Input file
        #[clap(long)]
        input: String,

        /// Skip validation
        #[clap(long)]
        skip_validation: bool,
    },
}

pub async fn handle_chain_command(command: &ChainCommands) -> Result<Value> {
    let client = ApiClient::new(ApiConfig::default())?;

    match command {
        ChainCommands::Info => {
            let network: Value = client.get("/network/info", None).await?;
            Ok(json!({
                "summary": "Chain info retrieved",
                "network": network
            }))
        },
        ChainCommands::Block { hash, height } => {
            let path = if let Some(h) = height {
                format!("/chain/block?height={}", h)
            } else if let Some(h) = hash {
                format!("/chain/block?hash={}", h)
            } else {
                return Err(anyhow!("Provide --hash or --height"));
            };
            let block: Value = client.get(&path, None).await?;
            Ok(json!({
                "summary": "Block details retrieved",
                "block": block
            }))
        },
        ChainCommands::ListBlocks {
            limit,
            start_height,
        } => {
            let path = if let Some(start) = start_height {
                format!("/chain/blocks?limit={}&startHeight={}", limit, start)
            } else {
                format!("/chain/blocks?limit={}", limit)
            };
            let blocks: Value = client.get(&path, None).await?;
            Ok(json!({
                "summary": "Blocks listed",
                "blocks": blocks
            }))
        },
        ChainCommands::PendingTransactions => {
            let pending: Value = client.get("/chain/pending-transactions", None).await?;
            Ok(json!({
                "summary": "Pending transactions retrieved",
                "pending_transactions": pending
            }))
        },
        ChainCommands::Export {
            output,
            format,
            start_height,
            end_height,
        } => {
            let body = json!({
                "format": format,
                "startHeight": start_height,
                "endHeight": end_height
            });
            let exported: Value = client.post("/chain/export", Some(body)).await?;
            fs::write(output, serde_json::to_string_pretty(&exported)?)?;
            Ok(json!({
                "summary": "Chain export completed",
                "output": output,
                "export": exported
            }))
        },
        ChainCommands::Import {
            input,
            skip_validation,
        } => {
            let raw = fs::read_to_string(input)?;
            let parsed = serde_json::from_str::<Value>(&raw).unwrap_or_else(|_| json!(raw));
            let body = json!({
                "data": parsed,
                "skipValidation": skip_validation
            });
            let imported: Value = client.post("/chain/import", Some(body)).await?;
            Ok(json!({
                "summary": "Chain import request accepted",
                "input": input,
                "result": imported
            }))
        },
    }
}
