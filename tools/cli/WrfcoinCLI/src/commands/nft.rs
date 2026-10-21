use anyhow::Result;
use clap::Subcommand;
use serde_json::{Value, json};
use wrfcoin_cli::{ApiClient, ApiConfig};

/// Commands for NFT management
#[derive(Subcommand)]
pub enum NftCommands {
    /// List owned NFTs
    List {
        /// Owner filter (if omitted, request the node's bounded page)
        #[clap(long)]
        owner: Option<String>,

        /// Filter by NFT type
        #[clap(long, hide = true)]
        nft_type: Option<String>,
    },

    /// Show details of an NFT
    Show {
        /// NFT ID
        id: String,
    },

    /// Unavailable computational NFT creation
    #[clap(hide = true)]
    Create {
        /// NFT title
        #[clap(long)]
        title: String,

        /// NFT description
        #[clap(long)]
        description: String,

        /// Computation type (Weather, Climate, AirQuality, etc.)
        #[clap(long)]
        computation_type: String,

        /// Input data for computation
        #[clap(long)]
        input_data: Option<String>,

        /// Reward amount
        #[clap(long)]
        reward: f64,

        /// Creator address (if omitted, use primary address)
        #[clap(long)]
        creator: Option<String>,

        /// Geographical region for the computation (optional)
        #[clap(long)]
        region: Option<String>,

        /// Additional metadata in JSON format
        #[clap(long)]
        metadata: Option<String>,
    },

    /// Unavailable computational result submission
    #[clap(hide = true)]
    Submit {
        /// NFT ID
        #[clap(long)]
        id: String,

        /// Computation result
        #[clap(long)]
        result: String,

        /// Executor address (if omitted, use primary address)
        #[clap(long)]
        executor: Option<String>,

        /// Proof of computation in JSON format
        #[clap(long)]
        proof: Option<String>,
    },

    /// Unavailable computation categories
    #[clap(hide = true)]
    Categories,

    /// Unavailable category creation
    #[clap(hide = true)]
    AddCategory {
        /// Category name
        name: String,

        /// Category description
        #[clap(long)]
        description: Option<String>,
    },

    /// Unavailable verifier registration
    #[clap(hide = true)]
    RegisterVerifier {
        /// Verification capacity (tasks per hour)
        #[clap(long, default_value = "10")]
        capacity: u32,

        /// Supported computation types (comma-separated list)
        #[clap(long)]
        supported_types: String,

        /// Fee percentage
        #[clap(long, default_value = "1.0")]
        fee_percentage: f64,
    },

    /// Unavailable verifier list
    #[clap(hide = true)]
    ListVerifiers {
        /// Filter by computation type
        #[clap(long)]
        computation_type: Option<String>,
    },

    /// Unavailable computational result verification
    #[clap(hide = true)]
    Verify {
        /// NFT ID
        #[clap(long)]
        id: String,

        /// Result hash to verify
        #[clap(long)]
        result_hash: String,

        /// Verification decision (approve, reject)
        #[clap(long)]
        decision: String,

        /// Verification notes
        #[clap(long)]
        notes: Option<String>,
    },
}

pub async fn handle_nft_command(command: &NftCommands) -> Result<Value> {
    let unavailable = match command {
        NftCommands::Create { .. } => Some("nft create"),
        NftCommands::Submit { .. } => Some("nft submit"),
        NftCommands::Categories => Some("nft categories"),
        NftCommands::AddCategory { .. } => Some("nft add-category"),
        NftCommands::RegisterVerifier { .. } => Some("nft register-verifier"),
        NftCommands::ListVerifiers { .. } => Some("nft list-verifiers"),
        NftCommands::Verify { .. } => Some("nft verify"),
        NftCommands::List {
            nft_type: Some(_), ..
        } => Some("nft list --nft-type"),
        _ => None,
    };
    if let Some(name) = unavailable {
        return Err(wrfcoin_cli::errors::CliError::new(
            wrfcoin_cli::CliErrorCode::NotFound,
            format!("{name}: not wired in this CLI to a served node API (see wrfcoin/core4#3247)"),
        )
        .into());
    }
    let client = ApiClient::new(ApiConfig::default())?;

    match command {
        NftCommands::List { owner, .. } => {
            let query = owner.as_ref().map(|value| [("owner", value.as_str())]);
            let listing: Value = client
                .get("/nft/list", query.as_ref().map(|q| q.as_slice()))
                .await?;
            Ok(json!({
                "summary": "Node NFT list retrieved",
                "listing": listing
            }))
        },
        NftCommands::Show { id } => {
            let nft: Value = client.get(&format!("/nft/{}", id), None).await?;
            Ok(json!({
                "summary": "NFT details retrieved",
                "nft": nft
            }))
        },
        NftCommands::Create {
            title,
            description,
            computation_type,
            input_data,
            reward,
            creator,
            region,
            metadata,
        } => {
            let metadata_json = metadata
                .as_ref()
                .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
                .unwrap_or_else(|| json!({}));

            let body = json!({
                "title": title,
                "description": description,
                "computationType": computation_type,
                "inputData": input_data,
                "reward": reward,
                "creator": creator,
                "region": region,
                "metadata": metadata_json
            });
            let created: Value = client.post("/nft/create", Some(body)).await?;
            Ok(json!({
                "summary": "NFT task created",
                "result": created
            }))
        },
        NftCommands::Submit {
            id,
            result,
            executor,
            proof,
        } => {
            let proof_json = proof
                .as_ref()
                .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
                .unwrap_or_else(|| json!({}));
            let submitted: Value = client
                .post(
                    &format!("/nft/{}/submit", id),
                    Some(json!({
                        "result": result,
                        "executor": executor,
                        "proof": proof_json
                    })),
                )
                .await?;
            Ok(json!({
                "summary": "NFT computation submitted",
                "result": submitted
            }))
        },
        NftCommands::Categories => {
            let categories: Value = client.get("/nft/categories", None).await?;
            Ok(json!({
                "summary": "NFT categories retrieved",
                "categories": categories
            }))
        },
        NftCommands::AddCategory { name, description } => {
            let category: Value = client
                .post(
                    "/nft/categories",
                    Some(json!({
                        "name": name,
                        "description": description
                    })),
                )
                .await?;
            Ok(json!({
                "summary": "NFT category added",
                "result": category
            }))
        },
        NftCommands::RegisterVerifier {
            capacity,
            supported_types,
            fee_percentage,
        } => {
            let supported: Vec<String> = supported_types
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToString::to_string)
                .collect();
            let verifier: Value = client
                .post(
                    "/nft/verifiers",
                    Some(json!({
                        "capacity": capacity,
                        "supportedTypes": supported,
                        "feePercentage": fee_percentage
                    })),
                )
                .await?;
            Ok(json!({
                "summary": "NFT verifier registered",
                "result": verifier
            }))
        },
        NftCommands::ListVerifiers { computation_type } => {
            let path = if let Some(kind) = computation_type {
                format!("/nft/verifiers?computationType={}", kind)
            } else {
                "/nft/verifiers".to_string()
            };
            let verifiers: Value = client.get(&path, None).await?;
            Ok(json!({
                "summary": "NFT verifiers listed",
                "verifiers": verifiers
            }))
        },
        NftCommands::Verify {
            id,
            result_hash,
            decision,
            notes,
        } => {
            let verified: Value = client
                .post(
                    &format!("/nft/{}/verify", id),
                    Some(json!({
                        "resultHash": result_hash,
                        "decision": decision,
                        "notes": notes
                    })),
                )
                .await?;
            Ok(json!({
                "summary": "NFT result verification submitted",
                "result": verified
            }))
        },
    }
}
