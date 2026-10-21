//! File-based forecast transaction submission shared by the CLI and route contracts.
use anyhow::{anyhow, bail, Context, Result};
use clap::Args;
use serde_json::{json, Value};
use std::{fs, io::Read, path::{Path, PathBuf}};
use wrfcoin_shared_protocol::{
    blockchain::transaction::{Transaction, ForecastArtifactManifest, ForecastVerificationReceipt,
        commit_authorization_message_with_domain},
    crypto::signature::{PrivateKey, PublicKey, Signature},
};
use crate::{ApiClient, ApiConfig};

#[derive(Args)]
pub struct ForecastInput {
    #[clap(long)]
    pub forecaster: String,
    #[clap(long)]
    pub station_id: String,
    /// Forecast JSON object used in the canonical commit preimage.
    #[clap(long)]
    pub data_file: PathBuf,
    #[clap(long)]
    pub nonce: String,
    /// Reuse this exact timestamp for commit and reveal.
    #[clap(long)]
    pub timestamp: i64,
}

#[derive(Args)]
pub struct ForecastConnection {
    #[clap(long)]
    pub node_url: Option<String>,
    /// Contributor bearer; defaults to WRFCOIN_OPERATOR_TOKEN / WRFCOIN_READER_TOKEN.
    #[clap(long)]
    pub token: Option<String>,
}

impl ForecastConnection {
    pub fn client(&self) -> Result<ApiClient> {
        let mut config = ApiConfig::default();
        if let Some(url) = &self.node_url { config.base_url = url.clone(); }
        if let Some(token) = &self.token { config.bearer_token = Some(token.clone()); }
        config.retry_attempts = 0;
        ApiClient::new(config)
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path, kind: &str) -> Result<T> {
    let raw = fs::read(path).with_context(|| format!("cannot read {kind} file"))?;
    serde_json::from_slice(&raw).map_err(|_| anyhow!("invalid {kind} JSON file"))
}

impl ForecastInput {
    /// Canonical identity shared by the receipt producer and commit/reveal.
    pub fn commit_id(&self) -> Result<String> {
        self.payload_and_id().map(|(_, id)| id)
    }

    fn payload_and_id(&self) -> Result<(Value, String)> {
        let payload: Value = read_json(&self.data_file, "forecast data")?;
        if !payload.is_object() { bail!("forecast data must be a JSON object"); }
        if self.forecaster.trim().is_empty() || self.station_id.trim().is_empty()
            || self.nonce.trim().is_empty() { bail!("forecaster, station-id and nonce must not be blank"); }
        let id = Transaction::compute_commit_id(&payload, &self.nonce, &self.station_id,
            self.timestamp, "forecast");
        Ok((payload, id))
    }
}

pub fn read_signer(path: &Path) -> Result<PrivateKey> {
    let file = fs::File::open(path).context("cannot open private key file")?;
    let metadata = file.metadata().context("cannot inspect private key file")?;
    if !metadata.is_file() { bail!("private key source must be a regular file"); }
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o7777 & !0o600 != 0 {
            bail!("private key file permissions must be at most 0600");
        }
    }
    #[cfg(not(unix))]
    bail!("private key file permissions cannot be verified on this platform");
    let mut raw = String::new();
    file.take(1025).read_to_string(&mut raw).context("cannot read private key file")?;
    if raw.len() > 1024 { bail!("private key input is too long"); }
    PrivateKey::from_hex(raw.trim()).map_err(|_| anyhow!("invalid private key file"))
}

pub fn commit_body(input: &ForecastInput, signer: &PrivateKey, chain_id: &str,
    network_id: &str) -> Result<Value> {
    let (_, id) = input.payload_and_id()?;
    let message = commit_authorization_message_with_domain(chain_id, network_id,
        &input.station_id, &id, "forecast");
    Ok(json!({"forecaster": input.forecaster, "station_id": input.station_id,
        "commit_hash": id, "auth_pubkey": PublicKey::from_private_key(signer).to_hex(),
        "auth_signature": signer.sign(&message).to_hex()}))
}

pub fn reveal_body(input: &ForecastInput, manifest_file: &Path,
    receipt_file: Option<&Path>) -> Result<Value> {
    let (payload, id) = input.payload_and_id()?;
    let manifest: ForecastArtifactManifest = read_json(manifest_file, "manifest")?;
    manifest.validate().map_err(|_| anyhow!("invalid forecast manifest"))?;
    let receipt = receipt_file.map(|path| -> Result<ForecastVerificationReceipt> {
        let receipt: ForecastVerificationReceipt = read_json(path, "receipt")?;
        receipt.validate().map_err(|_| anyhow!("invalid forecast receipt"))?;
        if receipt.commit_id != id || receipt.manifest_digest != manifest.manifest_digest()
            || receipt.nonce.trim().is_empty() || receipt.signature.is_none() {
            bail!("receipt must be signed and bind this commit and manifest");
        }
        PublicKey::from_hex(&receipt.oracle_id).map_err(|_| anyhow!("invalid receipt oracle key"))?;
        Signature::from_hex(receipt.signature.as_deref().unwrap())
            .map_err(|_| anyhow!("invalid receipt signature encoding"))?;
        Ok(receipt)
    }).transpose()?;
    Ok(json!({"forecaster": input.forecaster, "station_id": input.station_id,
        "payload": payload, "nonce": input.nonce, "timestamp": input.timestamp,
        "commit_hash": id, "manifest": manifest, "verification_receipt": receipt}))
}

pub async fn submit(connection: &ForecastConnection, kind: &str, body: Value) -> Result<Value> {
    let id = body["commit_hash"].clone();
    let response: Value = connection.client()?.post(
        &format!("/transactions/forecast/{kind}"), Some(body)).await?;
    let tx_id = response.get("id")
        .ok_or_else(|| anyhow!("forecast response missing tx_id"))?;
    Ok(json!({"commit_id": id, "tx_id": tx_id, "result": response}))
}
