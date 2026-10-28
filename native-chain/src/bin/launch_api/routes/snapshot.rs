use actix_web::{HttpRequest, HttpResponse, Responder, http::StatusCode, web};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use crate::helpers::{
    bearer_token_from_request, envelope_error, require_role, validate_nonce, validate_timestamp,
};
use crate::routes::quality::{quality_band_label, quality_event_type_label};
use crate::state::{AppState, MAX_SNAPSHOT_PAGE_SIZE, RECENT_DA_BLOCK_WINDOW};
pub use crate::state::snapshot_service::SnapshotServiceState;
use crate::state::snapshot_service::{
    SnapshotExportCut, ReceiptLedgerError, RECEIPT_FRESHNESS_WINDOW_SECS,
};
use wrfcoin_native_chain::blockchain::block::Block;
use wrfcoin_native_chain::blockchain::chain::{Blockchain, configured_max_in_memory_blocks};
use wrfcoin_native_chain::storage::{SNAPSHOT_INGEST_REQUIRES_FRESH_NODE, Storage, StorageError};
use wrfcoin_shared_protocol::blockchain::chain_snapshot::{
    ChainWindowError, block_entry_key, export_block_entries, split_snapshot_entries,
    verify_chain_window, window_floor,
};
use wrfcoin_shared_protocol::consensus::proof_of_publishing::MEDIAN_TIME_PAST_WINDOW;
use wrfcoin_shared_protocol::consensus::node_service_receipt::{
    snapshot_subject_hash, NodeService, NodeServiceReceipt, ReceiptError, SignedNodeServiceReceipt,
};
use wrfcoin_shared_protocol::crypto::chain_binding::{protocol_version_at, ChainIdentity};
use wrfcoin_native_chain::blockchain::snapshot_sync::{
    SignedSnapshotSyncPayload, SnapshotEntry, SnapshotInclusionProof, SnapshotRootBuilder,
    SnapshotSyncPayload,
    build_inclusion_proof, build_snapshot_sync_payload, build_state_snapshot,
    sign_snapshot_sync_payload_with_signer, verify_inclusion_proof,
};
use wrfcoin_native_chain::blockchain::state::State;
use wrfcoin_shared_protocol::blockchain::state_snapshot::encode_snapshot_value;
use wrfcoin_native_chain::crypto::hash::hash_bytes;
use wrfcoin_native_chain::security::api_auth::ApiRole;

// ---------------------------------------------------------------------------
// #2119: conflicting-snapshot blacklist
// ---------------------------------------------------------------------------

/// Snapshot roots that failed verification on ingest. A root that did not
/// reproduce from its entries, or whose applied state did not round-trip, is
/// refused on every later ingest **before any decoding or verification work**
/// — the conflicting-snapshot defence the adversarial harness exercises: a
/// bad snapshot costs this node its verification once, never repeatedly.
///
/// Process-wide (not `AppState`, which the boot path constructs) and never
/// pruned: a root is content-addressed, so a rejected one is rejected forever.
fn rejected_snapshot_roots() -> &'static Mutex<HashSet<String>> {
    static ROOTS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    ROOTS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn normalise_root(root: &str) -> String {
    root.trim().to_ascii_lowercase()
}

/// Record a snapshot root that failed verification.
pub fn blacklist_snapshot_root(root: &str) {
    rejected_snapshot_roots()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(normalise_root(root));
}

/// Has this root already failed verification on this node?
pub fn snapshot_root_blacklisted(root: &str) -> bool {
    rejected_snapshot_roots()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .contains(&normalise_root(root))
}

/// The blacklist key for a signed snapshot whose entries are fine but whose
/// SIGNED `state_root_hex` is not the root they produce (#3288 3b, review N1).
///
/// Under format v2 the applied state, and so its root, is a pure function of
/// the entries. A snapshot root paired with a signed state root it does not
/// produce is therefore conflicting on its own, on every node, forever: it is
/// blacklisted. The key is the PAIR, not the snapshot root alone, because the
/// same entries re-signed with the root they do produce are an honest snapshot
/// that must stay installable.
pub fn state_root_pair_key(snapshot_root_hex: &str, signed_state_root_hex: &str) -> String {
    format!(
        "{}+state_root:{}",
        normalise_root(snapshot_root_hex),
        normalise_root(signed_state_root_hex)
    )
}

/// The roots refused so far (operator diagnostics and tests).
pub fn blacklisted_snapshot_roots() -> Vec<String> {
    let mut roots: Vec<String> = rejected_snapshot_roots()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .cloned()
        .collect();
    roots.sort();
    roots
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct SnapshotSummaryResponse {
    pub height: u64,
    pub state_root_hex: String,
    pub snapshot_root_hex: String,
    pub entry_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SnapshotEntriesQuery {
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SnapshotEntriesResponse {
    pub height: u64,
    pub state_root_hex: String,
    pub snapshot_root_hex: String,
    pub offset: usize,
    pub limit: usize,
    pub total: usize,
    pub next_offset: Option<usize>,
    pub entries: Vec<SnapshotEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SnapshotProofResponse {
    pub height: u64,
    pub state_root_hex: String,
    pub proof: SnapshotInclusionProof,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SnapshotVerifyInclusionRequest {
    pub proof: SnapshotInclusionProof,
    pub key_hex: String,
    pub value_hex: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SnapshotVerifySignedPayloadRequest {
    pub signed_payload: SignedSnapshotSyncPayload,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SnapshotEntryKV {
    pub key_hex: String,
    pub value_hex: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SnapshotSyncIngestRequest {
    pub signed_payload: SignedSnapshotSyncPayload,
    /// Full `(key, value)` snapshot entries to reconstruct remote state.
    ///
    /// Hex-encoded so the payload survives JSON transport. The Merkle root
    /// built from these entries must equal
    /// `signed_payload.payload.snapshot_root_hex`; otherwise ingest aborts
    /// before touching local state (#1405).
    #[serde(default)]
    pub entries: Vec<SnapshotEntryKV>,
    /// The server the consumer fetched this snapshot from (its service id);
    /// when present and this node has a signer, the response carries a
    /// consumer-signed `NodeServiceReceipt` naming it (Lane C / #2848).
    #[serde(default)]
    pub served_by: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SnapshotSyncPayloadResponse {
    pub payload: SnapshotSyncPayload,
    pub signed_payload: Option<SignedSnapshotSyncPayload>,
}

/// One page of `GET /state/snapshot/export`: the entry VALUES a joiner needs
/// to ingest, bound to the height and roots of the snapshot they were cut
/// from (Lane C of the join loop; the serve half #2848 will settle).
#[derive(Debug, Serialize, Deserialize)]
pub struct SnapshotExportResponse {
    pub height: u64,
    pub state_root_hex: String,
    pub snapshot_root_hex: String,
    pub offset: usize,
    pub limit: usize,
    pub total: usize,
    pub next_offset: Option<usize>,
    pub entries: Vec<SnapshotEntryKV>,
}

/// `POST /network/snapshot-sync/receipt` body.
#[derive(Debug, Serialize, Deserialize)]
pub struct SnapshotServiceReceiptRequest {
    pub receipt: SignedNodeServiceReceipt,
}

async fn snapshot_export_cut(
    state: &AppState,
    blockchain: &Blockchain,
) -> Result<Arc<SnapshotExportCut>, String> {
    // Serialize cache misses, including construction; concurrent Readers build once.
    let mut cache = state.snapshot_service.export.lock().await;
    if let Some(cut) = cache.as_ref().filter(|cut| cut.matches_chain(blockchain)) {
        // Refresh the descriptor when this cut is actually served again.
        // determinism-ok: process-local served-subject retention, never a consensus input.
        state.snapshot_service.remember_cut(cut, Instant::now());
        return Ok(Arc::clone(cut));
    }
    let state_root_hex = recomputed_state_root_hex(&blockchain.state);
    let storage = state.storage.read().await;
    let mut entries = current_snapshot_entries(blockchain, &storage)?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let snapshot = build_state_snapshot(blockchain.height, state_root_hex.clone(), &entries);
    let cut = Arc::new(SnapshotExportCut {
        height: blockchain.height,
        tip_hash_hex: blockchain.tip.to_hex(),
        finalized_height: blockchain.finalized_height(),
        stored_state_root: hex::encode(blockchain.state.state_root.0),
        state_root_hex,
        snapshot_root_hex: snapshot.snapshot_root_hex,
        entries,
    });
    #[cfg(test)]
    state.snapshot_service.export_builds.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // determinism-ok: process-local served-subject retention, never a consensus input.
    state.snapshot_service.remember_cut(&cut, Instant::now());
    *cache = Some(Arc::clone(&cut));
    Ok(cut)
}

/// The identity a receipt names this server by: the configured external
/// service id, or the API host:port.
pub fn server_service_id() -> String {
    // determinism-ok: operator-facing identity string for service receipts; never a consensus input
    std::env::var("WRFCOIN_SERVICE_ID")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| {
            // determinism-ok: operator-facing identity string for service receipts; never a consensus input
            let host = std::env::var("WRFCOIN_API_HOST").unwrap_or_else(|_| "127.0.0.1".into());
            // determinism-ok: operator-facing identity string for service receipts; never a consensus input
            let port = std::env::var("WRFCOIN_API_PORT").unwrap_or_else(|_| "8556".into());
            format!("{host}:{port}")
        })
}

/// Build the receipt a consumer signs after a verified ingest.
pub fn build_snapshot_join_receipt(
    server_id: &str,
    consumer_pubkey: [u8; 32],
    height: u64,
    snapshot_root_hex: &str,
    entry_count: u64,
    timestamp: i64,
) -> NodeServiceReceipt {
    let mut nonce_material = Vec::with_capacity(64);
    nonce_material.extend_from_slice(&timestamp.to_be_bytes());
    nonce_material.extend_from_slice(snapshot_root_hex.as_bytes());
    nonce_material.extend_from_slice(server_id.as_bytes());
    NodeServiceReceipt {
        service: NodeService::SnapshotJoin,
        server_id: server_id.to_string(),
        consumer_pubkey,
        chain_height: height,
        subject_hash: snapshot_subject_hash(snapshot_root_hex),
        units: entry_count,
        timestamp,
        nonce: *hash_bytes(&nonce_material).as_bytes(),
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// The snapshot entries for `state`: format v2, the whole serialized `State`
/// (#3288 fix 3a). See `wrfcoin_shared_protocol::blockchain::state_snapshot`.
///
/// Fallible only for a value canonical CBOR cannot represent (a u128 above
/// i128::MAX). Callers surface that as an error, never a panic.
pub fn build_snapshot_entries(state: &State) -> Result<Vec<(Vec<u8>, Vec<u8>)>, String> {
    wrfcoin_shared_protocol::blockchain::state_snapshot::export_state_entries(state)
}

// ---------------------------------------------------------------------------
// #3288 3b: the block window a snapshot carries
// ---------------------------------------------------------------------------

/// Blocks, counting the tip, that the oracle pending-attestation lane reads
/// back to: it recovers each still-unattested mined reveal's payload from the
/// reveal's own block (`routes/oracle.rs::mined_reveal_payload`). Bounded by
/// `APPLIED_REVEAL_RETENTION_BLOCKS`, after which reveals are pruned.
fn pending_reveal_lookback(state: &State, height: u64) -> u64 {
    state
        .pending_reveals
        .values()
        .map(|reveal| reveal.revealed_at)
        .filter(|&revealed_at| revealed_at >= 1 && revealed_at <= height)
        .min()
        .map_or(0, |oldest| height - oldest + 1)
}

/// Every reader of blocks below the tip that a fast-synced node runs, and how
/// many blocks (counting the tip) each needs at `height` (#3288 3b).
///
/// Deliberately NOT here:
/// - reorg / fork choice (`MAX_REORG_DEPTH`): an ingested node holds no state
///   below the snapshot, so it finalizes at the snapshot height and refuses
///   any fork below it (see `ingest_snapshot_sync_payload`). Forks at or above
///   the snapshot height use blocks the node itself received.
/// - BFT finality signals (`finality_events.rs`): a signal at or below the
///   snapshot height cannot advance a finalized height that is already there.
/// - display metrics over the in-memory window (`/mining/stats`' last 20
///   blocks, the trust and privacy counters): they fill as blocks arrive and
///   gate nothing.
pub(crate) fn snapshot_window_consumers(state: &State, height: u64) -> [(&'static str, u64); 3] {
    [
        (
            "median-time-past (PoP validate_block and the producer's timestamp)",
            MEDIAN_TIME_PAST_WINDOW as u64,
        ),
        ("DA challenge producer (RECENT_DA_BLOCK_WINDOW)", RECENT_DA_BLOCK_WINDOW as u64),
        (
            "oracle pending-attestation lane (mined reveal payloads)",
            pending_reveal_lookback(state, height),
        ),
    ]
}

/// K: the number of blocks, counting the tip, a snapshot at `height` must
/// carry above genesis. The max over [`snapshot_window_consumers`].
pub(crate) fn snapshot_block_window(state: &State, height: u64) -> u64 {
    snapshot_window_consumers(state, height)
        .iter()
        .map(|(_, blocks)| *blocks)
        .max()
        .unwrap_or(1)
        .max(1)
}

/// The blocks a snapshot of `blockchain` carries: the height-1 genesis block
/// and the window `[window_floor ..= height]`, from memory where the window
/// still holds them and from the durable archive otherwise.
///
/// None at height 0: there is no block yet. That snapshot still serves the
/// inspection routes (summary, entries, proofs), and ingest refuses it
/// (`ChainWindowError::NoTip`).
fn snapshot_chain_blocks(blockchain: &Blockchain, storage: &Storage) -> Result<Vec<Block>, String> {
    let height = blockchain.height;
    if height == 0 {
        return Ok(Vec::new());
    }
    let floor = window_floor(height, snapshot_block_window(&blockchain.state, height));
    let lowest_in_memory = blockchain.blocks().keys().next().copied().unwrap_or(u64::MAX);
    let mut archived = std::collections::BTreeMap::new();
    // Genesis and the window may be far apart: read them separately, and only
    // the parts the in-memory window no longer holds.
    for (start, end) in [(1, 1), (floor, height.min(lowest_in_memory.saturating_sub(1)))] {
        if start > end || start >= lowest_in_memory {
            continue;
        }
        for block in storage
            .load_blocks_range(start, end)
            .map_err(|e| format!("archive read {start}..={end} failed: {e}"))?
        {
            archived.insert(block.height, block);
        }
    }
    std::iter::once(1)
        .chain(floor..=height)
        .map(|h| {
            blockchain
                .block_at_height(h)
                .cloned()
                .or_else(|| archived.get(&h).cloned())
                .ok_or_else(|| format!("block {h} is in neither the window nor the archive"))
        })
        .collect()
}

fn verify_snapshot_sync_trust(
    req: &HttpRequest,
    state: &AppState,
    signed_payload: &SignedSnapshotSyncPayload,
) -> Result<(), HttpResponse> {
    let trusted_signers = state.snapshot_trusted_signers.as_ref();
    if trusted_signers.is_empty() {
        return Err(envelope_error(
            req,
            StatusCode::SERVICE_UNAVAILABLE,
            "SNAPSHOT_TRUST_NOT_CONFIGURED",
            "snapshot trusted signer allow-list is not configured",
        ));
    }
    if !trusted_signers.contains(&signed_payload.signer_id) {
        return Err(envelope_error(
            req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "signed_payload signer is not in trusted signer allow-list",
        ));
    }
    if !signed_payload.has_valid_signature_from_trusted_signers(trusted_signers) {
        return Err(envelope_error(
            req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "signed_payload signature verification failed",
        ));
    }
    Ok(())
}

/// `state_root` as `State::recalculate_state_root` would set it, without
/// mutating: SHA3-256 over `state_root_material()`, first 32 bytes.
pub fn recomputed_state_root_hex(state: &State) -> String {
    use sha3::{Digest, Sha3_256};
    let digest = Sha3_256::digest(state.state_root_material());
    hex::encode(&digest[..32])
}

/// A snapshot of `blockchain` as served: the state entries plus the chain
/// entries (#3288 3b), the root it signs, and the tip it was taken at.
struct CurrentSnapshot {
    state_root_hex: String,
    snapshot: wrfcoin_native_chain::blockchain::snapshot_sync::StateSnapshot,
    tip_hash_hex: String,
    finalized_height: u64,
}

impl CurrentSnapshot {
    fn payload(&self) -> SnapshotSyncPayload {
        build_snapshot_sync_payload(&self.snapshot, self.tip_hash_hex.clone(), self.finalized_height)
    }
}

/// The raw `(key, value)` entries of a snapshot of `blockchain`: the state
/// entries, then the chain entries (#3288 3b).
fn current_snapshot_entries(
    blockchain: &Blockchain,
    storage: &Storage,
) -> Result<Vec<(Vec<u8>, Vec<u8>)>, String> {
    let mut entries = build_snapshot_entries(&blockchain.state)?;
    entries.extend(export_block_entries(&snapshot_chain_blocks(blockchain, storage)?)?);
    Ok(entries)
}

/// Test access to what this node would serve: the payload (unsigned) and the
/// raw entries an ingesting node submits. Used by the p2p harness.
#[cfg(all(test, feature = "p2p"))]
pub(crate) fn current_snapshot_for_tests(
    blockchain: &Blockchain,
    storage: &Storage,
) -> Result<(SnapshotSyncPayload, Vec<(Vec<u8>, Vec<u8>)>), String> {
    Ok((current_snapshot(blockchain, storage)?.payload(), current_snapshot_entries(blockchain, storage)?))
}

/// #3551: one full snapshot build at a time, process-wide.
///
/// Every route that materialises the whole state (summary, entries, export,
/// proof, sync payload, sync-payload verify) takes this lock first, so N
/// concurrent Reader requests cost the peak of ONE build, not N. Nothing is
/// cached: state also changes off-block (bridge audit events, height-0
/// configuration writers), so neither height nor tip identifies it.
pub(crate) fn snapshot_build_slot() -> &'static tokio::sync::Mutex<()> {
    static SLOT: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    SLOT.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// Test access: the four summary fields as the full export computes them.
#[cfg(test)]
pub(crate) fn current_snapshot_summary_for_tests(
    blockchain: &Blockchain,
    storage: &Storage,
) -> Result<SnapshotSummaryResponse, String> {
    let current = current_snapshot(blockchain, storage)?;
    Ok(SnapshotSummaryResponse {
        height: current.snapshot.height,
        state_root_hex: current.state_root_hex,
        snapshot_root_hex: current.snapshot.snapshot_root_hex,
        entry_count: current.snapshot.entries.len(),
    })
}

/// #3551: archived blocks the summary reads per storage call.
const SUMMARY_ARCHIVE_CHUNK: u64 = 64;

/// #3551: visit, in order, the blocks [`snapshot_chain_blocks`] returns, holding
/// at most one archive chunk at a time instead of cloning the whole window.
/// Same blocks, same order, same errors (a block below the in-memory window
/// comes from the archive; one at or above it must be in memory).
fn for_each_snapshot_chain_block(
    blockchain: &Blockchain,
    storage: &Storage,
    mut visit: impl FnMut(&Block) -> Result<(), String>,
) -> Result<(), String> {
    let height = blockchain.height;
    if height == 0 {
        return Ok(());
    }
    let floor = window_floor(height, snapshot_block_window(&blockchain.state, height));
    let lowest_in_memory = blockchain.blocks().keys().next().copied().unwrap_or(u64::MAX);
    let archive_top = height.min(lowest_in_memory.saturating_sub(1));
    let mut chunk = std::collections::BTreeMap::new();
    for h in std::iter::once(1).chain(floor..=height) {
        if let Some(block) = blockchain.block_at_height(h) {
            visit(block)?;
            continue;
        }
        if h < lowest_in_memory && !chunk.contains_key(&h) {
            // Genesis is read alone, as `snapshot_chain_blocks` does.
            let end = if h == 1 { 1 } else { h.saturating_add(SUMMARY_ARCHIVE_CHUNK - 1).min(archive_top) };
            chunk = storage
                .load_blocks_range(h, end)
                .map_err(|e| format!("archive read {h}..={end} failed: {e}"))?
                .into_iter()
                .map(|block| (block.height, block))
                .collect();
        }
        let block = chunk
            .get(&h)
            .ok_or_else(|| format!("block {h} is in neither the window nor the archive"))?;
        visit(block)?;
    }
    Ok(())
}

/// #3551: what `GET /state/snapshot` reports, without building the
/// `StateSnapshot`. The recomputed state root's canonical material is hashed
/// and freed first; then each state entry and each window block is encoded,
/// pushed into a [`SnapshotRootBuilder`] (which keeps only key + leaf hash) and
/// dropped. Peak is the state entries plus one archive chunk, not the old
/// entries + blocks + encoded blocks + per-entry `SnapshotEntry` + cloned
/// Merkle layers. Identical to the fields of [`current_snapshot`] by test.
pub(crate) fn snapshot_summary(
    blockchain: &Blockchain,
    storage: &Storage,
) -> Result<SnapshotSummaryResponse, String> {
    let state_root_hex = recomputed_state_root_hex(&blockchain.state);
    let stored_root_hex = hex::encode(blockchain.state.state_root.0);
    if stored_root_hex != state_root_hex {
        log::error!(
            "snapshot summary at height {}: stored state_root {stored_root_hex} is stale; \
             reporting the recomputed root {state_root_hex} (#3288)",
            blockchain.height
        );
    }
    let mut builder = SnapshotRootBuilder::new();
    for (key, value) in build_snapshot_entries(&blockchain.state)? {
        builder.push(key, &value);
    }
    for_each_snapshot_chain_block(blockchain, storage, |block| {
        let value = encode_snapshot_value(block)
            .map_err(|e| format!("block {} is not encodable: {e}", block.height))?;
        builder.push(block_entry_key(block.height), &value);
        Ok(())
    })?;
    let (snapshot_root_hex, entry_count) = builder.finish();
    Ok(SnapshotSummaryResponse {
        height: blockchain.height,
        state_root_hex,
        snapshot_root_hex,
        entry_count,
    })
}

fn current_snapshot(blockchain: &Blockchain, storage: &Storage) -> Result<CurrentSnapshot, String> {
    // #3288 review: sign the root the exported state really has. Ingest now
    // requires the applied state's recomputed root to equal the signed one, so
    // a stale stored root would make every snapshot this node serves
    // unusable -- and would be a lie about the state it describes.
    let state_root_hex = recomputed_state_root_hex(&blockchain.state);
    let stored_root_hex = hex::encode(blockchain.state.state_root.0);
    if stored_root_hex != state_root_hex {
        log::error!(
            "snapshot export at height {}: stored state_root {stored_root_hex} is stale; \
             signing the recomputed root {state_root_hex} (#3288)",
            blockchain.height
        );
    }
    let entries = current_snapshot_entries(blockchain, storage)?;
    let snapshot = build_state_snapshot(blockchain.height, state_root_hex.clone(), &entries);
    Ok(CurrentSnapshot {
        state_root_hex,
        snapshot,
        tip_hash_hex: blockchain.tip.to_hex(),
        finalized_height: blockchain.finalized_height(),
    })
}

fn replay_key_for_sync_payload(payload: &SignedSnapshotSyncPayload) -> String {
    let material = format!(
        "{}|{}|{}|{}|{}|{}",
        payload.signer_id,
        payload.timestamp,
        payload.nonce,
        payload.payload.height,
        payload.payload.snapshot_root_hex,
        payload.signature
    );
    hash_bytes(material.as_bytes()).to_string()
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub async fn get_state_snapshot(
    req: HttpRequest,
    state: web::Data<AppState>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    // #3551: single flight, and the streamed summary instead of a full build.
    let _snapshot_build = snapshot_build_slot().lock().await;
    let blockchain = state.blockchain.read().await;
    let summary = {
        let storage = state.storage.read().await;
        snapshot_summary(&blockchain, &storage)
    };
    match summary {
        Ok(summary) => HttpResponse::Ok().json(summary),
        Err(e) => {
            log::error!("snapshot export failed: {e} (#3288)");
            envelope_error(
                &req,
                StatusCode::INTERNAL_SERVER_ERROR,
                "SNAPSHOT_EXPORT_FAILED",
                "this node could not encode its state as a snapshot",
            )
        },
    }
}

pub async fn get_state_snapshot_entries(
    req: HttpRequest,
    state: web::Data<AppState>,
    query: web::Query<SnapshotEntriesQuery>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    // #3551: one full snapshot build at a time, process-wide.
    let _snapshot_build = snapshot_build_slot().lock().await;
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(100).clamp(1, MAX_SNAPSHOT_PAGE_SIZE);

    let blockchain = state.blockchain.read().await;
    let current = {
        let storage = state.storage.read().await;
        current_snapshot(&blockchain, &storage)
    };
    let current = match current {
        Ok(current) => current,
        Err(e) => {
            log::error!("snapshot export failed: {e} (#3288)");
            return envelope_error(
                &req,
                StatusCode::INTERNAL_SERVER_ERROR,
                "SNAPSHOT_EXPORT_FAILED",
                "this node could not encode its state as a snapshot",
            );
        },
    };
    let CurrentSnapshot { state_root_hex, snapshot, .. } = current;
    let total = snapshot.entries.len();
    let end = offset.saturating_add(limit).min(total);
    let entries = if offset >= total {
        Vec::new()
    } else {
        snapshot.entries[offset..end].to_vec()
    };
    let next_offset = if end < total { Some(end) } else { None };

    HttpResponse::Ok()
        .insert_header(("X-Total-Count", total.to_string()))
        .json(SnapshotEntriesResponse {
            height: snapshot.height,
            state_root_hex,
            snapshot_root_hex: snapshot.snapshot_root_hex,
            offset,
            limit,
            total,
            next_offset,
            entries,
        })
}

/// `GET /state/snapshot/export?offset&limit` — the values (Lane C).
pub async fn get_state_snapshot_export(
    req: HttpRequest,
    state: web::Data<AppState>,
    query: web::Query<SnapshotEntriesQuery>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    // #3551: one full snapshot build at a time, process-wide.
    let _snapshot_build = snapshot_build_slot().lock().await;
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(100).clamp(1, MAX_SNAPSHOT_PAGE_SIZE);

    let blockchain = state.blockchain.read().await;
    let cut = match snapshot_export_cut(&state, &blockchain).await {
        Ok(cut) => cut,
        Err(error) => {
            log::error!("snapshot export failed: {error}");
            return envelope_error(&req, StatusCode::INTERNAL_SERVER_ERROR,
                "SNAPSHOT_EXPORT_FAILED", "this node could not encode its state as a snapshot");
        }
    };
    drop(blockchain);
    let entries = &cut.entries;
    let total = entries.len();
    let end = offset.saturating_add(limit).min(total);
    let page: Vec<SnapshotEntryKV> = if offset >= total {
        Vec::new()
    } else {
        entries.get(offset..end).unwrap_or(&[])
            .iter()
            .map(|(k, v)| SnapshotEntryKV {
                key_hex: hex::encode(k),
                value_hex: hex::encode(v),
            })
            .collect()
    };
    let next_offset = if end < total { Some(end) } else { None };
    HttpResponse::Ok()
        .insert_header(("X-Total-Count", total.to_string()))
        .json(SnapshotExportResponse {
            height: cut.height,
            state_root_hex: cut.state_root_hex.clone(),
            snapshot_root_hex: cut.snapshot_root_hex.clone(),
            offset,
            limit,
            total,
            next_offset,
            entries: page,
        })
}

/// `POST /network/snapshot-sync/receipt` — a consumer hands this server the
/// receipt for a snapshot join it served (#2848 subject).
pub async fn post_snapshot_service_receipt(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<SnapshotServiceReceiptRequest>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let receipt = request.into_inner().receipt;
    // determinism-ok: freshness check on an operator-facing service receipt; never a consensus input
    let now = chrono::Utc::now().timestamp();
    let version = protocol_version_at(receipt.receipt.chain_height);
    let verdict = receipt.verify(
        &ChainIdentity::active(),
        version,
        &server_service_id(),
        now,
        RECEIPT_FRESHNESS_WINDOW_SECS,
    );
    if let Err(err) = verdict {
        let (status, code) = match err {
            ReceiptError::WrongServer { .. } => (StatusCode::UNPROCESSABLE_ENTITY, "RECEIPT_WRONG_SERVER"),
            ReceiptError::Stale { .. } => (StatusCode::UNPROCESSABLE_ENTITY, "RECEIPT_STALE"),
            _ => (StatusCode::UNPROCESSABLE_ENTITY, "RECEIPT_SIGNATURE_INVALID"),
        };
        return envelope_error(&req, status, code, err.to_string());
    }
    // A valid signature proves control of a consumer key, not service delivery.
    // Accept recently served cuts across height changes, with bounded retention.
    // determinism-ok: process-local admission clock, never a consensus input.
    let admitted_at = Instant::now();
    if !state.snapshot_service.matches_recent_receipt(&receipt.receipt, admitted_at) {
        return envelope_error(&req, StatusCode::UNPROCESSABLE_ENTITY,
            "RECEIPT_SUBJECT_NOT_SERVED", "receipt must match this node's exported snapshot height, root and entry count");
    }
    // Hash the already authenticated bearer in memory; never retain or output the token.
    let Some(caller_token) = bearer_token_from_request(&req) else {
        return envelope_error(&req, StatusCode::UNAUTHORIZED, "UNAUTHORIZED", "Missing authenticated caller");
    };
    let caller = *hash_bytes(caller_token.as_bytes()).as_bytes();
    let recorded = state.snapshot_service.ledger.lock().unwrap_or_else(|e| e.into_inner())
        // determinism-ok: process-local API abuse budget, not a consensus input.
        .record(receipt, caller, admitted_at);
    match recorded {
        Ok(count) => HttpResponse::Ok().json(serde_json::json!({
            "accepted": true,
            "recorded_receipts": count,
        })),
        Err(ReceiptLedgerError::Duplicate) => envelope_error(
            &req,
            StatusCode::CONFLICT,
            "RECEIPT_DUPLICATE",
            "a receipt for this consumer, server and snapshot is already recorded",
        ),
        Err(ReceiptLedgerError::Capacity | ReceiptLedgerError::RateLimited) => envelope_error(
            &req, StatusCode::TOO_MANY_REQUESTS, "RECEIPT_CAPACITY_OR_RATE_LIMIT",
            "receipt evidence window capacity or caller/key budget reached; unexpired evidence is retained"),
    }
}

/// `GET /network/snapshot-sync/receipts` — the node-local operator ledger.
/// Returns only receipts admitted less than 15 minutes ago; reading does not
/// extend their lifetime. Collect before expiry for durable archival (#2848).
pub async fn get_snapshot_service_receipts(req: HttpRequest, state: web::Data<AppState>) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }
    let receipts = {
        let mut ledger = state.snapshot_service.ledger.lock().unwrap_or_else(|e| e.into_inner());
        // determinism-ok: process-local operator evidence expiry, never a consensus input.
        ledger.expire(Instant::now());
        ledger.receipts.clone()
    };
    HttpResponse::Ok().json(serde_json::json!({
        "server_id": server_service_id(),
        "count": receipts.len(),
        "receipts": receipts,
    }))
}

pub async fn get_state_snapshot_proof(
    req: HttpRequest,
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    // #3551: one full snapshot build at a time, process-wide.
    let _snapshot_build = snapshot_build_slot().lock().await;
    let key_hex = path.into_inner();
    let key = match hex::decode(key_hex.trim()) {
        Ok(value) => value,
        Err(_) => {
            return envelope_error(
                &req,
                StatusCode::BAD_REQUEST,
                "BAD_REQUEST",
                "key_hex must be a valid hex-encoded key",
            );
        },
    };
    let blockchain = state.blockchain.read().await;
    let current = {
        let storage = state.storage.read().await;
        current_snapshot(&blockchain, &storage)
    };
    let current = match current {
        Ok(current) => current,
        Err(e) => {
            log::error!("snapshot export failed: {e} (#3288)");
            return envelope_error(
                &req,
                StatusCode::INTERNAL_SERVER_ERROR,
                "SNAPSHOT_EXPORT_FAILED",
                "this node could not encode its state as a snapshot",
            );
        },
    };
    let CurrentSnapshot { state_root_hex, snapshot, .. } = current;
    let Some(proof) = build_inclusion_proof(&snapshot, &key) else {
        return envelope_error(&req, StatusCode::NOT_FOUND, "NOT_FOUND", "key not found in current snapshot");
    };

    HttpResponse::Ok().json(SnapshotProofResponse {
        height: snapshot.height,
        state_root_hex,
        proof,
    })
}

pub async fn verify_state_snapshot_inclusion(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<SnapshotVerifyInclusionRequest>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let key = match hex::decode(request.key_hex.trim()) {
        Ok(value) => value,
        Err(_) => {
            return envelope_error(
                &req,
                StatusCode::BAD_REQUEST,
                "BAD_REQUEST",
                "key_hex must be valid hex-encoded bytes",
            );
        },
    };
    let value = match hex::decode(request.value_hex.trim()) {
        Ok(value) => value,
        Err(_) => {
            return envelope_error(
                &req,
                StatusCode::BAD_REQUEST,
                "BAD_REQUEST",
                "value_hex must be valid hex-encoded bytes",
            );
        },
    };
    let valid = verify_inclusion_proof(&request.proof, &key, &value);
    HttpResponse::Ok().json(serde_json::json!({ "valid": valid }))
}

pub async fn get_snapshot_sync_payload(
    req: HttpRequest,
    state: web::Data<AppState>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    // #3551: one full snapshot build at a time, process-wide.
    let _snapshot_build = snapshot_build_slot().lock().await;
    let blockchain = state.blockchain.read().await;
    let current = match snapshot_export_cut(&state, &blockchain).await {
        Ok(current) => current,
        Err(e) => {
            log::error!("snapshot export failed: {e} (#3288)");
            return envelope_error(&req, StatusCode::INTERNAL_SERVER_ERROR,
                "SNAPSHOT_EXPORT_FAILED", "this node could not encode its state as a snapshot");
        }
    };
    let payload = current.payload();
    let signed_payload = state.snapshot_signing_key.as_ref().and_then(|signer| {
        let timestamp = chrono::Utc::now().timestamp();
        let nonce_material = format!(
            "{}:{}:{}",
            payload.height, payload.snapshot_root_hex, payload.entry_count
        );
        let nonce = hash_bytes(nonce_material.as_bytes()).to_string();
        match sign_snapshot_sync_payload_with_signer(
            payload.clone(),
            signer.as_ref(),
            timestamp,
            nonce,
        ) {
            Ok(signed) => Some(signed),
            Err(e) => {
                log::error!(
                    "snapshot signer failed — returning unsigned payload (#1394 fail-observable): {e:#}"
                );
                None
            }
        }
    });

    // #3331: only an authenticated payload can install an irreversible anchor.
    // Preserve unsigned diagnostics when signing is unavailable or fails, but
    // fence signed exports once the source reports nonzero finality. PoP nodes
    // with inert BFT finality report zero and retain their bootstrap path.
    if signed_payload.is_some()
        && blockchain.finalized_height() > 0
        && blockchain.finalized_height() != blockchain.height
    {
        return snapshot_tip_not_finalized(
            &req,
            blockchain.height,
            blockchain.finalized_height(),
        );
    }

    HttpResponse::Ok().json(SnapshotSyncPayloadResponse {
        payload,
        signed_payload,
    })
}

pub async fn verify_snapshot_sync_payload(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<SnapshotVerifySignedPayloadRequest>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    // #3551: one full snapshot build at a time, process-wide.
    let _snapshot_build = snapshot_build_slot().lock().await;
    let signed_payload = &request.signed_payload;
    if let Err(resp) = validate_timestamp(&req, signed_payload.timestamp, "timestamp") {
        return resp;
    }
    if let Err(resp) = validate_nonce(&req, &signed_payload.nonce) {
        return resp;
    }
    if let Err(resp) = verify_snapshot_sync_trust(&req, &state, signed_payload) {
        return resp;
    }

    let blockchain = state.blockchain.read().await;
    let current = {
        let storage = state.storage.read().await;
        current_snapshot(&blockchain, &storage)
    };
    let current = match current {
        Ok(current) => current,
        Err(e) => {
            log::error!("snapshot export failed: {e} (#3288)");
            return envelope_error(
                &req,
                StatusCode::INTERNAL_SERVER_ERROR,
                "SNAPSHOT_EXPORT_FAILED",
                "this node could not encode its state as a snapshot",
            );
        },
    };
    let expected = current.payload();

    if signed_payload.payload != expected {
        return envelope_error(
            &req,
            StatusCode::CONFLICT,
            "CONFLICT",
            "signed_payload does not match current state snapshot payload",
        );
    }

    HttpResponse::Ok().json(serde_json::json!({ "valid": true }))
}

/// Why a remote snapshot was refused before touching local state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotApplyRefusal {
    /// The supplied entries do not reproduce the signed `snapshot_root_hex`.
    EntriesMismatch,
    /// `apply_snapshot_entries` rejected an entry.
    ApplyFailed(String),
    /// The applied state does not rebuild to the signed `snapshot_root_hex`.
    RoundTripMismatch,
    /// #3288: the applied state's recomputed `state_root` is not the signed
    /// `state_root_hex`. `snapshot_root_hex` commits to the exported entries
    /// only, so without this check a state whose uncarried sections kept this
    /// node's own values was installed and checkpointed silently.
    StateRootMismatch { signed: String, recomputed: String },
    /// #3288 3b: the carried blocks do not bind the state to the signed tip
    /// (see `chain_snapshot::verify_chain_window`).
    ChainWindow(ChainWindowError),
    /// #3331: with source finality active, an unfinalized tip can orphan below the
    /// recipient's mandatory state/WAL anchor. Refuse it before apply.
    TipNotFinalized { finalized: u64, height: u64 },
    /// #3288 3b: the signed finalized height is above the snapshot height.
    FinalizedAboveTip { finalized: u64, height: u64 },
    /// #3288 3b: the carried genesis block is not the one THIS node expects
    /// (its pinned genesis hash, or its configured chain identity). Checked
    /// against the node's own configuration, so a trusted signer cannot
    /// install another chain's genesis.
    GenesisNotOurs(String),
    /// #3288 3b: this node has no pinned genesis hash, so nothing but the
    /// signer would vouch for which chain's genesis it installs. Refused.
    GenesisUnpinned,
}

/// The refusal for a node without a pinned genesis (#3288 3b).
pub const SNAPSHOT_INGEST_REQUIRES_PINNED_GENESIS: &str =
    "snapshot ingest requires a pinned genesis (WRFCOIN_GENESIS_HASH)";

#[cfg(test)]
thread_local! {
    /// Test-only genesis pin (#3288 3b). Route tests cannot set
    /// `WRFCOIN_GENESIS_HASH`: the environment is process-wide and other tests
    /// read it concurrently. actix test handlers run on the test's own thread,
    /// so a thread-local override is scoped to exactly one test.
    // None inherits node configuration; Some(None) explicitly models an
    // unpinned node even when p2p supplies the canonical testnet pin (#3506).
    static TEST_GENESIS_PIN: std::cell::RefCell<Option<Option<String>>> =
        const { std::cell::RefCell::new(None) };
}

/// Pins this test thread's node to `hash` until dropped (#3288 3b).
#[cfg(test)]
pub(crate) struct TestGenesisPin(Option<Option<String>>);

#[cfg(test)]
impl TestGenesisPin {
    pub(crate) fn set(hash: impl Into<String>) -> Self {
        Self::override_pin(Some(hash.into()))
    }

    pub(crate) fn unpinned() -> Self {
        Self::override_pin(None)
    }

    fn override_pin(hash: Option<String>) -> Self {
        Self(TEST_GENESIS_PIN.with(|pin| pin.replace(Some(hash))))
    }
}

#[cfg(test)]
impl Drop for TestGenesisPin {
    fn drop(&mut self) {
        TEST_GENESIS_PIN.with(|pin| *pin.borrow_mut() = self.0.take());
    }
}

/// What this node independently expects of the chain's genesis (#3288 3b):
/// the same sources the boot gate uses.
#[derive(Debug, Clone)]
pub struct GenesisExpectation {
    /// The #2035 committing expectation: an explicit `WRFCOIN_GENESIS_HASH`,
    /// or the pinned testnet hash under `WRFCOIN_NETWORK_ID=testnet`
    /// (`wrfcoin_p2p::expected_genesis_hash`). `None` when the node has no pin,
    /// and always `None` in a build without `p2p`.
    pub pinned_hash: Option<String>,
    /// The node's configured chain identity. Block 1 must record the same one
    /// (`assert_chain_identity_matches_genesis`, the boot check).
    pub identity: wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity,
}

impl GenesisExpectation {
    /// This node's expectation, from its configuration.
    pub fn of_this_node() -> Self {
        #[cfg(feature = "p2p")]
        let pinned_hash = wrfcoin_p2p::expected_genesis_hash();
        #[cfg(not(feature = "p2p"))]
        let pinned_hash = None;
        #[cfg(test)]
        let pinned_hash = TEST_GENESIS_PIN.with(|pin| pin.borrow().clone()).unwrap_or(pinned_hash);
        Self {
            pinned_hash,
            identity: wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity::active().clone(),
        }
    }

    /// Whether `genesis` is this node's genesis. Requires a pin; see
    /// [`SnapshotApplyRefusal::GenesisUnpinned`].
    pub fn check(&self, genesis: &Block) -> Result<(), SnapshotApplyRefusal> {
        let norm = |h: &str| h.trim().trim_start_matches("0x").to_ascii_lowercase();
        let pinned = self.pinned_hash.as_deref().ok_or(SnapshotApplyRefusal::GenesisUnpinned)?;
        let carried = genesis.hash.map(|h| h.to_hex()).unwrap_or_default();
        if norm(&carried) != norm(pinned) {
            return Err(SnapshotApplyRefusal::GenesisNotOurs(format!(
                "carried genesis {carried} is not this node's pinned genesis {pinned} \
                 (WRFCOIN_GENESIS_HASH / WRFCOIN_NETWORK_ID=testnet, #2035)"
            )));
        }
        let recorded = wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity::from_genesis_metadata(
            genesis.metadata.iter().map(|(k, v)| (k.as_str(), v.as_str())),
        );
        wrfcoin_shared_protocol::crypto::chain_binding::assert_chain_identity_matches_genesis(
            &self.identity,
            recorded.as_ref(),
        )
        .map_err(|e| {
            SnapshotApplyRefusal::GenesisNotOurs(format!(
                "carried genesis records another chain identity: {e}"
            ))
        })
    }
}

/// What a verified snapshot installs (#3288 3b): the state, and the blocks
/// (the height-1 genesis block plus the window ending at the tip), ascending.
#[derive(Debug)]
pub struct VerifiedSnapshot {
    pub state: State,
    pub blocks: Vec<Block>,
    /// What the admitted tip is trusted for (#3331, SOL-R story 8). Read-only
    /// disposition: it changes no acceptance rule.
    pub trust: BootstrapTrust,
}

/// The explicit bootstrap-trust disposition of an ADMITTED snapshot (#3331).
///
/// Neither value is fork-final certification. The source's finality claim is
/// authenticated only by the trusted snapshot signer, not by an independently
/// verified BFT certificate (`docs/snapshot/finalized-tip-admission-3331.md`),
/// and the recipient's installed anchor is a LOCAL irreversible anchor at the
/// snapshot height. The disposition is a function of the signed
/// `finalized_height` and `height` alone, so a same-height alternate fork gets
/// the disposition its own signed fields declare; it is not persisted, so a
/// restarted node does not claim one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapTrust {
    /// The signed claim says the source's tip is final (`0 < finalized == height`).
    /// Signer-attested, not independently certified.
    SignerAttestedFinalTip,
    /// The source reports inert finality (`finalized == 0`). The owner-permitted
    /// bootstrap path: the recipient trusts the snapshot signer's word for the
    /// tip. Never a finality certificate.
    SignerTrustedBootstrap,
}

impl BootstrapTrust {
    /// Stable machine label for logs and the ingest response.
    pub const fn label(self) -> &'static str {
        match self {
            Self::SignerAttestedFinalTip => "signer_attested_final_tip",
            Self::SignerTrustedBootstrap => "signer_trusted_bootstrap",
        }
    }
}

/// The disposition for the signed `(finalized, height)` pair of an admitted
/// snapshot. Callers have already refused `0 < finalized < height` and
/// `finalized > height`, so only the two admitted shapes are named here.
pub const fn bootstrap_trust(finalized: u64, height: u64) -> BootstrapTrust {
    if finalized > 0 && finalized == height {
        BootstrapTrust::SignerAttestedFinalTip
    } else {
        BootstrapTrust::SignerTrustedBootstrap
    }
}

/// Verify a remote snapshot against `local` and return what to install.
///
/// No I/O or blacklist changes (the route owns those). On chain 888/testnet,
/// first authenticate the carried genesis against this node's independent pin
/// and install its immutable authority before state import reads epoch timing.
/// That authenticated policy remains installed even if later state checks fail;
/// no snapshot state is persisted here. Legacy 777 keeps its original ordering:
/// signed entry root, state import/round-trip/root, then the carried chain.
pub fn verify_and_apply_snapshot(
    local: State,
    payload: &SnapshotSyncPayload,
    entries: &[(Vec<u8>, Vec<u8>)],
) -> Result<VerifiedSnapshot, SnapshotApplyRefusal> {
    verify_and_apply_snapshot_at(
        wrfcoin_shared_protocol::crypto::chain_binding::protocol_version_at(payload.height),
        &GenesisExpectation::of_this_node(),
        local,
        payload,
        entries,
    )
}

/// Version-explicit [`verify_and_apply_snapshot`], the pattern of
/// `State::apply_block_finalization_checked_at`: production resolves the
/// version from the snapshot height; tests exercise the V2 contract directly.
pub fn verify_and_apply_snapshot_at(
    version: wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion,
    genesis: &GenesisExpectation,
    mut local: State,
    payload: &SnapshotSyncPayload,
    entries: &[(Vec<u8>, Vec<u8>)],
) -> Result<VerifiedSnapshot, SnapshotApplyRefusal> {
    // Step 0 (#3288 3b): without a pinned genesis only the signer would vouch
    // for which chain this is. Fail closed before doing any work.
    if genesis.pinned_hash.is_none() {
        return Err(SnapshotApplyRefusal::GenesisUnpinned);
    }

    if payload.finalized_height > 0 && payload.finalized_height < payload.height {
        return Err(SnapshotApplyRefusal::TipNotFinalized {
            finalized: payload.finalized_height,
            height: payload.height,
        });
    }

    // Step 1: the supplied entries reproduce the signed Merkle root.
    let remote_snapshot =
        build_state_snapshot(payload.height, payload.state_root_hex.clone(), entries);
    if remote_snapshot.snapshot_root_hex != payload.snapshot_root_hex {
        return Err(SnapshotApplyRefusal::EntriesMismatch);
    }

    // Step 2: apply the state entries to a copy of local state and check the
    // round trip. This catches any lossy field silently ignored on the way in.
    let (state_entries, chain_entries) = split_snapshot_entries(entries);
    if genesis.identity.chain_id == "888" && genesis.identity.network_id == "testnet" {
        // State import validates frozen bond epochs, so it must already have
        // the recorded timing. Verify hashes BEFORE comparing the carried hash
        // to the independent pin: trusting only Block.hash would permit a
        // forged policy with the original cached hash. A one-block minimum
        // avoids reading state-dependent timing before authority installation;
        // step 3 still enforces the full consumer window after state import.
        let blocks = verify_chain_window(
            &chain_entries, payload.height, &payload.tip_hash_hex,
            &payload.state_root_hex, 1, version,
        ).map_err(SnapshotApplyRefusal::ChainWindow)?;
        let carried = blocks.first().ok_or_else(||
            SnapshotApplyRefusal::GenesisNotOurs("carried genesis unavailable".into()))?;
        genesis.check(carried)?;
        if !wrfcoin_native_chain::upgrade::install_schedule_extension_authority_for_block1(carried) {
            return Err(SnapshotApplyRefusal::GenesisNotOurs(
                "carried genesis authority invalid or conflicts with installed authority".into(),
            ));
        }
    }
    local
        .apply_snapshot_entries(&state_entries)
        .map_err(|e| SnapshotApplyRefusal::ApplyFailed(e.to_string()))?;
    local.recalculate_state_root();
    let recomputed = hex::encode(local.state_root.0);
    // v2 round trip: the installed state must re-export to exactly the entries
    // received. The encoding is canonical (hash sets are sorted, see
    // `state_snapshot::CANONICAL_SET_FIELDS`), so this is byte equality.
    let rebuilt = build_snapshot_entries(&local)
        .map_err(|e| SnapshotApplyRefusal::ApplyFailed(format!("re-export failed: {e}")))?;
    if wrfcoin_shared_protocol::blockchain::state_snapshot::first_entry_difference(
        &state_entries,
        &rebuilt,
    )
    .is_some()
    {
        return Err(SnapshotApplyRefusal::RoundTripMismatch);
    }

    // Step 2b (#3288): the state we would install must BE the signed state.
    // The signed payload carries `state_root_hex` precisely so this can be
    // checked; before #3288 nothing did, and the round trip above cannot see
    // a section that is not exported at all.
    let signed = payload.state_root_hex.trim().to_ascii_lowercase();
    if recomputed != signed {
        return Err(SnapshotApplyRefusal::StateRootMismatch { signed, recomputed });
    }

    // Step 3 (#3288 3b): the carried blocks. The tip block must hash to the
    // SIGNED tip and commit to the SIGNED state root (which step 2b showed is
    // the installed state's root), so the state is the one the chain itself
    // committed to at this height. The window must reach back as far as this
    // node's consumers need, judged on the state being installed.
    if payload.finalized_height > payload.height {
        return Err(SnapshotApplyRefusal::FinalizedAboveTip {
            finalized: payload.finalized_height,
            height: payload.height,
        });
    }
    let blocks = verify_chain_window(
        &chain_entries,
        payload.height,
        &payload.tip_hash_hex,
        &signed,
        snapshot_block_window(&local, payload.height),
        version,
    )
    .map_err(SnapshotApplyRefusal::ChainWindow)?;
    // Block 1 against this node's OWN genesis expectation, not only the
    // signer's word (#3288 3b review).
    let carried_genesis = blocks.first().expect("verify_chain_window requires genesis"); // panic-ok: verify_chain_window returned Ok, and it rejects a window without the height-1 genesis first
    genesis.check(carried_genesis)?;
    let trust = bootstrap_trust(payload.finalized_height, payload.height);
    Ok(VerifiedSnapshot { state: local, blocks, trust })
}

/// #3331: admission/export refusal, not evidence of conflicting state.
fn snapshot_tip_not_finalized(req: &HttpRequest, height: u64, finalized: u64) -> HttpResponse {
    envelope_error(
        req,
        StatusCode::CONFLICT,
        "SNAPSHOT_TIP_NOT_FINALIZED",
        &format!(
            "source tip at height {height} is not finalized (source finalized height {finalized}); \
             retry at a finalized tip or use ordinary block sync"
        ),
    )
}

/// #3288 3b: the refusal for a node without a pinned genesis.
fn genesis_unpinned(req: &HttpRequest) -> HttpResponse {
    envelope_error(
        req,
        StatusCode::CONFLICT,
        "SNAPSHOT_GENESIS_UNPINNED",
        SNAPSHOT_INGEST_REQUIRES_PINNED_GENESIS,
    )
}

/// #3288 3b (E): the refusal for a node that is not fresh.
fn not_a_fresh_node(req: &HttpRequest) -> HttpResponse {
    envelope_error(
        req,
        StatusCode::CONFLICT,
        "SNAPSHOT_INGEST_REQUIRES_FRESH_NODE",
        SNAPSHOT_INGEST_REQUIRES_FRESH_NODE,
    )
}

/// Whether the in-memory chain is empty: height 0 and no blocks.
fn chain_is_empty(blockchain: &Blockchain) -> bool {
    blockchain.height == 0 && blockchain.in_memory_block_count() == 0
}

/// Why a verified snapshot was not installed.
#[derive(Debug)]
pub enum InstallRefusal {
    /// The node has chain history (in memory or on disk).
    NotAFreshNode,
    /// The atomic install transaction failed; nothing was installed.
    Persist(StorageError),
}

/// Install a verified snapshot as this node's whole chain (#3288 3b).
///
/// Persist first, then memory, under the chain write lock. `install_snapshot`
/// writes the blocks, the height, the tip, the finalized height and the
/// checkpoint in ONE transaction, re-checking there is no chain history, so a
/// crash leaves either the fresh node or the whole installed chain. Memory
/// changes only after that commit, so a refused or failed install changes
/// nothing.
///
/// With nonzero source finality, admission requires a finalized snapshot tip
/// (#3331). With inert finality, the original bootstrap path remains available.
/// The installed anchor stays at the snapshot height: this node holds no
/// state below the snapshot, so a reorg below it could only be
/// replayed from the genesis anchor with no WAL, which is wrong state.
/// `plan_reorg` refuses forks below the finalized height.
pub async fn install_verified_snapshot(
    blockchain: &tokio::sync::RwLock<Blockchain>,
    storage: &tokio::sync::RwLock<Storage>,
    verified: VerifiedSnapshot,
    height: u64,
) -> Result<(), InstallRefusal> {
    let tip = verified
        .blocks
        .last()
        .filter(|block| block.height == height)
        .and_then(|block| block.hash)
        .expect("verify_chain_window returns hashed blocks ending at the tip"); // panic-ok: verify_chain_window returned Ok, and it only accepts hashed blocks ending at the signed tip height
    let mut chain = blockchain.write().await;
    if !chain_is_empty(&chain) {
        return Err(InstallRefusal::NotAFreshNode);
    }
    storage
        .read()
        .await
        .install_snapshot(&verified.state, height, tip, &verified.blocks, height)
        .await
        .map_err(|e| match e {
            StorageError::ChainHistoryPresent => InstallRefusal::NotAFreshNode,
            other => InstallRefusal::Persist(other),
        })?;
    // The same in-memory window `Storage::load_state` restores on restart.
    let window = configured_max_in_memory_blocks() as u64;
    let in_memory = verified
        .blocks
        .iter()
        .filter(|block| block.height + window > height)
        .map(|block| (block.height, block.clone()))
        .collect();
    let total_blocks = verified.blocks.len() as u64;
    *chain = Blockchain::from_persisted(height, tip, State::default(), in_memory, total_blocks, height);
    // #3277 S5 (core4#3364 review P3-3): a snapshot-joined node never applies
    // block 1, but the snapshot carries it, verified against this node's
    // pinned genesis. Take the same block-1 hook an IBD node takes when block 1
    // applies: re-resolve the (provisional) base schedule from it, WITH the
    // per-chain bootstrap, and install the schedule-extension authority.
    // Without it this node would reject every post-H_ENABLE block carrying
    // an extension that its peers accept.
    if let Some(block1) = verified.blocks.iter().find(|block| block.height == 1) {
        wrfcoin_native_chain::upgrade::reresolve_base_schedule_if_block1(block1);
    }
    // #3277 S3 F2: the verified state goes in through `install_snapshot_state`,
    // which also refreshes the effective feature schedule from it. This happens
    // after the swap and still under the chain write lock, so `feature_active`
    // never answers from the list this node held before the sync.
    install_snapshot_state(&mut chain, verified.state, height);
    Ok(())
}

pub async fn ingest_snapshot_sync_payload(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<SnapshotSyncIngestRequest>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }
    let signed_payload = &request.signed_payload;
    if let Err(resp) = validate_timestamp(&req, signed_payload.timestamp, "timestamp") {
        return resp;
    }
    if let Err(resp) = validate_nonce(&req, &signed_payload.nonce) {
        return resp;
    }
    if let Err(resp) = verify_snapshot_sync_trust(&req, &state, signed_payload) {
        return resp;
    }

    // Check the authenticated finality claim before decoding, blacklisting,
    // replay bookkeeping or persistence. A later finalized retry must work.
    // Zero means the source's finality fence is inert, as on current PoP nodes.
    if signed_payload.payload.finalized_height > 0
        && signed_payload.payload.finalized_height < signed_payload.payload.height
    {
        return snapshot_tip_not_finalized(
            &req,
            signed_payload.payload.height,
            signed_payload.payload.finalized_height,
        );
    }

    let replay_key = replay_key_for_sync_payload(signed_payload);
    {
        let seen = state.snapshot_sync_seen.read().await;
        if seen.contains(&replay_key) {
            return envelope_error(
                &req,
                StatusCode::CONFLICT,
                "CONFLICT",
                "snapshot sync payload replay detected",
            );
        }
    }

    // #2119: a root this node already caught conflicting is refused here,
    // before any entry is decoded or verified.
    if snapshot_root_blacklisted(&signed_payload.payload.snapshot_root_hex)
        || snapshot_root_blacklisted(&state_root_pair_key(
            &signed_payload.payload.snapshot_root_hex,
            &signed_payload.payload.state_root_hex,
        ))
    {
        return envelope_error(
            &req,
            StatusCode::CONFLICT,
            "SNAPSHOT_ROOT_BLACKLISTED",
            "this snapshot root previously failed verification on this node and is refused",
        );
    }
    let remote_height = signed_payload.payload.height;

    // #3288 3b (E): ingest installs the snapshot's chain as this node's WHOLE
    // history, so it runs only on a fresh node: height 0, no blocks, no WAL.
    // A node with history would be left holding blocks, WAL and checkpoints
    // the snapshot does not describe. (Checked again, atomically, at install.)
    if remote_height == 0 {
        return envelope_error(
            &req,
            StatusCode::CONFLICT,
            "CONFLICT",
            "a snapshot at height 0 carries no chain; there is nothing to install",
        );
    }
    {
        let blockchain = state.blockchain.read().await;
        if !chain_is_empty(&blockchain) {
            return not_a_fresh_node(&req);
        }
    }
    // #3288 3b: a node without a pinned genesis cannot tell which chain a
    // snapshot is of; refuse before decoding anything.
    if GenesisExpectation::of_this_node().pinned_hash.is_none() {
        return genesis_unpinned(&req);
    }
    match state.storage.read().await.has_chain_history() {
        Ok(false) => {},
        Ok(true) => return not_a_fresh_node(&req),
        Err(e) => {
            log::error!("snapshot ingest: could not read chain history: {e}");
            return envelope_error(
                &req,
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_SERVER_ERROR",
                "could not determine whether this node has chain history",
            );
        },
    }

    // Entry count must match the advertised count.
    if request.entries.len() as u64 != signed_payload.payload.entry_count {
        return envelope_error(
            &req,
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            "entries length does not match signed payload entry_count",
        );
    }

    // Decode hex -> raw (key, value) pairs up front.
    let mut decoded_entries: Vec<(Vec<u8>, Vec<u8>)> = Vec::with_capacity(request.entries.len());
    for (idx, entry) in request.entries.iter().enumerate() {
        let key = match hex::decode(entry.key_hex.trim()) {
            Ok(v) => v,
            Err(_) => {
                return envelope_error(
                    &req,
                    StatusCode::BAD_REQUEST,
                    "BAD_REQUEST",
                    &format!("entries[{idx}].key_hex is not valid hex"),
                );
            },
        };
        let value = match hex::decode(entry.value_hex.trim()) {
            Ok(v) => v,
            Err(_) => {
                return envelope_error(
                    &req,
                    StatusCode::BAD_REQUEST,
                    "BAD_REQUEST",
                    &format!("entries[{idx}].value_hex is not valid hex"),
                );
            },
        };
        decoded_entries.push((key, value));
    }

    // Steps 1-2b (pure, see `verify_and_apply_snapshot`): entries reproduce
    // the signed Merkle root, the applied state round-trips, and the applied
    // state's recomputed state_root equals the SIGNED state_root_hex (#3288)
    // -- all before local state is touched.
    let local_state = {
        let blockchain = state.blockchain.read().await;
        blockchain.state.clone()
    };
    let verified = match verify_and_apply_snapshot(
        local_state,
        &signed_payload.payload,
        &decoded_entries,
    ) {
        Ok(verified) => verified,
        Err(SnapshotApplyRefusal::EntriesMismatch) => {
            // #2119: conflicting snapshot — remember the root so it is never verified again.
            blacklist_snapshot_root(&signed_payload.payload.snapshot_root_hex);
            log::warn!(
                "snapshot ingest: entries do not reproduce signed root {}; root blacklisted (#2119)",
                signed_payload.payload.snapshot_root_hex
            );
            return envelope_error(
                &req,
                StatusCode::CONFLICT,
                "CONFLICT",
                "entries do not reproduce signed snapshot_root_hex",
            );
        },
        Err(SnapshotApplyRefusal::ApplyFailed(e)) => {
            return envelope_error(
                &req,
                StatusCode::CONFLICT,
                "CONFLICT",
                &format!("failed to apply snapshot entries: {e}"),
            );
        },
        Err(SnapshotApplyRefusal::RoundTripMismatch) => {
            // #2119: the applied state does not round-trip to the signed root: conflicting.
            blacklist_snapshot_root(&signed_payload.payload.snapshot_root_hex);
            log::warn!(
                "snapshot ingest: applied state does not round-trip to signed root {}; root blacklisted (#2119)",
                signed_payload.payload.snapshot_root_hex
            );
            return envelope_error(
                &req,
                StatusCode::CONFLICT,
                "CONFLICT",
                "applied state round-trip snapshot_root_hex mismatch (lossy fast-sync)",
            );
        },
        Err(SnapshotApplyRefusal::StateRootMismatch { signed, recomputed }) => {
            // #3288 3b (review N1): under format v2 the entries carry every
            // state-root section, so the applied root is a pure function of
            // the entries. A signed root they do not produce makes this
            // (snapshot root, signed state root) PAIR conflicting on every
            // node: blacklist the pair. Not the snapshot root alone, because
            // the same entries signed with the root they produce are honest.
            blacklist_snapshot_root(&state_root_pair_key(
                &signed_payload.payload.snapshot_root_hex,
                &signed_payload.payload.state_root_hex,
            ));
            log::warn!(
                "snapshot ingest: refused — applied state_root {recomputed} != signed \
                 state_root_hex {signed} at height {remote_height}; (snapshot root, signed \
                 state root) pair blacklisted (#3288)"
            );
            return envelope_error(
                &req,
                StatusCode::CONFLICT,
                "SNAPSHOT_STATE_ROOT_MISMATCH",
                &format!(
                    "applied state_root {recomputed} does not equal the signed state_root_hex \
                     {signed}; refusing to install a state the chain would reject (#3288)"
                ),
            );
        },
        Err(SnapshotApplyRefusal::ChainWindow(why)) => {
            // Not blacklisted: the node's own required window (K) is part of
            // the check, so the same snapshot may suit a node with other needs.
            log::warn!(
                "snapshot ingest: refused at height {remote_height}: {why} (#3288 3b)"
            );
            return envelope_error(
                &req,
                StatusCode::CONFLICT,
                "SNAPSHOT_CHAIN_REFUSED",
                &format!("the snapshot's blocks do not bind it to the signed tip: {why}"),
            );
        },
        Err(SnapshotApplyRefusal::GenesisUnpinned) => return genesis_unpinned(&req),
        Err(SnapshotApplyRefusal::GenesisNotOurs(why)) => {
            log::warn!("snapshot ingest: refused at height {remote_height}: {why} (#3288 3b)");
            return envelope_error(
                &req,
                StatusCode::CONFLICT,
                "SNAPSHOT_GENESIS_MISMATCH",
                &format!("the snapshot is not of this node's chain: {why}"),
            );
        },
        Err(SnapshotApplyRefusal::TipNotFinalized { finalized, height }) => {
            return snapshot_tip_not_finalized(&req, height, finalized);
        },
        Err(SnapshotApplyRefusal::FinalizedAboveTip { finalized, height }) => {
            return envelope_error(
                &req,
                StatusCode::BAD_REQUEST,
                "BAD_REQUEST",
                &format!("signed finalized_height {finalized} is above the snapshot height {height}"),
            );
        },
    };

    // Step 3 (#3288 3b): persist atomically, then install in memory.
    let bootstrap_trust = verified.trust;
    let bonded_active = verified.state.node_receipt_registry.is_some()
        && verified.state.node_share_activation_height().is_some_and(|a| remote_height >= a);
    match install_verified_snapshot(&state.blockchain, &state.storage, verified, remote_height).await
    {
        Ok(()) => {},
        Err(InstallRefusal::NotAFreshNode) => return not_a_fresh_node(&req),
        Err(InstallRefusal::Persist(e)) => {
            log::error!("snapshot ingest: install failed at height {remote_height}: {e:#}");
            return envelope_error(
                &req,
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_SERVER_ERROR",
                "snapshot verified but could not be persisted; nothing was installed",
            );
        },
    }

    // Step 4: record replay protection.
    let mut seen = state.snapshot_sync_seen.write().await;
    if !seen.insert(replay_key) {
        return envelope_error(
            &req,
            StatusCode::CONFLICT,
            "CONFLICT",
            "snapshot sync payload replay detected",
        );
    }

    // Lane C: the consumer's receipt for the server that served this
    // snapshot, signed by this node's snapshot signer when one is configured.
    // A signer-less joiner (sync-only) gets `null` and the reason — never a
    // receipt somebody else could have minted.
    let (receipt, receipt_unavailable_reason) = match (&state.snapshot_signing_key, request.served_by.as_deref()) {
        (Some(signer), Some(server_id)) if !server_id.trim().is_empty() => {
            let consumer_pubkey: Option<[u8; 32]> = signer.public_key().as_bytes().try_into().ok();
            match consumer_pubkey {
                Some(pubkey) => {
                    let body = build_snapshot_join_receipt(
                        server_id.trim(),
                        pubkey,
                        signed_payload.payload.height,
                        &signed_payload.payload.snapshot_root_hex,
                        signed_payload.payload.entry_count,
                        // determinism-ok: issue time of an operator-facing service receipt; never a consensus input
                        chrono::Utc::now().timestamp(),
                    );
                    match SignedNodeServiceReceipt::sign_with(
                        body,
                        signer.as_ref(),
                        &ChainIdentity::active(),
                        protocol_version_at(signed_payload.payload.height),
                    ) {
                        Ok(signed) if !bonded_active => (Some(signed), None),
                        Ok(signed) => {
                            let archive = wrfcoin_native_chain::node_service_archive::InstalledNodeReceipt { receipt: signed.clone(), manifest: signed_payload.clone() };
                            let saved = (|| {
                                let bytes = serde_json::to_vec(&archive).ok()?;
                                let key = wrfcoin_shared_protocol::consensus::node_share_contracts::BondedServiceClaim::receipt_hash(&signed).ok()?;
                                Some((key, bytes))
                            })();
                            match saved {
                                Some((key, bytes)) if state.storage.read().await.archive_node_receipt(&key, &bytes).is_ok() => (Some(signed), None),
                                _ => (None, Some("installed snapshot receipt could not be durably archived".into())),
                            }
                        },
                        Err(e) => {
                            log::warn!("snapshot-sync: receipt signing failed: {e:#}");
                            (None, Some("receipt signing failed".to_string()))
                        },
                    }
                },
                None => (None, Some("snapshot signer public key is not 32 bytes".to_string())),
            }
        },
        (None, Some(_)) => (None, Some("this node has no snapshot signer; sync-only joiners issue no receipts".to_string())),
        (Some(_), Some(_)) => (None, Some("served_by is empty".to_string())),
        (_, None) => (None, Some("no served_by in the ingest request".to_string())),
    };

    HttpResponse::Ok().json(serde_json::json!({
        "accepted": true,
        "applied": true,
        "height": signed_payload.payload.height,
        "snapshot_root_hex": signed_payload.payload.snapshot_root_hex,
        "tip_hash_hex": signed_payload.payload.tip_hash_hex,
        "bootstrap_trust": bootstrap_trust.label(),
        "receipt": receipt,
        "receipt_unavailable_reason": receipt_unavailable_reason,
    }))
}

/// Step 3 of ingest: install a VERIFIED snapshot state as the canonical
/// state, then refresh the effective feature schedule from it (#3277 S3, F2).
///
/// The snapshot carries `feature_schedule_extensions` whole (format v2,
/// #3288 3a), and the import REPLACES the local list rather than merging
/// into it. A snapshot install is a canonical-state change like a block
/// apply or a reorg, so `feature_active` must answer from the installed list
/// (and a feature this binary lacks tightens the #3093 clean-halt deadline),
/// never from whatever this node held before the sync.
fn install_snapshot_state(blockchain: &mut Blockchain, new_state: State, height: u64) {
    blockchain.state = new_state;
    blockchain.height = height;
    wrfcoin_native_chain::upgrade::refresh_schedule_from_canonical_state(
        &blockchain.state,
        "snapshot import",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_native_chain::blockchain::state::{Account, State};
    use wrfcoin_shared_protocol::common::types::{Address, Balance};

    /// v2 (#3288): recorded values the source holds replace stale local ones.
    /// Previously this needed one hand-written test per v1 key
    /// (genesis/emission_config, producer/last_settlement). The whole-State
    /// codec now carries every field, and the shared-protocol round-trip
    /// tests cover them generically; this pins the two historic cases.
    #[test]
    fn source_values_replace_stale_local_values_3288() {
        use wrfcoin_shared_protocol::crypto::chain_binding::GenesisEmissionConfig;
        use wrfcoin_shared_protocol::oracle::attestation::ProducerSettlementRecord;

        let mut source = State::default();
        source.genesis_emission_config = Some(GenesisEmissionConfig {
            chain_id: "777".into(),
            faucet_address: Some(Address::new([37; 20])),
         ..Default::default() });
        source.last_producer_settlement = Some(ProducerSettlementRecord {
            height: 9,
            payout_address: "0xC1C1C1C1C1C1C1C1C1C1C1C1C1C1C1C1C1C1C1C1".into(),
            amount: 42,
        });
        source.recalculate_state_root();

        let mut synced = State::default();
        synced.genesis_emission_config = Some(GenesisEmissionConfig {
            chain_id: "stale-local-chain".into(),
            faucet_address: Some(Address::new([91; 20])),
         ..Default::default() });
        synced.last_producer_settlement = Some(ProducerSettlementRecord {
            height: 1,
            payout_address: "0xstale".into(),
            amount: 999,
        });
        synced
            .apply_snapshot_entries(&build_snapshot_entries(&source).expect("export"))
            .expect("snapshot applies");
        assert_eq!(synced.genesis_emission_config, source.genesis_emission_config);
        assert_eq!(synced.last_producer_settlement, source.last_producer_settlement);
        assert_eq!(synced.state_root, source.state_root);

        // And the other direction: a source WITHOUT them clears stale local ones.
        let mut empty_source = State::default();
        empty_source.recalculate_state_root();
        synced
            .apply_snapshot_entries(&build_snapshot_entries(&empty_source).expect("export"))
            .expect("snapshot applies");
        assert_eq!(synced.genesis_emission_config, None);
        assert_eq!(synced.last_producer_settlement, None);
    }

    #[test]
    fn malformed_recorded_value_refuses_3288() {
        use wrfcoin_shared_protocol::blockchain::state::ApplySnapshotError;

        let mut entries = build_snapshot_entries(&State::default()).expect("export");
        let slot = entries
            .iter_mut()
            .find(|(k, _)| k == b"state/genesis_emission_config")
            .expect("field entry exists");
        // A CBOR text string where an Option<GenesisEmissionConfig> belongs.
        slot.1 = vec![0x63, b'b', b'a', b'd'];
        let mut state = State::default();
        let err = state.apply_snapshot_entries(&entries).expect_err("must refuse");
        assert!(matches!(err, ApplySnapshotError::MalformedValue(..)), "got {err:?}");
    }

    /// S4 sync leg (#2055): the genesis oracle set must survive a snapshot
    /// round-trip with an identical `state_root`.
    ///
    /// **Scope: snapshot ENCODING coverage, not the S4 sync leg.** The source
    /// state is built by applying `ApplyOracleRegistration` directly; the real
    /// sync leg lives in `p2p_events.rs`, module `tests::genesis_oracle_set_s4`.
    #[test]
    fn genesis_oracle_set_survives_a_snapshot_sync_round_trip_s4() {
        use rand::SeedableRng;
        use wrfcoin_native_chain::security::key_registry::SigningRole;
        use wrfcoin_shared_protocol::blockchain::state::StateTransition;
        use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

        let id = |seed: u64| {
            PublicKey::from_private_key(&PrivateKey::generate(
                &mut rand::rngs::StdRng::seed_from_u64(seed),
            ))
            .to_hex()
        };
        let id_a = id(2055_0001);
        let id_b = id(2055_0002);

        let mut source = State::default();
        for (set, quorum, identity) in [
            (vec![id_a.clone()], 1usize, id_a.clone()),
            (vec![id_a.clone(), id_b.clone()], 2, id_b.clone()),
        ] {
            source
                .apply_transitions_batch(std::iter::once(
                    StateTransition::ApplyOracleRegistration {
                        oracle_set: set,
                        oracle_quorum: quorum,
                        identity: identity.clone(),
                        public_key: identity,
                        epoch: 1,
                        activated_at: 1_700_000_000,
                    },
                ))
                .expect("genesis registration must apply");
        }
        let expected_root = source.state_root;

        let entries = build_snapshot_entries(&source).expect("export");
        for key in [&b"state/oracle_set"[..], b"state/oracle_quorum", b"state/signing_keys"] {
            assert!(
                entries.iter().any(|(k, _)| k.as_slice() == key),
                "the snapshot must carry {}",
                String::from_utf8_lossy(key)
            );
        }

        let mut synced = State::default();
        synced.apply_snapshot_entries(&entries).expect("snapshot must apply cleanly");

        assert_eq!(synced.oracle_quorum, 2, "quorum must survive the sync");
        assert!(synced.oracle_set.contains(&id_a) && synced.oracle_set.contains(&id_b));
        let oracle_keys = synced.signing_keys.snapshot_for_role(SigningRole::Oracle);
        for who in [&id_a, &id_b] {
            assert!(
                oracle_keys.iter().any(|(identity, epochs)| identity == who
                    && epochs.iter().any(|e| e.epoch == 1)),
                "each synced oracle must keep its signing-key epoch"
            );
        }
        assert_eq!(synced.state_root, expected_root, "SYNC: same state_root as the source");
        assert_ne!(expected_root, State::default().state_root, "the root must have moved");
    }

    /// Regression test for issue #705: pagination returns correct next_offset
    /// and respects limit for large state. v2 keeps one entry per account.
    #[test]
    fn snapshot_entries_pagination_2000_accounts() {
        let mut state = State::new();
        for i in 0..2000u64 {
            let mut addr_bytes = [0u8; 20];
            addr_bytes[..8].copy_from_slice(&i.to_be_bytes());
            let address = Address(addr_bytes);
            let account = Account::new(Balance::new(1000 + i as u128));
            state.accounts.insert(address, account);
        }

        let entries = build_snapshot_entries(&state).expect("export");
        let total = entries.len();
        let per_account = entries.iter().filter(|(k, _)| k.starts_with(b"state/accounts/")).count();
        assert_eq!(per_account, 2000, "one entry per account");

        let limit = 500usize;
        let end = limit.min(total);
        assert_eq!(entries[..end].len(), 500);
        let offset = total.saturating_sub(50);
        let end = offset.saturating_add(limit).min(total);
        assert!(entries[offset..end].len() <= 50);
        assert!(end == total);
    }
}

#[cfg(test)]
mod snapshot_root_blacklist_tests_2119 {
    use super::*;

    #[test]
    fn a_root_that_failed_verification_is_refused_before_any_work() {
        let root = "AbCd2119000000000000000000000000000000000000000000000000000000ff";
        assert!(!snapshot_root_blacklisted(root));
        blacklist_snapshot_root(root);
        assert!(snapshot_root_blacklisted(root));
        // Case- and whitespace-insensitive: the same content address.
        assert!(snapshot_root_blacklisted(&format!("  {}  ", root.to_ascii_lowercase())));
        assert!(blacklisted_snapshot_roots().contains(&root.to_ascii_lowercase()));
        assert!(!snapshot_root_blacklisted("0000000000000000000000000000000000000000000000000000000000002119"));
    }

    /// #3288 3b (N1): the pair key distinguishes a wrong signed root from the
    /// honest one for the same entries, and never equals a bare root.
    #[test]
    fn the_state_root_pair_key_separates_the_honest_signature_3288() {
        let snap = "AA".repeat(32);
        let wrong = state_root_pair_key(&snap, &"11".repeat(32));
        let honest = state_root_pair_key(&format!(" {} ", snap.to_ascii_lowercase()), &"22".repeat(32));
        assert_ne!(wrong, honest);
        assert_ne!(wrong, normalise_root(&snap));
        assert_eq!(wrong, state_root_pair_key(&snap.to_ascii_lowercase(), &"11".repeat(32)));
        blacklist_snapshot_root(&wrong);
        assert!(snapshot_root_blacklisted(&wrong));
        assert!(!snapshot_root_blacklisted(&honest), "the honest signature stays installable");
        assert!(!snapshot_root_blacklisted(&snap), "the entries themselves are not blacklisted");
    }

    /// Source pin: both mismatch verdicts blacklist the root, and the check
    /// runs before the entries are decoded.
    #[test]
    fn ingest_blacklists_on_both_mismatches_and_checks_before_decoding() {
        let source = include_str!("snapshot.rs");
        let ingest = source.find("pub async fn ingest_snapshot_sync_payload(").unwrap();
        let body = &source[ingest..];
        let check = body.find("snapshot_root_blacklisted(&signed_payload.payload.snapshot_root_hex)").expect("pre-check");
        let pair_check = body.find("snapshot_root_blacklisted(&state_root_pair_key(").expect("pair pre-check");
        assert!(pair_check < body.find("Decode hex -> raw (key, value) pairs").unwrap());
        let decode = body.find("Decode hex -> raw (key, value) pairs").expect("decode step");
        assert!(check < decode, "the blacklist check must precede decoding");
        let first = body.find("entries do not reproduce signed snapshot_root_hex").unwrap();
        let second = body.find("applied state round-trip snapshot_root_hex mismatch").unwrap();
        for (label, at) in [("entries mismatch", first), ("round-trip mismatch", second)] {
            let window = &body[at.saturating_sub(600)..at];
            assert!(window.contains("blacklist_snapshot_root(&signed_payload.payload.snapshot_root_hex)"), "{label} must blacklist");
        }
    }
}

/// Test support (#3288 3b): chains whose tip commits to a given state, and
/// the full snapshot (state + chain entries) a source would serve.
#[cfg(test)]
pub(crate) mod snapshot_test_support {
    use super::*;
    use wrfcoin_native_chain::crypto::hash::Hash;
    use wrfcoin_shared_protocol::blockchain::block::STATE_ROOT_METADATA_KEY;

    /// A linked chain `1 ..= height`, each block carrying its hash, the tip
    /// committing to `state_root_hex`. Timestamps are one minute apart.
    pub(crate) fn chain_committing_to(state_root_hex: &str, height: u64) -> Vec<Block> {
        let mut blocks = Vec::new();
        let mut prev = Hash::default();
        for h in 1..=height {
            let mut block = Block::new(1, prev, Vec::new(), 1, h);
            block.timestamp = chrono::DateTime::from_timestamp(1_700_000_000 + 60 * h as i64, 0)
                .expect("valid timestamp");
            if h == height {
                block
                    .metadata
                    .insert(STATE_ROOT_METADATA_KEY.to_string(), state_root_hex.to_string());
            }
            let hash = block.calculate_hash();
            block.set_hash(hash);
            prev = hash;
            blocks.push(block);
        }
        blocks
    }

    /// `blockchain` with its state unchanged and a chain `1 ..= height` whose
    /// tip commits to that state's (recomputed) root.
    pub(crate) fn with_chain_committing_to_its_state(blockchain: Blockchain, height: u64) -> Blockchain {
        let root = recomputed_state_root_hex(&blockchain.state);
        let blocks = chain_committing_to(&root, height);
        let tip = blocks.last().and_then(|b| b.hash).expect("hashed");
        let finalized = blockchain.finalized_height();
        Blockchain::from_persisted(
            height,
            tip,
            blockchain.state,
            blocks.into_iter().map(|b| (b.height, b)).collect(),
            height,
            finalized,
        )
    }

    /// The snapshot a source holding `source` at `height` serves: state
    /// entries, genesis and the required window, and the payload (unsigned).
    /// Returns the whole source chain too.
    pub(crate) fn snapshot_of(
        source: &State,
        height: u64,
    ) -> (SnapshotSyncPayload, Vec<(Vec<u8>, Vec<u8>)>, Vec<Block>) {
        let root = recomputed_state_root_hex(source);
        let chain = chain_committing_to(&root, height);
        let floor = window_floor(height, snapshot_block_window(source, height));
        let carried: Vec<Block> =
            chain.iter().filter(|b| b.height == 1 || b.height >= floor).cloned().collect();
        let mut entries = build_snapshot_entries(source).expect("export");
        entries.extend(export_block_entries(&carried).expect("block export"));
        let snapshot = build_state_snapshot(height, root, &entries);
        let tip = chain.last().and_then(|b| b.hash).expect("hashed").to_hex();
        (build_snapshot_sync_payload(&snapshot, tip, 0), entries, chain)
    }

    pub(crate) fn assert_summary_matches_export_3551(blockchain: &Blockchain, storage: &Storage) {
        let current = current_snapshot(blockchain, storage).expect("export");
        let summary = snapshot_summary(blockchain, storage).expect("summary");
        assert_eq!(summary.height, current.snapshot.height);
        assert_eq!(summary.state_root_hex, current.state_root_hex);
        assert_eq!(summary.snapshot_root_hex, current.snapshot.snapshot_root_hex);
        assert_eq!(summary.entry_count, current.snapshot.entries.len());
    }
}

// ---------------------------------------------------------------------------
// #3288: ingest must install the SIGNED state, and every state-root section
// must be accounted for by the snapshot encoding.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod snapshot_state_root_3288_tests {
    use super::*;
    use wrfcoin_shared_protocol::blockchain::state::StateTransition;
    use wrfcoin_shared_protocol::consensus::producer_set::ProducerSetChange;
    use wrfcoin_shared_protocol::station_registry::{
        ContributionProvenanceClass, DataQualityTier, StationLocation, WeatherStation,
    };

    /// The body of the first `fn_signature` in `source`, up to `closing`.
    fn fn_body<'a>(source: &'a str, fn_signature: &str, closing: &str) -> &'a str {
        let start = source
            .find(fn_signature)
            .unwrap_or_else(|| panic!("`{fn_signature}` not found"));
        let rest = &source[start..];
        let end = rest.find(closing).unwrap_or_else(|| panic!("end of `{fn_signature}`"));
        &rest[..end]
    }

    fn station() -> WeatherStation {
        WeatherStation {
            station_id: "PWS-3288-01".to_string(),
            owner_address: "0".repeat(40),
            location: StationLocation { latitude: 46.79, longitude: -92.1, elevation_meters: None },
            hardware_model: "test".to_string(),
            calibration_date: None,
            is_active: true,
            reputation_score: 0,
            data_quality_tier: DataQualityTier::Unverified,
            provenance_class: ContributionProvenanceClass::Real,
            ring_public_key: None,
        }
    }

    /// A source that differs from a fresh node in the sections v1 could not
    /// carry: the live-chain shape #3288 found (a registered station, spent
    /// oracle nonces, a producer-set change after genesis).
    fn live_shaped_source() -> State {
        let mut source = State::default();
        let identity = "a1".repeat(32);
        source
            .apply_transitions_batch(std::iter::once(StateTransition::ApplyOracleRegistration {
                oracle_set: vec![identity.clone()],
                oracle_quorum: 1,
                identity: identity.clone(),
                public_key: identity,
                epoch: 1,
                activated_at: 1_700_000_000,
            }))
            .expect("oracle registration applies");
        source.station_registry.register(station()).expect("station registers");
        source.spent_oracle_nonces.insert("att-nonce-1".into());
        source.spent_oracle_nonces_height.insert("att-nonce-1".into(), 7);
        source.producer_set_history.push(ProducerSetChange::new(1, 42, Vec::new()));
        source.recalculate_state_root();
        source
    }

    /// #3288 3b: ingest requires a pinned genesis. Pin the genesis every
    /// `snapshot_of` chain shares, for this test thread.
    fn pin_test_genesis() -> TestGenesisPin {
        TestGenesisPin::set(
            super::snapshot_test_support::chain_committing_to(&"00".repeat(32), 2)[0]
                .hash
                .unwrap()
                .to_hex(),
        )
    }

    fn payload_for(source: &State, height: u64) -> (SnapshotSyncPayload, Vec<(Vec<u8>, Vec<u8>)>) {
        let (payload, entries, _) = super::snapshot_test_support::snapshot_of(source, height);
        (payload, entries)
    }

    /// The #3288 goal: a fresh node fast-syncs a live-shaped state and
    /// installs exactly the signed state. Before v2 this snapshot was refused
    /// (v1 could not carry station_registry, spent_oracle_nonces or
    /// producer_set_history); before #3289 it was silently installed wrong.
    #[test]
    fn fresh_node_installs_a_live_shaped_snapshot_exactly_3288() {
        let _pin = pin_test_genesis();
        let source = live_shaped_source();
        let (payload, entries) = payload_for(&source, 7);
        let installed = verify_and_apply_snapshot(State::default(), &payload, &entries)
            .expect("a v2 snapshot of a live-shaped state must install on a fresh node");
        let installed = installed.state;
        assert_eq!(installed.state_root, source.state_root);
        assert_eq!(hex::encode(installed.state_root.0), payload.state_root_hex);
        assert!(installed.station_registry.get("PWS-3288-01").is_some());
        assert!(installed.spent_oracle_nonces.contains("att-nonce-1"));
        assert_eq!(installed.producer_set_history.len(), 1);
        assert_eq!(
            wrfcoin_shared_protocol::blockchain::state_snapshot::first_carried_difference(
                &source, &installed
            )
            .unwrap(),
            None,
            "every carried field must match, not just the root"
        );
    }

    fn with_extension(mut state: State, sequence: u64, feature: &str, activation: u64) -> State {
        use wrfcoin_shared_protocol::governance::schedule_extension::{
            AppliedExtension, ScheduleEntry,
        };
        state.feature_schedule_extensions.push(AppliedExtension {
            sequence,
            carried_at_height: 3,
            entries: vec![ScheduleEntry::new(feature, activation)],
            version_steps: Vec::new(),
            min_protocol_version: 3,
            release_commit: format!("f2-{feature}"),
            signers: vec!["ab".repeat(32)],
        });
        state.recalculate_state_root();
        state
    }

    /// #3277 S3, F2 (the HARD GATE on submitting any extension): a v2
    /// snapshot carries `feature_schedule_extensions`, and ingest REPLACES
    /// the importing node's list with it — the stale local list is neither
    /// kept nor merged — then refreshes the effective schedule from it.
    ///
    /// `paymaster` is a feature this binary implements, so the refresh
    /// installs no clean-halt deadline; `u64::MAX` is never evaluated by
    /// another test. The overlay is process-global in this binary, so the
    /// overlay observation is retried rather than assumed race-free (the
    /// `active_features_reflect_applied_schedule_extensions_3277` pattern).
    #[test]
    fn snapshot_import_replaces_feature_schedule_extensions_3277() {
        use wrfcoin_shared_protocol::crypto::chain_binding::{
            feature_schedule_extension_entries, refresh_feature_schedule_extensions,
        };
        let _pin = pin_test_genesis(); // #3288 3b: ingest requires a pinned genesis
        let source = with_extension(live_shaped_source(), 0, "paymaster", u64::MAX);
        let local = with_extension(State::default(), 0, "session_keys", 9_000);
        assert_ne!(source.feature_schedule_extensions, local.feature_schedule_extensions);

        let (payload, entries) = payload_for(&source, 7);
        assert!(
            entries
                .iter()
                .any(|(k, _)| k.as_slice() == b"state/feature_schedule_extensions"),
            "the v2 snapshot must carry feature_schedule_extensions"
        );

        let installed = verify_and_apply_snapshot(local.clone(), &payload, &entries)
            .expect("a snapshot carrying extensions must install over a node holding others")
            .state;
        assert_eq!(
            installed.feature_schedule_extensions, source.feature_schedule_extensions,
            "import must REPLACE the local list with the snapshot's"
        );
        assert_eq!(installed.state_root, source.state_root);

        // Fail closed: a snapshot that omits the field (re-signed over the
        // truncated entries, so only the omission is under test) is refused.
        let truncated: Vec<_> = entries
            .iter()
            .filter(|(k, _)| k.as_slice() != b"state/feature_schedule_extensions")
            .cloned()
            .collect();
        let truncated_payload = build_snapshot_sync_payload(
            &build_state_snapshot(7, hex::encode(source.state_root.0), &truncated),
            payload.tip_hash_hex.clone(),
            payload.finalized_height,
        );
        assert!(
            verify_and_apply_snapshot(local, &truncated_payload, &truncated).is_err(),
            "a snapshot without feature_schedule_extensions must never install"
        );

        // Ingest's install step refreshes the effective schedule from the
        // installed state.
        let before = feature_schedule_extension_entries();
        let mut chain = Blockchain::default();
        let mut observed = false;
        for _ in 0..200 {
            install_snapshot_state(&mut chain, installed.clone(), 7);
            let overlay = feature_schedule_extension_entries();
            if overlay == vec![("paymaster".to_string(), u64::MAX)] {
                observed = true;
                break;
            }
        }
        assert!(observed, "the installed list must become the effective-schedule overlay");
        assert_eq!(chain.height, 7);
        assert_eq!(chain.state.state_root, source.state_root);
        refresh_feature_schedule_extensions(before);
    }

    /// Step 2b stays load-bearing: entries that are self-consistent but do not
    /// produce the SIGNED root are refused, never installed.
    #[test]
    fn a_signed_root_the_entries_do_not_produce_is_refused_3288() {
        let _pin = pin_test_genesis();
        let source = live_shaped_source();
        let (mut payload, entries) = payload_for(&source, 7);
        payload.state_root_hex = hex::encode([0x32u8; 32]);
        match verify_and_apply_snapshot(State::default(), &payload, &entries) {
            Err(SnapshotApplyRefusal::StateRootMismatch { signed, recomputed }) => {
                assert_eq!(signed, payload.state_root_hex);
                assert_eq!(recomputed, hex::encode(source.state_root.0));
            },
            other => panic!("expected StateRootMismatch, got {other:?}"),
        }
    }

    /// v1 is refused outright (#3288 decision 4).
    #[test]
    fn a_v1_snapshot_is_refused_3288() {
        let _pin = pin_test_genesis();
        let v1 = vec![(b"oracle/config".to_vec(), b"quorum=1|set=".to_vec())];
        let snapshot = build_state_snapshot(7, hex::encode([0u8; 32]), &v1);
        let payload = build_snapshot_sync_payload(&snapshot, "ab".repeat(32), 0);
        match verify_and_apply_snapshot(State::default(), &payload, &v1) {
            Err(SnapshotApplyRefusal::ApplyFailed(why)) => {
                assert!(why.contains("oracle/config") || why.contains("format"), "{why}");
            },
            other => panic!("expected ApplyFailed, got {other:?}"),
        }
    }

    /// One logical state, one encoding: a source whose hash sets have a
    /// different bucket history re-exports, after install on a fresh node,
    /// to byte-identical entries, so the byte-exact round-trip check passes.
    #[test]
    fn round_trip_is_byte_exact_across_hash_set_histories_3288() {
        let _pin = pin_test_genesis();
        let mut source = live_shaped_source();
        for i in 0..200 {
            source.spent_oracle_nonces.insert(format!("n{i:03}"));
        }
        // Force a different bucket layout on the source by growing then
        // shrinking a set, so its iteration order differs from a fresh one.
        for i in 200..2000 {
            source.spent_oracle_nonces.insert(format!("n{i:04}"));
        }
        for i in 200..2000 {
            source.spent_oracle_nonces.remove(&format!("n{i:04}"));
        }
        source.recalculate_state_root();
        let (payload, entries) = payload_for(&source, 9);
        let installed = verify_and_apply_snapshot(State::default(), &payload, &entries)
            .expect("hash-set order must not cause a false round-trip refusal");
        assert_eq!(installed.state.state_root, source.state_root);
        assert_eq!(
            build_snapshot_entries(&installed.state).expect("re-export"),
            split_snapshot_entries(&entries).0,
            "the installed state must re-export byte-identical state entries"
        );
    }

    /// Export signs a freshly recomputed root; the helper must agree with the
    /// mutating `recalculate_state_root` bit for bit.
    #[test]
    fn recomputed_root_matches_recalculate_3288() {
        let mut state = live_shaped_source();
        state.spent_oracle_nonces.insert("stale-marker".into());
        let stale_hex = hex::encode(state.state_root.0);
        let recomputed = recomputed_state_root_hex(&state);
        state.recalculate_state_root();
        assert_eq!(recomputed, hex::encode(state.state_root.0));
        assert_ne!(recomputed, stale_hex, "precondition: the stored root was stale");
    }

    /// The export path itself signs the recomputed root when the stored one
    /// is stale.
    #[test]
    fn current_snapshot_signs_the_recomputed_root_3288() {
        let mut blockchain = Blockchain::default();
        blockchain.state = live_shaped_source();
        let storage_dir = tempfile::tempdir().expect("tempdir");
        let storage = Storage::open(&storage_dir.path().join("db"), 100).expect("storage");
        blockchain = super::snapshot_test_support::with_chain_committing_to_its_state(blockchain, 3);
        blockchain.state.spent_oracle_nonces.insert("stale-marker".into());
        let current = current_snapshot(&blockchain, &storage).expect("export");
        let (signed_root, snapshot) = (current.state_root_hex, current.snapshot);
        let stale = hex::encode(blockchain.state.state_root.0);
        let mut fresh = blockchain.state.clone();
        fresh.recalculate_state_root();
        assert_ne!(signed_root, stale, "precondition: the stored root is stale");
        assert_eq!(signed_root, hex::encode(fresh.state_root.0));
        assert_eq!(snapshot.state_root_hex, signed_root);
    }

    /// The route runs the verifier, then persists (#3288 3b: one atomic
    /// install), and only then swaps the in-memory chain.
    #[test]
    fn ingest_verifies_then_persists_then_swaps_3288() {
        let body = fn_body(
            include_str!("snapshot.rs"),
            "pub async fn ingest_snapshot_sync_payload(",
            "\n}\n",
        );
        let verify = body.find("verify_and_apply_snapshot(").expect("ingest must call the verifier");
        let refusal = body
            .find("SnapshotApplyRefusal::StateRootMismatch")
            .expect("ingest must handle the state-root refusal");
        let install = body.find("install_verified_snapshot(").expect("install");
        assert!(verify < install && refusal < install);
        assert!(!body.contains(".force_checkpoint("), "the install is the only write");
        let installer = fn_body(
            include_str!("snapshot.rs"),
            "pub async fn install_verified_snapshot(",
            "\n}\n",
        );
        let persist = installer.find(".install_snapshot(").expect("persist");
        let swap = installer.find("*chain = Blockchain::from_persisted(").expect("swap");
        assert!(persist < swap, "durable before memory");
        // #3277 S3 F2 on the 3b path: the installed state goes in through
        // `install_snapshot_state`, which refreshes the effective schedule
        // from it, AFTER the swap and still under the chain write lock.
        let install_state = installer
            .find("install_snapshot_state(&mut chain, verified.state, height)")
            .expect("install_verified_snapshot must install the state through install_snapshot_state (#3277 F2)");
        assert!(swap < install_state, "refresh from the INSTALLED state, after the swap");

        let install = fn_body(
            include_str!("snapshot.rs"),
            "fn install_snapshot_state(",
            "\n}\n",
        );
        let assign = install.find("blockchain.state = new_state").expect("install assigns");
        let refresh = install
            .find("refresh_schedule_from_canonical_state(")
            .expect("install refreshes the effective schedule (#3277 F2)");
        assert!(assign < refresh, "refresh from the INSTALLED state, after the swap");
    }

    /// #3551: the streamed summary reports the recomputed root, as the
    /// export signs it, when the stored root is stale.
    #[test]
    fn snapshot_summary_reports_the_recomputed_root_3551() {
        let mut blockchain = Blockchain::default();
        blockchain.state = live_shaped_source();
        let storage_dir = tempfile::tempdir().expect("tempdir");
        let storage = Storage::open(&storage_dir.path().join("db"), 100).expect("storage");
        blockchain = super::snapshot_test_support::with_chain_committing_to_its_state(blockchain, 3);
        blockchain.state.spent_oracle_nonces.insert("stale-marker".into());
        super::snapshot_test_support::assert_summary_matches_export_3551(&blockchain, &storage);
    }

    /// #3551: `GET /state/snapshot` reports the streamed summary; a return to
    /// building the full snapshot there would keep every other test green.
    #[test]
    fn summary_route_uses_the_streamed_summary_3551() {
        let body = fn_body(include_str!("snapshot.rs"), "pub async fn get_state_snapshot(", "\n}\n");
        assert!(body.contains("snapshot_summary(&blockchain, &storage)"), "the route calls snapshot_summary");
        assert!(!body.contains("current_snapshot("), "the route must not build the full snapshot");
        assert!(!body.contains("build_state_snapshot("), "the route must not build the full snapshot");
    }

    /// #3551: every route that materialises the whole state takes the
    /// process-wide build slot before it reads the chain.
    #[test]
    fn whole_state_routes_take_the_build_slot_3551() {
        let source = include_str!("snapshot.rs");
        for handler in [
            "pub async fn get_state_snapshot(",
            "pub async fn get_state_snapshot_entries(",
            "pub async fn get_state_snapshot_export(",
            "pub async fn get_state_snapshot_proof(",
            "pub async fn get_snapshot_sync_payload(",
            "pub async fn verify_snapshot_sync_payload(",
        ] {
            let body = fn_body(source, handler, "\n}\n");
            let slot = body.find("snapshot_build_slot().lock().await").unwrap_or_else(|| panic!("{handler} takes no build slot"));
            let chain = body.find("state.blockchain.read().await").unwrap_or(usize::MAX);
            assert!(slot < chain, "{handler} reads the chain before taking the build slot");
        }
    }
}

// ---------------------------------------------------------------------------
// #3288 3b: the snapshot carries and binds the chain tip.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod snapshot_tip_3288_3b_tests {
    use super::snapshot_test_support::{chain_committing_to, snapshot_of};
    use super::*;
    use wrfcoin_shared_protocol::blockchain::block::STATE_ROOT_METADATA_KEY;
    use wrfcoin_shared_protocol::contributions::commit_reveal::RevealRecord;
    use wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion::{V1, V2};

    /// This node's configured identity, pinned to the genesis every
    /// `chain_committing_to` chain of height > 1 shares.
    fn test_pin() -> GenesisExpectation {
        let genesis = chain_committing_to(&"00".repeat(32), 2)[0].hash.unwrap().to_hex();
        GenesisExpectation { pinned_hash: Some(genesis), ..GenesisExpectation::of_this_node() }
    }

    fn with_pending_reveal(mut state: State, revealed_at: u64) -> State {
        state.pending_reveals.insert(
            "commit-3288".into(),
            RevealRecord {
                commit_id: "commit-3288".into(),
                station_id: "PWS-3288".into(),
                provenance_class: Default::default(),
                revealed_at,
                reveal_id: "reveal-3288".into(),
                canonical_calibration_attestation_cid: None,
                quality_score: None,
            },
        );
        state.recalculate_state_root();
        state
    }

    fn funded_state() -> State {
        let mut state = State::default();
        state.accounts.insert(
            wrfcoin_shared_protocol::common::types::Address::new([0x32; 20]),
            wrfcoin_native_chain::blockchain::state::Account::new(
                wrfcoin_shared_protocol::common::types::Balance(3288),
            ),
        );
        state.recalculate_state_root();
        state
    }

    /// K is the max over the named consumers, and covers each of them, with
    /// and without the dynamic pending-reveal lookback.
    #[test]
    fn k_covers_every_consumer_3288() {
        for (label, state, height) in [
            ("no pending reveals", funded_state(), 500),
            ("recent reveal", with_pending_reveal(funded_state(), 495), 500),
            ("old reveal", with_pending_reveal(funded_state(), 100), 500),
        ] {
            let k = snapshot_block_window(&state, height);
            let consumers = snapshot_window_consumers(&state, height);
            for (consumer, needs) in consumers {
                assert!(k >= needs, "[{label}] K={k} < {needs} needed by {consumer}");
            }
            assert_eq!(
                Some(k),
                consumers.iter().map(|(_, n)| *n).max(),
                "[{label}] K is exactly the max, no magic floor"
            );
        }
        // The named constants, so a change to either is a visible change to K.
        assert_eq!(MEDIAN_TIME_PAST_WINDOW, 11);
        assert_eq!(RECENT_DA_BLOCK_WINDOW, 64);
        assert_eq!(snapshot_block_window(&funded_state(), 500), 64);
        assert_eq!(snapshot_block_window(&with_pending_reveal(funded_state(), 100), 500), 401);
    }

    #[test]
    fn the_pending_reveal_lookback_reaches_the_oldest_reveal_block_3288() {
        assert_eq!(pending_reveal_lookback(&funded_state(), 500), 0);
        assert_eq!(pending_reveal_lookback(&with_pending_reveal(funded_state(), 500), 500), 1);
        assert_eq!(pending_reveal_lookback(&with_pending_reveal(funded_state(), 100), 500), 401);
    }

    /// V2: the installed blocks are exactly the source's genesis plus window,
    /// and the tip is the signed one.
    #[test]
    fn a_v2_snapshot_installs_genesis_the_window_and_the_signed_tip_3288() {
        let source = funded_state();
        let (payload, entries, chain) = snapshot_of(&source, 100);
        let verified = verify_and_apply_snapshot_at(V2, &test_pin(), State::default(), &payload, &entries)
            .expect("a v2 snapshot with a committed tip installs");
        let heights: Vec<u64> = verified.blocks.iter().map(|b| b.height).collect();
        let expected: Vec<u64> = std::iter::once(1).chain(37..=100).collect();
        assert_eq!(heights, expected, "genesis plus K=64 blocks ending at the tip");
        assert_eq!(verified.blocks.last().unwrap().hash, chain.last().unwrap().hash);
        assert_eq!(verified.state.state_root, source.state_root);
    }

    #[test]
    fn a_tip_other_than_the_signed_one_is_refused_3288() {
        let (mut payload, entries, _) = snapshot_of(&funded_state(), 100);
        payload.tip_hash_hex = "77".repeat(32);
        assert!(matches!( // secret-leak-ok: test_pin() is the public genesis-hash pin, not a secret
            verify_and_apply_snapshot_at(V2, &test_pin(), State::default(), &payload, &entries),
            Err(SnapshotApplyRefusal::ChainWindow(ChainWindowError::TipMismatch { .. }))
        ));
    }

    /// The binding is to the chain's OWN commitment: a tip block that commits
    /// to another state is refused at V2 even though the payload is internally
    /// consistent (entries -> signed Merkle root, state -> signed state root).
    #[test]
    fn a_tip_that_commits_to_another_state_is_refused_at_v2_3288() {
        let source = funded_state();
        let root = recomputed_state_root_hex(&source);
        let other_chain = chain_committing_to(&"44".repeat(32), 30);
        let floor = window_floor(30, snapshot_block_window(&source, 30));
        let carried: Vec<Block> =
            other_chain.iter().filter(|b| b.height == 1 || b.height >= floor).cloned().collect();
        let mut entries = build_snapshot_entries(&source).expect("export");
        entries.extend(export_block_entries(&carried).expect("export"));
        let snapshot = build_state_snapshot(30, root, &entries);
        let tip = other_chain.last().unwrap().hash.unwrap().to_hex();
        let payload = build_snapshot_sync_payload(&snapshot, tip, 0);
        assert!(matches!( // secret-leak-ok: test_pin() is the public genesis-hash pin, not a secret
            verify_and_apply_snapshot_at(V2, &test_pin(), State::default(), &payload, &entries),
            Err(SnapshotApplyRefusal::ChainWindow(
                ChainWindowError::StateRootCommitmentMismatch { .. }
            ))
        ));
    }

    #[test]
    fn at_v2_a_tip_without_a_commitment_is_refused_below_v2_it_is_not_3288() {
        let source = funded_state();
        let root = recomputed_state_root_hex(&source);
        let mut chain = chain_committing_to(&root, 30);
        let tip = chain.last_mut().unwrap();
        tip.metadata.remove(STATE_ROOT_METADATA_KEY);
        let rehash = tip.calculate_hash();
        tip.set_hash(rehash);
        let floor = window_floor(30, snapshot_block_window(&source, 30));
        let carried: Vec<Block> =
            chain.iter().filter(|b| b.height == 1 || b.height >= floor).cloned().collect();
        let mut entries = build_snapshot_entries(&source).expect("export");
        entries.extend(export_block_entries(&carried).expect("export"));
        let snapshot = build_state_snapshot(30, root, &entries);
        let payload =
            build_snapshot_sync_payload(&snapshot, chain.last().unwrap().hash.unwrap().to_hex(), 0);
        assert!(matches!( // secret-leak-ok: test_pin() is the public genesis-hash pin, not a secret
            verify_and_apply_snapshot_at(V2, &test_pin(), State::default(), &payload, &entries),
            Err(SnapshotApplyRefusal::ChainWindow(
                ChainWindowError::MissingStateRootCommitment { height: 30 }
            ))
        ));
        assert!(verify_and_apply_snapshot_at(V1, &test_pin(), State::default(), &payload, &entries).is_ok()); // secret-leak-ok: test_pin() is the public genesis-hash pin, not a secret
    }

    /// The window is judged against THIS node's K on the state being
    /// installed: a pending reveal at height 20 makes a snapshot at 100 carry
    /// back to 20, and one that stops at the constant floor (37) is refused.
    #[test]
    fn the_window_must_reach_the_installed_states_oldest_pending_reveal_3288() {
        let source = with_pending_reveal(funded_state(), 20);
        let (payload, entries, _) = snapshot_of(&source, 100);
        let verified = verify_and_apply_snapshot_at(V2, &test_pin(), State::default(), &payload, &entries)
            .expect("the full window installs");
        assert_eq!(verified.blocks[1].height, 20);

        let root = recomputed_state_root_hex(&source);
        let chain = chain_committing_to(&root, 100);
        let short: Vec<Block> =
            chain.iter().filter(|b| b.height == 1 || b.height >= 37).cloned().collect();
        let mut entries = build_snapshot_entries(&source).expect("export");
        entries.extend(export_block_entries(&short).expect("export"));
        let snapshot = build_state_snapshot(100, root, &entries);
        let payload =
            build_snapshot_sync_payload(&snapshot, chain.last().unwrap().hash.unwrap().to_hex(), 0);
        assert_eq!( // secret-leak-ok: test_pin() is the public genesis-hash pin, not a secret
            verify_and_apply_snapshot_at(V2, &test_pin(), State::default(), &payload, &entries).err(),
            Some(SnapshotApplyRefusal::ChainWindow(ChainWindowError::WindowTooShort {
                required_floor: 20,
                carried_floor: 37,
            }))
        );
    }

    /// (A): a node with no pinned genesis refuses every snapshot, however
    /// well signed and internally consistent, before doing any work.
    #[test]
    fn an_unpinned_node_refuses_ingest_3288() {
        let (payload, entries, _) = snapshot_of(&funded_state(), 30);
        let unpinned = GenesisExpectation { pinned_hash: None, ..test_pin() };
        assert_eq!(
            verify_and_apply_snapshot_at(V2, &unpinned, State::default(), &payload, &entries).err(),
            Some(SnapshotApplyRefusal::GenesisUnpinned)
        );
        // Also on garbage entries: the refusal comes first.
        assert_eq!(
            verify_and_apply_snapshot_at(V2, &unpinned, State::default(), &payload, &[]).err(),
            Some(SnapshotApplyRefusal::GenesisUnpinned)
        );
        assert_eq!(
            SNAPSHOT_INGEST_REQUIRES_PINNED_GENESIS,
            "snapshot ingest requires a pinned genesis (WRFCOIN_GENESIS_HASH)"
        );
    }

    /// Block 1 is checked against THIS node's genesis pin: a snapshot that is
    /// internally perfect but of another genesis is refused, whoever signed it.
    #[test]
    fn a_genesis_other_than_the_pinned_one_is_refused_3288() {
        let (payload, entries, chain) = snapshot_of(&funded_state(), 30);
        let ours = chain[0].hash.unwrap().to_hex();
        let pinned = |hash: String| GenesisExpectation { pinned_hash: Some(hash), ..test_pin() };

        verify_and_apply_snapshot_at(V2, &pinned(ours.clone()), State::default(), &payload, &entries)
            .expect("the pinned genesis installs");
        verify_and_apply_snapshot_at(V2, &pinned(format!("0x{}", ours.to_uppercase())), State::default(), &payload, &entries)
            .expect("the pin is compared normalised, as the boot gate does");
        let other = "e1".repeat(32);
        match verify_and_apply_snapshot_at(V2, &pinned(other.clone()), State::default(), &payload, &entries) {
            Err(SnapshotApplyRefusal::GenesisNotOurs(why)) => {
                assert!(why.contains(&other) && why.contains(&ours), "{why}")
            },
            other => panic!("expected GenesisNotOurs, got {other:?}"),
        }
    }

    /// Block 1 must record THIS node's chain identity (the boot check), pinned
    /// or not. A genesis recording no identity predates #2123 and is accepted
    /// as unknown, exactly as boot accepts it.
    #[test]
    fn a_genesis_recording_another_chain_identity_is_refused_3288() {
        use wrfcoin_shared_protocol::crypto::chain_binding::{GENESIS_CHAIN_ID_KEY, GENESIS_NETWORK_ID_KEY};
        let source = funded_state();
        let root = recomputed_state_root_hex(&source);
        let identity = test_pin().identity;
        for (label, chain_id, want_ok) in [
            ("same identity", identity.chain_id.clone(), true),
            ("other chain", format!("{}-other", identity.chain_id), false),
        ] {
            let mut chain = chain_committing_to(&root, 30);
            chain[0].metadata.insert(GENESIS_CHAIN_ID_KEY.into(), chain_id);
            chain[0].metadata.insert(GENESIS_NETWORK_ID_KEY.into(), identity.network_id.clone());
            // Relink: block 1's hash changed, so every block above it does.
            let mut prev = None;
            for block in chain.iter_mut() {
                if let Some(p) = prev {
                    block.prev_block_hash = p;
                }
                let hash = block.calculate_hash();
                block.set_hash(hash);
                prev = Some(hash);
            }
            let floor = window_floor(30, snapshot_block_window(&source, 30));
            let carried: Vec<Block> =
                chain.iter().filter(|b| b.height == 1 || b.height >= floor).cloned().collect();
            let mut entries = build_snapshot_entries(&source).expect("export");
            entries.extend(export_block_entries(&carried).expect("export"));
            let snapshot = build_state_snapshot(30, root.clone(), &entries);
            let payload = build_snapshot_sync_payload(&snapshot, chain[29].hash.unwrap().to_hex(), 0);
            let pin = GenesisExpectation {
                pinned_hash: Some(chain[0].hash.unwrap().to_hex()),
                ..test_pin()
            };
            let got = verify_and_apply_snapshot_at(V2, &pin, State::default(), &payload, &entries);
            if want_ok {
                got.unwrap_or_else(|e| panic!("[{label}] {e:?}"));
            } else {
                assert!(matches!(got, Err(SnapshotApplyRefusal::GenesisNotOurs(_))), "[{label}] {got:?}");
            }
        }
    }

    #[test]
    fn a_finalized_height_above_the_tip_is_refused_3288() {
        let (mut payload, entries, _) = snapshot_of(&funded_state(), 40);
        payload.finalized_height = 41;
        assert_eq!( // secret-leak-ok: test_pin() is the public genesis-hash pin, not a secret
            verify_and_apply_snapshot_at(V2, &test_pin(), State::default(), &payload, &entries).err(),
            Some(SnapshotApplyRefusal::FinalizedAboveTip { finalized: 41, height: 40 })
        );
    }

    /// Export reads genesis and any window block the in-memory window has
    /// evicted from the durable archive.
    #[tokio::test]
    async fn export_reads_evicted_blocks_from_the_archive_3288() {
        let source = funded_state();
        let root = recomputed_state_root_hex(&source);
        let chain = chain_committing_to(&root, 120);
        let dir = tempfile::tempdir().expect("tempdir");
        let mut storage = Storage::open(&dir.path().join("db"), 1_000).expect("storage");
        for block in &chain {
            storage.persist_block(block.clone(), Vec::new(), None).await.expect("persist");
        }
        // Memory holds only the last 30 blocks: genesis and 57..=90 are disk-only.
        let tip = chain.last().unwrap().hash.unwrap();
        let blockchain = Blockchain::from_persisted(
            120,
            tip,
            source.clone(),
            chain.iter().filter(|b| b.height > 90).map(|b| (b.height, b.clone())).collect(),
            120,
            0,
        );
        let current = current_snapshot(&blockchain, &storage).expect("export");
        let (_, chain_entries) = split_snapshot_entries(
            &current
                .snapshot
                .entries
                .iter()
                .map(|e| (hex::decode(&e.key_hex).unwrap(), Vec::new()))
                .collect::<Vec<_>>(),
        );
        assert_eq!(chain_entries.len(), 1 + 64, "genesis plus K=64");
        let blocks = snapshot_chain_blocks(&blockchain, &storage).expect("blocks");
        assert_eq!(blocks[0].height, 1);
        assert_eq!(blocks[1].height, 57);
        assert_eq!(blocks.last().unwrap().height, 120);
        for (got, want) in blocks.iter().zip(
            chain.iter().filter(|b| b.height == 1 || b.height >= 57),
        ) {
            assert_eq!(got.hash, want.hash);
        }
    }

    /// #3551: the streamed summary reports exactly what the full export
    /// builds: an empty chain and a window whose
    /// genesis and evicted blocks (57..=90, more than one archive chunk away
    /// from genesis) come from the archive.
    #[tokio::test]
    async fn snapshot_summary_matches_the_full_export_3551() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut storage = Storage::open(&dir.path().join("db"), 1_000).expect("storage");
        super::snapshot_test_support::assert_summary_matches_export_3551(&Blockchain::default(), &storage);


        let source = funded_state();
        let root = recomputed_state_root_hex(&source);
        let chain = chain_committing_to(&root, 120);
        for block in &chain {
            storage.persist_block(block.clone(), Vec::new(), None).await.expect("persist");
        }
        let tip = chain.last().unwrap().hash.unwrap();
        let in_memory_above = |floor: u64| {
            Blockchain::from_persisted(
                120,
                tip,
                source.clone(),
                chain.iter().filter(|b| b.height > floor).map(|b| (b.height, b.clone())).collect(),
                120,
                0,
            )
        };
        super::snapshot_test_support::assert_summary_matches_export_3551(&in_memory_above(90), &storage);
        super::snapshot_test_support::assert_summary_matches_export_3551(&in_memory_above(0), &storage);
        super::snapshot_test_support::assert_summary_matches_export_3551(&in_memory_above(119), &storage);
    }

    /// #3551: a block missing from both memory and the archive fails the
    /// summary as it fails the export.
    #[tokio::test]
    async fn snapshot_summary_fails_where_the_export_fails_3551() {
        let source = funded_state();
        let root = recomputed_state_root_hex(&source);
        let chain = chain_committing_to(&root, 120);
        let dir = tempfile::tempdir().expect("tempdir");
        let mut storage = Storage::open(&dir.path().join("db"), 1_000).expect("storage");
        for block in chain.iter().filter(|b| b.height != 70) {
            storage.persist_block(block.clone(), Vec::new(), None).await.expect("persist");
        }
        let blockchain = Blockchain::from_persisted(
            120,
            chain.last().unwrap().hash.unwrap(),
            source,
            chain.iter().filter(|b| b.height > 90).map(|b| (b.height, b.clone())).collect(),
            120,
            0,
        );
        let export = current_snapshot(&blockchain, &storage).map(|_| ());
        let summary = snapshot_summary(&blockchain, &storage).map(|_| ());
        assert!(export.is_err(), "precondition: the export refuses a hole");
        assert_eq!(summary, export);
    }

}
