use anyhow::Result;
use clap::Subcommand;
use serde_json::{Value, json};
use wrfcoin_cli::{ApiClient, ApiConfig, ServiceTarget};

#[derive(Subcommand)]
pub enum AiMlCommands {
    /// List available AI/ML networks
    List,
    /// Show AI/ML network status
    Status {
        /// Network name
        network: Option<String>,
    },
    /// Submit AI/ML job
    Submit {
        /// Job type (training, inference, analysis)
        job_type: String,
        /// Target network
        network: String,
        /// Data input
        data: String,
    },
    /// Check job status
    Job {
        /// Job ID
        id: String,
    },
    /// List available models
    Models {
        /// Network name
        network: String,
    },
}

/// `aiml` talks to backend's ai-ml-networks service (port 6001), not the
/// node — see `ServiceTarget::AiMl` (#3204).
pub async fn handle_aiml_command(command: &AiMlCommands) -> Result<Value> {
    let config = ApiConfig::for_service(ServiceTarget::AiMl);
    let client = ApiClient::new(config.clone())?;
    let mut out = handle_aiml_inner(&client, command).await?;
    ServiceTarget::AiMl.attach(&config, &mut out);
    Ok(out)
}

async fn handle_aiml_inner(client: &ApiClient, command: &AiMlCommands) -> Result<Value> {
    match command {
        AiMlCommands::List => {
            let networks = json!([
                {"name": "SingularityNET", "services": 65536, "status": "catalog"},
                {"name": "Bittensor", "validators": 4296, "status": "catalog"},
                {"name": "Near Protocol", "type": "AI marketplace", "status": "catalog"},
                {"name": "Ocean Protocol", "type": "Data marketplace", "status": "catalog"},
                {"name": "Fetch.ai", "type": "Autonomous agents", "status": "catalog"}
            ]);
            Ok(json!({
                "summary": "AI/ML network catalog (static; not a live status)",
                "catalog_only": true,
                "networks": networks
            }))
        },
        AiMlCommands::Status { network: _ } => {
            // backend registers /aiml/networks/status, not /aiml/status.
            let result: Value = client.get("/aiml/networks/status", None).await?;
            Ok(json!({ "summary": "AI/ML status retrieved", "status": result }))
        },
        AiMlCommands::Submit {
            job_type,
            network,
            data,
        } => {
            let result: Value = client
                // backend's inference route is /ai/inference and reads
                // { model_id, model_type, network, input_data }.
                .post(
                    "/ai/inference",
                    Some(json!({
                        "model_id": format!("{}-default", network),
                        "model_type": job_type,
                        "network": network,
                        "input_data": data,
                        "parameters": { "jobType": job_type }
                    })),
                )
                .await?;
            Ok(json!({ "summary": "AI/ML job submitted", "result": result }))
        },
        AiMlCommands::Job { id } => {
            // backend tracks inference by id at /ai/inference/{id}.
            let result: Value = client.get(&format!("/ai/inference/{}", id), None).await?;
            Ok(json!({ "summary": "Job status retrieved", "job": result }))
        },
        AiMlCommands::Models { network } => {
            let models = match network.as_str() {
                "singularitynet" => json!([
                    {"name": "GPT variants", "type": "language"},
                    {"name": "Computer vision", "type": "vision"},
                    {"name": "NLP services", "type": "nlp"}
                ]),
                "bittensor" => json!([
                    {"name": "Text generation", "type": "language"},
                    {"name": "Image processing", "type": "vision"},
                    {"name": "Data analysis", "type": "analytics"}
                ]),
                _ => json!([]),
            };
            Ok(json!({ "summary": format!("Models for {}", network), "models": models }))
        },
    }
}
