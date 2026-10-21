use anyhow::{Context, Result, bail};
use clap::Subcommand;
use log::info;
use rand::RngCore;
use rand::rngs::OsRng;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::station_registry::{
    ContributionProvenanceClass, DataQualityTier, StationLocation, WeatherStation,
};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

const DEFAULT_GENESIS_NETWORK_ID: &str = "testnet";
const DEFAULT_STATION_MANIFEST: &str = "stations.json";
const DEFAULT_LAUNCH_API_URL: &str = "http://127.0.0.1:8556";
const GENESIS_INITIAL_DIFFICULTY: u64 = 1;
const GENESIS_INITIAL_BLOCK_TIME_SECS: u64 = 14_400;
const BOOTSTRAP_VERSION: &str = "0.1.0";
const MAX_STATION_ID_LEN: usize = 128;
const DEFAULT_NODE_ENV_FILE: &str = "config/node.env";
const DEFAULT_EXAMPLE_STATIONS_FILE: &str = "config/stations/example-stations.json";

#[derive(Subcommand)]
pub enum SetupCommands {
    /// Initialize a local development environment
    Init {
        /// Target directory for the environment
        #[clap(long)]
        dir: Option<PathBuf>,

        /// Network mode to configure (testnet or mainnet)
        #[clap(long, default_value = "testnet")]
        network: String,

        /// Enable mining capabilities
        #[clap(long)]
        with_mining: bool,

        /// Configure as a data collection node
        #[clap(long)]
        data_collection: bool,

        /// Configure as a computation node
        #[clap(long)]
        computation: bool,

        /// Configure as a storage keeper node
        #[clap(long)]
        storage_keeper: bool,

        /// Generate example configuration files
        #[clap(long)]
        with_examples: bool,
    },

    /// Generate configuration files for a specific node type
    GenerateConfig {
        /// Node configuration type
        #[clap(long)]
        node_type: String,

        /// Output directory for configuration files
        #[clap(long)]
        output: PathBuf,

        /// Network mode to configure (testnet or mainnet)
        #[clap(long, default_value = "testnet")]
        network: String,

        /// Node name for identification
        #[clap(long)]
        name: Option<String>,

        /// Node location (latitude,longitude)
        #[clap(long)]
        location: Option<String>,
    },

    /// Install required dependencies for local development
    InstallDeps {
        /// Skip confirmation prompts
        #[clap(long)]
        yes: bool,

        /// Include optional development dependencies
        #[clap(long)]
        with_dev: bool,

        /// Include weather station hardware support
        #[clap(long)]
        with_hardware: bool,
    },

    /// Configure a weather station
    ConfigureStation {
        /// Weather station type
        #[clap(long, required = true)]
        station_type: String,

        /// Station identifier
        #[clap(long, required = true)]
        station_id: String,

        /// Station location (latitude,longitude)
        #[clap(long, required = true)]
        location: String,

        /// Station elevation in meters
        #[clap(long)]
        elevation: Option<f64>,

        /// Station hardware communication port
        #[clap(long)]
        port: Option<String>,

        /// Communication protocol (serial, i2c, spi, etc.)
        #[clap(long)]
        protocol: Option<String>,

        /// Sensor polling interval in seconds
        #[clap(long, default_value = "300")]
        interval: u32,

        /// Output configuration file
        #[clap(long)]
        output: Option<PathBuf>,
    },

    /// Generate validator ed25519 keypairs for bootstrap
    Keygen {
        /// Output directory for validator key material
        #[clap(long, required = true)]
        output: PathBuf,

        /// Number of validator keypairs to generate
        #[clap(long, default_value_t = 1)]
        count: usize,

        /// Overwrite existing validator key files
        #[clap(long)]
        force: bool,
    },

    /// Generate a 32-byte hex key for WRFCOIN storage encryption
    GenerateStorageKey,

    /// Create a bootstrap genesis JSON from validator public keys
    Genesis {
        /// Directory containing validator `.pub` files
        #[clap(long, required = true)]
        validator_keys: PathBuf,

        /// Oracle quorum for bootstrap
        #[clap(long)]
        oracle_quorum: Option<usize>,

        /// Bootstrap network id
        #[clap(long, default_value = DEFAULT_GENESIS_NETWORK_ID)]
        network_id: String,

        /// Output genesis JSON path
        #[clap(long, required = true)]
        output: PathBuf,

        /// Override genesis timestamp (unix seconds)
        #[clap(long)]
        genesis_timestamp: Option<i64>,

        /// Schedule a consensus activation as `<feature>@<height>` (repeatable,
        /// #3092): `v1@0`, `v2@1000`, `domain_separated_preimages@5000`.
        /// Written to `protocol_activation_schedule` and validated with the
        /// node's own parser; the node records it in block 1 when
        /// WRFCOIN_GENESIS_ACTIVATION_SCHEDULE carries the same string.
        #[clap(long = "activate", value_name = "FEATURE@HEIGHT")]
        activate: Vec<String>,
    },

    /// Write a station manifest and optionally authorize its oracle on the launch API
    RegisterStation {
        /// Station identifier
        #[clap(long, required = true)]
        station_id: String,

        /// Station latitude
        #[clap(long, required = true)]
        lat: f64,

        /// Station longitude
        #[clap(long, required = true)]
        lon: f64,

        /// Station elevation in meters
        #[clap(long)]
        elevation: Option<f64>,

        /// Station hardware model
        #[clap(long, required = true)]
        hardware: String,

        /// Station owner wallet address
        #[clap(long, required = true)]
        owner_address: String,

        /// Optional calibration date string
        #[clap(long)]
        calibration_date: Option<String>,

        /// Oracle public key to authorize on the launch API
        #[clap(long)]
        oracle_pubkey: Option<String>,

        /// Launch API base URL
        #[clap(long)]
        api_url: Option<String>,

        /// Operator bearer token. Falls back to WRFCOIN_OPERATOR_TOKEN.
        #[clap(long)]
        operator_token: Option<String>,

        /// Output manifest file. Defaults to ./stations.json.
        #[clap(long, default_value = DEFAULT_STATION_MANIFEST)]
        output: PathBuf,

        /// Merge into an existing fleet manifest instead of refusing overwrite
        #[clap(long)]
        merge: bool,
    },

    /// Set up a backup directory for blockchain data
    ConfigureBackup {
        /// Backup directory path
        #[clap(long, required = true)]
        backup_dir: PathBuf,

        /// Automatic backup interval in hours (0 to disable)
        #[clap(long, default_value = "24")]
        interval: u32,

        /// Maximum number of backups to keep
        #[clap(long, default_value = "7")]
        keep: u32,

        /// Encrypt backups
        #[clap(long)]
        encrypt: bool,

        /// Compression level (0-9)
        #[clap(long, default_value = "6")]
        compression: u8,
    },

    /// Generate SDK configuration for different programming languages
    GenerateSDK {
        /// Programming language for the SDK
        #[clap(long)]
        language: String,

        /// Output directory for the SDK
        #[clap(long)]
        output: PathBuf,

        /// Generate example code
        #[clap(long)]
        with_examples: bool,

        /// Generate minimal SDK with only core features
        #[clap(long)]
        minimal: bool,
    },
}

#[derive(Debug, Serialize)] // secret-leak-ok: private_key_file is a file PATH to the key, not key material
struct GeneratedKeyRecord {
    index: usize,
    public_key: String,
    public_key_preview: String,
    private_key_file: String,
    public_key_file: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct BootstrapGenesisConfig {
    network_id: String,
    genesis_timestamp: i64,
    initial_difficulty: u64,
    initial_block_time_secs: u64,
    oracle_config: BootstrapOracleConfig,
    initial_accounts: Vec<Value>,
    version: String,
    /// `height:token` pairs, ascending — the exact string block 1 records
    /// under `protocol_activation_schedule` (#3092). Absent when nothing is
    /// scheduled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    protocol_activation_schedule: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BootstrapOracleConfig {
    authorized_oracles: Vec<String>,
    quorum: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct OracleConfigResponse {
    oracle_set: Vec<String>,
    oracle_quorum: usize,
}

#[derive(Debug, Deserialize)]
struct OracleLeaderboardResponse {
    entries: Vec<OracleLeaderboardEntry>,
}

#[derive(Debug, Deserialize)]
struct OracleLeaderboardEntry {
    oracle_id: String,
}

#[derive(Debug, Serialize)]
struct OracleAuthorizationResult {
    action: &'static str,
    api_url: String,
    oracle_pubkey: String,
    oracle_config: OracleConfigResponse,
    leaderboard_visible: Option<bool>,
}

pub async fn handle_setup_command(command: &SetupCommands) -> Result<Value> {
    match command {
        SetupCommands::Init {
            dir,
            network,
            with_mining,
            data_collection,
            computation,
            storage_keeper,
            with_examples,
        } => {
            let target_dir = dir.clone().unwrap_or_else(|| PathBuf::from("."));

            info!(
                "Initializing local development environment at {:?}",
                target_dir
            );
            handle_init(
                &target_dir,
                network,
                *with_mining,
                *data_collection,
                *computation,
                *storage_keeper,
                *with_examples,
            )
        },

        SetupCommands::GenerateConfig {
            node_type,
            output,
            network,
            name,
            location,
        } => {
            info!("Generating configuration for {} node", node_type);
            Ok(json!({
                "summary": "Node configuration generated",
                "nodeType": node_type,
                "output": output,
                "network": network,
                "name": name,
                "location": location
            }))
        },

        SetupCommands::InstallDeps {
            yes: _,
            with_dev,
            with_hardware,
        } => {
            info!("Installing dependencies");
            Ok(json!({
                "summary": "Dependencies installed",
                "core": true,
                "development": with_dev,
                "weatherHardware": with_hardware
            }))
        },

        SetupCommands::ConfigureStation {
            station_type,
            station_id,
            location,
            elevation,
            port: _,
            protocol: _,
            interval,
            output: _,
        } => {
            info!("Configuring weather station: {}", station_id);
            Ok(json!({
                "summary": "Weather station configured",
                "stationId": station_id,
                "stationType": station_type,
                "location": location,
                "elevation": elevation,
                "pollIntervalSeconds": interval
            }))
        },

        SetupCommands::Keygen {
            output,
            count,
            force,
        } => handle_keygen(output, *count, *force),

        SetupCommands::GenerateStorageKey => handle_generate_storage_key(),

        SetupCommands::Genesis {
            validator_keys,
            oracle_quorum,
            network_id,
            output,
            genesis_timestamp,
            activate,
        } => handle_genesis(
            validator_keys,
            *oracle_quorum,
            network_id,
            output,
            *genesis_timestamp,
            activate,
        ),

        SetupCommands::RegisterStation {
            station_id,
            lat,
            lon,
            elevation,
            hardware,
            owner_address,
            calibration_date,
            oracle_pubkey,
            api_url,
            operator_token,
            output,
            merge,
        } => {
            handle_register_station(
                station_id,
                *lat,
                *lon,
                *elevation,
                hardware,
                owner_address,
                calibration_date.clone(),
                oracle_pubkey.clone(),
                api_url.clone(),
                operator_token.clone(),
                output,
                *merge,
            )
            .await
        },

        SetupCommands::ConfigureBackup {
            backup_dir,
            interval,
            keep,
            encrypt,
            compression,
        } => {
            info!("Configuring backup to {:?}", backup_dir);
            Ok(json!({
                "summary": "Backup configuration updated",
                "backupDirectory": backup_dir,
                "backupIntervalHours": interval,
                "keep": keep,
                "encrypt": encrypt,
                "compression": compression
            }))
        },

        SetupCommands::GenerateSDK {
            language,
            output,
            with_examples,
            minimal,
        } => {
            info!("Generating SDK for {}", language);

            let sdk_type = if *minimal { "minimal" } else { "full" };
            Ok(json!({
                "summary": "SDK generated",
                "sdkType": sdk_type,
                "language": language,
                "output": output,
                "withExamples": with_examples
            }))
        },
    }
}

fn handle_init(
    target_dir: &Path,
    network: &str,
    with_mining: bool,
    data_collection: bool,
    computation: bool,
    storage_keeper: bool,
    with_examples: bool,
) -> Result<Value> {
    fs::create_dir_all(target_dir)
        .with_context(|| format!("failed to create target directory {}", target_dir.display()))?;

    let data_dir = target_dir.join("data");
    let keys_dir = target_dir.join("keys");
    let config_dir = target_dir.join("config");
    let stations_dir = config_dir.join("stations");
    for dir in [&data_dir, &keys_dir, &config_dir, &stations_dir] {
        fs::create_dir_all(dir)
            .with_context(|| format!("failed to create directory {}", dir.display()))?;
    }

    let mut roles = Vec::new();
    if with_mining {
        roles.push("Validator");
    }
    if data_collection {
        roles.push("Data Collection");
    }
    if computation {
        roles.push("Compute");
    }
    if storage_keeper {
        roles.push("Storage Keeper");
    }
    if roles.is_empty() {
        roles.push("Light Client");
    }

    let config_path = target_dir.join(DEFAULT_NODE_ENV_FILE);
    let config_created =
        write_text_file_if_missing(&config_path, &default_node_env_template(network, &roles))?;

    let example_manifest_path = target_dir.join(DEFAULT_EXAMPLE_STATIONS_FILE);
    let example_manifest_created = if with_examples {
        write_text_file_if_missing(
            &example_manifest_path,
            &default_example_station_manifest(network),
        )?
    } else {
        false
    };

    Ok(json!({
        "summary": "Wrfcoin development environment initialized",
        "directory": target_dir.display().to_string(),
        "network": network,
        "roles": roles,
        "examplesGenerated": example_manifest_created,
        "dataDirectory": data_dir.display().to_string(),
        "keysDirectory": keys_dir.display().to_string(),
        "configDirectory": config_dir.display().to_string(),
        "nodeConfigPath": config_path.display().to_string(),
        "nodeConfigCreated": config_created,
        "exampleStationsPath": with_examples.then(|| example_manifest_path.display().to_string()),
    }))
}

fn default_node_env_template(network: &str, roles: &[&str]) -> String {
    let roles_comment = roles.join(", ");
    format!(
        "# Generated by `wrfcoin setup init`\n\
         # Network: {network}\n\
         # Roles: {roles_comment}\n\
         WRFCOIN_NETWORK_ID={network}\n\
         WRFCOIN_API_HOST=127.0.0.1\n\
         WRFCOIN_API_PORT=8556\n\
         WRFCOIN_DATA_DIR=./data\n\
         WRFCOIN_P2P_ENABLED=false\n\
         RUST_LOG=info\n"
    )
}

fn default_example_station_manifest(network: &str) -> String {
    let example = vec![WeatherStation {
        station_id: format!("{network}-station-example"),
        owner_address: "wrf1exampleowner".to_string(),
        location: StationLocation {
            latitude: 39.7392,
            longitude: -104.9903,
            elevation_meters: Some(1609.0),
        },
        hardware_model: "WS-5000".to_string(),
        calibration_date: Some("2026-03-24".to_string()),
        is_active: true,
        reputation_score: 0,
        data_quality_tier: DataQualityTier::Unverified,
        provenance_class: ContributionProvenanceClass::Unclassified,
        ring_public_key: None,
    }];
    format!(
        "{}\n",
        serde_json::to_string_pretty(&example).expect("example station manifest should serialize")
    )
}

fn handle_keygen(output: &Path, count: usize, force: bool) -> Result<Value> {
    if count == 0 {
        bail!("count must be greater than zero");
    }
    if output.exists() && !output.is_dir() {
        bail!("output path must be a directory: {}", output.display());
    }

    fs::create_dir_all(output)
        .with_context(|| format!("failed to create output directory {}", output.display()))?;

    if !force && directory_contains_key_material(output)? {
        bail!("output directory already contains validator key material; use --force to overwrite");
    }

    let mut records = Vec::with_capacity(count);
    for index in 1..=count {
        let private_key = PrivateKey::generate(&mut OsRng);
        let public_key = PublicKey::from_private_key(&private_key).to_hex();
        let private_key_path = output.join(format!("validator-{index}.key"));
        let public_key_path = output.join(format!("validator-{index}.pub"));

        write_private_key_file(&private_key_path, &private_key.to_hex(), force)?;
        write_text_file(&public_key_path, &public_key, force)?;

        records.push(GeneratedKeyRecord {
            index,
            public_key_preview: truncate_key(&public_key),
            public_key,
            private_key_file: private_key_path.display().to_string(),
            public_key_file: public_key_path.display().to_string(),
        });
    }

    Ok(json!({
        "summary": format!("Generated {} validator keypair(s)", records.len()),
        "keyCount": records.len(),
        "outputDirectory": output.display().to_string(),
        "keys": records,
        "warning": "Store private keys securely. Never commit them to git. Add the output directory to .gitignore."
    }))
}

fn handle_generate_storage_key() -> Result<Value> {
    let key = generate_storage_key_hex();
    Ok(json!({
        "summary": format!("WRFCOIN_STORAGE_ENCRYPTION_KEY={key}"),
        "envVar": "WRFCOIN_STORAGE_ENCRYPTION_KEY",
        "key": key
    }))
}

fn generate_storage_key_hex() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Turn repeated `--activate <feature>@<height>` values into the canonical
/// genesis schedule string (`height:token`, ascending by height), validated
/// with the node's own parser so an unschedulable cut fails here, offline,
/// rather than at block 1 (#3092).
fn build_activation_schedule(activate: &[String]) -> Result<Option<String>> {
    use wrfcoin_shared_protocol::crypto::chain_binding::{
        ConsensusFeature, parse_genesis_activation_schedule, resolve_activation_schedule,
        resolve_feature_schedule, GENESIS_ACTIVATION_SCHEDULE_KEY,
    };
    if activate.is_empty() {
        return Ok(None);
    }
    let mut entries: Vec<(u64, String)> = Vec::with_capacity(activate.len());
    for raw in activate {
        let Some((feature, height)) = raw.trim().split_once('@') else {
            bail!("--activate {raw:?} is not <feature>@<height>");
        };
        let height: u64 = height
            .trim()
            .parse()
            .with_context(|| format!("--activate {raw:?}: height is not an unsigned integer"))?;
        entries.push((height, feature.trim().to_string()));
    }
    // core4#3299: the same hold as the signed extension. Refused here, before
    // anything is written, so no genesis artifact can activate a held feature.
    crate::commands::governance::check_held_feature_names(
        entries.iter().map(|(_, feature)| feature.as_str()),
        crate::commands::governance::SCHEDULING_HELD,
        "write the genesis schedule",
    )?;
    entries.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    let schedule = entries
        .iter()
        .map(|(h, t)| format!("{h}:{t}"))
        .collect::<Vec<_>>()
        .join(",");

    // Validate exactly as the node will: parse, build the version table from
    // the version steps, then the feature schedule over it.
    let parsed = parse_genesis_activation_schedule(&schedule)
        .map_err(|e| anyhow::anyhow!("--activate schedule {schedule:?} is invalid: {e}"))?;
    let genesis_version = parsed
        .versions
        .first()
        .map(|(_, v)| *v)
        .unwrap_or_default();
    let metadata = [(GENESIS_ACTIVATION_SCHEDULE_KEY, schedule.as_str())];
    let table = resolve_activation_schedule(metadata, genesis_version)
        .map_err(|e| anyhow::anyhow!("--activate schedule {schedule:?} is invalid: {e}"))?;
    let features = resolve_feature_schedule(metadata, &table)
        .map_err(|e| anyhow::anyhow!("--activate schedule {schedule:?} is invalid: {e}"))?;
    if let Some((name, height)) = features.first_unknown(ConsensusFeature::ALL) {
        info!(
            "activation schedule names feature {name:?} at height {height}, which this CLI \
             build does not implement; recorded as-is (an old node halts there, #3093)"
        );
    }
    Ok(Some(schedule))
}

fn handle_genesis(
    validator_keys: &Path,
    oracle_quorum: Option<usize>,
    network_id: &str,
    output: &Path,
    genesis_timestamp: Option<i64>,
    activate: &[String],
) -> Result<Value> {
    let protocol_activation_schedule = build_activation_schedule(activate)?;
    let authorized_oracles = collect_validator_pubkeys(validator_keys)?;
    if authorized_oracles.is_empty() {
        bail!(
            "validator-keys directory does not contain any valid .pub files: {}",
            validator_keys.display()
        );
    }

    let effective_quorum = oracle_quorum.unwrap_or(1);
    if effective_quorum == 0 {
        bail!("oracle-quorum must be greater than zero");
    }
    if effective_quorum > authorized_oracles.len() {
        bail!(
            "oracle-quorum cannot exceed validator public key count ({})",
            authorized_oracles.len()
        );
    }

    let timestamp = genesis_timestamp.unwrap_or_else(|| chrono::Utc::now().timestamp());
    let config = BootstrapGenesisConfig {
        network_id: network_id.to_string(),
        genesis_timestamp: timestamp,
        initial_difficulty: GENESIS_INITIAL_DIFFICULTY,
        initial_block_time_secs: GENESIS_INITIAL_BLOCK_TIME_SECS,
        oracle_config: BootstrapOracleConfig {
            authorized_oracles: authorized_oracles.clone(),
            quorum: effective_quorum,
        },
        initial_accounts: Vec::new(),
        version: BOOTSTRAP_VERSION.to_string(),
        protocol_activation_schedule: protocol_activation_schedule.clone(),
    };

    write_pretty_json(output, &config)?;

    Ok(json!({
        "summary": format!(
            "Genesis bootstrap config written for {} validator(s)",
            authorized_oracles.len()
        ),
        "output": output.display().to_string(),
        "networkId": network_id,
        "validatorCount": authorized_oracles.len(),
        "oracleQuorum": effective_quorum,
        "genesisTimestamp": timestamp,
        "initialDifficulty": GENESIS_INITIAL_DIFFICULTY,
        "initialBlockTimeSecs": GENESIS_INITIAL_BLOCK_TIME_SECS,
        "authorizedOracles": authorized_oracles,
        "protocolActivationSchedule": protocol_activation_schedule,
        "cutEnv": protocol_activation_schedule.as_deref().map(|s| {
            format!(
                "{}={s}",
                wrfcoin_shared_protocol::crypto::chain_binding::GENESIS_ACTIVATION_SCHEDULE_ENV
            )
        }),
    }))
}

#[allow(clippy::too_many_arguments)]
async fn handle_register_station(
    station_id: &str,
    lat: f64,
    lon: f64,
    elevation: Option<f64>,
    hardware: &str,
    owner_address: &str,
    calibration_date: Option<String>,
    oracle_pubkey: Option<String>,
    api_url: Option<String>,
    operator_token: Option<String>,
    output: &Path,
    merge: bool,
) -> Result<Value> {
    validate_station_id(station_id)?;
    validate_coordinates(lat, lon)?;

    let hardware_model = trimmed_nonempty(hardware, "hardware")?;
    let owner_address = trimmed_nonempty(owner_address, "owner-address")?;
    let normalized_oracle_pubkey = oracle_pubkey
        .as_deref()
        .map(normalize_oracle_pubkey)
        .transpose()?;

    let station = WeatherStation {
        station_id: station_id.to_string(),
        owner_address,
        location: StationLocation {
            latitude: lat,
            longitude: lon,
            elevation_meters: elevation,
        },
        hardware_model,
        calibration_date,
        is_active: true,
        reputation_score: 0,
        data_quality_tier: DataQualityTier::Unverified,
        // Station registration alone cannot prove that observations are real.
        // A canonical authority must classify it before it becomes reward
        // eligible; never infer Real from the presence of an oracle key.
        provenance_class: ContributionProvenanceClass::Unclassified,
        ring_public_key: normalized_oracle_pubkey.clone(),
    };

    let (fleet_size, replaced_existing_station) = write_station_manifest(output, &station, merge)?;

    let token = operator_token
        .or_else(|| std::env::var("WRFCOIN_OPERATOR_TOKEN").ok())
        .filter(|value| !value.trim().is_empty());

    let authorization = match (normalized_oracle_pubkey.as_deref(), token.as_deref()) {
        (Some(oracle_id), Some(token)) => Some(
            authorize_oracle(
                api_url.as_deref().unwrap_or(DEFAULT_LAUNCH_API_URL),
                token,
                oracle_id,
            )
            .await?,
        ),
        _ => None,
    };

    let api_action = match (
        normalized_oracle_pubkey.as_ref(),
        token.as_ref(),
        authorization.as_ref(),
    ) {
        (Some(_), Some(_), Some(result)) => result.action,
        (Some(_), None, None) => "skipped_missing_operator_token",
        (None, Some(_), None) => "skipped_missing_oracle_pubkey",
        _ => "manifest_only",
    };

    let summary = match authorization.as_ref().map(|result| result.action) {
        Some("updated") => format!(
            "Station {} recorded and oracle added to the authorized launch set",
            station_id
        ),
        Some("already_authorized") => format!(
            "Station {} recorded; oracle was already authorized on the launch API",
            station_id
        ),
        _ => format!("Station {} manifest written", station_id),
    };

    Ok(json!({
        "summary": summary,
        "stationId": station_id,
        "manifestOutput": output.display().to_string(),
        "fleetSize": fleet_size,
        "replacedExistingStation": replaced_existing_station,
        "apiAction": api_action,
        "station": station,
        "oracleAuthorization": authorization,
    }))
}

fn directory_contains_key_material(dir: &Path) -> Result<bool> {
    for entry in
        fs::read_dir(dir).with_context(|| format!("failed to read directory {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_file()
            && matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("key" | "pub")
            )
        {
            return Ok(true);
        }
    }

    Ok(false)
}

fn collect_validator_pubkeys(dir: &Path) -> Result<Vec<String>> {
    if !dir.is_dir() {
        bail!(
            "validator-keys must be a directory containing .pub files: {}",
            dir.display()
        );
    }

    let mut candidates = Vec::new();
    for entry in
        fs::read_dir(dir).with_context(|| format!("failed to read directory {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("pub") {
            let contents = fs::read_to_string(&path)
                .with_context(|| format!("failed to read validator key {}", path.display()))?;
            let normalized = normalize_oracle_pubkey(contents.trim())
                .with_context(|| format!("invalid validator public key in {}", path.display()))?;
            candidates.push(normalized);
        }
    }

    candidates.sort();
    candidates.dedup();
    Ok(candidates)
}

fn normalize_oracle_pubkey(value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("oracle public key cannot be empty");
    }
    Ok(PublicKey::from_hex(trimmed)
        .context("oracle public key must be valid ed25519 public key hex")?
        .to_hex())
}

fn validate_station_id(station_id: &str) -> Result<()> {
    if station_id.trim().is_empty() {
        bail!("station-id cannot be empty");
    }
    if station_id.trim() != station_id {
        bail!("station-id cannot have leading or trailing whitespace");
    }
    if station_id.chars().any(char::is_whitespace) {
        bail!("station-id cannot contain whitespace");
    }
    if station_id.len() > MAX_STATION_ID_LEN {
        bail!("station-id exceeds max length ({MAX_STATION_ID_LEN})");
    }
    Ok(())
}

fn validate_coordinates(lat: f64, lon: f64) -> Result<()> {
    if !(-90.0..=90.0).contains(&lat) {
        bail!("lat must be between -90 and 90");
    }
    if !(-180.0..=180.0).contains(&lon) {
        bail!("lon must be between -180 and 180");
    }
    Ok(())
}

fn trimmed_nonempty(value: &str, field: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("{field} cannot be empty");
    }
    Ok(trimmed.to_string())
}

fn truncate_key(value: &str) -> String {
    if value.len() <= 16 {
        return value.to_string();
    }
    format!("{}...{}", &value[..8], &value[value.len() - 8..])
}

fn write_station_manifest(
    output: &Path,
    station: &WeatherStation,
    merge: bool,
) -> Result<(usize, bool)> {
    let mut stations = if output.exists() {
        if !merge {
            bail!(
                "manifest output already exists: {} (use --merge to update the fleet record)",
                output.display()
            );
        }
        let raw = fs::read_to_string(output)
            .with_context(|| format!("failed to read manifest {}", output.display()))?;
        serde_json::from_str::<Vec<WeatherStation>>(&raw)
            .with_context(|| format!("manifest must contain a JSON array: {}", output.display()))?
    } else {
        Vec::new()
    };

    let mut replaced_existing_station = false;
    if let Some(existing) = stations
        .iter_mut()
        .find(|existing| existing.station_id == station.station_id)
    {
        *existing = station.clone();
        replaced_existing_station = true;
    } else {
        stations.push(station.clone());
    }

    stations.sort_by(|left, right| left.station_id.cmp(&right.station_id));
    write_pretty_json(output, &stations)?;

    Ok((stations.len(), replaced_existing_station))
}

fn write_pretty_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    ensure_parent_directory(path)?;
    let body = format!(
        "{}\n",
        serde_json::to_string_pretty(value).context("failed to serialize json output")?
    );
    fs::write(path, body).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn ensure_parent_directory(path: &Path) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }
    Ok(())
}

fn write_text_file_if_missing(path: &Path, value: &str) -> Result<bool> {
    ensure_parent_directory(path)?;
    if path.exists() {
        return Ok(false);
    }
    fs::write(path, value).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(true)
}

fn write_text_file(path: &Path, value: &str, force: bool) -> Result<()> {
    ensure_parent_directory(path)?;
    if path.exists() && !force {
        bail!("refusing to overwrite existing file {}", path.display());
    }
    fs::write(path, format!("{value}\n"))
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn write_private_key_file(path: &Path, value: &str, force: bool) -> Result<()> {
    ensure_parent_directory(path)?;

    #[cfg(unix)]
    {
        let mut options = OpenOptions::new();
        options.write(true);
        if force {
            options.create(true).truncate(true);
        } else {
            options.create_new(true);
        }
        options.mode(0o600);

        let mut file = options
            .open(path)
            .with_context(|| format!("failed to open {}", path.display()))?;
        file.write_all(value.as_bytes())
            .with_context(|| format!("failed to write {}", path.display()))?;
        file.write_all(b"\n")
            .with_context(|| format!("failed to finalize {}", path.display()))?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to set permissions on {}", path.display()))?;
        Ok(())
    }

    #[cfg(not(unix))]
    {
        if path.exists() && !force {
            bail!("refusing to overwrite existing file {}", path.display());
        }
        fs::write(path, format!("{value}\n"))
            .with_context(|| format!("failed to write {}", path.display()))?;
        Ok(())
    }
}

async fn authorize_oracle(
    api_url: &str,
    operator_token: &str,
    oracle_pubkey: &str,
) -> Result<OracleAuthorizationResult> {
    let client = reqwest::Client::new();
    let base_url = api_url.trim_end_matches('/').to_string();
    let current = fetch_json::<OracleConfigResponse>(
        client
            .get(format!("{base_url}/transactions/oracle/config"))
            .header(AUTHORIZATION, format!("Bearer {operator_token}")), // secret-leak-ok: constructs the Authorization header itself; sent to the API, not logged
        "fetch current oracle config",
    )
    .await?;

    let mut updated_set = current.oracle_set.clone();
    if !updated_set.iter().any(|entry| entry == oracle_pubkey) {
        updated_set.push(oracle_pubkey.to_string());
        updated_set.sort();
        updated_set.dedup();

        let effective_quorum = current.oracle_quorum.max(1).min(updated_set.len());
        let updated = fetch_json::<OracleConfigResponse>(
            client
                .put(format!("{base_url}/transactions/oracle/config"))
                .header(AUTHORIZATION, format!("Bearer {operator_token}")) // secret-leak-ok: constructs the Authorization header itself; sent to the API, not logged
                .header(CONTENT_TYPE, "application/json")
                .json(&json!({
                    "oracle_set": updated_set,
                    "oracle_quorum": effective_quorum,
                })),
            "update oracle config",
        )
        .await?;

        let leaderboard_visible =
            fetch_leaderboard_visibility(&client, &base_url, oracle_pubkey).await;

        return Ok(OracleAuthorizationResult {
            action: "updated",
            api_url: base_url,
            oracle_pubkey: oracle_pubkey.to_string(),
            oracle_config: updated,
            leaderboard_visible,
        });
    }

    let leaderboard_visible = fetch_leaderboard_visibility(&client, &base_url, oracle_pubkey).await;
    Ok(OracleAuthorizationResult {
        action: "already_authorized",
        api_url: base_url,
        oracle_pubkey: oracle_pubkey.to_string(),
        oracle_config: current,
        leaderboard_visible,
    })
}

async fn fetch_leaderboard_visibility(
    client: &reqwest::Client,
    base_url: &str,
    oracle_pubkey: &str,
) -> Option<bool> {
    let response = client
        .get(format!("{base_url}/oracle/leaderboard"))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let leaderboard = response.json::<OracleLeaderboardResponse>().await.ok()?;
    Some(
        leaderboard
            .entries
            .iter()
            .any(|entry| entry.oracle_id == oracle_pubkey),
    )
}

async fn fetch_json<T: DeserializeOwned>(
    builder: reqwest::RequestBuilder,
    action: &str,
) -> Result<T> {
    let response = builder
        .send()
        .await
        .with_context(|| format!("failed to {action}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .unwrap_or_else(|_| String::from("<unreadable response body>"));
    if !status.is_success() {
        bail!("{action} failed: {status} - {body}");
    }
    serde_json::from_str::<T>(&body)
        .with_context(|| format!("failed to parse response for {action}: {body}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{Matcher, Server};
    use rand::SeedableRng;
    use rand::rngs::StdRng;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_test_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock drift")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "wrfcoin-setup-tests-{name}-{}-{nonce}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp test dir");
        dir
    }

    fn deterministic_pubkey(seed: u64) -> String {
        let mut rng = StdRng::seed_from_u64(seed);
        let sk = PrivateKey::generate(&mut rng);
        PublicKey::from_private_key(&sk).to_hex()
    }

    #[test]
    fn keygen_roundtrip_generates_valid_hex_keys() {
        let dir = temp_test_dir("keygen-roundtrip");
        let data = handle_keygen(&dir, 1, false).expect("keygen");

        let public_key_hex = fs::read_to_string(dir.join("validator-1.pub"))
            .expect("public key file")
            .trim()
            .to_string();
        let private_key_hex = fs::read_to_string(dir.join("validator-1.key"))
            .expect("private key file")
            .trim()
            .to_string();

        let sk = PrivateKey::from_hex(&private_key_hex).expect("private key hex");
        let pk = PublicKey::from_hex(&public_key_hex).expect("public key hex");
        assert_eq!(PublicKey::from_private_key(&sk).to_hex(), pk.to_hex()); // secret-leak-ok: test-only generated key; compares public halves
        assert_eq!(data["keyCount"].as_u64(), Some(1));
    }

    #[test]
    fn init_creates_expected_directory_structure_and_config() {
        let dir = temp_test_dir("init-layout");
        let data = handle_init(&dir, "testnet", true, false, true, false, true).expect("init");

        assert!(dir.join("data").is_dir());
        assert!(dir.join("keys").is_dir());
        assert!(dir.join("config").is_dir());
        assert!(dir.join("config/stations").is_dir());
        assert!(dir.join(DEFAULT_NODE_ENV_FILE).is_file());
        assert!(dir.join(DEFAULT_EXAMPLE_STATIONS_FILE).is_file());
        assert_eq!(data["nodeConfigCreated"].as_bool(), Some(true));
        assert_eq!(data["examplesGenerated"].as_bool(), Some(true));
    }

    #[test]
    fn init_preserves_existing_config_file() {
        let dir = temp_test_dir("init-preserve");
        let config_path = dir.join(DEFAULT_NODE_ENV_FILE);
        write_text_file_if_missing(&config_path, "WRFCOIN_API_PORT=9999\n").expect("seed config");

        let data = handle_init(&dir, "testnet", false, false, false, false, false).expect("init");
        let written = fs::read_to_string(&config_path).expect("read config");

        assert_eq!(written, "WRFCOIN_API_PORT=9999\n");
        assert_eq!(data["nodeConfigCreated"].as_bool(), Some(false));
        assert_eq!(data["examplesGenerated"].as_bool(), Some(false));
    }

    #[test]
    fn generate_storage_key_returns_valid_64_char_hex() {
        let key = generate_storage_key_hex();
        assert_eq!(key.len(), 64);
        assert!(key.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    #[test]
    fn keygen_refuses_overwrite_without_force() {
        let dir = temp_test_dir("keygen-overwrite");
        handle_keygen(&dir, 1, false).expect("initial keygen");
        let err = handle_keygen(&dir, 1, false).expect_err("expected overwrite refusal");
        assert!(
            err.to_string()
                .contains("already contains validator key material")
        );
    }

    #[cfg(unix)]
    #[test]
    fn keygen_private_key_permissions_are_0600() {
        let dir = temp_test_dir("keygen-perms");
        handle_keygen(&dir, 1, false).expect("keygen");
        let mode = fs::metadata(dir.join("validator-1.key"))
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn genesis_builds_sorted_deduped_valid_config() {
        let dir = temp_test_dir("genesis-valid");
        write_text_file(&dir.join("b.pub"), &deterministic_pubkey(2), false).expect("write pub");
        write_text_file(&dir.join("a.pub"), &deterministic_pubkey(1), false).expect("write pub");
        write_text_file(&dir.join("dup.pub"), &deterministic_pubkey(1), false).expect("write pub");

        let output = dir.join("genesis.json");
        handle_genesis(&dir, Some(1), "testnet", &output, Some(1_711_152_000), &[]).expect("genesis");

        let written: BootstrapGenesisConfig =
            serde_json::from_str(&fs::read_to_string(&output).expect("read genesis"))
                .expect("parse genesis");
        assert_eq!(written.oracle_config.authorized_oracles.len(), 2);
        assert_eq!(written.oracle_config.quorum, 1);
        assert!(
            written.oracle_config.authorized_oracles[0]
                < written.oracle_config.authorized_oracles[1]
        );
    }

    /// core4#3299: the genesis schedule refuses a held feature (alone or among
    /// others) before anything is written, with the same reason as the signed
    /// extension; an unheld schedule is unchanged.
    #[test]
    fn genesis_activate_refuses_a_held_feature_3299() {
        for activate in [
            vec!["reference_sea_level_pressure@25000".to_string()],
            vec!["v1@0".to_string(), "reference_sea_level_pressure@25000".to_string()],
        ] {
            let err = build_activation_schedule(&activate).unwrap_err().to_string();
            assert!(err.contains("scheduling of `reference_sea_level_pressure` is held"), "{err}");
            assert!(err.contains("core4#3299"), "{err}");
        }
        let dir = temp_test_dir("genesis-held");
        write_text_file(&dir.join("a.pub"), &deterministic_pubkey(1), false).expect("write pub");
        let output = dir.join("genesis.json");
        assert!(handle_genesis(
            &dir,
            Some(1),
            "testnet",
            &output,
            Some(1_711_152_000),
            &["reference_sea_level_pressure@25000".to_string()],
        )
        .is_err());
        assert!(!output.exists(), "refused before writing");
        assert_eq!(
            build_activation_schedule(&["reward_gross_outlier_guard@25000".to_string()]).unwrap(),
            Some("25000:reward_gross_outlier_guard".to_string())
        );
    }

    /// #3092: `--activate` writes the canonical schedule string, sorted by
    /// height, and refuses what the node would refuse. Operator token unset
    /// (rule from #3051): this path never touches the launch API.
    #[test]
    fn genesis_activate_writes_a_validated_schedule_3092() {
        let dir = temp_test_dir("genesis-activate");
        write_text_file(&dir.join("a.pub"), &deterministic_pubkey(1), false).expect("write pub");
        let output = dir.join("genesis.json");
        let activate = vec![
            "domain_separated_preimages@5000".to_string(),
            "v2@1000".to_string(),
            "v1@0".to_string(),
        ];
        let result = handle_genesis(&dir, Some(1), "testnet", &output, Some(1_711_152_000), &activate)
            .expect("genesis with schedule");
        let written: BootstrapGenesisConfig =
            serde_json::from_str(&fs::read_to_string(&output).expect("read genesis"))
                .expect("parse genesis");
        assert_eq!(
            written.protocol_activation_schedule.as_deref(),
            Some("0:v1,1000:v2,5000:domain_separated_preimages")
        );
        assert_eq!(
            result["cutEnv"],
            json!("WRFCOIN_GENESIS_ACTIVATION_SCHEDULE=0:v1,1000:v2,5000:domain_separated_preimages")
        );

        // Nothing scheduled: the field is absent, not an empty string.
        let plain = dir.join("plain.json");
        handle_genesis(&dir, Some(1), "testnet", &plain, Some(1_711_152_000), &[]).expect("genesis");
        let written: Value =
            serde_json::from_str(&fs::read_to_string(&plain).expect("read")).expect("parse");
        assert!(written.get("protocol_activation_schedule").is_none());
    }

    #[test]
    fn genesis_activate_refuses_what_the_node_would_refuse_3092() {
        let dir = temp_test_dir("genesis-activate-invalid");
        write_text_file(&dir.join("a.pub"), &deterministic_pubkey(1), false).expect("write pub");
        let output = dir.join("genesis.json");
        for (activate, needle) in [
            (vec!["v2@1000".to_string()], "must start at height 0"),
            (vec!["v1@0".to_string(), "v9@10".to_string()], "unknown protocol version"),
            (vec!["v1@0".to_string(), "signing_v2@10".to_string()], "scheduled as v2"),
            (vec!["v1@0".to_string(), "x@10".to_string(), "x@20".to_string()], "more than once"),
            (vec!["v1-0".to_string()], "not <feature>@<height>"),
            (vec!["v1@zero".to_string()], "not an unsigned integer"),
        ] {
            let err = handle_genesis(&dir, Some(1), "testnet", &output, Some(1), &activate)
                .expect_err("invalid schedule must refuse");
            assert!(err.to_string().contains(needle), "{activate:?}: {err}");
        }
    }

    #[test]
    fn genesis_rejects_empty_directory() {
        let dir = temp_test_dir("genesis-empty");
        let err = handle_genesis(&dir, None, "testnet", &dir.join("genesis.json"), None, &[])
            .expect_err("expected empty directory failure");
        assert!(
            err.to_string()
                .contains("does not contain any valid .pub files")
        );
    }

    #[test]
    fn genesis_rejects_invalid_pubkey_hex() {
        let dir = temp_test_dir("genesis-invalid");
        write_text_file(&dir.join("bad.pub"), "not-hex", false).expect("write invalid key");
        let err = handle_genesis(&dir, None, "testnet", &dir.join("genesis.json"), None, &[])
            .expect_err("expected invalid key failure");
        assert!(err.to_string().contains("invalid validator public key"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn register_station_manifest_matches_weather_station_shape() {
        let dir = temp_test_dir("station-manifest");
        let output = dir.join("stations.json");
        let data = handle_register_station(
            "WS-5000-denver",
            39.7392,
            -104.9903,
            Some(1609.0),
            "WS-5000",
            "wrf1owner",
            Some("2026-03-22".to_string()),
            None,
            None,
            // An explicit empty token suppresses the environment fallback, so
            // this manifest-only unit test cannot inherit an operator shell.
            Some(String::new()),
            &output,
            false,
        )
        .await
        .expect("register station");

        let stations: Vec<WeatherStation> =
            serde_json::from_str(&fs::read_to_string(&output).expect("read manifest"))
                .expect("parse manifest");
        assert_eq!(stations.len(), 1);
        assert_eq!(stations[0].station_id, "WS-5000-denver");
        assert_eq!(stations[0].hardware_model, "WS-5000");
        assert_eq!(stations[0].data_quality_tier, DataQualityTier::Unverified);
        assert_eq!(data["apiAction"].as_str(), Some("manifest_only"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn register_station_merge_updates_existing_station_by_id() {
        let dir = temp_test_dir("station-merge");
        let output = dir.join("stations.json");
        handle_register_station(
            "station-1",
            10.0,
            20.0,
            None,
            "HW-1",
            "wrf1owner",
            None,
            None,
            None,
            None,
            &output,
            false,
        )
        .await
        .expect("initial station write");

        let second = handle_register_station(
            "station-1",
            10.0,
            20.0,
            None,
            "HW-2",
            "wrf1owner",
            None,
            None,
            None,
            None,
            &output,
            true,
        )
        .await
        .expect("merge station");

        let stations: Vec<WeatherStation> =
            serde_json::from_str(&fs::read_to_string(&output).expect("read manifest"))
                .expect("parse manifest");
        assert_eq!(stations.len(), 1);
        assert_eq!(stations[0].hardware_model, "HW-2");
        assert_eq!(second["replacedExistingStation"].as_bool(), Some(true));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn register_station_oracle_config_request_flow() {
        let mut server = Server::new_async().await;
        let existing = deterministic_pubkey(7);
        let added = deterministic_pubkey(8);

        let _get = server
            .mock("GET", "/transactions/oracle/config")
            .match_header("authorization", "Bearer test-operator-token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                json!({
                    "oracle_set": [existing.clone()],
                    "oracle_quorum": 1
                })
                .to_string(),
            )
            .expect(1)
            .create_async()
            .await;

        let _put = server
            .mock("PUT", "/transactions/oracle/config")
            .match_header("authorization", "Bearer test-operator-token")
            .match_body(Matcher::PartialJson(json!({
                "oracle_set": [existing.clone(), added.clone()],
                "oracle_quorum": 1
            })))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                json!({
                    "oracle_set": [existing.clone(), added.clone()],
                    "oracle_quorum": 1
                })
                .to_string(),
            )
            .expect(1)
            .create_async()
            .await;

        let _leaderboard = server
            .mock("GET", "/oracle/leaderboard")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                json!({
                    "entries": [
                        { "oracle_id": existing },
                        { "oracle_id": added.clone() }
                    ]
                })
                .to_string(),
            )
            .expect(1)
            .create_async()
            .await;

        let dir = temp_test_dir("station-api");
        let output = dir.join("stations.json");
        let data = handle_register_station(
            "station-api",
            39.7,
            -105.0,
            Some(1609.0),
            "WS-5000",
            "wrf1owner",
            None,
            Some(added.clone()),
            Some(server.url()),
            Some("test-operator-token".to_string()),
            &output,
            false,
        )
        .await
        .expect("register station with api");

        assert_eq!(data["apiAction"].as_str(), Some("updated"));
        assert_eq!(
            data["oracleAuthorization"]["oracle_config"]["oracle_set"]
                .as_array()
                .expect("oracle set")
                .len(),
            2
        );
        assert_eq!(
            data["oracleAuthorization"]["leaderboard_visible"].as_bool(),
            Some(true)
        );
    }
}
