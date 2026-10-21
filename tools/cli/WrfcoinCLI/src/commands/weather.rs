use anyhow::Result;
use clap::Subcommand;
use log::info;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use wrfcoin_cli::{ApiClient, ApiConfig, KeyManager, SecurityService};

#[derive(Subcommand)]
pub enum WeatherCommands {
    /// Unavailable node-targeted weather submission
    #[clap(hide = true)]
    Submit {
        /// Weather parameter type (temperature, humidity, pressure, wind-speed, etc.)
        #[clap(long, required = true)]
        parameter: String,

        /// Value of the reading
        #[clap(long, required = true)]
        value: f64,

        /// Latitude of the reading location
        #[clap(long, required = true)]
        latitude: f64,

        /// Longitude of the reading location
        #[clap(long, required = true)]
        longitude: f64,

        /// Unit of measurement (celsius, fahrenheit, hpa, etc.)
        #[clap(long, required = true)]
        unit: String,

        /// Station ID (defaults to wallet primary address or "station-cli")
        #[clap(long)]
        station_id: Option<String>,

        /// Optional device ID for the sensor
        #[clap(long)]
        device_id: Option<String>,

        /// Source of the data
        #[clap(long)]
        source: Option<String>,

        /// Elevation in meters
        #[clap(long)]
        elevation: Option<f64>,

        /// Additional metadata in JSON format
        #[clap(long)]
        metadata: Option<String>,
    },

    /// Unavailable node-targeted batch submission
    #[clap(hide = true)]
    SubmitBatch {
        /// Path to JSON file containing batch data
        #[clap(long, required = true)]
        file: PathBuf,

        /// Validate data before submission
        #[clap(long)]
        validate: bool,
    },

    /// Unavailable node-targeted wind submission
    #[clap(hide = true)]
    SubmitWind {
        /// Wind speed value
        #[clap(long, required = true)]
        speed: f64,

        /// Wind direction in degrees
        #[clap(long, required = true)]
        direction: f64,

        /// Latitude of the reading location
        #[clap(long, required = true)]
        latitude: f64,

        /// Longitude of the reading location
        #[clap(long, required = true)]
        longitude: f64,

        /// Unit for wind speed (km/h, m/s, mph, knots)
        #[clap(long, required = true)]
        unit: String,

        /// Optional device ID for the sensor
        #[clap(long)]
        device_id: Option<String>,

        /// Source of the data
        #[clap(long)]
        source: Option<String>,

        /// Elevation in meters
        #[clap(long)]
        elevation: Option<f64>,
    },

    /// Unavailable weather NFT creation
    #[clap(hide = true)]
    CreateNFT {
        /// Title of the NFT
        #[clap(long, required = true)]
        title: String,

        /// Description of the NFT
        #[clap(long)]
        description: Option<String>,

        /// Comma-separated list of data point IDs to include
        #[clap(long, required = true)]
        data_points: String,

        /// Category for the NFT
        #[clap(long, default_value = "weather-data")]
        category: String,

        /// License for the data
        #[clap(long, default_value = "CC BY 4.0")]
        license: String,

        /// Creator identifier
        #[clap(long)]
        creator: Option<String>,

        /// Comma-separated tags
        #[clap(long)]
        tags: Option<String>,
    },

    /// Unavailable node-targeted weather history
    #[clap(hide = true)]
    QueryHistory {
        /// Start date (YYYY-MM-DD or YYYY-MM-DDThh:mm:ssZ)
        #[clap(long, required = true)]
        start_date: String,

        /// End date (YYYY-MM-DD or YYYY-MM-DDThh:mm:ssZ)
        #[clap(long, required = true)]
        end_date: String,

        /// Minimum latitude for bounding box
        #[clap(long)]
        min_lat: Option<f64>,

        /// Maximum latitude for bounding box
        #[clap(long)]
        max_lat: Option<f64>,

        /// Minimum longitude for bounding box
        #[clap(long)]
        min_lon: Option<f64>,

        /// Maximum longitude for bounding box
        #[clap(long)]
        max_lon: Option<f64>,

        /// Sensor types to query (comma-separated)
        #[clap(long)]
        sensor_types: Option<String>,

        /// Aggregation level (hourly, daily, weekly)
        #[clap(long)]
        aggregation: Option<String>,

        /// Statistics to calculate (min, max, avg, median)
        #[clap(long)]
        statistics: Option<String>,

        /// Output format (json, csv, table)
        #[clap(long, default_value = "table")]
        format: String,

        /// Output file path (if not provided, prints to stdout)
        #[clap(long)]
        output: Option<PathBuf>,
    },

    /// Unavailable node-targeted weather queue sync
    #[clap(hide = true)]
    Sync {
        /// Maximum number of entries to sync per batch
        #[clap(long, default_value_t = 100)]
        batch: usize,
    },

    /// Show weather queue status
    Queue {
        /// Show verbose details for each entry
        #[clap(long)]
        verbose: bool,

        /// Filter by queue status (pending, synced, conflict, error)
        #[clap(long)]
        status: Option<String>,
    },

    /// Purge synced or old entries from the weather queue
    QueuePurge {
        /// Only purge entries older than N days
        #[clap(long)]
        older_than: Option<u32>,

        /// Purge all entries regardless of status
        #[clap(long)]
        all: bool,
    },

    /// Inspect a single queued weather entry
    QueueInspect {
        /// Queue entry ID to inspect
        #[clap(long, required = true)]
        id: i64,
    },
}

pub async fn handle_weather_command(command: &WeatherCommands) -> Result<Value> {
    let unavailable = match command {
        WeatherCommands::Submit { .. } => Some("weather submit"),
        WeatherCommands::SubmitBatch { .. } => Some("weather submit-batch"),
        WeatherCommands::SubmitWind { .. } => Some("weather submit-wind"),
        WeatherCommands::CreateNFT { .. } => Some("weather create-nft"),
        WeatherCommands::QueryHistory { .. } => Some("weather query-history"),
        WeatherCommands::Sync { .. } => Some("weather sync"),
        _ => None,
    };
    // Refuse before reading a batch file, wallet key or offline queue. The
    // backend's real submission service needs a separate authenticated client;
    // these commands currently use the node client, which serves no such path.
    if let Some(name) = unavailable {
        return Err(wrfcoin_cli::errors::CliError::new(
            wrfcoin_cli::CliErrorCode::NotFound,
            format!("{name}: not wired in this CLI; backend weather submission requires a separate authenticated integration (see wrfcoin/core4#3247)"),
        ).into());
    }
    let client = ApiClient::new(ApiConfig::default())?;

    match command {
        WeatherCommands::Submit {
            parameter,
            value,
            latitude,
            longitude,
            unit,
            station_id,
            device_id,
            source,
            elevation,
            metadata,
        } => {
            info!("Submitting weather data: {} = {}{}", parameter, value, unit);

            let resolved_station_id = station_id.clone().unwrap_or_else(|| {
                resolve_wallet_station_id().unwrap_or_else(|| "station-cli".to_string())
            });

            let mut observation = json!({
                "type": parameter,
                "value": value,
                "unit": unit,
                "latitude": latitude,
                "longitude": longitude,
                "timestamp": chrono::Utc::now().to_rfc3339(),
            });
            if let Some(dev) = device_id {
                observation["device_id"] = json!(dev);
            }
            if let Some(src) = source {
                observation["source"] = json!(src);
            }
            if let Some(elev) = elevation {
                observation["elevation"] = json!(elev);
            }
            if let Some(meta) = metadata {
                if let Ok(parsed) = serde_json::from_str::<Value>(meta) {
                    observation["metadata"] = parsed;
                }
            }

            // Generate replay-safe nonce
            let nonce = SecurityService::generate_time_based_nonce();
            let nonce_str = nonce.to_string();

            // Sign observation + nonce together for replay safety
            let observation_json = serde_json::to_string(&observation)?;
            let sign_payload = format!("{}:{}", observation_json, nonce_str);
            let signature = sign_with_wallet_key(sign_payload.as_bytes())
                .unwrap_or_else(|| "local-cli-signature".to_string());

            // Try online submission
            match client
                .submit_weather_observation(&resolved_station_id, observation.clone(), &signature)
                .await
            {
                Ok(response) => Ok(json!({
                    "summary": "Weather observation submitted",
                    "mode": "online",
                    "observation_type": parameter,
                    "station_id": resolved_station_id,
                    "nonce": nonce_str,
                    "location": { "latitude": latitude, "longitude": longitude },
                    "result": response
                })),
                Err(e) => {
                    // Offline fallback: enqueue locally
                    info!("API unavailable, queuing observation offline: {}", e);
                    let db = open_db()?;
                    let created_at = chrono::Utc::now().to_rfc3339();
                    let queue_id = db.enqueue_weather(
                        &nonce_str,
                        &resolved_station_id,
                        &observation_json,
                        &signature,
                        &created_at,
                    )?;

                    Ok(json!({
                        "summary": "Weather observation queued offline",
                        "mode": "offline",
                        "queue_id": queue_id,
                        "observation_type": parameter,
                        "station_id": resolved_station_id,
                        "nonce": nonce_str,
                        "location": { "latitude": latitude, "longitude": longitude },
                        "reason": e.to_string()
                    }))
                },
            }
        },

        WeatherCommands::SubmitBatch { file, validate: _ } => {
            let raw = fs::read_to_string(file)?;
            let payload = serde_json::from_str::<Value>(&raw)?;
            let array = payload
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("submit-batch file must contain a JSON array"))?;

            let mut accepted = Vec::new();
            for item in array {
                let sensor_type = item
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("generic");
                let sensor_value = item.get("value").and_then(Value::as_f64).unwrap_or(0.0);
                let observation = json!({
                    "type": sensor_type,
                    "value": sensor_value,
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                });
                let response = client
                    .submit_weather_observation("station-cli", observation, "local-cli-signature")
                    .await?;
                accepted.push(response);
            }

            Ok(json!({
                "summary": "Weather batch submitted",
                "count": accepted.len(),
                "results": accepted
            }))
        },

        WeatherCommands::SubmitWind {
            speed,
            direction,
            latitude,
            longitude,
            unit,
            device_id,
            source,
            elevation,
        } => {
            info!(
                "Submitting wind data: speed = {}, direction = {}°",
                speed, direction
            );
            let resolved_station_id =
                resolve_wallet_station_id().unwrap_or_else(|| "station-cli".to_string());
            let mut observation = json!({
                "type": "wind",
                "value": speed,
                "unit": unit,
                "direction": direction,
                "latitude": latitude,
                "longitude": longitude,
                "timestamp": chrono::Utc::now().to_rfc3339(),
            });
            if let Some(dev) = device_id {
                observation["device_id"] = json!(dev);
            }
            if let Some(src) = source {
                observation["source"] = json!(src);
            }
            if let Some(elev) = elevation {
                observation["elevation"] = json!(elev);
            }
            let observation_bytes = serde_json::to_string(&observation)?;
            let signature = sign_with_wallet_key(observation_bytes.as_bytes())
                .unwrap_or_else(|| "local-cli-signature".to_string());
            let response = client
                .submit_weather_observation(&resolved_station_id, observation, &signature)
                .await?;
            Ok(json!({
                "summary": "Wind observation submitted",
                "location": { "latitude": latitude, "longitude": longitude },
                "result": response
            }))
        },

        WeatherCommands::CreateNFT {
            title,
            description,
            data_points,
            category,
            license,
            creator,
            tags,
        } => {
            let body = json!({
                "title": title,
                "description": description.clone().unwrap_or_else(|| "Weather data bundle NFT".to_string()),
                "computationType": category,
                "reward": 0.1,
                "creator": creator,
                "metadata": {
                    "dataPoints": data_points,
                    "license": license,
                    "tags": tags
                }
            });
            let created: Value = client.post("/nft/create", Some(body)).await?;
            Ok(json!({
                "summary": "Weather NFT created",
                "result": created
            }))
        },

        WeatherCommands::QueryHistory {
            start_date,
            end_date,
            min_lat,
            max_lat,
            min_lon,
            max_lon,
            sensor_types,
            aggregation,
            statistics,
            format: _,
            output,
        } => {
            let query = vec![
                ("start".to_string(), start_date.clone()),
                ("end".to_string(), end_date.clone()),
                (
                    "minLat".to_string(),
                    min_lat.map_or(String::new(), |v| v.to_string()),
                ),
                (
                    "maxLat".to_string(),
                    max_lat.map_or(String::new(), |v| v.to_string()),
                ),
                (
                    "minLon".to_string(),
                    min_lon.map_or(String::new(), |v| v.to_string()),
                ),
                (
                    "maxLon".to_string(),
                    max_lon.map_or(String::new(), |v| v.to_string()),
                ),
                (
                    "types".to_string(),
                    sensor_types.clone().unwrap_or_default(),
                ),
                (
                    "aggregation".to_string(),
                    aggregation.clone().unwrap_or_default(),
                ),
                (
                    "statistics".to_string(),
                    statistics.clone().unwrap_or_default(),
                ),
            ];
            let filtered: Vec<(String, String)> =
                query.into_iter().filter(|(_, v)| !v.is_empty()).collect();
            let path = if filtered.is_empty() {
                "/weather/history".to_string()
            } else {
                let parts: Vec<String> = filtered
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect();
                format!("/weather/history?{}", parts.join("&"))
            };
            let history: Value = client.get(&path, None).await?;
            if let Some(path) = output {
                fs::write(path, serde_json::to_string_pretty(&history)?)?;
            }
            Ok(json!({
                "summary": "Weather history retrieved",
                "history": history,
                "output": output
            }))
        },

        WeatherCommands::Sync { batch } => {
            let db = open_db()?;
            let pending = db.get_pending_weather(*batch)?;

            if pending.is_empty() {
                return Ok(json!({
                    "summary": "No pending weather observations to sync",
                    "synced": 0,
                    "conflicts": 0,
                    "errors": 0
                }));
            }

            let mut synced = 0u64;
            let mut conflicts = 0u64;
            let mut errors = 0u64;

            for entry in &pending {
                let payload: Value =
                    serde_json::from_str(&entry.payload).unwrap_or_else(|_| json!({}));

                match client
                    .submit_weather_observation(&entry.station_id, payload, &entry.signature)
                    .await
                {
                    Ok(_) => {
                        db.mark_weather_synced(entry.id)?;
                        synced += 1;
                    },
                    Err(e) => {
                        let err_str = e.to_string();
                        if err_str.contains("409")
                            || err_str.contains("duplicate")
                            || err_str.contains("conflict")
                        {
                            db.mark_weather_conflict(entry.id, &err_str)?;
                            conflicts += 1;
                        } else {
                            db.mark_weather_error(entry.id, &err_str)?;
                            errors += 1;
                        }
                    },
                }
            }

            Ok(json!({
                "summary": format!("Sync complete: {} synced, {} conflicts, {} errors", synced, conflicts, errors),
                "synced": synced,
                "conflicts": conflicts,
                "errors": errors,
                "total_processed": pending.len()
            }))
        },

        WeatherCommands::Queue { verbose, status } => {
            let db = open_db()?;
            let stats = db.get_weather_queue_stats()?;

            let mut result = json!({
                "summary": format!("Weather queue: {} total entries", stats.get("total").and_then(Value::as_i64).unwrap_or(0)),
                "stats": stats,
            });

            if *verbose {
                let limit = 50;
                let entries = if let Some(ref s) = status {
                    // Filter by status - get all pending and filter
                    let all = db.get_pending_weather(limit)?;
                    all.into_iter()
                        .filter(|e| e.status == *s)
                        .collect::<Vec<_>>()
                } else {
                    db.get_pending_weather(limit)?
                };
                result["entries"] = serde_json::to_value(&entries)?;
            }

            Ok(result)
        },

        WeatherCommands::QueuePurge { older_than, all } => {
            let db = open_db()?;

            let purge_status = if *all { None } else { Some("synced") };
            let count = db.purge_weather_queue(purge_status, *older_than)?;

            Ok(json!({
                "summary": format!("Purged {} entries from weather queue", count),
                "purged": count,
                "filter_status": purge_status,
                "older_than_days": older_than,
            }))
        },

        WeatherCommands::QueueInspect { id } => {
            let db = open_db()?;
            let entry = db.get_weather_queue_entry(*id)?;

            let payload: Value = serde_json::from_str(&entry.payload).unwrap_or(json!(null));

            Ok(json!({
                "summary": format!("Queue entry #{}: {} ({})", entry.id, entry.station_id, entry.status),
                "id": entry.id,
                "nonce": entry.nonce,
                "station_id": entry.station_id,
                "payload": payload,
                "signature": entry.signature,
                "created_at": entry.created_at,
                "status": entry.status,
                "last_error": entry.last_error,
                "sync_attempts": entry.sync_attempts,
            }))
        },
    }
}

/// Try to resolve a station ID from the wallet's primary address.
pub fn resolve_wallet_station_id() -> Option<String> {
    let home = dirs::home_dir()?;
    let base_dir = home.join(".wrfcoin");
    let manager = KeyManager::new(&base_dir).ok()?;
    let pairs = manager.list_key_pairs().ok()?;
    pairs.first().map(|(addr, _)| addr.clone())
}

/// Try to sign data with the wallet's primary key. Returns None if no key is available.
pub fn sign_with_wallet_key(data: &[u8]) -> Option<String> {
    let home = dirs::home_dir()?;
    let base_dir = home.join(".wrfcoin");
    let manager = KeyManager::new(&base_dir).ok()?;
    let pairs = manager.list_key_pairs().ok()?;
    let (addr, _) = pairs.first()?;
    let key_pair = manager.load_key_pair(addr).ok()?;
    key_pair.sign(data).ok()
}

/// Open the local SQLite database for offline queue operations.
fn open_db() -> Result<wrfcoin_cli::DbClient> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home directory"))?;
    wrfcoin_cli::DbClient::new(home.join(".wrfcoin/data/wrfcoin.db"))
}
