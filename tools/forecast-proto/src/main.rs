//! Prototype forecast producer + scorer for the WRFCoin forecast earning loop.
//!
//! Written for the 2026-09-09 prototype (see
//! `docs/plans/2026-09-09-forecast-prototype-design.md`). It closes the loop:
//! read attested observations off the chain, run a documented analysis model,
//! publish a `ForecastCommit`, score the forecast against a truth observation,
//! sign a `ForecastVerificationReceipt` with a genesis-oracle-set key, publish
//! the `ForecastReveal` carrying manifest + receipt, attest the commit, and
//! leave the block producer to mint the skill-scaled reward.
//!
//! Every signature this tool produces folds in the chain id and network id
//! resolved from `WRFCOIN_CHAIN_ID` / `WRFCOIN_NETWORK_ID`. Those MUST match
//! the node's, or the receipt is rejected — see the design note.

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use rand::rngs::OsRng;
use serde::Serialize;
use sha3::{Digest, Sha3_256};

use wrfcoin_shared_protocol::blockchain::transaction::{
    ForecastAccuracyMetrics, ForecastArtifactManifest, ForecastVerificationReceipt,
    OracleAttestationPayload, Transaction, commit_authorization_message_with_domain,
    forecast_manifest_digest, forecast_receipt_signing_message,
};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

mod learned;
mod skill;

#[cfg(test)]
mod skill_tests;


/// Variables the prototype forecasts. Sorted — the manifest requires it.
const FORECAST_VARIABLES: [&str; 3] = ["humidity_pct", "pressure_hpa", "temperature_c"];
/// Forecast horizon, hours.
const HORIZON_HOURS: u32 = 6;
/// Trend damping toward pure persistence (see `Model` in the design note).
const TREND_DAMPING: f64 = 0.5;
const MODEL_ID: &str = "persistence-trend/v1";

#[derive(Parser)]
#[command(name = "forecast-proto", about = "WRFCoin forecast earning-loop prototype")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate the throwaway keypairs this prototype needs.
    Keygen {
        #[arg(long)]
        out: String,
    },
    /// Run the whole earning loop against a node.
    Run {
        #[arg(long, default_value = "http://127.0.0.1:8556")]
        node_url: String,
        #[arg(long)]
        station_id: String,
        /// File containing the oracle hex private key (permissions at most 0600).
        /// If omitted, read WRFCOIN_FORECAST_ORACLE_KEY from the environment.
        #[arg(long)]
        oracle_key: Option<std::path::PathBuf>,
        /// File containing the forecaster hex private key (permissions at most 0600).
        /// If omitted, read WRFCOIN_FORECAST_FORECASTER_KEY from the environment.
        #[arg(long)]
        forecaster_key: Option<std::path::PathBuf>,
        /// Operator/contributor bearer token.
        #[arg(long)]
        token: String,
        /// Oracle bearer token (attestation submission).
        #[arg(long)]
        oracle_token: String,
        /// Where to write the run's evidence.
        #[arg(long)]
        evidence_dir: String,
        /// Directory of `wrfcoin station build-observation` artifacts to read
        /// the input series from.
        ///
        /// PROTOTYPE FALLBACK, and it is the default because it has to be:
        /// the node exposes **no** HTTP surface that returns a revealed
        /// contribution's observation values. `GET /chain/block/{h}` decodes a
        /// `ContributionReveal` down to `{"commit_id": ...}` only
        /// (`routes/chain.rs::canonical_transaction_proof_payload`), and
        /// `/transactions/contributions/status` carries status and reward
        /// fields but no payload. A forecaster therefore cannot read its own
        /// input data back off the chain over HTTP — it must hold the
        /// collector-side artifacts. See the report's follow-up issues.
        #[arg(long)]
        observations_dir: Option<String>,
        /// Forecast model; learned mode requires a trained ONNX path.
        #[arg(long, env = "WRFCOIN_FORECAST_MODEL", default_value = MODEL_ID,
              value_parser = [MODEL_ID, learned::MODEL_ID])]
        model: String,
        /// ONNX file with [1,24,3] physical-unit input and [1,3] T+6h output.
        #[arg(long, env = "WRFCOIN_FORECAST_ONNX")]
        onnx_path: Option<std::path::PathBuf>,
        /// Sign the verification receipt under a chain binding this node does
        /// NOT resolve, to demonstrate the #2034/#2065 hazard end to end.
        #[arg(long, default_value_t = false)]
        foreign_receipt_binding: bool,
    },
}

// ---------------------------------------------------------------------------
// Chain client
// ---------------------------------------------------------------------------

struct Node {
    base: String,
    client: reqwest::blocking::Client,
}

impl Node {
    fn new(base: &str) -> Self {
        Self {
            base: base.trim_end_matches('/').to_string(),
            client: reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(20))
                .build()
                .expect("http client"),
        }
    }

    fn get(&self, path: &str, token: &str) -> Result<serde_json::Value> {
        let resp = self
            .client
            .get(format!("{}{path}", self.base))
            .bearer_auth(token)
            .send()
            .with_context(|| format!("GET {path}"))?;
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        if !status.is_success() {
            bail!("GET {path} -> HTTP {status}: {body}");
        }
        let parsed: serde_json::Value =
            serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
        // Read routes wrap results in {ok, data, error, meta}.
        Ok(match parsed.get("data") {
            Some(inner) if parsed.get("ok").is_some() && !inner.is_null() => inner.clone(),
            _ => parsed,
        })
    }

    fn post(&self, path: &str, token: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
        let resp = self
            .client
            .post(format!("{}{path}", self.base))
            .bearer_auth(token)
            .json(body)
            .send()
            .with_context(|| format!("POST {path}"))?;
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        if !status.is_success() {
            bail!("POST {path} -> HTTP {status}: {text}");
        }
        println!("    POST {path} -> HTTP {status} {text}");
        Ok(serde_json::from_str(&text).unwrap_or(serde_json::Value::Null))
    }
}

// ---------------------------------------------------------------------------
// Observations read off the chain
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
struct Observation {
    timestamp: i64,
    values: std::collections::BTreeMap<String, f64>,
}

/// Read the observation series from `wrfcoin station build-observation`
/// artifacts — the collector-side record of what it published.
///
/// Each artifact is `{"version":"station_observation_v1","commit_id":...,
/// "reveal_body":{"station_id":...,"timestamp":...,"payload":{...}}}`.
fn read_observations_from_artifacts(dir: &str, station_id: &str) -> Result<Vec<Observation>> {
    let mut out = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .with_context(|| format!("reading observation artifacts from {dir}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| {
                    // `obs-3.json` yes; `obs-3.commit.json` / `.reveal.json` /
                    // `.att.json` no.
                    n.starts_with("obs-")
                        && n.ends_with(".json")
                        && !n.trim_end_matches(".json").contains('.')
                })
                .unwrap_or(false)
        })
        .collect();
    entries.sort();
    for path in entries {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let Ok(doc) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        let Some(reveal) = doc.get("reveal_body") else { continue };
        if reveal.get("station_id").and_then(|v| v.as_str()) != Some(station_id) {
            continue;
        }
        let timestamp = reveal
            .get("timestamp")
            .and_then(|v| v.as_i64())
            .unwrap_or_default();
        let Some(data) = reveal.get("payload").and_then(|v| v.as_object()) else { continue };
        let mut values = std::collections::BTreeMap::new();
        for var in FORECAST_VARIABLES {
            if let Some(v) = data.get(var).and_then(|v| v.as_f64()) {
                values.insert(var.to_string(), v);
            }
        }
        if values.len() == FORECAST_VARIABLES.len() {
            out.push(Observation { timestamp, values });
        }
    }
    out.sort_by_key(|o| o.timestamp);
    out.dedup_by_key(|o| o.timestamp);
    Ok(out)
}

/// Walk every block and collect this station's revealed contribution
/// observations. This is the prototype's input data: it reads what the chain
/// actually holds, not a local fixture.
fn read_observations(node: &Node, token: &str, station_id: &str) -> Result<Vec<Observation>> {
    let info = node.get("/network/info", token)?;
    let tip = info
        .get("chain_height")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow!("/network/info has no chain_height: {info}"))?;

    let mut out = Vec::new();
    for height in 1..=tip {
        let block = match node.get(&format!("/chain/block/{height}"), token) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let txs = block
            .get("transactions")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        for tx in txs {
            let Some(raw) = tx.get("data").and_then(|v| v.as_str()) else {
                continue;
            };
            let Ok(payload) = serde_json::from_str::<serde_json::Value>(raw) else {
                continue;
            };
            if payload.get("station_id").and_then(|v| v.as_str()) != Some(station_id) {
                continue;
            }
            if payload.get("contribution_type").and_then(|v| v.as_str()) != Some("contribution") {
                continue;
            }
            // A reveal carries `data`; a commit does not.
            let Some(data) = payload.get("data").and_then(|v| v.as_object()) else {
                continue;
            };
            let timestamp = payload
                .get("timestamp")
                .and_then(|v| v.as_i64())
                .unwrap_or_default();
            let mut values = std::collections::BTreeMap::new();
            for var in FORECAST_VARIABLES {
                if let Some(v) = data.get(var).and_then(|v| v.as_f64()) {
                    values.insert(var.to_string(), v);
                }
            }
            if values.len() == FORECAST_VARIABLES.len() {
                out.push(Observation { timestamp, values });
            }
        }
    }
    out.sort_by_key(|o| o.timestamp);
    out.dedup_by_key(|o| o.timestamp);
    Ok(out)
}

// ---------------------------------------------------------------------------
// The analysis model
// ---------------------------------------------------------------------------

/// `persistence-trend/v1`.
///
/// For each variable, fit an ordinary least-squares trend `y = a + b·t` over
/// the observation window (t in hours), then forecast
/// `x̂(t_last + H) = y_last + damping · b · H`. Damping pulls the forecast
/// back toward persistence so a short, noisy window cannot extrapolate wildly.
/// With a single sample the slope is zero and the model degrades to pure
/// persistence. Deterministic, and needs no network access.
fn forecast_variable(window: &[f64], hours: &[f64], horizon_hours: f64) -> f64 {
    let n = window.len() as f64;
    let last = *window.last().expect("non-empty window");
    if window.len() < 2 {
        return last;
    }
    let mean_t = hours.iter().sum::<f64>() / n;
    let mean_y = window.iter().sum::<f64>() / n;
    let mut num = 0.0;
    let mut den = 0.0;
    for (t, y) in hours.iter().zip(window.iter()) {
        num += (t - mean_t) * (y - mean_y);
        den += (t - mean_t) * (t - mean_t);
    }
    let slope = if den.abs() < f64::EPSILON { 0.0 } else { num / den };
    last + TREND_DAMPING * slope * horizon_hours
}

#[derive(Debug, Serialize)]
struct ForecastOutput {
    model_id: String,
    issued_at: i64,
    valid_at: i64,
    horizon_hours: u32,
    station_id: String,
    /// Sorted variable -> forecast value.
    values: std::collections::BTreeMap<String, f64>,
}

/// Canonical SHA3-256 digest of a serializable value, matching the digest
/// discipline the manifest itself uses.
fn digest_hex<T: Serialize>(value: &T) -> String {
    let mut hasher = Sha3_256::new();
    hasher.update(serde_json::to_vec(value).unwrap_or_default());
    hex::encode(hasher.finalize())
}

// ---------------------------------------------------------------------------
// Scoring
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct ScoreReport {
    per_variable: std::collections::BTreeMap<String, VariableScore>,
    metrics: ForecastAccuracyMetricsView,
}

#[derive(Debug, Serialize)]
struct VariableScore {
    forecast: f64,
    truth: f64,
    climatology: f64,
    abs_error: f64,
    climatology_abs_error: f64,
    skill: f64,
}

#[derive(Debug, Serialize)]
struct ForecastAccuracyMetricsView {
    mae: f64,
    rmse: f64,
    bias: f64,
    skill_score: f64,
}

/// Score the forecast against a truth observation.
///
/// Skill per variable is `max(0, 1 − MAE_forecast / MAE_climatology)` where the
/// climatology reference is the window mean — the cheapest honest reference
/// that a persistence model does not trivially beat. Aggregate skill is the
/// mean over variables; MAE/RMSE/bias are aggregated over variables too.
fn score(
    forecast: &ForecastOutput,
    truth: &Observation,
    climatology: &std::collections::BTreeMap<String, f64>,
) -> ScoreReport {
    let mut per_variable = std::collections::BTreeMap::new();
    let mut errs = Vec::new();
    let mut skills = Vec::new();
    for var in FORECAST_VARIABLES {
        let f = *forecast.values.get(var).unwrap_or(&0.0);
        let t = *truth.values.get(var).unwrap_or(&0.0);
        let c = *climatology.get(var).unwrap_or(&0.0);
        let err = f - t;
        let abs_error = err.abs();
        let clim_abs = (c - t).abs();
        let skill = if clim_abs <= f64::EPSILON {
            if abs_error <= f64::EPSILON { 1.0 } else { 0.0 }
        } else {
            (1.0 - abs_error / clim_abs).clamp(0.0, 1.0)
        };
        errs.push(err);
        skills.push(skill);
        per_variable.insert(
            var.to_string(),
            VariableScore {
                forecast: f,
                truth: t,
                climatology: c,
                abs_error,
                climatology_abs_error: clim_abs,
                skill,
            },
        );
    }
    let n = errs.len() as f64;
    let mae = errs.iter().map(|e| e.abs()).sum::<f64>() / n;
    let rmse = (errs.iter().map(|e| e * e).sum::<f64>() / n).sqrt();
    let bias = errs.iter().sum::<f64>() / n;
    let skill_score = (skills.iter().sum::<f64>() / n).clamp(0.0, 1.0);
    ScoreReport {
        per_variable,
        metrics: ForecastAccuracyMetricsView { mae, rmse, bias, skill_score },
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn key_from_hex(hex_str: &str) -> Result<PrivateKey> {
    PrivateKey::from_hex(hex_str.trim()).map_err(|_| anyhow!("invalid private key encoding"))
}

/// Check the opened file, so a path replacement cannot bypass the mode check.
fn read_private_key(path: Option<&std::path::Path>, env_name: &str) -> Result<String> {
    use std::io::Read;
    let text = if let Some(path) = path {
        let file = std::fs::File::open(path).context("cannot open private key file")?;
        let metadata = file.metadata().context("cannot inspect private key file")?;
        if !metadata.is_file() {
            bail!("private key source must be a regular file");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o7777 & !0o600 != 0 {
                bail!("private key file permissions must be at most 0600");
            }
        }
        #[cfg(not(unix))]
        bail!("private key file permissions cannot be verified on this platform; use the environment");
        let mut text = String::new();
        file.take(1025).read_to_string(&mut text).context("cannot read private key file")?;
        text
    } else {
        std::env::var(env_name).map_err(|_| anyhow!("private key file or environment variable required"))?
    };
    if text.len() > 1024 {
        bail!("private key input is too long");
    }
    key_from_hex(&text)?;
    Ok(text)
}

fn write_private_key(path: &std::path::Path, key: &str) -> Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    bail!("secure key generation requires Unix file permissions");
    let mut file = options.open(path).context("cannot create private key file")?;
    file.write_all(key.as_bytes()).context("cannot write private key file")?;
    Ok(())
}

fn write_evidence<T: Serialize>(dir: &str, name: &str, value: &T) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = format!("{dir}/{name}");
    std::fs::write(&path, serde_json::to_vec_pretty(value)?)?;
    println!("    evidence -> {path}");
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Keygen { out } => keygen(&out),
        Command::Run {
            node_url,
            station_id,
            oracle_key,
            forecaster_key,
            token,
            oracle_token,
            evidence_dir,
            observations_dir,
            model,
            onnx_path,
            foreign_receipt_binding,
        } => {
            let oracle_key = read_private_key(oracle_key.as_deref(), "WRFCOIN_FORECAST_ORACLE_KEY")?;
            let forecaster_key = read_private_key(forecaster_key.as_deref(), "WRFCOIN_FORECAST_FORECASTER_KEY")?;
            run(
                &node_url,
                &station_id,
                &oracle_key,
                &forecaster_key,
                &token,
                &oracle_token,
                &evidence_dir,
                observations_dir.as_deref(),
                &model,
                onnx_path.as_deref(),
                foreign_receipt_binding,
            )
        },
    }
}

fn keygen(out: &str) -> Result<()> {
    std::fs::create_dir_all(out)?;
    let mut keys = serde_json::Map::new();
    for role in ["oracle", "governance", "station_owner", "forecaster", "producer"] {
        let sk = PrivateKey::generate(&mut OsRng);
        let pk = PublicKey::from_private_key(&sk);
        let entry = serde_json::json!({
            "public_key_hex": pk.to_hex(),
            "address": pk.to_address(),
        });
        write_private_key(&std::path::Path::new(out).join(format!("{role}.key")), &sk.to_hex())?;
        std::fs::write(format!("{out}/{role}.pub"), pk.to_hex())?;
        keys.insert(role.to_string(), entry);
    }
    let value = serde_json::Value::Object(keys);
    std::fs::write(format!("{out}/keys.json"), serde_json::to_vec_pretty(&value)?)?;
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run(
    node_url: &str,
    station_id: &str,
    oracle_key_hex: &str,
    forecaster_key_hex: &str,
    token: &str,
    oracle_token: &str,
    evidence_dir: &str,
    observations_dir: Option<&str>,
    model: &str,
    onnx_path: Option<&std::path::Path>,
    foreign_receipt_binding: bool,
) -> Result<()> {
    let chain_id = std::env::var("WRFCOIN_CHAIN_ID").unwrap_or_default();
    let network_id = std::env::var("WRFCOIN_NETWORK_ID").unwrap_or_default();
    if chain_id.is_empty() || network_id.is_empty() {
        bail!(
            "WRFCOIN_CHAIN_ID and WRFCOIN_NETWORK_ID must be set and must match the node's, \
             or every receipt this tool signs will be rejected by the node (see design note)"
        );
    }
    println!("[0] chain binding: chain_id={chain_id} network_id={network_id}");

    let node = Node::new(node_url);
    let oracle_sk = key_from_hex(oracle_key_hex)?;
    let oracle_id = PublicKey::from_private_key(&oracle_sk).to_hex();
    let forecaster_sk = key_from_hex(forecaster_key_hex)?;
    // `to_address()` emits `wrf<40hex>`; every on-chain address surface and the
    // reward payee resolution use the canonical `0x` form.
    let forecaster_addr = format!(
        "0x{}",
        &PublicKey::from_private_key(&forecaster_sk).to_address()[3..]
    );
    println!("[0] oracle_id={oracle_id}");
    println!("[0] forecaster={forecaster_addr}");

    // -- 1. Read the input data --------------------------------------------
    let observations = match observations_dir {
        Some(dir) => {
            let obs = read_observations_from_artifacts(dir, station_id)?;
            println!(
                "[1] read {} observation(s) for {station_id} from collector artifacts in {dir} \
                 (the chain exposes no reveal payloads over HTTP — see --observations-dir)",
                obs.len()
            );
            obs
        },
        None => {
            let obs = read_observations(&node, token, station_id)?;
            println!(
                "[1] read {} observation(s) for {station_id} from the chain",
                obs.len()
            );
            obs
        },
    };
    if observations.len() < 2 {
        bail!(
            "need at least 2 on-chain observations for {station_id} (a window plus a truth \
             sample); found {}",
            observations.len()
        );
    }
    write_evidence(evidence_dir, "01-observations.json", &observations)?;

    // Chain-derived height+1 view, including applied schedule extensions.
    // No CLI/env switch can select the payout rule. Older nodes omit the list.
    let pending = node.get("/transactions/oracle/pending?limit=1", oracle_token)?;
    let skill_policy = skill::SkillPolicy::from_pending(&pending, &chain_id, &network_id)?;

    // Backtest split: everything but the last sample is the input window; the
    // last sample is the truth the forecast is scored against. This is what
    // makes the loop runnable in one sitting — see "prototype-only" in the
    // design note.
    let (window, truth_slice) = observations.split_at(observations.len() - 1);
    let truth = &truth_slice[0];

    // -- 2. Run the model --------------------------------------------------
    let t0 = window[0].timestamp as f64;
    let hours: Vec<f64> = window
        .iter()
        .map(|o| (o.timestamp as f64 - t0) / 3600.0)
        .collect();
    let issued_at = window.last().unwrap().timestamp;
    let mut values = std::collections::BTreeMap::new();
    let mut climatology = std::collections::BTreeMap::new();
    for var in FORECAST_VARIABLES {
        let series: Vec<f64> = window
            .iter()
            .map(|o| *o.values.get(var).unwrap_or(&0.0))
            .collect();
        climatology.insert(
            var.to_string(),
            series.iter().sum::<f64>() / series.len() as f64,
        );
        values.insert(
            var.to_string(),
            forecast_variable(&series, &hours, HORIZON_HOURS as f64),
        );
    }
    let (actual_model_id, values) = learned::select_forecast(model, onnx_path, window, values);
    let forecast = ForecastOutput {
        model_id: actual_model_id.to_string(),
        issued_at,
        valid_at: issued_at + (HORIZON_HOURS as i64) * 3600,
        horizon_hours: HORIZON_HOURS,
        station_id: station_id.to_string(),
        values,
    };
    println!("[2] {} forecast: {:?}", forecast.model_id, forecast.values);
    write_evidence(evidence_dir, "02-forecast.json", &forecast)?;

    // Validate the selected score before publishing even the commit.
    let report = skill::score(
        &forecast,
        truth,
        &climatology,
        window.last().unwrap(),
        skill_policy,
    )?;

    // -- 3. Build the manifest and the commit id ---------------------------
    let mut input_variables: Vec<String> =
        FORECAST_VARIABLES.iter().map(|s| s.to_string()).collect();
    input_variables.sort();
    let output_variables = input_variables.clone();
    let manifest = ForecastArtifactManifest {
        schema_version: 1,
        model_id: skill_policy.manifest_model_id(&forecast.model_id),
        input_hash: digest_hex(&window),
        output_hash: digest_hex(&forecast),
        produced_at: issued_at,
        region: station_id.to_string(),
        horizon_hours: HORIZON_HOURS,
        input_variables,
        output_variables,
    };
    manifest
        .validate()
        .map_err(|e| anyhow!("manifest does not validate: {e}"))?;
    let manifest_digest = forecast_manifest_digest(&manifest);

    let reveal_data = serde_json::to_value(&forecast)?;
    // The nonce feeds the commit id, so discriminate the two demonstration
    // runs — otherwise the second one replays the first one's commit.
    let nonce = format!(
        "{}{}",
        &digest_hex(&forecast)[..16],
        if foreign_receipt_binding { "-foreign" } else { "-ok" }
    );
    let reveal_timestamp = issued_at;
    let commit_id = Transaction::compute_commit_id(
        &reveal_data,
        &nonce,
        station_id,
        reveal_timestamp,
        "forecast",
    );
    println!("[3] manifest_digest={manifest_digest}");
    println!("[3] commit_id={commit_id}");
    write_evidence(evidence_dir, "03-manifest.json", &manifest)?;

    // -- 4. Publish the ForecastCommit -------------------------------------
    // #1848: the commit must carry a signature from the committing party over
    // the domain-separated commit-authorization message, or the mempool
    // refuses it with "commit sender authorization failed". The preimage folds
    // in the chain binding, so this too must agree with the node.
    let commit_auth_pubkey = PublicKey::from_private_key(&forecaster_sk).to_hex();
    let commit_auth_message = commit_authorization_message_with_domain(
        &chain_id,
        &network_id,
        station_id,
        &commit_id,
        "forecast",
    );
    let commit_auth_signature = forecaster_sk.sign(&commit_auth_message).to_string();
    let commit_body = serde_json::json!({
        "forecaster": forecaster_addr,
        "station_id": station_id,
        "commit_hash": commit_id,
        "auth_pubkey": commit_auth_pubkey,
        "auth_signature": commit_auth_signature,
    });
    println!("[4] submitting ForecastCommit");
    let commit_resp = node.post("/transactions/forecast/commit", token, &commit_body)?;
    write_evidence(evidence_dir, "04-commit-response.json", &commit_resp)?;

    // -- 5. Score, and sign the receipt with the oracle key ----------------
    println!(
        "[5] skill={:.4} mae={:.4} rmse={:.4} bias={:.4}",
        report.metrics.skill_score, report.metrics.mae, report.metrics.rmse, report.metrics.bias
    );
    write_evidence(evidence_dir, "05-score.json", &report)?;

    let mut receipt = ForecastVerificationReceipt {
        manifest_digest: manifest_digest.clone(),
        commit_id: commit_id.clone(),
        oracle_id: oracle_id.clone(),
        metrics: ForecastAccuracyMetrics {
            mae: report.metrics.mae,
            rmse: report.metrics.rmse,
            bias: report.metrics.bias,
            skill_score: report.metrics.skill_score,
        },
        verified_at: truth.timestamp.max(issued_at + 1),
        nonce: format!("receipt-{}", &commit_id[..16]),
        zk_proof: None,
        signature: None,
    };
    // Folds WRFCOIN_CHAIN_ID / WRFCOIN_NETWORK_ID into the preimage.
    let message = forecast_receipt_signing_message(&receipt);
    if foreign_receipt_binding {
        // Rebuild the canonical preimage with the MAINNET chain binding — the
        // exact mistake an operator makes by forgetting to export
        // WRFCOIN_CHAIN_ID in the signer's environment.
        let mut parts: Vec<String> = message.split('|').map(str::to_string).collect();
        if parts.len() != 8 {
            bail!("unexpected forecast signing preimage shape: {message}");
        }
        parts[2] = "wrfcoin-main".to_string();
        parts[3] = "mainnet".to_string();
        let foreign = parts.join("|");
        receipt.signature = Some(oracle_sk.sign(foreign.as_bytes()).to_string());
        println!(
            "[5] receipt DELIBERATELY signed under wrfcoin-main/mainnet while the node runs \
             {chain_id}/{network_id}; locally valid = {}",
            receipt.has_valid_signature()
        );
    } else {
        receipt.signature = Some(oracle_sk.sign(message.as_bytes()).to_string());
        if !receipt.has_valid_signature() {
            bail!(
                "receipt signature does not verify locally — refusing to submit a reveal that \
                 would slash the oracle"
            );
        }
        println!(
            "[5] receipt signed and locally verified under chain binding {chain_id}/{network_id}"
        );
    }
    write_evidence(evidence_dir, "06-receipt.json", &receipt)?;

    // -- 6. Publish the ForecastReveal carrying manifest + receipt ---------
    let reveal_body = serde_json::json!({
        "forecaster": forecaster_addr,
        "station_id": station_id,
        "payload": reveal_data,
        "nonce": nonce,
        "commit_hash": commit_id,
        "timestamp": reveal_timestamp,
        "manifest": manifest,
        "verification_receipt": receipt,
    });
    println!("[6] submitting ForecastReveal (manifest + receipt)");
    let reveal_resp = node.post("/transactions/forecast/reveal", token, &reveal_body)?;
    write_evidence(evidence_dir, "07-reveal-response.json", &reveal_resp)?;

    // -- 7. Attest the commit so it reaches oracle quorum ------------------
    let mut attestation = OracleAttestationPayload {
        commit_id: commit_id.clone(),
        oracle_id: oracle_id.clone(),
        approved: true,
        signature: None,
        confidence: Some(1.0),
        timestamp: receipt.verified_at,
        nonce: Some(format!("att-{}", &commit_id[..16])),
    };
    let att_message = attestation.signing_message();
    attestation.signature = Some(oracle_sk.sign(att_message.as_bytes()).to_string());
    let att_body = serde_json::json!({
        "commit_hash": commit_id,
        "oracle_id": oracle_id,
        "approved": true,
        "confidence": 1.0,
        "nonce": attestation.nonce,
        "timestamp": attestation.timestamp,
        "signature": attestation.signature,
    });
    println!("[7] submitting OracleAttestation");
    let att_resp = node.post("/transactions/oracle/attestation", oracle_token, &att_body)?;
    write_evidence(evidence_dir, "08-attestation-response.json", &att_resp)?;

    let summary = serde_json::json!({
        "chain_id": chain_id,
        "network_id": network_id,
        "station_id": station_id,
        "oracle_id": oracle_id,
        "forecaster": forecaster_addr,
        "commit_id": commit_id,
        "manifest_digest": manifest_digest,
        "model_id": forecast.model_id,
        "horizon_hours": HORIZON_HOURS,
        "observations_read": observations.len(),
        "skill_score": report.metrics.skill_score,
    });
    write_evidence(evidence_dir, "09-run-summary.json", &summary)?;
    println!("[8] loop submitted. commit_id={commit_id} — mine a block to settle the reward.");
    Ok(())
}
