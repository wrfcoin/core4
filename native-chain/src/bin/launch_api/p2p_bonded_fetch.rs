//! Serves bonded snapshot fetches arriving over P2P (owner decision 2026-10-31).
//!
//! Same answers as the HTTP routes a bonded fetch uses today
//! (`/network/info`, `/state/snapshot/sync-payload`, `/state/snapshot/export`,
//! `/network/snapshot-sync/bonded-ticket`, `/network/snapshot-sync/receipt`),
//! authorized differently: instead of the Reader bearer, each request carries
//! a [`BondedFetchAuth`] that the server accepts only from a consumer key in
//! its current frozen bond roster, sent by the libp2p peer it names. The
//! receipt, ticket and claim formats are the HTTP ones, unchanged.
//!
//! One difference is deliberate: the payload request pins its cut, so the
//! export pages of one fetch come from one cut even if a block lands while a
//! slow home uplink is still sending them.

use std::sync::Arc;
use std::time::Instant;

use actix_web::web;
use libp2p::PeerId;
use tokio::sync::mpsc;
use wrfcoin_native_chain::node_service_archive::{InstalledNodeReceipt, server_claim};
use wrfcoin_p2p::bonded_fetch::{
    BondedFetchAuth, BondedFetchRequest, BondedFetchResponse, ExportEntry, MAX_RESPONSE_BYTES,
    export_entry_cost, export_page_end,
};
use wrfcoin_p2p::bonded_fetch_serve::InboundBondedFetch;
use wrfcoin_p2p::{P2pClient, P2pCommand};
use wrfcoin_shared_protocol::blockchain::snapshot_sync::sign_snapshot_sync_payload_with_signer;
use wrfcoin_shared_protocol::consensus::node_bond_registry::epoch_at;
use wrfcoin_shared_protocol::consensus::node_service_receipt::{ReceiptError, SignedNodeServiceReceipt};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ChainIdentity, ConsensusFeature, feature_active, protocol_version_at,
};
use wrfcoin_shared_protocol::crypto::hash::hash_bytes;

use crate::routes::snapshot::{server_service_id, snapshot_build_slot, snapshot_export_cut};
use crate::state::AppState;
use crate::state::snapshot_service::{RECEIPT_FRESHNESS_WINDOW_SECS, ReceiptLedgerError, SnapshotExportCut};

/// Fetch requests answered at once. Snapshot work also takes the process-wide
/// build slot, so this mainly bounds hex encoding of pages in parallel.
const MAX_CONCURRENT_BONDED_FETCHES: usize = 4;

/// Answer admitted fetches until the node's queue closes.
pub(crate) fn spawn_bonded_fetch_server(
    mut requests: mpsc::Receiver<InboundBondedFetch>,
    state: web::Data<AppState>,
    client: P2pClient,
) {
    tokio::spawn(async move {
        log::info!("Serving bonded snapshot fetches over P2P");
        let permits = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_BONDED_FETCHES));
        while let Some(inbound) = requests.recv().await {
            let Ok(permit) = Arc::clone(&permits).acquire_owned().await else {
                break;
            };
            let state = state.clone();
            let client = client.clone();
            tokio::spawn(async move {
                let label = inbound.request.label();
                let response = answer(&state, &client.local_peer_id, &inbound.peer_id, inbound.request).await;
                if let BondedFetchResponse::Error { code, .. } = &response {
                    log::debug!("bonded fetch {label} from {} refused: {code}", inbound.peer_id);
                }
                if client
                    .command_tx
                    .send(P2pCommand::RespondToBondedFetch { request_id: inbound.request_id, response })
                    .await
                    .is_err()
                {
                    log::warn!("bonded fetch answer dropped: P2P node stopped");
                }
                drop(permit);
            });
        }
    });
}

/// The whole server side of one request, for a request `remote` sent to `local`.
pub(crate) async fn answer(
    state: &AppState,
    local: &PeerId,
    remote: &PeerId,
    request: BondedFetchRequest,
) -> BondedFetchResponse {
    match request {
        BondedFetchRequest::Info => info(state).await,
        BondedFetchRequest::SyncPayload { auth } => match authorize(state, &auth, local, remote).await {
            Ok(_) => sync_payload(state).await,
            Err(refused) => refused,
        },
        BondedFetchRequest::ExportPage { auth, height, snapshot_root_hex, offset, limit } => {
            match authorize(state, &auth, local, remote).await {
                Ok(_) => export_page(state, height, &snapshot_root_hex, offset, limit).await,
                Err(refused) => refused,
            }
        },
        BondedFetchRequest::BondedTicket { auth, receipt, manifest } => {
            match authorize(state, &auth, local, remote).await {
                Ok(consumer) => ticket(state, consumer, InstalledNodeReceipt { receipt, manifest }).await,
                Err(refused) => refused,
            }
        },
        BondedFetchRequest::Receipt { auth, receipt } => match authorize(state, &auth, local, remote).await {
            Ok(consumer) => record_receipt(state, consumer, receipt),
            Err(refused) => refused,
        },
    }
}

async fn info(state: &AppState) -> BondedFetchResponse {
    let identity = ChainIdentity::active();
    let registry_genesis = state
        .blockchain
        .read()
        .await
        .state
        .node_bond_registry
        .as_ref()
        .map(|r| format!("0x{}", hex::encode(r.genesis_hash)));
    BondedFetchResponse::Info {
        chain_id: identity.chain_id.clone(),
        network_id: identity.network_id.clone(),
        genesis_hash: registry_genesis
            .or_else(|| state.genesis_hash.get())
            .unwrap_or_else(|| "unknown".to_string()),
        service_id: server_service_id(),
    }
}

/// Verify the authorization and that its key is a registered bond consumer in
/// this node's current frozen roster. Returns the consumer key.
async fn authorize(
    state: &AppState,
    auth: &BondedFetchAuth,
    local: &PeerId,
    remote: &PeerId,
) -> Result<[u8; 32], BondedFetchResponse> {
    // determinism-ok: off-chain request authorization freshness, never a consensus input.
    let now = chrono::Utc::now().timestamp();
    let consumer = auth
        .verify(ChainIdentity::active(), &local.to_string(), &remote.to_string(), now)
        .map_err(|e| BondedFetchResponse::error(e.code(), e.message()))?;
    let chain = state.blockchain.read().await;
    let Some(registry) = chain.state.node_bond_registry.as_ref() else {
        return Err(BondedFetchResponse::error("NODE_SERVICE_INACTIVE", "Node service inactive"));
    };
    let Some(epoch) = epoch_at(registry.activation_height, chain.height) else {
        return Err(BondedFetchResponse::error("NODE_SERVICE_INACTIVE", "Node service inactive"));
    };
    let registered = registry
        .frozen_epochs
        .get(&epoch)
        .is_some_and(|frozen| frozen.bindings.values().any(|b| b.consumer_key == consumer));
    if !registered {
        return Err(BondedFetchResponse::error(
            "NOT_BONDED_CONSUMER",
            "consumer key is not registered in this epoch's bond roster",
        ));
    }
    Ok(consumer)
}

async fn sync_payload(state: &AppState) -> BondedFetchResponse {
    let Some(signer) = state.snapshot_signing_key.as_ref() else {
        return BondedFetchResponse::error("NODE_SERVER_SIGNER_UNAVAILABLE", "registered server signer unavailable");
    };
    // #3551: one full snapshot build at a time, process-wide.
    let _snapshot_build = snapshot_build_slot().lock().await;
    let blockchain = state.blockchain.read().await;
    let cut = match snapshot_export_cut(state, &blockchain).await {
        Ok(cut) => cut,
        Err(error) => {
            log::error!("snapshot export failed: {error}");
            return BondedFetchResponse::error(
                "SNAPSHOT_EXPORT_FAILED",
                "this node could not encode its state as a snapshot",
            );
        },
    };
    // #3331: the same fence as the HTTP payload route.
    let (height, finalized) = (blockchain.height, blockchain.finalized_height());
    drop(blockchain);
    if finalized > 0 && finalized != height {
        return BondedFetchResponse::retry_later(
            "SNAPSHOT_TIP_NOT_FINALIZED",
            format!("snapshot tip {height} is not finalized (finalized height {finalized}); retry"),
            5_000,
        );
    }
    let payload = cut.payload();
    // determinism-ok: signed-payload timestamp, the same as the HTTP route; never a consensus input.
    let timestamp = chrono::Utc::now().timestamp();
    let nonce = hash_bytes(
        format!("{}:{}:{}", payload.height, payload.snapshot_root_hex, payload.entry_count).as_bytes(),
    )
    .to_string();
    let signed_payload =
        match sign_snapshot_sync_payload_with_signer(payload.clone(), signer.as_ref(), timestamp, nonce) {
            Ok(signed) => signed,
            Err(error) => {
                log::error!("snapshot signer failed for a P2P bonded fetch: {error:#}");
                return BondedFetchResponse::error("SNAPSHOT_SIGNER_FAILED", "snapshot signer failed");
            },
        };
    // determinism-ok: process-local served-cut retention, never a consensus input.
    state.snapshot_service.pin_cut(&cut, Instant::now());
    BondedFetchResponse::SyncPayload { payload, signed_payload }
}

/// The cut a fetch pinned, or the current cached cut if it is still that one.
async fn cut_for(state: &AppState, height: u64, snapshot_root_hex: &str, now: Instant) -> Option<Arc<SnapshotExportCut>> {
    if let Some(cut) = state.snapshot_service.pinned_cut(height, snapshot_root_hex, now) {
        return Some(cut);
    }
    let cached = state.snapshot_service.export.lock().await.clone();
    cached.filter(|cut| cut.height == height && cut.snapshot_root_hex == snapshot_root_hex)
}

async fn export_page(
    state: &AppState,
    height: u64,
    snapshot_root_hex: &str,
    offset: u64,
    limit: u32,
) -> BondedFetchResponse {
    // determinism-ok: process-local served-cut retention, never a consensus input.
    let now = Instant::now();
    let Some(cut) = cut_for(state, height, snapshot_root_hex, now).await else {
        return BondedFetchResponse::error(
            "SNAPSHOT_CUT_EXPIRED",
            "that cut is no longer held; request the sync payload again",
        );
    };
    // Serving a page keeps the cut's receipt descriptor fresh, as on HTTP.
    state.snapshot_service.remember_cut(&cut, now);
    let total = cut.entries.len();
    let offset = usize::try_from(offset).unwrap_or(usize::MAX).min(total);
    let rest = cut.entries.get(offset..).unwrap_or(&[]);
    let end = export_page_end(offset, total, limit, rest.iter().map(|(k, v)| (k.len(), v.len())));
    let page = cut.entries.get(offset..end).unwrap_or(&[]);
    if page.iter().map(|(k, v)| export_entry_cost(k.len(), v.len())).sum::<usize>() > MAX_RESPONSE_BYTES - 4096 {
        return BondedFetchResponse::error(
            "SNAPSHOT_ENTRY_TOO_LARGE",
            "a snapshot entry exceeds the P2P frame ceiling; fetch over HTTP",
        );
    }
    let entries = page
        .iter()
        .map(|(k, v)| ExportEntry { key_hex: hex::encode(k), value_hex: hex::encode(v) })
        .collect();
    BondedFetchResponse::ExportPage {
        height: cut.height,
        snapshot_root_hex: cut.snapshot_root_hex.clone(),
        total: total as u64,
        next_offset: (end < total).then_some(end as u64),
        entries,
    }
}

async fn ticket(state: &AppState, consumer: [u8; 32], installed: InstalledNodeReceipt) -> BondedFetchResponse {
    if !feature_active(ConsensusFeature::NodeShareSettlement, u64::MAX) {
        return BondedFetchResponse::error("NODE_SERVICE_INACTIVE", "Node service inactive");
    }
    // The authorized consumer may ask only for tickets on its own receipts.
    if installed.receipt.receipt.consumer_pubkey != consumer {
        return BondedFetchResponse::error("NODE_TICKET_WRONG_CONSUMER", "receipt names another consumer");
    }
    let Some(signer) = state.snapshot_signing_key.as_ref() else {
        return BondedFetchResponse::error("NODE_SERVER_SIGNER_UNAVAILABLE", "registered server signer unavailable");
    };
    let Some(identity) = crate::routes::node_receipts::identity() else {
        return BondedFetchResponse::error("NODE_SERVICE_INACTIVE", "Node service authority unavailable");
    };
    if !state
        .snapshot_service
        // determinism-ok: process-local served-cut retention, never consensus freshness.
        .matches_recent_receipt(&installed.receipt.receipt, Instant::now())
    {
        return BondedFetchResponse::error("NODE_MANIFEST_NOT_SERVED", "receipt does not match an exported cut");
    }
    let chain = state.blockchain.read().await;
    let storage = state.storage.read().await;
    match server_claim(&chain.state, &installed, chain.height, &identity, signer.as_ref(), &storage)
        .and_then(|claim| Ok(hex::encode(claim.to_wire()?)))
    {
        Ok(claim_hex) => BondedFetchResponse::BondedTicket { claim_hex },
        Err(_) => BondedFetchResponse::error("NODE_TICKET_REFUSED", "bonded ticket refused"),
    }
}

fn record_receipt(state: &AppState, consumer: [u8; 32], receipt: SignedNodeServiceReceipt) -> BondedFetchResponse {
    if receipt.receipt.consumer_pubkey != consumer {
        return BondedFetchResponse::error("RECEIPT_WRONG_CONSUMER", "receipt names another consumer");
    }
    // determinism-ok: freshness check on an operator-facing service receipt; never a consensus input
    let now = chrono::Utc::now().timestamp();
    let version = protocol_version_at(receipt.receipt.chain_height);
    if let Err(err) = receipt.verify(
        ChainIdentity::active(),
        version,
        &server_service_id(),
        now,
        RECEIPT_FRESHNESS_WINDOW_SECS,
    ) {
        let code = match err {
            ReceiptError::WrongServer { .. } => "RECEIPT_WRONG_SERVER",
            ReceiptError::Stale { .. } => "RECEIPT_STALE",
            _ => "RECEIPT_SIGNATURE_INVALID",
        };
        return BondedFetchResponse::error(code, err.to_string());
    }
    // determinism-ok: process-local admission clock, never a consensus input.
    let admitted_at = Instant::now();
    if !state.snapshot_service.matches_recent_receipt(&receipt.receipt, admitted_at) {
        return BondedFetchResponse::error(
            "RECEIPT_SUBJECT_NOT_SERVED",
            "receipt must match this node's exported snapshot height, root and entry count",
        );
    }
    // The P2P caller is the authenticated consumer key, in its own namespace so
    // it never shares a budget with an HTTP bearer.
    let mut material = b"p2p-bonded-fetch|".to_vec();
    material.extend_from_slice(&consumer);
    let caller = *hash_bytes(&material).as_bytes();
    let recorded = state
        .snapshot_service
        .ledger
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        // determinism-ok: process-local API abuse budget, not a consensus input.
        .record(receipt, caller, admitted_at);
    match recorded {
        Ok(count) => BondedFetchResponse::ReceiptAck { accepted: true, recorded_receipts: count as u64 },
        Err(ReceiptLedgerError::Duplicate) => BondedFetchResponse::error(
            "RECEIPT_DUPLICATE",
            "a receipt for this consumer, server and snapshot is already recorded",
        ),
        Err(ReceiptLedgerError::Capacity | ReceiptLedgerError::RateLimited) => BondedFetchResponse::retry_later(
            "RECEIPT_CAPACITY_OR_RATE_LIMIT",
            "receipt evidence window capacity or caller/key budget reached",
            60_000,
        ),
    }
}
