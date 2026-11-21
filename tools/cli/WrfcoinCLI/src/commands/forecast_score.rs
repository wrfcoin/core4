//! `wrfcoin forecast score`: the forecast late score from the oracle and relay
//! side (`shared-protocol/src/consensus/forecast_late_score.rs`).
//!
//! - `attest` (oracle job): station readings + one commit block → the
//!   observation attestation, built with the shared rule's helpers. With
//!   `--key` it also signs it.
//! - `sign-observation`: one oracle signs an attestation file. Offline.
//! - `build`: attestation + quorum signatures (+ optional inline `--key`s) +
//!   forecast commit ids → the `ForecastScore` transaction, the body of
//!   `POST /forecast/late-score/submit`. Offline.
//! - `submit`, `status`, `bucket`, `record`: talk to the node
//!   (`WRFCOIN_API_URL`, token from `WRFCOIN_OPERATOR_TOKEN` / `WRFCOIN_READER_TOKEN`).
//!
//! Keys are read from files (hex, at most 0600) and are never printed. The
//! node refuses a score while `forecast_late_score_v2` is not in force.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, ensure};
use clap::Subcommand;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use wrfcoin_cli::forecast_late_score::{
    CommitBlock, RelayStep, ScoredState, SubmitAnswer, TypedSample, attest_observation, build_score_payload,
    build_score_transaction, classify_submit, default_oracle_identity, relay_step, score_window,
    sign_observation, weather_readings_only,
};
use wrfcoin_cli::forecast_submission::read_signer;
use wrfcoin_cli::{ApiClient, ApiConfig};
use wrfcoin_shared_protocol::blockchain::transaction::Transaction;
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::consensus::forecast_late_score::{
    ForecastScorePayload, LateScoreVariable, ObservationAttestation, ObservationSignature,
};

#[derive(Subcommand, Debug)]
pub enum ForecastScoreCommands {
    /// Oracle job: build the observation attestation for the forecasts of one
    /// station, variable and lead committed in one block.
    Attest {
        /// JSON array of the station's readings of this variable:
        /// `[{id, observed_at, value, height, block_time, has_quorum, provenance, contribution_type}]`,
        /// quorum as of the closing block (target + 35 min). `contribution_type` is the
        /// reveal's type as the chain holds it; forecast-type reveals are left out.
        #[clap(long)]
        samples: PathBuf,
        /// Accept samples without a `contribution_type` (they cannot be told apart
        /// from forecast reveals, so this is off by default).
        #[clap(long)]
        allow_untyped: bool,
        #[clap(long)]
        station_id: String,
        /// temperature_c (0.01 °C) or pressure_hpa (Pa).
        #[clap(long)]
        variable: String,
        /// 6 or 12 (the leads the chain pays).
        #[clap(long)]
        lead_hours: u32,
        #[clap(long)]
        commit_height: u64,
        /// Time (unix seconds) of the commit block.
        #[clap(long)]
        commit_time: i64,
        /// Attestation output file.
        #[clap(long)]
        out: PathBuf,
        /// Also sign it with this oracle key file, for the `--forecast` ids; the
        /// signature goes to `<out>.sig.json`.
        #[clap(long, requires_all = ["chain_id", "network_id", "forecasts"])]
        key: Option<PathBuf>,
        /// Forecast commit ids the signature covers (repeat). The oracle
        /// signature covers the full score payload: the observation and these ids.
        #[clap(long = "forecast")]
        forecasts: Vec<String>,
        /// The oracle identity as the chain's oracle set holds it (default: the key's public key hex).
        #[clap(long, requires = "key")]
        oracle: Option<String>,
        #[clap(long)]
        chain_id: Option<String>,
        #[clap(long)]
        network_id: Option<String>,
    },
    /// Sign a score as one oracle: an observation attestation and the forecast
    /// commit ids it scores (the full score payload).
    SignObservation {
        #[clap(long)]
        attestation: PathBuf,
        /// Forecast commit ids to score (repeat; up to 64). `build` must list the same ids.
        #[clap(long = "forecast", required = true)]
        forecasts: Vec<String>,
        /// The oracle's ed25519 private key file (hex, at most 0600).
        #[clap(long)]
        key: PathBuf,
        /// The oracle identity as the chain's oracle set holds it (default: the key's public key hex).
        #[clap(long)]
        oracle: Option<String>,
        /// Chain id the signature binds (`chain_id` in `forecast score status`).
        #[clap(long)]
        chain_id: String,
        #[clap(long)]
        network_id: String,
        /// Signature output file.
        #[clap(long)]
        out: PathBuf,
    },
    /// Build the ForecastScore transaction from an attestation, a quorum of
    /// signatures and the forecast commit ids it scores.
    Build {
        #[clap(long)]
        attestation: PathBuf,
        /// Signature files written by `sign-observation` (repeat).
        #[clap(long = "signature")]
        signatures: Vec<PathBuf>,
        /// Oracle key files to sign with here, as their public key identity (repeat).
        #[clap(long = "key")]
        keys: Vec<PathBuf>,
        /// Forecast commit ids to score (repeat; up to 64).
        #[clap(long = "forecast", required = true)]
        forecasts: Vec<String>,
        #[clap(long)]
        chain_id: String,
        #[clap(long)]
        network_id: String,
        /// The relaying address: a label only, it is paid nothing and debited nothing.
        #[clap(long)]
        relay: String,
        /// Transaction timestamp (unix seconds); default now. Signatures are
        /// checked against the oracle keys active at this time.
        #[clap(long)]
        submitted_at: Option<i64>,
        /// Output artifact path (the submit body).
        #[clap(long)]
        out: PathBuf,
    },
    /// Submit an artifact written by `build`, once. Refused here before its
    /// window opens (target + 35 min) or after the bucket closes: a score
    /// mined early is dropped from the mempool, not held. Use `relay` to wait
    /// and resubmit.
    Submit {
        #[clap(long)]
        artifact: PathBuf,
    },
    /// Relay an artifact written by `build` until it is scored: hold it until
    /// its window opens, submit it, and submit it again each round while its
    /// forecasts are unscored and it is not pending (a block takes at most 8
    /// scores; the rest are dropped from the mempool). Stops when every listed
    /// forecast is scored, the bucket closes, or the node refuses it.
    Relay {
        #[clap(long)]
        artifact: PathBuf,
        /// Seconds between rounds (default one block, 77 s; at least 10).
        #[clap(long, default_value_t = 77)]
        poll_secs: u64,
        /// Stop after this many seconds even if unscored (default: run to the bucket's close).
        #[clap(long)]
        max_secs: Option<u64>,
    },
    /// Whether the lane is in force, its limits, the chain and network id to sign for, and counts.
    Status,
    /// One bucket by id.
    Bucket {
        #[clap(long)]
        id: String,
    },
    /// One recorded forecast by commit id.
    Record {
        #[clap(long)]
        commit_id: String,
    },
}

fn read_json<T: DeserializeOwned>(path: &Path, what: &str) -> Result<T> {
    let raw = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    serde_json::from_slice(&raw).with_context(|| format!("{} is not {what}", path.display()))
}

fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    let body = serde_json::to_vec_pretty(value)?;
    super::station::atomic_artifact_write(path, |file| {
        use std::io::Write;
        file.write_all(&body)
    })
    .with_context(|| format!("cannot write {}", path.display()))
}

fn parse_variable(text: &str) -> Result<LateScoreVariable> {
    serde_json::from_value(json!(text.trim()))
        .map_err(|_| anyhow!("variable must be temperature_c or pressure_hpa, not {text}"))
}

/// Percent-encode one URL path segment: everything but RFC 3986 unreserved
/// characters, so an id can never add a path segment or a query.
fn path_segment(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn sig_path(out: &Path) -> PathBuf {
    let mut name = out.as_os_str().to_owned();
    name.push(".sig.json");
    PathBuf::from(name)
}

/// Whether each forecast is scored, as the node's records say. A forecast
/// with no record reads as unscored; the submit then says why.
async fn scored_state(api: &ApiClient, forecasts: &[String]) -> Result<ScoredState> {
    let mut scored = Vec::with_capacity(forecasts.len());
    for id in forecasts {
        let path = format!("/forecast/late-score/records/{}", path_segment(id));
        match api.receipt_request("GET", &path, None).await {
            Ok(record) => scored.push(record["scored"].as_bool().unwrap_or(false)),
            Err(e) if e.to_string().starts_with("RECEIPT_HTTP_404") => scored.push(false),
            Err(e) => return Err(anyhow!("cannot read the record of {id} ({e})")),
        }
    }
    Ok(ScoredState::of(&scored))
}

/// `forecast score relay`: see [`ForecastScoreCommands::Relay`].
async fn relay(artifact: &Path, poll_secs: u64, max_secs: Option<u64>) -> Result<Value> {
    let tx: Transaction = read_json(artifact, "a transaction artifact")?;
    let payload = ForecastScorePayload::parse(&tx)
        .ok_or_else(|| anyhow!("{} is not a ForecastScore transaction", artifact.display()))?;
    let window = score_window(&payload.attestation)?;
    let forecasts = payload.forecast_commit_ids.clone();
    let body = serde_json::to_value(&tx)?;
    let api = ApiClient::new(ApiConfig::default())?;
    let started = chrono::Utc::now().timestamp();
    let deadline = max_secs.map(|secs| started.saturating_add(secs.min(i64::MAX as u64) as i64));
    let (mut submits, mut resubmits, mut pending_rounds, mut retries) = (0u32, 0u32, 0u32, 0u32);
    let mut last_retry: Option<String> = None;
    let outcome = loop {
        let now = chrono::Utc::now().timestamp();
        let state = scored_state(&api, &forecasts).await?;
        let step = relay_step(now, window, state);
        let wait = match step {
            RelayStep::Done => break "scored",
            RelayStep::PartlyScored => break "partly_scored",
            RelayStep::Closed => break "bucket_closed",
            RelayStep::WaitUntil(at) => (at - now).max(1) as u64,
            RelayStep::Submit => {
                let result = api.receipt_request("POST", "/forecast/late-score/submit", Some(body.clone())).await;
                match classify_submit(&result) {
                    SubmitAnswer::Admitted => {
                        if submits > 0 {
                            resubmits += 1;
                        }
                        submits += 1;
                        eprintln!("forecast score relay: admitted (send {submits})");
                    },
                    SubmitAnswer::Pending => pending_rounds += 1,
                    SubmitAnswer::Retry(e) => {
                        retries += 1;
                        eprintln!("forecast score relay: will retry ({e})");
                        last_retry = Some(e);
                    },
                    SubmitAnswer::Refused(e) => {
                        // A refusal may race a block that just scored it.
                        if scored_state(&api, &forecasts).await? == ScoredState::All {
                            break "scored";
                        }
                        return Err(anyhow!("node refused the score ({e})"));
                    },
                }
                poll_secs
            },
        };
        let now = chrono::Utc::now().timestamp();
        if deadline.is_some_and(|d| now >= d) {
            break "max_secs";
        }
        let wait = deadline.map_or(wait, |d| wait.min((d - now).max(1) as u64));
        tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
    };
    let done = outcome == "scored";
    let result = json!({
        "tx_id": tx.id.map(|id| id.to_hex()),
        "outcome": outcome,
        "forecasts": forecasts,
        "opens_at": window.opens_at,
        "closes_at": window.closes_at,
        "submits": submits,
        "resubmits": resubmits,
        "pending_rounds": pending_rounds,
        "retries": retries,
        "last_retry_error": last_retry,
        "note": match outcome {
            "partly_scored" => "some listed forecasts are scored; build a new score for the unscored ids",
            "bucket_closed" => "the bucket closed before the score landed",
            "max_secs" => "stopped at --max-secs; run relay again to continue",
            _ => "every listed forecast is scored",
        },
    });
    ensure!(done, "{}", serde_json::to_string(&result)?);
    Ok(result)
}

pub async fn handle_forecast_score_command(command: &ForecastScoreCommands) -> Result<Value> {
    use ForecastScoreCommands as C;
    match command {
        C::Attest {
            samples,
            allow_untyped,
            station_id,
            variable,
            lead_hours,
            commit_height,
            commit_time,
            out,
            key,
            forecasts,
            oracle,
            chain_id,
            network_id,
        } => {
            let samples: Vec<TypedSample> = read_json(samples, "a JSON array of chain samples")?;
            let filtered = weather_readings_only(samples, *allow_untyped)?;
            let samples = filtered.kept;
            let block = CommitBlock {
                station_id: station_id.trim().to_string(),
                variable: parse_variable(variable)?,
                lead_hours: *lead_hours,
                commit_height: *commit_height,
                commit_time: *commit_time,
            };
            let report = attest_observation(&samples, &block)?;
            write_json(out, &report.attestation)?;
            let mut result = json!({
                "attestation_file": out.display().to_string(),
                "target": report.attestation.target,
                "truth_commit_id": report.attestation.truth_commit_id,
                "truth": report.attestation.truth,
                "reference": report.attestation.reference,
                "mean24": report.attestation.mean24,
                "last": report.attestation.last,
                "hourly_samples": report.samples,
                "window_readings": report.attestation.window_commit_ids.len(),
                "dropped_forecast_type": filtered.dropped_forecast_type,
                "untyped_samples": filtered.untyped,
            });
            if let (Some(key), Some(chain_id), Some(network_id)) = (key, chain_id, network_id) {
                let key = read_signer(key)?;
                let oracle = oracle.clone().unwrap_or_else(|| default_oracle_identity(&key));
                let signature =
                    sign_observation(&key, &oracle, chain_id, network_id, &report.attestation, forecasts)?;
                let path = sig_path(out);
                write_json(&path, &signature)?;
                result["signature_file"] = json!(path.display().to_string());
                result["oracle"] = json!(oracle);
            }
            Ok(result)
        },
        C::SignObservation { attestation, forecasts, key, oracle, chain_id, network_id, out } => {
            let attestation: ObservationAttestation = read_json(attestation, "an observation attestation")?;
            let key = read_signer(key)?;
            let oracle = oracle.clone().unwrap_or_else(|| default_oracle_identity(&key));
            let signature = sign_observation(&key, &oracle, chain_id, network_id, &attestation, forecasts)?;
            write_json(out, &signature)?;
            Ok(json!({
                "signature_file": out.display().to_string(),
                "oracle": oracle,
                "station_id": attestation.station_id,
                "commit_height": attestation.commit_height,
                "target": attestation.target,
            }))
        },
        C::Build {
            attestation,
            signatures,
            keys,
            forecasts,
            chain_id,
            network_id,
            relay,
            submitted_at,
            out,
        } => {
            let attestation: ObservationAttestation = read_json(attestation, "an observation attestation")?;
            let mut collected = Vec::new();
            for path in signatures {
                collected.push(read_json::<ObservationSignature>(path, "an observation signature")?);
            }
            for path in keys {
                let key = read_signer(path)?;
                collected.push(sign_observation(
                    &key,
                    &default_oracle_identity(&key),
                    chain_id,
                    network_id,
                    &attestation,
                    forecasts,
                )?);
            }
            ensure!(!collected.is_empty(), "give at least one --signature or --key");
            let payload = build_score_payload(attestation, collected, forecasts.iter().cloned())?;
            let relay: Address = relay
                .trim()
                .parse()
                .map_err(|e| anyhow!("--relay {relay} is not a 40-hex-character address: {e:?}"))?;
            let tx = build_score_transaction(
                &payload,
                chain_id,
                network_id,
                relay,
                submitted_at.unwrap_or_else(|| chrono::Utc::now().timestamp()),
            )?;
            write_json(out, &tx)?;
            Ok(json!({
                "artifact": out.display().to_string(),
                "tx_id": tx.id.map(|id| id.to_hex()),
                "signers": payload.signatures.iter().map(|s| s.oracle.clone()).collect::<Vec<_>>(),
                "forecasts": payload.forecast_commit_ids,
                "next": "wrfcoin forecast score submit --artifact <artifact>",
            }))
        },
        C::Submit { artifact } => {
            let tx: Transaction = read_json(artifact, "a transaction artifact")?;
            let payload = ForecastScorePayload::parse(&tx)
                .ok_or_else(|| anyhow!("{} is not a ForecastScore transaction", artifact.display()))?;
            let window = score_window(&payload.attestation)?;
            let now = chrono::Utc::now().timestamp();
            ensure!(
                now >= window.opens_at,
                "too early: this score can land from {} ({} s from now); a score sent before then is \
                 dropped from the mempool. Use `forecast score relay` to wait and resubmit",
                window.opens_at,
                window.opens_at - now
            );
            ensure!(now < window.closes_at, "the bucket closed at {}; this score can no longer land", window.closes_at);
            let api = ApiClient::new(ApiConfig::default())?;
            let ack = api
                .receipt_request("POST", "/forecast/late-score/submit", Some(serde_json::to_value(&tx)?))
                .await
                .map_err(|e| anyhow!("node refused the score ({e})"))?;
            Ok(json!({
                "tx_id": tx.id.map(|id| id.to_hex()),
                "node": ack,
                "note": "Submitted is not scored: a block takes at most 8 scores and drops the rest. \
                         Check `forecast score record` after a few blocks, or use `forecast score relay`.",
            }))
        },
        C::Relay { artifact, poll_secs, max_secs } => relay(artifact, (*poll_secs).max(10), *max_secs).await,
        C::Status => {
            ApiClient::new(ApiConfig::default())?.receipt_request("GET", "/forecast/late-score", None).await
        },
        C::Bucket { id } => {
            ApiClient::new(ApiConfig::default())?
                .receipt_request("GET", &format!("/forecast/late-score/buckets/{}", path_segment(id.trim())), None)
                .await
        },
        C::Record { commit_id } => {
            ApiClient::new(ApiConfig::default())?
                .receipt_request("GET", &format!("/forecast/late-score/records/{}", path_segment(commit_id.trim())), None)
                .await
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variables_parse_by_their_wire_names() {
        assert_eq!(parse_variable("temperature_c").unwrap(), LateScoreVariable::TemperatureC);
        assert_eq!(parse_variable(" pressure_hpa ").unwrap(), LateScoreVariable::PressureHpa);
        assert!(parse_variable("temp").is_err());
    }

    #[test]
    fn ids_are_percent_encoded_as_one_path_segment() {
        assert_eq!(path_segment("0000000069f1-ab12"), "0000000069f1-ab12");
        assert_eq!(path_segment("../x?y=1 z"), "..%2Fx%3Fy%3D1%20z");
    }

    #[test]
    fn the_signature_file_sits_next_to_the_attestation() {
        assert_eq!(sig_path(Path::new("/x/a.json")), PathBuf::from("/x/a.json.sig.json"));
    }
}
