use anyhow::Result;
use clap::Subcommand;
use serde_json::{Value, json};
use std::fs;
use wrfcoin_cli::{ApiClient, ApiConfig, ServiceTarget};

/// Commands for computational task management
#[derive(Subcommand)]
pub enum ComputeCommands {
    /// List all computational providers and their status
    ListProviders,

    /// Submit a computational task to a provider
    SubmitTask {
        /// Provider to use (e.g., "bacalhau", "golem", "akash")
        #[clap(long)]
        provider: String,

        /// Task specification file (JSON format)
        #[clap(long)]
        spec_file: String,

        /// Budget for the task (in WRF tokens)
        #[clap(long)]
        budget: Option<f64>,

        /// Timeout for the task in seconds
        #[clap(long, default_value = "3600")]
        timeout: u64,
    },

    /// Check the status of a submitted task
    TaskStatus {
        /// Task ID
        #[clap(long)]
        id: String,
    },

    /// Retrieve results from a completed task
    GetResults {
        /// Task ID
        #[clap(long)]
        id: String,

        /// Output file to save results
        #[clap(long)]
        output: Option<String>,
    },

    /// Register as a compute provider
    RegisterProvider {
        /// Provider type
        #[clap(long)]
        provider_type: String,

        /// Available resources in JSON format
        #[clap(long)]
        resources: String,

        /// Pricing model in JSON format
        #[clap(long)]
        pricing: String,
    },

    /// Setup the Golem Network integration
    SetupGolem {
        /// Golem node address
        #[clap(long)]
        node_address: String,

        /// Golem API key
        #[clap(long)]
        api_key: String,

        /// Resource limits in JSON format
        #[clap(long)]
        resource_limits: Option<String>,

        /// Payment configuration in JSON format
        #[clap(long)]
        payment_config: Option<String>,
    },

    /// Submit a computation task to Golem
    GolemTask {
        /// Task specification file (JSON format)
        #[clap(long)]
        spec_file: String,

        /// Budget limit in GLM tokens
        #[clap(long)]
        budget: Option<f64>,
    },

    /// Configure Bacalhau integration
    SetupBacalhau {
        /// Bacalhau endpoint URL
        #[clap(long)]
        endpoint: String,

        /// Authentication token
        #[clap(long)]
        auth_token: Option<String>,
    },

    /// Step-by-step guide for setting up a WRF compute node and earning rewards
    HowToMine {
        /// Node type to configure: wrf-model | validator | storage
        #[clap(long, default_value = "wrf-model")]
        node_type: String,
    },
}

/// `compute` talks to backend's compute-networks service (port 6000), not the
/// node — see `ServiceTarget::Compute` (#3204). Every result carries a
/// `target` block naming the base URL and the service's honest status.
pub async fn handle_compute_command(command: &ComputeCommands) -> Result<Value> {
    let config = ApiConfig::for_service(ServiceTarget::Compute);
    let client = ApiClient::new(config.clone())?;
    let mut out = handle_compute_inner(&client, command).await?;
    ServiceTarget::Compute.attach(&config, &mut out);
    Ok(out)
}

async fn handle_compute_inner(client: &ApiClient, command: &ComputeCommands) -> Result<Value> {
    match command {
        ComputeCommands::ListProviders => {
            // backend serves the provider catalog at /compute/networks; it is a
            // design catalog (`integration.wired == false`, backend#1118).
            let providers: Value = client.get("/compute/networks", None).await?;
            let wired = providers
                .pointer("/integration/wired")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            Ok(json!({
                "summary": if wired {
                    "Compute providers listed"
                } else {
                    "Compute provider catalog listed (NOT wired: no provider client is integrated on the target)"
                },
                "wired": wired,
                "providers": providers
            }))
        },
        ComputeCommands::SubmitTask {
            provider,
            spec_file,
            budget,
            timeout,
        } => {
            let raw = fs::read_to_string(spec_file)?;
            let payload =
                serde_json::from_str::<Value>(&raw).unwrap_or_else(|_| json!({ "raw": raw }));
            let result: Value = client
                .post(
                    "/compute/submit",
                    Some(json!({
                        "task": payload,
                        "network": provider,
                        "taskType": provider,
                        "payload": {
                            "spec": payload,
                            "provider": provider,
                            "budget": budget,
                            "timeout": timeout
                        },
                        "priority": 0
                    })),
                )
                .await?;
            Ok(json!({
                "summary": "Compute task submitted",
                "result": result
            }))
        },
        ComputeCommands::TaskStatus { id } => {
            let status: Value = client
                .get(&format!("/compute/jobs/{}", id), None)
                .await?;
            Ok(json!({
                "summary": "Compute task status retrieved",
                "status": status
            }))
        },
        ComputeCommands::GetResults { id, output } => {
            let results: Value = client
                // backend has no separate results route; the job record carries `result`.
                .get(&format!("/compute/jobs/{}", id), None)
                .await?;
            if let Some(path) = output {
                fs::write(path, serde_json::to_string_pretty(&results)?)?;
            }
            Ok(json!({
                "summary": "Compute task results retrieved",
                "results": results,
                "output": output
            }))
        },
        ComputeCommands::RegisterProvider {
            provider_type,
            resources,
            pricing,
        } => {
            let resources_json =
                serde_json::from_str::<Value>(resources).unwrap_or_else(|_| json!({}));
            let pricing_json = serde_json::from_str::<Value>(pricing).unwrap_or_else(|_| json!({}));
            let result: Value = client
                .post(
                    "/compute/providers/register",
                    Some(json!({
                        "providerType": provider_type,
                        "resources": resources_json,
                        "pricing": pricing_json
                    })),
                )
                .await?;
            Ok(json!({
                "summary": "Compute provider registered",
                "result": result
            }))
        },
        ComputeCommands::SetupGolem {
            node_address,
            api_key: _,
            resource_limits,
            payment_config,
        } => Ok(json!({
            "summary": "Golem setup recorded (local workflow)",
            "config": {
                "nodeAddress": node_address,
                "resourceLimits": resource_limits,
                "paymentConfig": payment_config
            }
        })),
        ComputeCommands::GolemTask { spec_file, budget } => {
            let raw = fs::read_to_string(spec_file)?;
            let payload =
                serde_json::from_str::<Value>(&raw).unwrap_or_else(|_| json!({ "raw": raw }));
            let result: Value = client
                .post(
                    "/compute/submit",
                    Some(json!({
                        "task": payload,
                        "network": "golem",
                        "taskType": "golem",
                        "payload": {
                            "spec": payload,
                            "budget": budget
                        },
                        "priority": 0
                    })),
                )
                .await?;
            Ok(json!({
                "summary": "Golem task submitted",
                "result": result
            }))
        },
        ComputeCommands::SetupBacalhau {
            endpoint,
            auth_token,
        } => Ok(json!({
            "summary": "Bacalhau setup recorded (local workflow)",
            "config": {
                "endpoint": endpoint,
                "authTokenConfigured": auth_token.is_some()
            }
        })),

        ComputeCommands::HowToMine { node_type } => {
            let steps = build_how_to_mine_steps(node_type);
            Ok(json!({
                "summary": format!("WRF compute node setup guide — {}", node_type),
                "node_type": node_type,
                "steps": steps,
            }))
        },
    }
}

/// Build the ordered setup steps for a given node type.
fn build_how_to_mine_steps(node_type: &str) -> Value {
    let mut steps = vec![
        json!({
            "step": 1,
            "section": "Prerequisites",
            "action": "Verify hardware: 4+ CPU cores, 8 GB RAM, 50 GB SSD, stable 10 Mbps network",
            "command": ""
        }),
        json!({
            "step": 2,
            "section": "Prerequisites",
            "action": "Install Rust stable toolchain",
            "command": "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh && rustup default stable"
        }),
        json!({
            "step": 3,
            "section": "Build",
            "action": "Clone the core4 repository",
            "command": "git clone https://github.com/wrfcoin/core4.git && cd core4"
        }),
    ];

    match node_type {
        "validator" => {
            steps.push(json!({
                "step": 4,
                "section": "Build",
                "action": "Build the validator binary (consensus + p2p features)",
                "command": "cargo build -p wrfcoin-native-chain --release --features p2p"
            }));
            steps.push(json!({
                "step": 5,
                "section": "Configure",
                "action": "Copy and edit the validator config template",
                "command": "cp configs/development/.env.example config/node.env && $EDITOR config/node.env"
            }));
            steps.push(json!({
                "step": 6,
                "section": "Start",
                "action": "Start the validator node with metrics",
                "command": "WRFCOIN_API_HOST=127.0.0.1 WRFCOIN_API_PORT=8556 ./target/release/launch_api --network testnet --metrics"
            }));
            steps.push(json!({
                "step": 7,
                "section": "Verify",
                "action": "Check node status via the CLI",
                "command": "wrfcoin node status"
            }));
            steps.push(json!({
                "step": 8,
                "section": "Earn",
                "action": "Register as a validator and stake WRF tokens",
                "command": "wrfcoin governance register-validator --stake 1000"
            }));
        },
        "storage" => {
            steps.push(json!({
                "step": 4,
                "section": "Build",
                "action": "Build the storage-keeper binary",
                "command": "cargo build -p wrfcoin-native-chain --release"
            }));
            steps.push(json!({
                "step": 5,
                "section": "Configure",
                "action": "Set IPFS endpoint and storage allocation in node.env",
                "command": "cp configs/development/.env.example config/node.env && $EDITOR config/node.env"
            }));
            steps.push(json!({
                "step": 6,
                "section": "Start",
                "action": "Start the storage node",
                "command": "WRFCOIN_API_HOST=127.0.0.1 WRFCOIN_API_PORT=8556 ./target/release/launch_api --network testnet"
            }));
            steps.push(json!({
                "step": 7,
                "section": "Verify",
                "action": "List available storage providers",
                "command": "wrfcoin storage list"
            }));
            steps.push(json!({
                "step": 8,
                "section": "Earn",
                "action": "Register as a storage provider and set your pricing",
                "command": "wrfcoin compute register-provider --provider-type storage --resources '{\"gb\":100}' --pricing '{\"per_gb_hour\":0.01}'"
            }));
        },
        _ => {
            // wrf-model (default)
            steps.push(json!({
                "step": 4,
                "section": "Build",
                "action": "Build the full-mining binary (WRF modeling + compute)",
                "command": "cargo build -p wrfcoin-native-chain --release --features full-mining,p2p"
            }));
            steps.push(json!({
                "step": 5,
                "section": "Configure",
                "action": "Copy and edit the node config (set WRFCOIN_MINER_ADDRESS to your wallet)",
                "command": "cp configs/development/.env.example config/node.env && $EDITOR config/node.env"
            }));
            steps.push(json!({
                "step": 6,
                "section": "Start",
                "action": "Start the node with mining and API enabled",
                "command": "WRFCOIN_API_HOST=127.0.0.1 WRFCOIN_API_PORT=8556 ./target/release/launch_api --network testnet --mine --metrics"
            }));
            steps.push(json!({
                "step": 7,
                "section": "Verify",
                "action": "Check mining stats",
                "command": "wrfcoin node mining-stats"
            }));
            steps.push(json!({
                "step": 8,
                "section": "Earn",
                "action": "Submit a compute task to verify your node earns rewards",
                "command": "wrfcoin compute list-providers"
            }));
        },
    }

    Value::Array(steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- how-to-mine guide --------------------------------------------------

    #[test]
    fn how_to_mine_wrf_model_has_eight_steps() {
        let steps = build_how_to_mine_steps("wrf-model");
        let arr = steps.as_array().expect("steps must be an array");
        assert_eq!(arr.len(), 8, "wrf-model guide should have 8 steps");
    }

    #[test]
    fn how_to_mine_validator_has_eight_steps() {
        let steps = build_how_to_mine_steps("validator");
        let arr = steps.as_array().expect("steps must be an array");
        assert_eq!(arr.len(), 8, "validator guide should have 8 steps");
    }

    #[test]
    fn how_to_mine_storage_has_eight_steps() {
        let steps = build_how_to_mine_steps("storage");
        let arr = steps.as_array().expect("steps must be an array");
        assert_eq!(arr.len(), 8, "storage guide should have 8 steps");
    }

    #[test]
    fn how_to_mine_unknown_type_falls_back_to_wrf_model() {
        let steps = build_how_to_mine_steps("unknown");
        let arr = steps.as_array().expect("steps must be an array");
        assert_eq!(arr.len(), 8, "unknown type should fall back to wrf-model (8 steps)");
    }

    #[test]
    fn how_to_mine_steps_have_required_fields() {
        for node_type in &["wrf-model", "validator", "storage"] {
            let steps = build_how_to_mine_steps(node_type);
            let arr = steps.as_array().unwrap();
            for (i, step) in arr.iter().enumerate() {
                assert!(
                    step.get("step").is_some(),
                    "step {} of {} missing 'step' field",
                    i,
                    node_type
                );
                assert!(
                    step.get("section").is_some(),
                    "step {} of {} missing 'section' field",
                    i,
                    node_type
                );
                assert!(
                    step.get("action").is_some(),
                    "step {} of {} missing 'action' field",
                    i,
                    node_type
                );
                assert!(
                    step.get("command").is_some(),
                    "step {} of {} missing 'command' field",
                    i,
                    node_type
                );
            }
        }
    }

    #[test]
    fn how_to_mine_steps_are_ordered() {
        let steps = build_how_to_mine_steps("wrf-model");
        let arr = steps.as_array().unwrap();
        for (i, step) in arr.iter().enumerate() {
            let step_num = step["step"].as_u64().expect("step number must be u64");
            assert_eq!(
                step_num,
                (i + 1) as u64,
                "step numbers must be monotonically increasing from 1"
            );
        }
    }

    #[test]
    fn how_to_mine_first_step_is_prerequisites() {
        for node_type in &["wrf-model", "validator", "storage"] {
            let steps = build_how_to_mine_steps(node_type);
            let first = &steps.as_array().unwrap()[0];
            assert_eq!(
                first["section"].as_str().unwrap(),
                "Prerequisites",
                "first step of {} must be in Prerequisites section",
                node_type
            );
        }
    }
}
