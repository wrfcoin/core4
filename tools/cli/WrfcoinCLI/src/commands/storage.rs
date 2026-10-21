use anyhow::Result;
use clap::Subcommand;
use serde_json::{Value, json};
use wrfcoin_cli::{ApiClient, ApiConfig, ServiceTarget};

#[derive(Subcommand)]
pub enum StorageCommands {
    /// List available storage networks
    List,
    /// Show storage network status
    Status {
        /// Network name
        network: Option<String>,
    },
    /// Check storage capacity
    Capacity {
        /// Network name
        network: String,
    },
    /// Store data on decentralized storage
    Store {
        /// Data to store
        data: String,
        /// Target network (filecoin, arweave, storj, sia, ipfs)
        network: String,
    },
    /// Retrieve data from storage
    Retrieve {
        /// Content hash
        hash: String,
        /// Source network
        network: String,
    },
}

/// `storage` talks to backend's storage-networks service (port 7000), not the
/// node — see `ServiceTarget::Storage` (#3204). That service is still a
/// simulation (backend#1117); every result says so in its `target` block.
pub async fn handle_storage_command(command: &StorageCommands) -> Result<Value> {
    let config = ApiConfig::for_service(ServiceTarget::Storage);
    let client = ApiClient::new(config.clone())?;
    let mut out = handle_storage_inner(&client, command).await?;
    ServiceTarget::Storage.attach(&config, &mut out);
    Ok(out)
}

async fn handle_storage_inner(client: &ApiClient, command: &StorageCommands) -> Result<Value> {
    match command {
        StorageCommands::List => {
            let networks = json!([
                {"name": "filecoin", "capacity": "8.5 EiB", "status": "catalog"},
                {"name": "arweave", "capacity": "135+ TB", "status": "catalog"},
                {"name": "storj", "capacity": "29.5 PB", "status": "catalog"},
                {"name": "sia", "capacity": "3.8 PB", "status": "catalog"},
                {"name": "ipfs", "type": "content-addressed", "status": "catalog"},
                {"name": "bigchaindb", "type": "blockchain-database", "status": "catalog"}
            ]);
            Ok(json!({
                "summary": "Storage network catalog (static; not a live status)",
                "catalog_only": true,
                "networks": networks
            }))
        },
        StorageCommands::Status { network: _ } => {
            // backend registers /storage/networks (+ /storage/metrics), not /storage/status.
            let result: Value = client.get("/storage/networks", None).await?;
            Ok(json!({ "summary": "Storage status retrieved", "status": result }))
        },
        StorageCommands::Capacity { network } => Ok(json!({
            "summary": format!("Storage capacity for {}", network),
            "network": network,
            "capacity": match network.as_str() {
                "filecoin" => "8.5 EiB",
                "arweave" => "135+ TB",
                "storj" => "29.5 PB",
                "sia" => "3.8 PB",
                _ => "unknown",
            }
        })),
        StorageCommands::Store { data, network } => {
            let result: Value = client
                .post(
                    "/storage/store",
                    Some(json!({
                        "data": data,
                        "storageType": network,
                        "redundancy": 3
                    })),
                )
                .await?;
            Ok(json!({ "summary": "Data stored (SIMULATED by the target; see target.note)", "result": result }))
        },
        StorageCommands::Retrieve { hash, network } => {
            // backend's retrieve is POST /storage/retrieve { network, hash }.
            let result: Value = client
                .post(
                    "/storage/retrieve",
                    Some(json!({ "hash": hash, "network": network })),
                )
                .await?;
            Ok(json!({ "summary": "Data retrieved (SIMULATED by the target; see target.note)", "result": result }))
        },
    }
}
