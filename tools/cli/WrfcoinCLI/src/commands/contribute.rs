use anyhow::{Result, anyhow};
use chrono::Utc;
use clap::Subcommand;
use serde::Serialize;
use serde_json::{Value, json};
use std::time::Duration;
use wrfcoin_cli::output::OutputFormat;
use wrfcoin_cli::{ApiClient, ApiConfig};

use super::emit::emit_success;
use super::weather::{resolve_wallet_station_id, sign_with_wallet_key};
use crate::cli::commands::ExecutionContext;

/// Contributor workflow commands (#286).
#[derive(Subcommand)]
pub enum ContributeCommands {
    /// Start submitting weather data in a loop
    Start {
        /// Weather station ID (defaults to wallet primary address)
        #[clap(long)]
        station_id: Option<String>,

        /// Seconds between submissions
        #[clap(long, default_value_t = 60)]
        interval: u64,

        /// Submit once and exit
        #[clap(long)]
        once: bool,

        /// Skip preflight checks
        #[clap(long)]
        skip_preflight: bool,

        /// Maximum number of submissions (0 = unlimited)
        #[clap(long, default_value_t = 0)]
        max_submissions: u64,
    },

    /// Run preflight checks without submitting data
    Preflight {
        /// Weather station ID (defaults to wallet primary address)
        #[clap(long)]
        station_id: Option<String>,
    },

    /// Show contributor session status
    Status,

    /// Show a summary of past contribution periods
    Summary {
        /// Time period to summarise (today, week, month, all)
        #[clap(long, default_value = "today")]
        period: String,
    },
}

#[derive(Debug, Serialize)]
struct PreflightCheck {
    name: &'static str,
    passed: bool,
    detail: String,
}

#[derive(Debug, Default)]
struct SessionStats {
    started_at: String,
    attempted: u64,
    succeeded: u64,
    failed: u64,
    quality_scores: Vec<f64>,
    errors: Vec<String>,
}

impl SessionStats {
    fn avg_quality(&self) -> f64 {
        if self.quality_scores.is_empty() {
            0.0
        } else {
            self.quality_scores.iter().sum::<f64>() / self.quality_scores.len() as f64
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "started_at": self.started_at,
            "attempted": self.attempted,
            "succeeded": self.succeeded,
            "failed": self.failed,
            "avg_quality": self.avg_quality(),
            "error_count": self.errors.len(),
        })
    }
}

pub async fn handle_contribute_command(
    action: &ContributeCommands,
    ctx: &ExecutionContext,
) -> Result<Value> {
    match action {
        ContributeCommands::Preflight { station_id } => {
            let sid = resolve_station(station_id)?;
            let checks = run_preflight(&sid).await;
            let all_passed = checks.iter().all(|c| c.passed);
            render_preflight(ctx, &checks);
            Ok(json!({
                "summary": if all_passed { "All preflight checks passed" } else { "Some preflight checks failed" },
                "all_passed": all_passed,
                "checks": checks,
            }))
        },

        ContributeCommands::Start {
            station_id,
            interval,
            once,
            skip_preflight,
            max_submissions,
        } => {
            let sid = resolve_station(station_id)?;

            // Preflight
            if !skip_preflight {
                let checks = run_preflight(&sid).await;
                let all_passed = checks.iter().all(|c| c.passed);
                render_preflight(ctx, &checks);
                if !all_passed {
                    return Err(anyhow!(
                        "Preflight checks failed — use --skip-preflight to override"
                    ));
                }
            }

            // Submission loop
            let mut stats = SessionStats {
                started_at: Utc::now().to_rfc3339(),
                ..Default::default()
            };

            let client = ApiClient::new(ApiConfig::default())?;
            let max = if *max_submissions == 0 {
                u64::MAX
            } else {
                *max_submissions
            };

            loop {
                if stats.attempted >= max {
                    break;
                }

                stats.attempted += 1;
                match submit_tick(&client, &sid).await {
                    Ok(result) => {
                        stats.succeeded += 1;
                        if let Some(q) = result.get("quality_score").and_then(Value::as_f64) {
                            stats.quality_scores.push(q);
                        }
                        let tick_data = json!({
                            "summary": format!("Submission #{} succeeded", stats.attempted),
                            "tick": stats.attempted,
                            "station_id": sid,
                            "result": result,
                        });
                        let _ = emit_success(ctx, "contribute", &tick_data);
                    },
                    Err(err) => {
                        stats.failed += 1;
                        stats.errors.push(err.to_string());
                        if matches!(ctx.output_format, OutputFormat::Human) {
                            eprintln!("Submission #{} failed: {}", stats.attempted, err);
                        }
                    },
                }

                if *once {
                    break;
                }

                // Sleep with Ctrl-C race
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(*interval)) => {}
                    _ = tokio::signal::ctrl_c() => {
                        if matches!(ctx.output_format, OutputFormat::Human) {
                            eprintln!("\nContribution stopped by user");
                        }
                        break;
                    }
                }
            }

            let summary = json!({
                "summary": format!(
                    "Session complete: {}/{} succeeded (avg quality: {:.1})",
                    stats.succeeded, stats.attempted, stats.avg_quality()
                ),
                "session": stats.to_json(),
            });
            Ok(summary)
        },

        ContributeCommands::Status => {
            // Status is informational — show what's configured
            let station = resolve_wallet_station_id();
            Ok(json!({
                "summary": "Contributor status",
                "station_id": station,
                "wallet_configured": station.is_some(),
            }))
        },

        ContributeCommands::Summary { period } => {
            let client = ApiClient::new(ApiConfig::default())?;
            let station = resolve_wallet_station_id()
                .ok_or_else(|| anyhow!("No wallet configured — run: wrfcoin wallet new-address"))?;
            let path = format!("/weather/stations/{}/summary?period={}", station, period);
            let data: Value = client.get(&path, None).await?;
            Ok(json!({
                "summary": format!("Contribution summary ({})", period),
                "station_id": station,
                "period": period,
                "data": data,
            }))
        },
    }
}

fn resolve_station(explicit: &Option<String>) -> Result<String> {
    match explicit {
        Some(id) => Ok(id.clone()),
        None => resolve_wallet_station_id()
            .ok_or_else(|| anyhow!("No station ID — provide --station-id or configure a wallet")),
    }
}

async fn run_preflight(station_id: &str) -> Vec<PreflightCheck> {
    let mut checks = Vec::with_capacity(7);
    let client_result = ApiClient::new(ApiConfig::default());

    // 1. Config
    checks.push(PreflightCheck {
        name: "config",
        passed: client_result.is_ok(),
        detail: if client_result.is_ok() {
            "Profile loaded, API URL resolved".into()
        } else {
            "Failed to load configuration".into()
        },
    });

    let client = match client_result {
        Ok(c) => c,
        Err(e) => {
            // All remaining checks fail if we can't build a client
            for name in &[
                "api_connectivity",
                "station_registered",
                "station_health",
                "wallet_configured",
                "clock_sync",
                "network",
            ] {
                checks.push(PreflightCheck {
                    name,
                    passed: false,
                    detail: format!("Skipped — config error: {}", e),
                });
            }
            return checks;
        },
    };

    // 2. API connectivity
    let health_result = client.get::<Value>("/health", None).await;
    let (api_ok, server_timestamp) = match &health_result {
        Ok(data) => {
            let ts = data
                .get("timestamp")
                .and_then(Value::as_str)
                .map(String::from);
            (true, ts)
        },
        Err(_) => (false, None),
    };
    checks.push(PreflightCheck {
        name: "api_connectivity",
        passed: api_ok,
        detail: if api_ok {
            "API server reachable".into()
        } else {
            format!("Cannot reach API: {}", health_result.unwrap_err())
        },
    });

    // 3. Station registered
    let station_result = if api_ok {
        client
            .get::<Value>(&format!("/weather/stations/{}", station_id), None)
            .await
    } else {
        Err(anyhow!("API unreachable"))
    };
    let station_ok = station_result.is_ok();
    checks.push(PreflightCheck {
        name: "station_registered",
        passed: station_ok,
        detail: if station_ok {
            format!("Station '{}' found", station_id)
        } else {
            format!("Station '{}' not found or API error", station_id)
        },
    });

    // 4. Station health / quality
    let quality_ok = if let Ok(ref data) = station_result {
        data.get("quality_score")
            .and_then(Value::as_f64)
            .map(|q| q >= 50.0)
            .unwrap_or(true) // If no score yet, pass
    } else {
        false
    };
    checks.push(PreflightCheck {
        name: "station_health",
        passed: quality_ok,
        detail: if let Ok(ref data) = station_result {
            let score = data
                .get("quality_score")
                .and_then(Value::as_f64)
                .map(|q| format!("{:.1}", q))
                .unwrap_or_else(|| "N/A".into());
            format!("Quality score: {}", score)
        } else {
            "Cannot check — station not found".into()
        },
    });

    // 5. Wallet configured
    let wallet = resolve_wallet_station_id();
    checks.push(PreflightCheck {
        name: "wallet_configured",
        passed: wallet.is_some(),
        detail: if let Some(ref addr) = wallet {
            format!("Wallet primary: {}", addr)
        } else {
            "No wallet found — run: wrfcoin wallet new-address".into()
        },
    });

    // 6. Clock sync
    let clock_ok = if let Some(ref ts) = server_timestamp {
        if let Ok(server_time) = chrono::DateTime::parse_from_rfc3339(ts) {
            let drift = (Utc::now().timestamp() - server_time.timestamp()).unsigned_abs();
            drift < 30
        } else {
            true // Can't parse, assume ok
        }
    } else {
        !api_ok // If API is down, clock check is N/A, pass trivially if api down
    };
    checks.push(PreflightCheck {
        name: "clock_sync",
        passed: clock_ok || !api_ok,
        detail: if !api_ok {
            "Skipped — API unreachable".into()
        } else if clock_ok {
            "Clock drift within tolerance".into()
        } else {
            "Clock drift exceeds 30s — check system time".into()
        },
    });

    // 7. Network / chain sync
    let network_ok = if api_ok {
        client.get::<Value>("/network/info", None).await.is_ok()
    } else {
        false
    };
    checks.push(PreflightCheck {
        name: "network",
        passed: network_ok || !api_ok,
        detail: if !api_ok {
            "Skipped — API unreachable".into()
        } else if network_ok {
            "Chain synced".into()
        } else {
            "Cannot reach network info endpoint".into()
        },
    });

    checks
}

fn render_preflight(ctx: &ExecutionContext, checks: &[PreflightCheck]) {
    if !matches!(ctx.output_format, OutputFormat::Human) {
        return; // JSON/CSV/etc. handled by the caller via emit_success
    }
    println!("Preflight checks:");
    for check in checks {
        let icon = if check.passed { "\u{2714}" } else { "\u{2718}" };
        println!("  {} {} — {}", icon, check.name, check.detail);
    }
}

async fn submit_tick(client: &ApiClient, station_id: &str) -> Result<Value> {
    // Fetch latest sensor data
    let sensor_data = client.get_iot_sensor_data(station_id).await?;

    // Build observation from sensor data
    let observation = json!({
        "type": sensor_data.get("type").and_then(Value::as_str).unwrap_or("generic"),
        "value": sensor_data.get("value").unwrap_or(&json!(0.0)),
        "unit": sensor_data.get("unit").and_then(Value::as_str).unwrap_or("raw"),
        "latitude": sensor_data.get("latitude").unwrap_or(&json!(0.0)),
        "longitude": sensor_data.get("longitude").unwrap_or(&json!(0.0)),
        "timestamp": Utc::now().to_rfc3339(),
        "source": "contribute-cli",
    });

    // Sign
    let obs_bytes = serde_json::to_string(&observation)?;
    let signature =
        sign_with_wallet_key(obs_bytes.as_bytes()).unwrap_or_else(|| "cli-contribute".to_string());

    // Submit
    let result = client
        .submit_weather_observation(station_id, observation, &signature)
        .await?;

    Ok(result)
}
