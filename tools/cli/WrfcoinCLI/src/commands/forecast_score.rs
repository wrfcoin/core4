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
//! node refuses a score while `forecast_late_score` is not in force.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, ensure};
use clap::Subcommand;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use wrfcoin_cli::forecast_late_score::{
    CommitBlock, attest_observation, build_score_payload, build_score_transaction, default_oracle_identity,
    sign_observation,
};
use wrfcoin_cli::forecast_submission::read_signer;
use wrfcoin_cli::{ApiClient, ApiConfig};
use wrfcoin_forecast_score::late_score::ChainSample;
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
        /// `[{id, observed_at, value, height, block_time, has_quorum, provenance}]`,
        /// quorum as of the closing block (target + 35 min).
        #[clap(long)]
        samples: PathBuf,
        #[clap(long)]
        station_id: String,
        /// temperature_c (0.01 °C) or pressure_hpa (Pa).
        #[clap(long)]
        variable: String,
        /// 1, 6, 12 or 24.
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
        /// Also sign it with this oracle key file; the signature goes to `<out>.sig.json`.
        #[clap(long, requires_all = ["chain_id", "network_id"])]
        key: Option<PathBuf>,
        /// The oracle identity as the chain's oracle set holds it (default: the key's public key hex).
        #[clap(long, requires = "key")]
        oracle: Option<String>,
        #[clap(long)]
        chain_id: Option<String>,
        #[clap(long)]
        network_id: Option<String>,
    },
    /// Sign an observation attestation as one oracle.
    SignObservation {
        #[clap(long)]
        attestation: PathBuf,
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
    /// Submit an artifact written by `build`.
    Submit {
        #[clap(long)]
        artifact: PathBuf,
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

fn sig_path(out: &Path) -> PathBuf {
    let mut name = out.as_os_str().to_owned();
    name.push(".sig.json");
    PathBuf::from(name)
}

pub async fn handle_forecast_score_command(command: &ForecastScoreCommands) -> Result<Value> {
    use ForecastScoreCommands as C;
    match command {
        C::Attest {
            samples,
            station_id,
            variable,
            lead_hours,
            commit_height,
            commit_time,
            out,
            key,
            oracle,
            chain_id,
            network_id,
        } => {
            let samples: Vec<ChainSample> = read_json(samples, "a JSON array of chain samples")?;
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
            });
            if let (Some(key), Some(chain_id), Some(network_id)) = (key, chain_id, network_id) {
                let key = read_signer(key)?;
                let oracle = oracle.clone().unwrap_or_else(|| default_oracle_identity(&key));
                let signature = sign_observation(&key, &oracle, chain_id, network_id, &report.attestation)?;
                let path = sig_path(out);
                write_json(&path, &signature)?;
                result["signature_file"] = json!(path.display().to_string());
                result["oracle"] = json!(oracle);
            }
            Ok(result)
        },
        C::SignObservation { attestation, key, oracle, chain_id, network_id, out } => {
            let attestation: ObservationAttestation = read_json(attestation, "an observation attestation")?;
            let key = read_signer(key)?;
            let oracle = oracle.clone().unwrap_or_else(|| default_oracle_identity(&key));
            let signature = sign_observation(&key, &oracle, chain_id, network_id, &attestation)?;
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
            ensure!(
                ForecastScorePayload::parse(&tx).is_some(),
                "{} is not a ForecastScore transaction",
                artifact.display()
            );
            let api = ApiClient::new(ApiConfig::default())?;
            let ack = api
                .receipt_request("POST", "/forecast/late-score/submit", Some(serde_json::to_value(&tx)?))
                .await
                .map_err(|e| anyhow!("node refused the score ({e})"))?;
            Ok(json!({
                "tx_id": tx.id.map(|id| id.to_hex()),
                "node": ack,
                "note": "Submitted is not scored: check `forecast score record` after a few blocks.",
            }))
        },
        C::Status => {
            ApiClient::new(ApiConfig::default())?.receipt_request("GET", "/forecast/late-score", None).await
        },
        C::Bucket { id } => {
            ApiClient::new(ApiConfig::default())?
                .receipt_request("GET", &format!("/forecast/late-score/buckets/{}", id.trim()), None)
                .await
        },
        C::Record { commit_id } => {
            ApiClient::new(ApiConfig::default())?
                .receipt_request("GET", &format!("/forecast/late-score/records/{}", commit_id.trim()), None)
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
    fn the_signature_file_sits_next_to_the_attestation() {
        assert_eq!(sig_path(Path::new("/x/a.json")), PathBuf::from("/x/a.json.sig.json"));
    }
}
