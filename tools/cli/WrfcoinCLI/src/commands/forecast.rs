use anyhow::Result;
use clap::Subcommand;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use wrfcoin_cli::{ApiClient, ApiConfig};

use super::watch::{WatchConfig, watch_loop};
use crate::cli::commands::ExecutionContext;

#[derive(Subcommand)]
pub enum ForecastCommands {
    /// Submit a signed on-chain forecast commitment.
    Commit {
        #[clap(flatten)]
        input: wrfcoin_cli::forecast_submission::ForecastInput,
        #[clap(flatten)]
        connection: wrfcoin_cli::forecast_submission::ForecastConnection,
        /// Hex private key file, permissions at most 0600.
        #[clap(long)]
        signer_key: PathBuf,
        #[clap(long)]
        chain_id: String,
        #[clap(long, default_value = "testnet")]
        network_id: String,
    },
    /// Reveal a forecast with its manifest and optional oracle receipt.
    Reveal {
        #[clap(flatten)]
        input: wrfcoin_cli::forecast_submission::ForecastInput,
        #[clap(flatten)]
        connection: wrfcoin_cli::forecast_submission::ForecastConnection,
        #[clap(long)]
        manifest_file: PathBuf,
        /// Signed oracle receipt JSON; omit for baseline scoring.
        #[clap(long)]
        receipt_file: Option<PathBuf>,
    },
    /// Sign a forecast verification receipt with an oracle-set key (#3149).
    ///
    /// Emits the `--receipt-file` that `forecast reveal` consumes. Without one a
    /// reveal scores at `BASELINE_FORECAST_SKILL` however good the forecast was.
    SignReceipt {
        /// Revealed forecast manifest JSON; its digest is derived, not supplied.
        #[clap(long)]
        manifest_file: PathBuf,
        /// Scorer output: `{"mae":..,"rmse":..,"bias":..,"skill_score":..}`.
        #[clap(long)]
        metrics_file: PathBuf,
        /// Exact canonical inputs used by commit/reveal; commit id is derived.
        #[clap(flatten)]
        input: wrfcoin_cli::forecast_submission::ForecastInput,
        /// Debug rehearsal only; production uses the oracle PKCS#11 role.
        #[clap(long, requires = "rehearsal")]
        oracle_key: Option<PathBuf>,
        /// Explicitly allow a file key in a debug rehearsal build.
        #[clap(long, requires = "oracle_key")]
        rehearsal: bool,
        /// Must equal WRFCOIN_CHAIN_ID; signing is refused otherwise.
        #[clap(long)]
        chain_id: String,
        /// Must equal WRFCOIN_NETWORK_ID; signing is refused otherwise.
        #[clap(long)]
        network_id: String,
        /// Verification time as a Unix epoch; defaults to now.
        #[clap(long)]
        verified_at: Option<i64>,
        /// Anti-replay nonce; defaults to one derived from the commit id.
        #[clap(long)]
        receipt_nonce: Option<String>,
        /// Destination file; an existing file is never overwritten.
        #[clap(long)]
        out: PathBuf,
    },

    /// Publish a new forecast to the blockchain
    Publish {
        /// Title of the forecast
        #[clap(long, required = true)]
        title: String,

        /// Description of the forecast
        #[clap(long)]
        description: Option<String>,

        /// Type of forecast (weather, climate, wildfire, etc.)
        #[clap(long, required = true)]
        forecast_type: String,

        /// Geographic region the forecast covers
        #[clap(long, required = true)]
        region: String,

        /// Path to JSON file containing forecast data
        #[clap(long, required = true)]
        data_file: PathBuf,

        /// Forecast validity period in hours (default: 24)
        #[clap(long, default_value = "24")]
        validity: u32,

        /// Confidence level (0-100%)
        #[clap(long)]
        confidence: Option<u8>,

        /// Tags for the forecast (comma-separated)
        #[clap(long)]
        tags: Option<String>,
    },

    /// Publish an ensemble forecast with multiple members/scenarios
    PublishEnsemble {
        /// Title of the ensemble forecast
        #[clap(long, required = true)]
        title: String,

        /// Description of the ensemble forecast
        #[clap(long)]
        description: Option<String>,

        /// Type of forecast (weather, climate, wildfire, etc.)
        #[clap(long, required = true)]
        forecast_type: String,

        /// Geographic region the forecast covers
        #[clap(long, required = true)]
        region: String,

        /// Path to directory containing JSON files for each ensemble member
        #[clap(long, required = true)]
        ensemble_dir: PathBuf,

        /// Pattern to match ensemble member files (default: "member_*.json")
        #[clap(long, default_value = "member_*.json")]
        file_pattern: String,

        /// Forecast validity period in hours (default: 24)
        #[clap(long, default_value = "24")]
        validity: u32,

        /// Tags for the forecast (comma-separated)
        #[clap(long)]
        tags: Option<String>,
    },

    /// List published forecasts
    List {
        /// Filter by forecast type
        #[clap(long)]
        forecast_type: Option<String>,

        /// Filter by region
        #[clap(long)]
        region: Option<String>,

        /// Show only forecasts published after this date (YYYY-MM-DD)
        #[clap(long)]
        after: Option<String>,

        /// Show only forecasts published before this date (YYYY-MM-DD)
        #[clap(long)]
        before: Option<String>,

        /// Maximum number of forecasts to list
        #[clap(long, default_value = "10")]
        limit: u32,

        /// Filter by validity (still valid or expired)
        #[clap(long)]
        valid: Option<bool>,

        /// Output format (json, table)
        #[clap(long, default_value = "table")]
        format: String,
    },

    /// Show details of a specific forecast
    Show {
        /// ID of the forecast to show
        #[clap(required = true)]
        id: String,

        /// Show member details if this is an ensemble forecast
        #[clap(long)]
        show_members: bool,

        /// Output format (json, table)
        #[clap(long, default_value = "table")]
        format: String,
    },

    /// Validate a forecast against observed data
    Validate {
        /// ID of the forecast to validate
        #[clap(required = true)]
        id: String,

        /// Path to JSON file containing observed data for validation
        #[clap(long, required = true)]
        observed_data: PathBuf,

        /// Validation method (rmse, mae, correlation, categorical)
        #[clap(long, default_value = "rmse")]
        method: String,

        /// Save the validation results to the blockchain
        #[clap(long)]
        save_results: bool,
    },

    /// Compare multiple forecasts for the same event/region
    Compare {
        /// Comma-separated list of forecast IDs to compare
        #[clap(required = true)]
        ids: String,

        /// Path to JSON file containing observed data (if available)
        #[clap(long)]
        observed_data: Option<PathBuf>,

        /// Comparison metrics (comma-separated: rmse, bias, spread, etc.)
        #[clap(long, default_value = "rmse,bias,spread")]
        metrics: String,

        /// Generate visualization of the comparison
        #[clap(long)]
        visualize: bool,

        /// Output format (json, table, html)
        #[clap(long, default_value = "table")]
        format: String,
    },

    /// Stream latest forecasts continuously
    Watch {
        /// Refresh interval in seconds
        #[clap(long, default_value = "10")]
        interval: u64,

        /// Fetch once and exit (for automation)
        #[clap(long)]
        once: bool,

        /// Filter by geographic region
        #[clap(long)]
        region: Option<String>,

        /// Filter by forecast type
        #[clap(long)]
        forecast_type: Option<String>,
    },
}

fn parse_pattern(pattern: &str) -> (&str, &str) {
    if let Some((prefix, suffix)) = pattern.split_once('*') {
        (prefix, suffix)
    } else {
        (pattern, "")
    }
}

pub async fn handle_forecast_command(
    command: &ForecastCommands,
    ctx: &ExecutionContext,
) -> Result<Value> {
    // Submission flags select their client before the legacy API default is read.
    if let ForecastCommands::Commit {
        input,
        connection,
        signer_key,
        chain_id,
        network_id,
    } = command
    {
        use wrfcoin_cli::forecast_submission::{commit_body, read_signer, submit};
        let body = commit_body(input, &read_signer(signer_key)?, chain_id, network_id)?;
        return submit(connection, "commit", body).await;
    }
    if let ForecastCommands::Reveal {
        input,
        connection,
        manifest_file,
        receipt_file,
    } = command
    {
        use wrfcoin_cli::forecast_submission::{reveal_body, submit};
        let body = reveal_body(input, manifest_file, receipt_file.as_deref())?;
        return submit(connection, "reveal", body).await;
    }
    if let ForecastCommands::SignReceipt {
        manifest_file,
        metrics_file,
        input,
        oracle_key,
        rehearsal,
        chain_id,
        network_id,
        verified_at,
        receipt_nonce,
        out,
    } = command
    {
        use wrfcoin_cli::forecast_receipt::{
            binding_from_env, read_inputs, sign_receipt_for_input, write_receipt,
        };
        let binding = binding_from_env(chain_id, network_id)?;
        let (manifest, metrics) = read_inputs(manifest_file, metrics_file)?;
        let signer = receipt_signer(*rehearsal, oracle_key.as_deref())?;
        let receipt = sign_receipt_for_input(
            input,
            &manifest,
            metrics,
            verified_at.unwrap_or_else(|| chrono::Utc::now().timestamp()),
            receipt_nonce.as_deref(),
            signer.as_ref(),
            &binding,
        )?;
        write_receipt(&receipt, out)?;
        return Ok(json!({
            "receipt_file": out.display().to_string(),
            "commit_id": receipt.commit_id,
            "oracle_id": receipt.oracle_id,
            "manifest_digest": receipt.manifest_digest,
            "skill_score": receipt.metrics.skill_score,
        }));
    }
    let client = ApiClient::new(ApiConfig::default())?;

    match command {
        ForecastCommands::Commit { .. }
        | ForecastCommands::Reveal { .. }
        | ForecastCommands::SignReceipt { .. } => unreachable!(),
        ForecastCommands::Publish {
            title,
            description: _,
            forecast_type,
            region,
            data_file,
            validity,
            confidence,
            tags: _,
        } => {
            let raw = fs::read_to_string(data_file)?;
            let data = serde_json::from_str::<Value>(&raw)?;
            let result: Value = client
                .post(
                    "/forecasts",
                    Some(json!({
                        "title": title,
                        "forecastType": forecast_type,
                        "region": region,
                        "data": data,
                        "validity": validity,
                        "confidence": confidence
                    })),
                )
                .await?;
            Ok(json!({
                "summary": "Forecast published",
                "result": result
            }))
        },
        ForecastCommands::PublishEnsemble {
            title,
            description: _,
            forecast_type,
            region,
            ensemble_dir,
            file_pattern,
            validity,
            tags: _,
        } => {
            let (prefix, suffix) = parse_pattern(file_pattern);
            let mut members = Vec::new();
            for entry in fs::read_dir(ensemble_dir)? {
                let entry = entry?;
                if !entry.file_type()?.is_file() {
                    continue;
                }
                let file_name = entry.file_name().to_string_lossy().to_string();
                if !file_name.starts_with(prefix) || !file_name.ends_with(suffix) {
                    continue;
                }
                let raw = fs::read_to_string(entry.path())?;
                let member = serde_json::from_str::<Value>(&raw)?;
                members.push(member);
            }
            let result: Value = client
                .post(
                    "/forecasts/ensemble",
                    Some(json!({
                        "title": title,
                        "forecastType": forecast_type,
                        "region": region,
                        "members": members,
                        "validity": validity
                    })),
                )
                .await?;
            Ok(json!({
                "summary": "Ensemble forecast published",
                "result": result
            }))
        },
        ForecastCommands::List {
            forecast_type,
            region,
            after: _,
            before: _,
            limit,
            valid: _,
            format: _,
        } => {
            let mut query = vec![format!("limit={}", limit)];
            if let Some(t) = forecast_type {
                query.push(format!("forecastType={}", t));
            }
            if let Some(r) = region {
                query.push(format!("region={}", r));
            }
            let path = format!("/forecasts?{}", query.join("&"));
            let forecasts: Value = client.get(&path, None).await?;
            Ok(json!({
                "summary": "Forecasts listed",
                "forecasts": forecasts
            }))
        },
        ForecastCommands::Show {
            id,
            show_members: _,
            format: _,
        } => {
            let forecast: Value = client.get(&format!("/forecasts/{}", id), None).await?;
            Ok(json!({
                "summary": "Forecast details retrieved",
                "forecast": forecast
            }))
        },
        ForecastCommands::Validate {
            id,
            observed_data,
            method,
            save_results: _,
        } => {
            let raw = fs::read_to_string(observed_data)?;
            let observed = serde_json::from_str::<Value>(&raw)?;
            let result: Value = client
                .post(
                    &format!("/forecasts/{}/validate", id),
                    Some(json!({
                        "observedData": observed,
                        "method": method
                    })),
                )
                .await?;
            Ok(json!({
                "summary": "Forecast validation completed",
                "result": result
            }))
        },
        ForecastCommands::Compare {
            ids,
            observed_data: _,
            metrics,
            visualize: _,
            format: _,
        } => {
            let id_list: Vec<String> = ids
                .split(',')
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(ToString::to_string)
                .collect();
            let metric_list: Vec<String> = metrics
                .split(',')
                .map(str::trim)
                .filter(|metric| !metric.is_empty())
                .map(ToString::to_string)
                .collect();
            let result: Value = client
                .post(
                    "/forecasts/compare",
                    Some(json!({
                        "ids": id_list,
                        "metrics": metric_list
                    })),
                )
                .await?;
            Ok(json!({
                "summary": "Forecast comparison completed",
                "result": result
            }))
        },
        ForecastCommands::Watch {
            interval,
            once,
            region,
            forecast_type,
        } => {
            let region = region.clone();
            let forecast_type = forecast_type.clone();
            let cfg = WatchConfig {
                interval: *interval,
                once: *once,
            };
            watch_loop(cfg, ctx, "forecast", || {
                let r = region.clone();
                let ft = forecast_type.clone();
                async move {
                    let c = ApiClient::new(ApiConfig::default())?;
                    let mut query = vec!["limit=5".to_string()];
                    if let Some(ref region) = r {
                        query.push(format!("region={}", region));
                    }
                    if let Some(ref ft) = ft {
                        query.push(format!("forecastType={}", ft));
                    }
                    let path = format!("/forecasts?{}", query.join("&"));
                    let forecasts: Value = c.get(&path, None).await?;
                    Ok(json!({
                        "summary": "Latest forecasts",
                        "forecasts": forecasts
                    }))
                }
            })
            .await
        },
    }
}

/// No production fallback to an exported oracle key, even in a debug build.
fn receipt_signer(
    rehearsal: bool,
    key: Option<&std::path::Path>,
) -> Result<std::sync::Arc<dyn wrfcoin_shared_protocol::crypto::signer::BlockSigner>> {
    use wrfcoin_shared_protocol::crypto::signer::env_vars;
    if rehearsal {
        let key = key.ok_or_else(|| anyhow::anyhow!("--rehearsal requires --oracle-key"))?;
        #[cfg(debug_assertions)]
        {
            return Ok(std::sync::Arc::new(
                wrfcoin_shared_protocol::crypto::signer::EnvKeySigner::from_private_key(
                    wrfcoin_cli::forecast_submission::read_signer(key)?,
                    "explicit-rehearsal-file",
                ),
            ));
        }
        #[cfg(not(debug_assertions))]
        {
            let _ = key;
            anyhow::bail!("file-key rehearsal requires a debug build");
        }
    }
    if key.is_some() {
        anyhow::bail!("--oracle-key requires --rehearsal");
    }
    if !cfg!(feature = "hsm") {
        anyhow::bail!("production receipts require a build with --features hsm");
    }
    if std::env::var_os(env_vars::HSM_MODULE_PATH).is_none() {
        anyhow::bail!(
            "production receipts require WRFCOIN_HSM_MODULE_PATH and the oracle PKCS#11 role"
        );
    }
    // Ceremony resolver rejects PIN-bearing environment variables before its
    // hidden tty prompt, and uses the oracle-specific labels with no file fallback.
    Ok(super::ceremony::resolve_signer(super::ceremony::CeremonyRole::Oracle)?.signer)
}
