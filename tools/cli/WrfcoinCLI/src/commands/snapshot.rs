//! `wrfcoin snapshot fetch` — the verified snapshot-join client (core4#3126).
//!
//! Pulls a signed state snapshot from a TRUSTED node, verifies it locally —
//! the signer must be in the operator's trusted-signer list and the exported
//! entries must reproduce the signed root — and only then hands it to the
//! local node's ingest route, which re-verifies (#2119). If the local node
//! returns a consumer-signed `NodeServiceReceipt`, the receipt is posted back
//! to the serving node: the proof of service #2848 settles.
//!
//! The CLI never forwards a payload it did not verify: a payload from an
//! untrusted signer, or an export page that does not reproduce the signed
//! root, is refused here with no request to the local node.

use anyhow::Result;
use clap::Subcommand;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashSet;
use wrfcoin_cli::errors::CliError;
use wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity;
use wrfcoin_cli::{ApiClient, ApiConfig, CliErrorCode};
use wrfcoin_shared_protocol::blockchain::snapshot_sync::{
    SignedSnapshotSyncPayload, SnapshotSyncPayload, build_state_snapshot,
};

use crate::cli::commands::ExecutionContext;

/// The bonded-fetch wire protocol, compiled from the node's own source so the
/// two ends cannot drift (owner decision 2026-10-31: serve over P2P).
#[allow(dead_code)]
#[path = "../../../../../p2p/src/bonded_fetch.rs"]
mod bonded_fetch;
use bonded_fetch::{BondedFetchAuth, BondedFetchRequest, BondedFetchResponse};

/// Snapshot join: fetch a verified state snapshot into the local node.
#[derive(Subcommand)]
pub enum SnapshotCommands {
    /// Fetch a signed snapshot from a trusted node, verify it locally, ingest
    /// it into the local node, and return the service receipt to the server.
    Fetch {
        /// The trusted serving node: its HTTP API base URL (e.g. http://seed:8556),
        /// or its P2P address ending in its peer id
        /// (e.g. /dns4/laptop.example/tcp/9000/p2p/12D3Koo…). The P2P form needs
        /// --bonded and --to: the receiver signs the fetch authorization.
        #[clap(long)]
        from: String,
        /// Base URL of the local node to ingest into (default: WRFCOIN_API_URL).
        #[clap(long)]
        to: Option<String>,
        /// Comma-separated hex ed25519 public keys allowed to sign the snapshot.
        #[clap(long, env = "WRFCOIN_SNAPSHOT_TRUSTED_SIGNERS", value_delimiter = ',')]
        trusted_signers: Vec<String>,
        /// Entries per export page.
        #[clap(long, default_value = "500")]
        page_size: usize,
        /// Verify and report only; do not ingest.
        #[clap(long)]
        dry_run: bool,
        /// Service id the receipt should name the server by (default: the
        /// `--from` URL authority).
        #[clap(long)]
        server_id: Option<String>,
        /// Request bonded server/receiver endorsements after actual installation.
        #[clap(long)]
        bonded: bool,
        /// Read bounded server Reader / receiver Operator credentials from stdin.
        /// Credentials never enter command arguments or public receipt archives.
        #[clap(long)]
        auth_stdin: bool,
        /// Durable public receipt archive, independent of HTTP acknowledgement.
        #[clap(long)]
        receipt_archive: Option<std::path::PathBuf>,
        /// Timeout in seconds for the single ingest POST to the receiver (#3586).
        /// Real testnet state is hundreds of MiB, which can exceed the 30 s default.
        #[clap(long, default_value_t = DEFAULT_INGEST_TIMEOUT_SECS, value_parser = clap::value_parser!(u64).range(1..=MAX_INGEST_TIMEOUT_SECS))]
        ingest_timeout_secs: u64,
    },
    /// Submit a bounded, offline owner-authorized Node receipt transaction.
    SubmitReceipt {
        #[clap(long)]
        transaction_file: std::path::PathBuf,
    },
}

#[derive(Debug, Deserialize)]
struct PayloadResponse {
    payload: SnapshotSyncPayload,
    signed_payload: Option<SignedSnapshotSyncPayload>,
}

#[derive(Debug, Deserialize, Clone)]
struct ExportEntry {
    key_hex: String,
    value_hex: String,
}

#[derive(Debug, Deserialize)]
struct ExportPage {
    height: u64,
    snapshot_root_hex: String,
    total: usize,
    next_offset: Option<usize>,
    entries: Vec<ExportEntry>,
}

fn user_error(message: impl Into<String>) -> anyhow::Error {
    CliError::new(CliErrorCode::ValidationError, message).into()
}

#[derive(Deserialize)]
struct SnapshotAuth {
    /// The server's Reader token. Not used, and may be omitted, for a P2P server.
    #[serde(default)]
    server_reader: String,
    receiver_operator: String,
}
fn parse_snapshot_auth(bytes: &[u8], needs_server_reader: bool) -> Result<SnapshotAuth> {
    anyhow::ensure!(bytes.len() <= 16 * 1024, "snapshot credential input exceeds bound");
    let auth: SnapshotAuth = serde_json::from_slice(bytes).map_err(|_| user_error("invalid snapshot credential input"))?;
    anyhow::ensure!((!needs_server_reader || !auth.server_reader.trim().is_empty()) && !auth.receiver_operator.trim().is_empty(), "snapshot credentials unavailable");
    Ok(auth)
}
fn read_snapshot_auth(needs_server_reader: bool) -> Result<SnapshotAuth> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::io::stdin().take(16 * 1024 + 1).read_to_end(&mut bytes)?;
    parse_snapshot_auth(&bytes, needs_server_reader)
}

/// `--from` names a P2P server when it is a multiaddr (it starts with `/`).
fn p2p_from(from: &str) -> Result<Option<libp2p::Multiaddr>> {
    if !from.starts_with('/') {
        return Ok(None);
    }
    let addr: libp2p::Multiaddr = from.parse().map_err(|_| user_error("--from is not a valid P2P multiaddr"))?;
    bonded_fetch::client::server_peer_id(&addr).map_err(user_error)?;
    Ok(Some(addr))
}

/// How long to wait for the P2P server's Noise handshake.
const P2P_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
/// Busy / rate-limited answers are retried this many times, honoring the
/// server's hint (capped).
const P2P_BUSY_RETRIES: usize = 5;

/// A P2P bonded-fetch server, plus the receiver API that signs this run's
/// fetch authorization with the receiver's registered consumer key.
struct P2pServer {
    client: bonded_fetch::client::BondedFetchClient,
    receiver: ApiClient,
    auth: Option<BondedFetchAuth>,
    service_id: Option<String>,
}

impl P2pServer {
    async fn authorization(&mut self) -> Result<BondedFetchAuth> {
        let now = chrono::Utc::now().timestamp();
        if let Some(auth) = self.auth.as_ref().filter(|a| now.saturating_sub(a.issued_at) < bonded_fetch::AUTH_REFRESH_AFTER_SECS) {
            return Ok(auth.clone());
        }
        let server = self.client.server_peer_id().to_string();
        let client = self.client.local_peer_id().to_string();
        let auth: BondedFetchAuth = self.receiver
            .post("/network/snapshot-sync/p2p-fetch-auth", Some(json!({"server_peer_id": server, "client_peer_id": client})))
            .await?;
        anyhow::ensure!(auth.server_peer_id == server && auth.client_peer_id == client, "receiver signed a fetch authorization for other peers");
        self.auth = Some(auth.clone());
        Ok(auth)
    }

    async fn call(&mut self, request: BondedFetchRequest) -> Result<BondedFetchResponse> {
        for attempt in 0..=P2P_BUSY_RETRIES {
            let response = self.client.call(request.clone()).await.map_err(user_error)?;
            match response {
                BondedFetchResponse::Error { code, retry_after_ms: Some(wait), .. }
                    if attempt < P2P_BUSY_RETRIES && matches!(code.as_str(), "BUSY" | "RATE_LIMITED") =>
                {
                    tokio::time::sleep(std::time::Duration::from_millis(wait.clamp(250, 60_000))).await;
                },
                BondedFetchResponse::Error { code, message, .. } => {
                    return Err(user_error(format!("P2P server refused {}: {code}: {message}", request.label())));
                },
                other => return Ok(other),
            }
        }
        Err(user_error("P2P server stayed busy; retry later"))
    }
}

/// The serving end of a fetch: the HTTP API (Reader bearer) or P2P.
enum SnapshotServer {
    Http(ApiClient),
    P2p(Box<P2pServer>),
}

fn unexpected(what: &str) -> anyhow::Error {
    user_error(format!("P2P server answered {what} with the wrong message"))
}

impl SnapshotServer {
    async fn identity(&mut self) -> Result<(ChainIdentity, [u8; 32])> {
        match self {
            SnapshotServer::Http(api) => network_identity(api).await,
            SnapshotServer::P2p(p2p) => match p2p.call(BondedFetchRequest::Info).await? {
                BondedFetchResponse::Info { chain_id, network_id, genesis_hash, service_id } => {
                    let identity = ChainIdentity::new(chain_id, network_id);
                    anyhow::ensure!(identity.chain_id != "unknown" && identity.network_id != "unknown", "target network identity unavailable");
                    let genesis = super::node_deposit::api_hash(&genesis_hash)
                        .filter(|hash| *hash != [0; 32])
                        .ok_or_else(|| user_error("target genesis unavailable"))?;
                    p2p.service_id = Some(service_id);
                    Ok((identity, genesis))
                },
                _ => Err(unexpected("info")),
            },
        }
    }

    async fn sync_payload(&mut self) -> Result<PayloadResponse> {
        match self {
            SnapshotServer::Http(api) => api.get("/state/snapshot/sync-payload", None).await,
            SnapshotServer::P2p(p2p) => {
                let auth = p2p.authorization().await?;
                match p2p.call(BondedFetchRequest::SyncPayload { auth }).await? {
                    BondedFetchResponse::SyncPayload { payload, signed_payload } => {
                        Ok(PayloadResponse { payload, signed_payload: Some(signed_payload) })
                    },
                    _ => Err(unexpected("sync_payload")),
                }
            },
        }
    }

    async fn export_page(&mut self, offset: usize, limit: usize, signed: &SnapshotSyncPayload) -> Result<ExportPage> {
        match self {
            SnapshotServer::Http(api) => {
                let offset_s = offset.to_string();
                let limit_s = limit.to_string();
                api.get("/state/snapshot/export", Some(&[("offset", offset_s.as_str()), ("limit", limit_s.as_str())])).await
            },
            SnapshotServer::P2p(p2p) => {
                let auth = p2p.authorization().await?;
                let request = BondedFetchRequest::ExportPage {
                    auth,
                    height: signed.height,
                    snapshot_root_hex: signed.snapshot_root_hex.clone(),
                    offset: offset as u64,
                    limit: u32::try_from(limit).unwrap_or(bonded_fetch::MAX_EXPORT_PAGE_ENTRIES),
                };
                match p2p.call(request).await? {
                    BondedFetchResponse::ExportPage { height, snapshot_root_hex, total, next_offset, entries } => Ok(ExportPage {
                        height,
                        snapshot_root_hex,
                        total: usize::try_from(total).map_err(|_| user_error("export total exceeds this platform"))?,
                        next_offset: next_offset.map(|n| usize::try_from(n).unwrap_or(usize::MAX)),
                        entries: entries.into_iter().map(|e| ExportEntry { key_hex: e.key_hex, value_hex: e.value_hex }).collect(),
                    }),
                    _ => Err(unexpected("export_page")),
                }
            },
        }
    }

    async fn bonded_ticket(&mut self, installed: Value) -> Result<Value> {
        match self {
            SnapshotServer::Http(api) => api.post("/network/snapshot-sync/bonded-ticket", Some(installed)).await,
            SnapshotServer::P2p(p2p) => {
                let receipt = serde_json::from_value(installed["receipt"].clone())?;
                let manifest = serde_json::from_value(installed["manifest"].clone())?;
                let auth = p2p.authorization().await?;
                match p2p.call(BondedFetchRequest::BondedTicket { auth, receipt, manifest }).await? {
                    BondedFetchResponse::BondedTicket { claim_hex } => Ok(json!({ "claim_hex": claim_hex })),
                    _ => Err(unexpected("bonded_ticket")),
                }
            },
        }
    }

    async fn deliver_receipt(&mut self, receipt: &Value) -> Result<Value> {
        match self {
            SnapshotServer::Http(api) => api.post::<Value>("/network/snapshot-sync/receipt", Some(json!({ "receipt": receipt }))).await,
            SnapshotServer::P2p(p2p) => {
                let receipt = serde_json::from_value(receipt.clone())?;
                let auth = p2p.authorization().await?;
                match p2p.call(BondedFetchRequest::Receipt { auth, receipt }).await? {
                    BondedFetchResponse::ReceiptAck { accepted, recorded_receipts } => {
                        Ok(json!({ "accepted": accepted, "recorded_receipts": recorded_receipts }))
                    },
                    _ => Err(unexpected("receipt")),
                }
            },
        }
    }

    /// The service id a receipt names this server by when `--server-id` is not
    /// given: the HTTP authority, or the id the P2P server announced.
    fn default_server_id(&self, from: &str) -> String {
        match self {
            SnapshotServer::P2p(p2p) => p2p.service_id.clone().unwrap_or_default(),
            SnapshotServer::Http(_) => from
                .trim_end_matches('/')
                .trim_start_matches("http://")
                .trim_start_matches("https://")
                .to_string(),
        }
    }
}
/// Default and maximum timeout for the receiver ingest POST (#3586).
const DEFAULT_INGEST_TIMEOUT_SECS: u64 = 30;
const MAX_INGEST_TIMEOUT_SECS: u64 = 3600;

/// The ingest POST installs a snapshot only on a fresh receiver, so a second
/// attempt after a timeout cannot succeed: send it exactly once, with the
/// operator-chosen timeout. Only this command's ingest client is affected.
fn ingest_config(mut config: ApiConfig, timeout_secs: u64) -> ApiConfig {
    config.retry_attempts = 0;
    config.timeout = std::time::Duration::from_secs(timeout_secs.clamp(1, MAX_INGEST_TIMEOUT_SECS));
    config
}

fn client_with_role(base_url: &str, bearer: Option<&str>) -> Result<ApiClient> {
    let parsed = url::Url::parse(base_url)?;
    anyhow::ensure!(parsed.username().is_empty() && parsed.password().is_none(), "credentials in URL refused");
    let mut config = ApiConfig { base_url: base_url.trim_end_matches('/').to_string(), ..ApiConfig::default() };
    if let Some(value) = bearer { config.bearer_token = Some(value.to_owned()); }
    Ok(ApiClient::new(config)?)
}

/// Read a concrete identity from the target, never the process's legacy default.
/// An unresolved genesis cannot establish a safe signing/submission domain.
async fn network_identity(api: &ApiClient) -> Result<(ChainIdentity, [u8; 32])> {
    let (identity, genesis) = receiver_network_identity(api).await?;
    Ok((identity, genesis.ok_or_else(|| user_error("target genesis unavailable"))?))
}

/// A fresh receiver reports exactly "unknown" before installing block 1.
/// Only snapshot ingest may defer that comparison to its independent genesis
/// pin check; source snapshots and retained receipt submissions stay strict.
async fn receiver_network_identity(api: &ApiClient) -> Result<(ChainIdentity, Option<[u8; 32]>)> {
    let info = api.receipt_request("GET", "/network/info", None).await?;
    let field = |name: &str| info[name].as_str().filter(|v| !v.trim().is_empty())
        .ok_or_else(|| user_error("target network identity unavailable"));
    let identity = ChainIdentity::new(field("chain_id")?, field("network_id")?);
    anyhow::ensure!(identity.chain_id != "unknown" && identity.network_id != "unknown", "target network identity unavailable");
    let genesis = match field("genesis_hash")? {
        "unknown" => None,
        hash => {
            let hash = super::node_deposit::api_hash(hash)
                .ok_or_else(|| user_error("target genesis unavailable"))?;
            anyhow::ensure!(hash != [0; 32], "target genesis unavailable");
            Some(hash)
        },
    };
    Ok((identity, genesis))
}

/// Pure verification core: the payload must be signed by a trusted signer
/// and the entries must reproduce its root. Returns the decoded entries.
pub fn verify_fetched_snapshot(
    signed: &SignedSnapshotSyncPayload,
    trusted_signers: &HashSet<String>,
    entries: &[(String, String)],
    identity: &ChainIdentity,
) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
    if trusted_signers.is_empty() {
        return Err(user_error(
            "no trusted signers given (--trusted-signers or WRFCOIN_SNAPSHOT_TRUSTED_SIGNERS); refusing to trust whoever signed",
        ));
    }
    if !trusted_signers.contains(&signed.signer_id) {
        return Err(user_error(format!(
            "snapshot signer {} is not in the trusted signer list; refusing",
            signed.signer_id
        )));
    }
    if !signed.has_valid_signature_for(identity) {
        return Err(user_error(
            "snapshot payload signature does not verify; refusing",
        ));
    }
    if entries.len() as u64 != signed.payload.entry_count {
        return Err(user_error(format!(
            "export returned {} entries but the signed payload commits to {}; refusing",
            entries.len(),
            signed.payload.entry_count
        )));
    }
    let mut decoded = Vec::with_capacity(entries.len());
    for (idx, (key_hex, value_hex)) in entries.iter().enumerate() {
        let key = hex::decode(key_hex.trim())
            .map_err(|_| user_error(format!("entries[{idx}].key_hex is not hex")))?;
        let value = hex::decode(value_hex.trim())
            .map_err(|_| user_error(format!("entries[{idx}].value_hex is not hex")))?;
        decoded.push((key, value));
    }
    let rebuilt = build_state_snapshot(
        signed.payload.height,
        signed.payload.state_root_hex.clone(),
        &decoded,
    );
    if rebuilt.snapshot_root_hex != signed.payload.snapshot_root_hex {
        return Err(user_error(format!(
            "exported entries reproduce root {} but the signed payload names {}; refusing",
            rebuilt.snapshot_root_hex, signed.payload.snapshot_root_hex
        )));
    }
    Ok(decoded)
}

const MAX_NODE_PUBLIC_ARTIFACT_BYTES: usize = 40 * 1024;
fn read_public_artifact(path: &std::path::Path) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?.take(MAX_NODE_PUBLIC_ARTIFACT_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= MAX_NODE_PUBLIC_ARTIFACT_BYTES, "Node artifact exceeds bound");
    Ok(bytes)
}
fn bounded_claim_hex(response: &Value) -> Result<String> {
    use wrfcoin_shared_protocol::consensus::node_share_contracts::{BondedServiceClaim, MAX_NODE_CLAIM_BYTES};
    let claim = response.get("claim_hex").and_then(Value::as_str).ok_or_else(|| user_error("missing bonded claim"))?;
    anyhow::ensure!(claim.len() <= 2 * MAX_NODE_CLAIM_BYTES, "bonded claim exceeds bound");
    let claim = BondedServiceClaim::from_wire(&hex::decode(claim)?)?;
    Ok(hex::encode(claim.to_wire()?))
}

/// Public signed artifacts only, fsync before acknowledgement/submission.
fn archive_public_receipt(directory: &std::path::Path, stage: &str, body: &Value) -> Result<()> {
    use std::io::Write;
    let bytes = serde_json::to_vec(body)?;
    if bytes.len() > MAX_NODE_PUBLIC_ARTIFACT_BYTES { anyhow::bail!("receipt archive exceeds bound"); }
    std::fs::create_dir_all(directory)?;
    let digest = wrfcoin_shared_protocol::crypto::hash::hash_bytes(&bytes).to_hex();
    let path = directory.join(format!("{stage}-{digest}.json"));
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => { file.write_all(&bytes)?; file.sync_all()?; std::fs::File::open(directory)?.sync_all()?; },
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            anyhow::ensure!(read_public_artifact(&path)? == bytes, "archive collision");
        },
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

pub async fn handle_snapshot_command(
    command: &SnapshotCommands,
    _ctx: &ExecutionContext,
) -> Result<Value> {
    match command {
        SnapshotCommands::SubmitReceipt { transaction_file } => {
            let bytes = read_public_artifact(transaction_file)?;
            let tx: wrfcoin_shared_protocol::blockchain::transaction::Transaction = serde_json::from_slice(&bytes)?;
            let submission = wrfcoin_shared_protocol::consensus::node_receipt_registry::NodeReceiptSubmission::parse(&tx)
                .ok_or_else(|| user_error("invalid Node receipt transaction"))?;
            // Explicit retry reuses the public packet and still sends only once.
            let api = ApiClient::new_without_redirects(ApiConfig::default())?;
            let (identity, genesis) = network_identity(&api).await?;
            submission.verify_authorization(&identity, &genesis)
                .map_err(|_| user_error("receipt target chain/genesis mismatch"))?;
            let ack = api.receipt_request("POST", "/node/receipts/submit", Some(serde_json::to_value(&tx)?)).await
                .map_err(|_| user_error("UNCERTAIN; retained packet requires canonical inspection before another explicit retry"))?;
            let tx_id = tx.id.map(|id| id.to_hex());
            if ack["admitted"] != true || ack["tx_id"].as_str() != tx_id.as_deref() {
                return Err(user_error("UNCERTAIN; receipt acknowledgement unavailable"));
            }
            Ok(json!({"tx_id":tx_id,"admitted":true,"included":false}))
        },
        SnapshotCommands::Fetch {
            from,
            to,
            trusted_signers,
            page_size,
            dry_run,
            server_id,
            bonded,
            auth_stdin,
            receipt_archive,
            ingest_timeout_secs,
        } => {
            let p2p_addr = p2p_from(from)?;
            if p2p_addr.is_some() && !*bonded {
                return Err(user_error("a P2P server serves bonded consumers only; add --bonded"));
            }
            if p2p_addr.is_some() && to.is_none() {
                return Err(user_error("a P2P fetch needs --to: the receiver signs the fetch authorization"));
            }
            if *auth_stdin && to.is_none() { return Err(user_error("stdin credentials require an explicit receiver URL")); }
            let auth = if *auth_stdin { Some(read_snapshot_auth(p2p_addr.is_none())?) } else { None };
            if *bonded && (*dry_run || receipt_archive.is_none()) {
                return Err(user_error("bonded fetch requires actual installation and --receipt-archive").into());
            }
            let trusted: HashSet<String> = trusted_signers
                .iter()
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .collect();
            let mut server = match &p2p_addr {
                None => SnapshotServer::Http(client_with_role(from, auth.as_ref().map(|a| a.server_reader.as_str()))?),
                Some(addr) => {
                    let receiver = match to {
                        Some(url) => client_with_role(url, auth.as_ref().map(|a| a.receiver_operator.as_str()))?,
                        None => ApiClient::new(ApiConfig::default())?,
                    };
                    let client = bonded_fetch::client::BondedFetchClient::connect(addr.clone(), P2P_CONNECT_TIMEOUT)
                        .await
                        .map_err(user_error)?;
                    SnapshotServer::P2p(Box::new(P2pServer { client, receiver, auth: None, service_id: None }))
                },
            };
            let source_identity = server.identity().await?;

            // 1. The signed payload — refused unless trusted.
            let payload: PayloadResponse = server.sync_payload().await?;
            let signed = payload.signed_payload.ok_or_else(|| {
                user_error("serving node returned an unsigned snapshot payload; refusing")
            })?;
            let signed = SignedSnapshotSyncPayload {
                signer_id: signed.signer_id.to_lowercase(),
                ..signed
            };
            if signed.payload != payload.payload {
                return Err(user_error(
                    "signed payload does not match the served payload; refusing",
                ));
            }

            // 2. The values — every page bound to the signed height and root.
            let page_size = (*page_size).clamp(1, 1_000);
            let mut offset = 0usize;
            let mut entries: Vec<(String, String)> = Vec::new();
            loop {
                let page: ExportPage = server.export_page(offset, page_size, &signed.payload).await?;
                if page.height != signed.payload.height
                    || page.snapshot_root_hex != signed.payload.snapshot_root_hex
                {
                    return Err(user_error(format!(
                        "export page at offset {offset} is from height {} root {} but the signed payload is height {} root {}; the server's state moved — retry",
                        page.height,
                        page.snapshot_root_hex,
                        signed.payload.height,
                        signed.payload.snapshot_root_hex
                    )));
                }
                entries.extend(
                    page.entries
                        .iter()
                        .map(|e| (e.key_hex.clone(), e.value_hex.clone())),
                );
                match page.next_offset {
                    Some(next) if next > offset && entries.len() < page.total => offset = next,
                    _ => break,
                }
            }

            // 3. Local verification before anything touches the local node.
            let decoded = verify_fetched_snapshot(&signed, &trusted, &entries, &source_identity.0)?;
            let verified = json!({
                "from": from,
                "height": signed.payload.height,
                "snapshot_root_hex": signed.payload.snapshot_root_hex,
                "entry_count": decoded.len(),
                "signer_id": signed.signer_id,
            });
            if *dry_run {
                return Ok(
                    json!({ "summary": "Snapshot verified (dry run, not ingested)", "verified": verified }),
                );
            }

            // 4. Ingest into the local node (which re-verifies and root-checks).
            let local = match to {
                Some(url) => client_with_role(url, auth.as_ref().map(|a| a.receiver_operator.as_str()))?,
                None => ApiClient::new(ApiConfig::default())?,
            };
            let receiver_identity = receiver_network_identity(&local).await?;
            anyhow::ensure!(receiver_identity.0 == source_identity.0
                && receiver_identity.1.is_none_or(|hash| hash == source_identity.1),
                "snapshot source/receiver chain/genesis mismatch");
            // A fresh receiver's unknown genesis is checked by ingest against
            // its independent pin before carried authority or state is installed.
            let server_id = server_id.clone().unwrap_or_else(|| server.default_server_id(from));
            let ingest_body = json!({
                "signed_payload": signed,
                "entries": entries.iter().map(|(k, v)| json!({"key_hex": k, "value_hex": v})).collect::<Vec<_>>(),
                "served_by": server_id,
            });
            let ingest_client = match to {
                Some(url) => {
                    let mut config = ApiConfig { base_url: url.trim_end_matches('/').to_string(), ..ApiConfig::default() };
                    if let Some(a) = auth.as_ref() { config.bearer_token = Some(a.receiver_operator.clone()); }
                    ApiClient::new(ingest_config(config, *ingest_timeout_secs))?
                }
                None => ApiClient::new(ingest_config(ApiConfig::default(), *ingest_timeout_secs))?,
            };
            let ingest: Value = ingest_client
                .post(if *bonded { "/network/snapshot-sync/receiver-ingest" } else { "/network/snapshot-sync/ingest" }, Some(ingest_body))
                .await?;

            // Archive before the expiring server acknowledgement or endorsement.
            let mut bonded_result = Value::Null;
            if let Some(receipt) = ingest.get("receipt").filter(|r| !r.is_null()) {
                let receipt_value = if *bonded || receipt_archive.is_some() {
                    let typed: wrfcoin_shared_protocol::consensus::node_service_receipt::SignedNodeServiceReceipt = serde_json::from_value(receipt.clone())?;
                    serde_json::to_value(typed)?
                } else { receipt.clone() };
                let installed = json!({"receipt":receipt_value,"manifest":signed});
                if let Some(directory) = receipt_archive {
                    archive_public_receipt(directory, "installed", &installed)?;
                }
                if *bonded {
                    let ticket: Value = server.bonded_ticket(installed).await?;
                    let partial = json!({"claim_hex":bounded_claim_hex(&ticket)?});
                    let endorsed: Value = local.post("/network/snapshot-sync/bonded-endorsement", Some(partial)).await?;
                    let claim_hex = bounded_claim_hex(&endorsed)?;
                    let typed = wrfcoin_shared_protocol::consensus::node_share_contracts::BondedServiceClaim::from_wire(&hex::decode(&claim_hex)?)?;
                    let receipt_id = hex::encode(wrfcoin_shared_protocol::consensus::node_receipt_registry::claim_hash(&typed)?);
                    let endorsed = json!({"claim_hex":claim_hex,"receipt_id":receipt_id,"included":false});
                    archive_public_receipt(receipt_archive.as_ref().expect("checked"), "bonded", &endorsed)?;
                    bonded_result = endorsed;
                }
            } else if *bonded {
                return Err(user_error("installation returned no signed receipt; bonded evidence unavailable").into());
            }
            // 5. Hand the receipt back to the server, when the local node issued one.
            let receipt_status = match ingest.get("receipt").filter(|r| !r.is_null()) {
                Some(receipt) => match server.deliver_receipt(receipt).await {
                    Ok(ack) => json!({ "delivered": true, "server_ack": ack }),
                    Err(err) => json!({ "delivered": false, "error": err.to_string() }),
                },
                None => json!({
                    "delivered": false,
                    "reason": ingest.get("receipt_unavailable_reason").cloned().unwrap_or(Value::Null),
                }),
            };

            Ok(json!({
                "summary": format!("Snapshot at height {} ingested into the local node", signed.payload.height),
                "verified": verified,
                "ingest": ingest,
                "receipt": receipt_status,
                "bonded_claim": bonded_result,
            }))
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;
    use wrfcoin_shared_protocol::blockchain::snapshot_sync::{
        build_snapshot_sync_payload, sign_snapshot_sync_payload,
    };
    use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

    fn fixture() -> (SignedSnapshotSyncPayload, Vec<(String, String)>, String) {
        let entries_raw = vec![
            (b"account:alice".to_vec(), b"100".to_vec()),
            (b"account:bob".to_vec(), b"7".to_vec()),
        ];
        let snapshot = build_state_snapshot(42, "ab".repeat(32), &entries_raw);
        let payload = build_snapshot_sync_payload(&snapshot, "cd".repeat(32), snapshot.height);
        let sk = PrivateKey::generate(&mut StdRng::seed_from_u64(3126));
        let signer_id = PublicKey::from_private_key(&sk).to_hex().to_lowercase();
        let signed = sign_snapshot_sync_payload(payload, &sk, 1_000, "nonce-3126".to_string());
        // Keep a scheduling gap in this regression fixture to exercise the
        // lock's guarantee that the signing domain stays stable until verification.
        std::thread::sleep(std::time::Duration::from_millis(10));
        let entries = entries_raw
            .iter()
            .map(|(k, v)| (hex::encode(k), hex::encode(v)))
            .collect();
        (signed, entries, signer_id)
    }

    #[test]
    fn from_selects_p2p_only_for_a_multiaddr_naming_the_server_peer() {
        assert!(p2p_from("http://seed:8556").unwrap().is_none());
        assert!(p2p_from("https://seed.example").unwrap().is_none());
        let peer = libp2p::identity::Keypair::generate_ed25519().public().to_peer_id();
        let addr = p2p_from(&format!("/dns4/laptop.example/tcp/9000/p2p/{peer}")).unwrap().unwrap();
        assert_eq!(bonded_fetch::client::server_peer_id(&addr).unwrap(), peer);
        assert!(p2p_from(&format!("/ip4/203.0.113.7/tcp/9000/p2p/{peer}")).unwrap().is_some());
        // A P2P address without the peer id cannot authenticate the server.
        assert!(p2p_from("/ip4/203.0.113.7/tcp/9000").is_err());
        assert!(p2p_from("/not-a-multiaddr").is_err());
    }

    #[test]
    fn stdin_credentials_need_the_server_reader_only_for_http() {
        let both = br#"{"server_reader":"r","receiver_operator":"o"}"#;
        let receiver_only = br#"{"receiver_operator":"o"}"#;
        assert!(parse_snapshot_auth(both, true).is_ok());
        assert!(parse_snapshot_auth(receiver_only, true).is_err());
        assert_eq!(parse_snapshot_auth(receiver_only, false).unwrap().receiver_operator, "o");
        assert!(parse_snapshot_auth(br#"{"server_reader":"r"}"#, false).is_err());
        assert!(parse_snapshot_auth(br#"{"server_reader":"r","receiver_operator":" "}"#, false).is_err());
        assert!(parse_snapshot_auth(&vec![b' '; 16 * 1024 + 1], false).is_err());
    }

    #[test]
    fn ingest_config_is_single_shot_with_bounded_timeout() {
        let base = ApiConfig::default();
        assert_eq!(base.retry_attempts, 5, "global default must stay untouched");
        let cfg = ingest_config(base, 900);
        assert_eq!(cfg.retry_attempts, 0);
        assert_eq!(cfg.timeout, std::time::Duration::from_secs(900));
        assert_eq!(ingest_config(ApiConfig::default(), 999_999).timeout, std::time::Duration::from_secs(3600));
        assert_eq!(ingest_config(ApiConfig::default(), 0).timeout, std::time::Duration::from_secs(1));
    }

    #[tokio::test]
    async fn ingest_post_is_sent_exactly_once_on_transport_failure() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = counter.clone();
        std::thread::spawn(move || {
            // Accept and drop every connection: each is a failed POST.
            for conn in listener.incoming() {
                if conn.is_ok() { seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst); }
            }
        });
        let mut config = ApiConfig { base_url: format!("http://127.0.0.1:{port}"), ..ApiConfig::default() };
        config.retry_delay = std::time::Duration::from_millis(10);
        let client = ApiClient::new(ingest_config(config, 5)).unwrap();
        let result = client
            .post::<Value>("/network/snapshot-sync/receiver-ingest", Some(json!({"entries":[]})))
            .await;
        assert!(result.is_err());
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 1, "ingest must not be retried");
    }

    #[test]
    fn ingest_timeout_flag_parses_defaults_and_is_bounded() {
        use clap::Parser;
        #[derive(Parser)]
        struct Cli { #[clap(subcommand)] cmd: SnapshotCommands }
        let parse = |extra: &[&str]| {
            let mut args = vec!["x", "fetch", "--from", "http://s:1"];
            args.extend_from_slice(extra);
            Cli::try_parse_from(args)
        };
        let timeout = |c: Cli| match c.cmd { SnapshotCommands::Fetch { ingest_timeout_secs, .. } => ingest_timeout_secs, _ => panic!() };
        assert_eq!(timeout(parse(&[]).unwrap()), 30);
        assert_eq!(timeout(parse(&["--ingest-timeout-secs", "3600"]).unwrap()), 3600);
        assert!(parse(&["--ingest-timeout-secs", "3601"]).is_err());
        assert!(parse(&["--ingest-timeout-secs", "0"]).is_err());
    }

    #[test]
    fn public_archive_is_durable_idempotent_and_refuses_tampered_or_oversized_files() {
        let directory = std::env::temp_dir().join(format!("wrfcoin-node-public-archive-{}", uuid::Uuid::new_v4()));
        let body = json!({"claim_hex":"00","included":false});
        archive_public_receipt(&directory, "bonded", &body).unwrap();
        archive_public_receipt(&directory, "bonded", &body).unwrap();
        let files: Vec<_> = std::fs::read_dir(&directory).unwrap().map(|e|e.unwrap().path()).collect();
        assert_eq!(files.len(), 1);
        assert_eq!(serde_json::from_slice::<Value>(&read_public_artifact(&files[0]).unwrap()).unwrap(), body);
        std::fs::write(&files[0], b"tampered public artifact").unwrap();
        assert!(archive_public_receipt(&directory, "bonded", &body).is_err());
        std::fs::write(&files[0], vec![0; MAX_NODE_PUBLIC_ARTIFACT_BYTES + 1]).unwrap();
        assert!(read_public_artifact(&files[0]).is_err());
        assert!(archive_public_receipt(&directory, "bonded", &body).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_trusted_signed_snapshot_whose_entries_reproduce_the_root_is_accepted() {
        let _domain = crate::crypto::SIGNING_ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (signed, entries, signer) = fixture();
        let trusted: HashSet<String> = [signer].into_iter().collect();
        let decoded = verify_fetched_snapshot(&signed, &trusted, &entries, ChainIdentity::active()).unwrap();
        assert_eq!(decoded.len(), 2);
    }

    #[test]
    fn untrusted_signer_wrong_root_and_missing_entries_are_refused_client_side() {
        let _domain = crate::crypto::SIGNING_ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (signed, entries, signer) = fixture();
        // Untrusted signer.
        let other: HashSet<String> = ["ab".repeat(32)].into_iter().collect();
        assert!(verify_fetched_snapshot(&signed, &other, &entries, ChainIdentity::active()).is_err());
        // No trusted signers at all.
        assert!(verify_fetched_snapshot(&signed, &HashSet::new(), &entries, ChainIdentity::active()).is_err());
        let trusted: HashSet<String> = [signer].into_iter().collect();
        // A tampered value no longer reproduces the signed root.
        let mut tampered = entries.clone();
        tampered[1].1 = hex::encode(b"8");
        assert!(verify_fetched_snapshot(&signed, &trusted, &tampered, ChainIdentity::active()).is_err());
        // A missing entry: count mismatch.
        assert!(verify_fetched_snapshot(&signed, &trusted, &entries[..1], ChainIdentity::active()).is_err());
        // A tampered payload (root swapped) fails the signature.
        let mut forged = signed.clone();
        forged.payload.snapshot_root_hex = "cd".repeat(32);
        assert!(verify_fetched_snapshot(&forged, &trusted, &entries, ChainIdentity::active()).is_err());
    }
}
