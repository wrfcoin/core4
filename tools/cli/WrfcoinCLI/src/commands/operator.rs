use anyhow::{Result, anyhow};
use clap::Subcommand;
use log::warn;
use serde_json::{Value, json};
use std::time::Instant;
use wrfcoin_cli::{ApiClient, ApiConfig};

use super::watch::{WatchConfig, watch_loop};
use crate::cli::commands::ExecutionContext;

// ---------------------------------------------------------------------------
// Subcommand enum
// ---------------------------------------------------------------------------

/// Operator incident-mode commands for health monitoring and remediation.
#[derive(Subcommand)]
pub enum OperatorCommands {
    /// Run health checks across all subsystems
    Status {
        /// Treat degraded subsystems as failures (non-zero exit)
        #[clap(long)]
        strict: bool,

        /// Continuously monitor health at regular intervals
        #[clap(long)]
        watch: bool,

        /// Watch interval in seconds
        #[clap(long, default_value_t = 30)]
        interval: u64,
    },
}

// ---------------------------------------------------------------------------
// Health types
// ---------------------------------------------------------------------------

/// Status of an individual subsystem.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Critical,
    Unknown,
}

impl HealthStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Degraded => "degraded",
            Self::Critical => "critical",
            Self::Unknown => "unknown",
        }
    }
}

/// Result of a single subsystem health check.
pub struct SubsystemHealth {
    name: &'static str,
    status: HealthStatus,
    score: u8,
    message: String,
    latency_ms: Option<u64>,
    remediation: Option<Vec<&'static str>>,
}

impl SubsystemHealth {
    fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "status": self.status.as_str(),
            "score": self.score,
            "message": self.message,
            "latency_ms": self.latency_ms,
            "remediation": self.remediation,
        })
    }
}

// ---------------------------------------------------------------------------
// Overall score
// ---------------------------------------------------------------------------

fn calculate_overall(subsystems: &[SubsystemHealth]) -> (HealthStatus, u8) {
    if subsystems.is_empty() {
        return (HealthStatus::Unknown, 0);
    }

    let total_score: u32 = subsystems.iter().map(|s| s.score as u32).sum();
    let avg_score = (total_score / subsystems.len() as u32) as u8;

    let has_critical = subsystems
        .iter()
        .any(|s| s.status == HealthStatus::Critical);
    let has_degraded = subsystems
        .iter()
        .any(|s| s.status == HealthStatus::Degraded);

    let overall = if has_critical {
        HealthStatus::Critical
    } else if has_degraded {
        HealthStatus::Degraded
    } else {
        HealthStatus::Healthy
    };

    (overall, avg_score)
}

// ---------------------------------------------------------------------------
// Individual health checks
// ---------------------------------------------------------------------------

/// Latency threshold (milliseconds) above which the API is considered degraded.
const API_LATENCY_DEGRADED_MS: u64 = 2_000;

/// Minimum peer count considered healthy.
const MIN_HEALTHY_PEERS: u64 = 5;

/// Weather data staleness threshold in seconds (15 minutes).
const WEATHER_STALE_SECS: i64 = 15 * 60;

/// Block production stall warning threshold in seconds (2 minutes).
const BLOCK_STALL_WARN_SECS: i64 = 2 * 60;

/// Block production stall critical threshold in seconds (10 minutes).
const BLOCK_STALL_CRITICAL_SECS: i64 = 10 * 60;

async fn check_api(client: &ApiClient) -> SubsystemHealth {
    let start = Instant::now();
    match client.get::<Value>("/health", None).await {
        Ok(_) => {
            let elapsed = start.elapsed().as_millis() as u64;
            if elapsed > API_LATENCY_DEGRADED_MS {
                warn!("API response slow: {}ms", elapsed);
                SubsystemHealth {
                    name: "API",
                    status: HealthStatus::Degraded,
                    score: 50,
                    message: format!("API responding but slow ({}ms)", elapsed),
                    latency_ms: Some(elapsed),
                    remediation: Some(vec![
                        "Check node resource utilization",
                        "Verify API port configuration",
                        "Review recent deployment changes",
                    ]),
                }
            } else {
                SubsystemHealth {
                    name: "API",
                    status: HealthStatus::Healthy,
                    score: 100,
                    message: format!("API responding in {}ms", elapsed),
                    latency_ms: Some(elapsed),
                    remediation: None,
                }
            }
        },
        Err(e) => {
            let elapsed = start.elapsed().as_millis() as u64;
            let msg = e.to_string();
            // Distinguish 5xx from connection-refused.
            if msg.contains("500") || msg.contains("502") || msg.contains("503") {
                warn!("API returned server error: {}", msg);
                SubsystemHealth {
                    name: "API",
                    status: HealthStatus::Degraded,
                    score: 30,
                    message: format!("API returned server error ({}ms)", elapsed),
                    latency_ms: Some(elapsed),
                    remediation: Some(vec![
                        "Check if wrfcoin node is running",
                        "Verify API port configuration",
                        "Check firewall rules",
                    ]),
                }
            } else {
                warn!("API unreachable: {}", msg);
                SubsystemHealth {
                    name: "API",
                    status: HealthStatus::Critical,
                    score: 0,
                    message: "API unreachable".to_string(),
                    latency_ms: Some(elapsed),
                    remediation: Some(vec![
                        "Check if wrfcoin node is running",
                        "Verify API port configuration",
                        "Check firewall rules",
                    ]),
                }
            }
        },
    }
}

async fn check_node_sync(client: &ApiClient) -> SubsystemHealth {
    let start = Instant::now();
    match client.get::<Value>("/network/info", None).await {
        Ok(info) => {
            let elapsed = start.elapsed().as_millis() as u64;
            let has_sync = info.get("sync").is_some()
                || info.get("syncing").is_some()
                || info.get("currentBlock").is_some()
                || info.get("chainId").is_some();

            if has_sync {
                SubsystemHealth {
                    name: "Node Sync",
                    status: HealthStatus::Healthy,
                    score: 100,
                    message: "Node sync data present".to_string(),
                    latency_ms: Some(elapsed),
                    remediation: None,
                }
            } else {
                warn!("Node sync data missing or stale");
                SubsystemHealth {
                    name: "Node Sync",
                    status: HealthStatus::Degraded,
                    score: 50,
                    message: "Node sync data stale or incomplete".to_string(),
                    latency_ms: Some(elapsed),
                    remediation: Some(vec![
                        "Check node synchronization status",
                        "Verify network connectivity",
                        "Restart node if sync is stuck",
                    ]),
                }
            }
        },
        Err(e) => {
            let elapsed = start.elapsed().as_millis() as u64;
            warn!("Node sync check failed: {}", e);
            SubsystemHealth {
                name: "Node Sync",
                status: HealthStatus::Critical,
                score: 0,
                message: "Node sync unreachable".to_string(),
                latency_ms: Some(elapsed),
                remediation: Some(vec![
                    "Check node synchronization status",
                    "Verify network connectivity",
                    "Restart node if sync is stuck",
                ]),
            }
        },
    }
}

async fn check_peers(client: &ApiClient) -> SubsystemHealth {
    let start = Instant::now();
    match client.get::<Value>("/network/peers", None).await {
        Ok(val) => {
            let elapsed = start.elapsed().as_millis() as u64;
            let peer_count = if let Some(n) = val.get("connected_peers").and_then(Value::as_u64) {
                n
            } else if let Some(n) = val.get("peer_count").and_then(Value::as_u64) {
                n
            } else if let Some(arr) = val.as_array() {
                arr.len() as u64
            } else if let Some(n) = val.get("count").and_then(Value::as_u64) {
                n
            } else {
                0
            };

            if peer_count >= MIN_HEALTHY_PEERS {
                SubsystemHealth {
                    name: "Peers",
                    status: HealthStatus::Healthy,
                    score: 100,
                    message: format!("{} peers connected", peer_count),
                    latency_ms: Some(elapsed),
                    remediation: None,
                }
            } else if peer_count > 0 {
                warn!("Low peer count: {}", peer_count);
                SubsystemHealth {
                    name: "Peers",
                    status: HealthStatus::Degraded,
                    score: 50,
                    message: format!("Low peer count: {}", peer_count),
                    latency_ms: Some(elapsed),
                    remediation: Some(vec![
                        "Check network connectivity",
                        "Verify peer discovery configuration",
                        "Restart node",
                    ]),
                }
            } else {
                warn!("No peers connected");
                SubsystemHealth {
                    name: "Peers",
                    status: HealthStatus::Critical,
                    score: 0,
                    message: "No peers connected".to_string(),
                    latency_ms: Some(elapsed),
                    remediation: Some(vec![
                        "Check network connectivity",
                        "Verify peer discovery configuration",
                        "Restart node",
                    ]),
                }
            }
        },
        Err(e) => {
            let elapsed = start.elapsed().as_millis() as u64;
            warn!("Peer check failed: {}", e);
            SubsystemHealth {
                name: "Peers",
                status: HealthStatus::Critical,
                score: 0,
                message: "Peer endpoint unreachable".to_string(),
                latency_ms: Some(elapsed),
                remediation: Some(vec![
                    "Check network connectivity",
                    "Verify peer discovery configuration",
                    "Restart node",
                ]),
            }
        },
    }
}

async fn check_mining(client: &ApiClient) -> SubsystemHealth {
    let start = Instant::now();
    match client.get::<Value>("/mining/stats", None).await {
        Ok(stats) => {
            let elapsed = start.elapsed().as_millis() as u64;
            let hashrate = stats
                .get("hashrate")
                .or_else(|| stats.get("hashRate"))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);

            if hashrate > 0.0 {
                SubsystemHealth {
                    name: "Mining",
                    status: HealthStatus::Healthy,
                    score: 100,
                    message: format!("Mining active (hashrate: {:.2})", hashrate),
                    latency_ms: Some(elapsed),
                    remediation: None,
                }
            } else {
                warn!("Mining hashrate is zero");
                SubsystemHealth {
                    name: "Mining",
                    status: HealthStatus::Critical,
                    score: 0,
                    message: "Mining hashrate is zero".to_string(),
                    latency_ms: Some(elapsed),
                    remediation: Some(vec![
                        "Check mining configuration",
                        "Verify GPU/CPU resources",
                        "Restart mining process",
                    ]),
                }
            }
        },
        Err(e) => {
            let elapsed = start.elapsed().as_millis() as u64;
            warn!("Mining stats check failed: {}", e);
            SubsystemHealth {
                name: "Mining",
                status: HealthStatus::Critical,
                score: 0,
                message: "Mining stats unreachable".to_string(),
                latency_ms: Some(elapsed),
                remediation: Some(vec![
                    "Check mining configuration",
                    "Verify GPU/CPU resources",
                    "Restart mining process",
                ]),
            }
        },
    }
}

async fn check_bridges(client: &ApiClient) -> SubsystemHealth {
    let start = Instant::now();
    match client.get::<Value>("/bridge/safety/status", None).await {
        Ok(val) => {
            let elapsed = start.elapsed().as_millis() as u64;
            let bridges = val
                .as_array()
                .or_else(|| val.get("bridges").and_then(Value::as_array));

            match bridges {
                Some(arr) if !arr.is_empty() => {
                    let total = arr.len();
                    let active = arr
                        .iter()
                        .filter(|b| {
                            b.get("status")
                                .and_then(Value::as_str)
                                .map(|s| s == "active" || s == "healthy" || s == "connected")
                                .unwrap_or(false)
                        })
                        .count();

                    if active == total {
                        SubsystemHealth {
                            name: "Bridges",
                            status: HealthStatus::Healthy,
                            score: 100,
                            message: format!("All {} bridges active", total),
                            latency_ms: Some(elapsed),
                            remediation: None,
                        }
                    } else if active > 0 {
                        warn!("{}/{} bridges degraded", total - active, total);
                        SubsystemHealth {
                            name: "Bridges",
                            status: HealthStatus::Degraded,
                            score: 50,
                            message: format!("{}/{} bridges active", active, total),
                            latency_ms: Some(elapsed),
                            remediation: Some(vec![
                                "Check bridge service status",
                                "Verify cross-chain connectivity",
                            ]),
                        }
                    } else {
                        warn!("All bridges down");
                        SubsystemHealth {
                            name: "Bridges",
                            status: HealthStatus::Critical,
                            score: 0,
                            message: format!("All {} bridges down", total),
                            latency_ms: Some(elapsed),
                            remediation: Some(vec![
                                "Check bridge service status",
                                "Verify cross-chain connectivity",
                            ]),
                        }
                    }
                },
                _ => {
                    // No bridges array returned; treat as healthy-with-info.
                    SubsystemHealth {
                        name: "Bridges",
                        status: HealthStatus::Healthy,
                        score: 100,
                        message: "Bridge endpoint reachable (no bridges configured)".to_string(),
                        latency_ms: Some(elapsed),
                        remediation: None,
                    }
                },
            }
        },
        Err(e) => {
            let elapsed = start.elapsed().as_millis() as u64;
            warn!("Bridge check failed: {}", e);
            SubsystemHealth {
                name: "Bridges",
                status: HealthStatus::Critical,
                score: 0,
                message: "Bridge endpoint unreachable".to_string(),
                latency_ms: Some(elapsed),
                remediation: Some(vec![
                    "Check bridge service status",
                    "Verify cross-chain connectivity",
                ]),
            }
        },
    }
}

async fn check_weather(client: &ApiClient) -> SubsystemHealth {
    let start = Instant::now();
    match client.get::<Value>("/weather/stations/default", None).await {
        Ok(data) => {
            let elapsed = start.elapsed().as_millis() as u64;

            // Try to determine data freshness from a timestamp field.
            let stale = if let Some(ts_str) = data
                .get("lastUpdate")
                .or_else(|| data.get("timestamp"))
                .or_else(|| data.get("last_reading"))
                .and_then(Value::as_str)
            {
                chrono::DateTime::parse_from_rfc3339(ts_str)
                    .map(|dt| {
                        let age = chrono::Utc::now().signed_duration_since(dt);
                        age.num_seconds() > WEATHER_STALE_SECS
                    })
                    .unwrap_or(false)
            } else {
                false
            };

            if stale {
                warn!("Weather data stale (>15 min old)");
                SubsystemHealth {
                    name: "Weather",
                    status: HealthStatus::Degraded,
                    score: 50,
                    message: "Weather data stale (>15 min)".to_string(),
                    latency_ms: Some(elapsed),
                    remediation: Some(vec![
                        "Check weather station connectivity",
                        "Verify sensor data pipeline",
                    ]),
                }
            } else {
                SubsystemHealth {
                    name: "Weather",
                    status: HealthStatus::Healthy,
                    score: 100,
                    message: format!("Weather pipeline healthy ({}ms)", elapsed),
                    latency_ms: Some(elapsed),
                    remediation: None,
                }
            }
        },
        Err(e) => {
            let elapsed = start.elapsed().as_millis() as u64;
            let msg = e.to_string();
            // A 404 likely means no station is configured -- treat as unknown, not critical.
            if msg.contains("404") || msg.contains("not found") {
                SubsystemHealth {
                    name: "Weather",
                    status: HealthStatus::Unknown,
                    score: 50,
                    message: "No default weather station configured".to_string(),
                    latency_ms: Some(elapsed),
                    remediation: None,
                }
            } else {
                warn!("Weather pipeline check failed: {}", msg);
                SubsystemHealth {
                    name: "Weather",
                    status: HealthStatus::Critical,
                    score: 0,
                    message: "Weather pipeline unreachable".to_string(),
                    latency_ms: Some(elapsed),
                    remediation: Some(vec![
                        "Check weather station connectivity",
                        "Verify sensor data pipeline",
                    ]),
                }
            }
        },
    }
}

async fn check_block_production(client: &ApiClient) -> SubsystemHealth {
    let start = Instant::now();
    let query: &[(&str, &str)] = &[("limit", "2")];
    match client.get::<Value>("/chain/blocks", Some(query)).await {
        Ok(val) => {
            let elapsed = start.elapsed().as_millis() as u64;
            let blocks = val
                .as_array()
                .or_else(|| val.get("blocks").and_then(Value::as_array));

            match blocks {
                Some(arr) if !arr.is_empty() => {
                    // Determine staleness from the most recent block timestamp.
                    let latest_ts = arr
                        .first()
                        .and_then(|b| {
                            b.get("timestamp")
                                .or_else(|| b.get("time"))
                                .and_then(Value::as_str)
                        })
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok());

                    match latest_ts {
                        Some(ts) => {
                            let age = chrono::Utc::now().signed_duration_since(ts).num_seconds();

                            if age < BLOCK_STALL_WARN_SECS {
                                SubsystemHealth {
                                    name: "Block Production",
                                    status: HealthStatus::Healthy,
                                    score: 100,
                                    message: format!("Latest block {}s ago", age),
                                    latency_ms: Some(elapsed),
                                    remediation: None,
                                }
                            } else if age < BLOCK_STALL_CRITICAL_SECS {
                                warn!("Block production stalled ({}s)", age);
                                SubsystemHealth {
                                    name: "Block Production",
                                    status: HealthStatus::Degraded,
                                    score: 50,
                                    message: format!(
                                        "Block production slow ({}s since last block)",
                                        age
                                    ),
                                    latency_ms: Some(elapsed),
                                    remediation: Some(vec![
                                        "Check consensus participation",
                                        "Verify validator status",
                                        "Check for chain forks",
                                    ]),
                                }
                            } else {
                                warn!("Block production stalled ({}s)", age);
                                SubsystemHealth {
                                    name: "Block Production",
                                    status: HealthStatus::Critical,
                                    score: 0,
                                    message: format!(
                                        "Block production stalled ({}s since last block)",
                                        age
                                    ),
                                    latency_ms: Some(elapsed),
                                    remediation: Some(vec![
                                        "Check consensus participation",
                                        "Verify validator status",
                                        "Check for chain forks",
                                    ]),
                                }
                            }
                        },
                        None => {
                            // Blocks exist but no parseable timestamp -- assume OK.
                            SubsystemHealth {
                                name: "Block Production",
                                status: HealthStatus::Healthy,
                                score: 100,
                                message: "Blocks present (timestamp unparseable)".to_string(),
                                latency_ms: Some(elapsed),
                                remediation: None,
                            }
                        },
                    }
                },
                _ => {
                    warn!("No blocks returned from chain endpoint");
                    SubsystemHealth {
                        name: "Block Production",
                        status: HealthStatus::Degraded,
                        score: 50,
                        message: "No blocks returned".to_string(),
                        latency_ms: Some(elapsed),
                        remediation: Some(vec![
                            "Check consensus participation",
                            "Verify validator status",
                            "Check for chain forks",
                        ]),
                    }
                },
            }
        },
        Err(e) => {
            let elapsed = start.elapsed().as_millis() as u64;
            warn!("Block production check failed: {}", e);
            SubsystemHealth {
                name: "Block Production",
                status: HealthStatus::Critical,
                score: 0,
                message: "Block production endpoint unreachable".to_string(),
                latency_ms: Some(elapsed),
                remediation: Some(vec![
                    "Check consensus participation",
                    "Verify validator status",
                    "Check for chain forks",
                ]),
            }
        },
    }
}

// ---------------------------------------------------------------------------
// Aggregate runner
// ---------------------------------------------------------------------------

async fn run_health_checks() -> Result<(Vec<SubsystemHealth>, HealthStatus, u8)> {
    let client = ApiClient::new(ApiConfig::default())?;

    let (api, sync, peers, mining, bridges, weather, blocks) = tokio::join!(
        check_api(&client),
        check_node_sync(&client),
        check_peers(&client),
        check_mining(&client),
        check_bridges(&client),
        check_weather(&client),
        check_block_production(&client),
    );

    let subsystems = vec![api, sync, peers, mining, bridges, weather, blocks];
    let (overall, avg_score) = calculate_overall(&subsystems);

    Ok((subsystems, overall, avg_score))
}

fn build_output(subsystems: Vec<SubsystemHealth>, overall: HealthStatus, avg_score: u8) -> Value {
    // Collect all remediation hints from non-healthy subsystems.
    let remediation_hints: Vec<Value> = subsystems
        .iter()
        .filter(|s| s.status != HealthStatus::Healthy)
        .filter_map(|s| {
            s.remediation.as_ref().map(|hints| {
                json!({
                    "subsystem": s.name,
                    "hints": hints,
                })
            })
        })
        .collect();

    let subsystem_json: Vec<Value> = subsystems.iter().map(|s| s.to_json()).collect();

    json!({
        "summary": format!(
            "System health: {} (score: {}/100)",
            capitalize(overall.as_str()),
            avg_score
        ),
        "overall_status": overall.as_str(),
        "severity_score": avg_score,
        "subsystems": subsystem_json,
        "remediation_hints": remediation_hints,
    })
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().to_string() + c.as_str(),
    }
}

// ---------------------------------------------------------------------------
// Public handler
// ---------------------------------------------------------------------------

pub async fn handle_operator_command(
    command: &OperatorCommands,
    ctx: &ExecutionContext,
) -> Result<Value> {
    match command {
        OperatorCommands::Status {
            strict,
            watch,
            interval,
        } => {
            let strict = *strict;

            // Watch mode: delegate to the shared polling loop.
            if *watch {
                let cfg = WatchConfig {
                    interval: *interval,
                    once: false,
                };
                return watch_loop(cfg, ctx, "operator", || async {
                    let (subsystems, overall, avg_score) = run_health_checks().await?;
                    let data = build_output(subsystems, overall, avg_score);
                    Ok(data)
                })
                .await;
            }

            // Single-shot health check.
            let (subsystems, overall, avg_score) = run_health_checks().await?;
            let data = build_output(subsystems, overall, avg_score);

            match overall {
                HealthStatus::Critical => {
                    // "connection" keyword maps to exit code 2 via classify_exit_code.
                    Err(anyhow!(
                        "connection: system health critical (score: {}/100) -- {}",
                        avg_score,
                        data.get("summary")
                            .and_then(Value::as_str)
                            .unwrap_or("critical")
                    ))
                },
                HealthStatus::Degraded if strict => {
                    // Generic message gives exit code 1 via classify_exit_code.
                    Err(anyhow!(
                        "degraded: system health degraded (score: {}/100) -- {}",
                        avg_score,
                        data.get("summary")
                            .and_then(Value::as_str)
                            .unwrap_or("degraded")
                    ))
                },
                _ => Ok(data),
            }
        },
    }
}
