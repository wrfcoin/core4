use std::time::Instant;

use actix_web::{HttpRequest, HttpResponse, Responder, web};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::helpers::{envelope_error, require_role, sync_status_from_block_age};
use crate::state::AppState;
use wrfcoin_native_chain::blockchain::state::{AuditEventKind, AuditEventRecord};
use wrfcoin_native_chain::security::api_auth::ApiRole;

static NODE_PROCESS_START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// Record the process start for `/node/resources`'s `uptime_secs`. `main`
/// calls this before the server binds; otherwise the clock starts at the
/// first request and reads 0 for it (#3599).
pub fn record_process_start() {
    // determinism-ok: boot-time monotonic clock for /node/resources uptime_secs; read once at startup, never a consensus input (block validity, signing preimage or state root)
    NODE_PROCESS_START.get_or_init(Instant::now);
}

#[derive(Debug, Deserialize)]
pub struct AuditLogQuery {
    pub kind: Option<String>,
    pub subject_id: Option<String>,
    pub actor_id: Option<String>,
    pub from_block: Option<u64>,
    pub to_block: Option<u64>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuditLogResponse {
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub items: Vec<AuditEventRecord>,
}

const DEFAULT_AUDIT_LOG_PAGE_SIZE: usize = 50;
const MAX_AUDIT_LOG_PAGE_SIZE: usize = 500;

fn parse_audit_kind(value: &str) -> Option<AuditEventKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "governance_proposal_created" => Some(AuditEventKind::GovernanceProposalCreated),
        "governance_proposal_executed" => Some(AuditEventKind::GovernanceProposalExecuted),
        "quality_challenge_triggered" => Some(AuditEventKind::QualityChallengeTriggered),
        "quality_slashing_applied" => Some(AuditEventKind::QualitySlashingApplied),
        "bridge_safety_halt" => Some(AuditEventKind::BridgeSafetyHalt),
        "bridge_safety_resume" => Some(AuditEventKind::BridgeSafetyResume),
        "bridge_challenge_opened" => Some(AuditEventKind::BridgeChallengeOpened),
        "bridge_challenge_resolved" => Some(AuditEventKind::BridgeChallengeResolved),
        "bridge_caps_updated" => Some(AuditEventKind::BridgeCapsUpdated),
        _ => None,
    }
}

pub async fn get_network_audit_log(
    req: HttpRequest,
    state: web::Data<AppState>,
    query: web::Query<AuditLogQuery>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }

    let kind = if let Some(value) = &query.kind {
        match parse_audit_kind(value) {
            Some(kind) => Some(kind),
            None => {
                return envelope_error(
                    &req,
                    actix_web::http::StatusCode::BAD_REQUEST,
                    "BAD_REQUEST",
                    "unknown audit kind filter",
                );
            },
        }
    } else {
        None
    };

    let offset = query.offset.unwrap_or(0);
    let limit = query
        .limit
        .unwrap_or(DEFAULT_AUDIT_LOG_PAGE_SIZE)
        .min(MAX_AUDIT_LOG_PAGE_SIZE);
    let blockchain = state.blockchain.read().await;
    let mut items = blockchain
        .state
        .audit_events()
        .iter()
        .filter(|event| {
            if let Some(kind) = kind {
                if event.kind != kind {
                    return false;
                }
            }
            if let Some(subject_id) = &query.subject_id {
                if event.subject_id.as_deref() != Some(subject_id.as_str()) {
                    return false;
                }
            }
            if let Some(actor_id) = &query.actor_id {
                if event.actor_id.as_deref() != Some(actor_id.as_str()) {
                    return false;
                }
            }
            if let Some(from_block) = query.from_block {
                if event.block_height < from_block {
                    return false;
                }
            }
            if let Some(to_block) = query.to_block {
                if event.block_height > to_block {
                    return false;
                }
            }
            true
        })
        .cloned()
        .collect::<Vec<_>>();
    items.sort_by_key(|event| event.event_id);
    let total = items.len();
    let items = items.into_iter().skip(offset).take(limit).collect::<Vec<_>>();

    HttpResponse::Ok().json(AuditLogResponse {
        total,
        offset,
        limit,
        items,
    })
}

// ---------------------------------------------------------------------------
// Node introspection (#985)
// ---------------------------------------------------------------------------

pub async fn get_network_info(state: web::Data<AppState>) -> impl Responder {
    let blockchain = state.blockchain.read().await;

    // Derive sync status from block age using the same shared grace window as /readyz.
    let sync_status = if let Some(last_block) = blockchain.last_block() {
        let age_secs = chrono::Utc::now()
            .signed_duration_since(last_block.timestamp)
            .num_seconds()
            .max(0);
        sync_status_from_block_age(blockchain.height, age_secs)
    } else {
        "genesis"
    };

    // Genesis block hash for chain-identity verification (#1714).
    //
    // Prefer the value cached at boot in AppState. This chain's genesis is the
    // height-1 block (not height 0), and on a long-running node it has been
    // evicted from the ~200-block in-memory window, so a per-request lookup
    // would report "unknown". The cached value resolves the hash once at
    // startup (in-memory genesis anchor or the lowest block in the durable
    // archive). The fallback below only applies to states constructed without a
    // cache (e.g. some tests): use the lowest in-memory block when it is the
    // height-1 genesis anchor, else "unknown".
    // #2797: a fresh-P2P-synced node boots with an EMPTY datadir, so the
    // boot-time cache is unset — and once sync completes, genesis has left the
    // ~200-block in-memory window, which made the old window-only fallback
    // report "unknown" forever. When the cache is empty, try the in-memory
    // anchor while we still hold the guard, and remember whether the durable
    // archive must be consulted — but do that AFTER the guard drops, on the
    // blocking pool: a synchronous redb read under the held read lock would
    // stall this worker and every blockchain writer behind it on each poll.
    //
    // #2762 residual: whichever fallback resolves is PROMOTED into the cache
    // (one-way, `GenesisHashCell`), so the archive is read at most once per
    // process instead of on every poll for the remaining life of a
    // fresh-synced joiner. Both fallback values already passed the height-1
    // anchor check — the window filter below, or the archive resolver's own —
    // which is the promotion precondition (a promotable cache that accepts
    // unvalidated writers is a poisonable one).
    let cached = state.genesis_hash.get();
    let from_window = if cached.is_none() {
        blockchain
            .blocks()
            .values()
            .next()
            .filter(|block| {
                block.height == 1
                    && block.prev_block_hash
                        == wrfcoin_shared_protocol::crypto::hash::Hash::default()
            })
            .map(|block| {
                let hash = block.hash.unwrap_or_else(|| block.calculate_hash());
                format!("0x{}", hash.to_hex())
            })
    } else {
        None
    };

    let height = blockchain.height;
    drop(blockchain);
    let transfer_fee = transfer_fee_info(height.saturating_add(1));

    let genesis_hash = if let Some(hash) = cached {
        hash
    } else if let Some(hash) = from_window {
        state.genesis_hash.promote(hash.clone());
        hash
    } else {
        // Clone the redb handle briefly under the async lock, then move
        // the decrypt/deserialize to the blocking pool — the same
        // discipline as the block-sync serve path (#2522/#2119). The
        // helper refuses to name a non-anchor lowest block "genesis".
        let storage_read = state.storage.read().await.clone();
        match tokio::task::spawn_blocking(move || {
            crate::helpers::resolve_genesis_from_archive(&storage_read)
        })
        .await
        .ok()
        .flatten()
        {
            Some(hash) => {
                state.genesis_hash.promote(hash.clone());
                hash
            },
            // NOT promoted: "unknown" is the absence of an answer, and caching
            // it would freeze a still-syncing joiner on "unknown" forever —
            // the original #2762 defect reintroduced through the fix.
            None => "unknown".to_string(),
        }
    };

    #[cfg(feature = "p2p")]
    let peer_count: u64 = state
        .p2p
        .as_ref()
        .map(|c| c.stats().connected_peers as u64)
        .unwrap_or(0);
    #[cfg(not(feature = "p2p"))]
    let peer_count: u64 = 0;

    #[cfg(feature = "p2p")]
    let local_peer_id: Option<String> =
        state.p2p.as_ref().map(|c| c.local_peer_id.to_string());
    #[cfg(not(feature = "p2p"))]
    let local_peer_id: Option<String> = None;

    HttpResponse::Ok().json(serde_json::json!({
        "network_id": std::env::var("WRFCOIN_NETWORK_ID").unwrap_or_else(|_| "unknown".into()),
        "chain_id": std::env::var("WRFCOIN_CHAIN_ID").unwrap_or_else(|_| "777".into()),
        "disclaimer_url": "https://wrfcoin.com/testnet-disclaimer.html",
        "node_version": env!("CARGO_PKG_VERSION"),
        // Short commit of the source, set at compile time by release builds (the Windows
        // installer workflow); null for local builds, so they stay reproducible.
        "node_build": option_env!("WRFCOIN_BUILD_COMMIT"),
        "genesis_hash": genesis_hash,
        "peer_count": peer_count,
        "sync_status": sync_status,
        "chain_height": height,
        "local_peer_id": local_peer_id,
        "transfer_fee": transfer_fee,
    }))
}

/// The burned transfer fee (`transfer_fee_burn`) for a transfer mined at
/// `next_height`, for wallets to show before a send. `base_units` is `"0"` and
/// `active` false on a chain that charges no fee.
pub(crate) fn transfer_fee_info(next_height: u64) -> serde_json::Value {
    use wrfcoin_shared_protocol::consensus::transfer_fee as fee;
    let charged = fee::transfer_fee_at(next_height);
    let base_units = charged.unwrap_or(0);
    serde_json::json!({
        "feature": wrfcoin_shared_protocol::crypto::chain_binding::ConsensusFeature::TransferFeeBurn.name(),
        "active": charged.is_some(),
        "scheduled": fee::transfer_fee_scheduled(),
        "next_height": next_height,
        "base_units": base_units.to_string(),
        "wrf": fee::format_wrf(base_units),
        "burned": true,
        "sponsorable": wrfcoin_shared_protocol::accounts::paymaster::sponsorship_active(next_height),
    })
}

// ---------------------------------------------------------------------------
// /network/peer_id — focused peer identity endpoint (infra#392)
// ---------------------------------------------------------------------------

#[cfg(feature = "p2p")]
pub async fn get_network_peer_id(state: web::Data<AppState>) -> impl Responder {
    if let Some(ref client) = state.p2p {
        let peer_id = client.local_peer_id.to_string();
        let p2p_port = std::env::var("WRFCOIN_P2P_PORT")
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(8555);
        let listen_addr = format!("/ip4/0.0.0.0/tcp/{p2p_port}");
        let listen_with_peer = format!("{listen_addr}/p2p/{peer_id}");
        let external_addr = std::env::var("WRFCOIN_P2P_EXTERNAL_ADDR").ok().map(|addr| {
            if addr.contains("/p2p/") {
                addr
            } else {
                format!("{addr}/p2p/{peer_id}")
            }
        });
        HttpResponse::Ok().json(serde_json::json!({
            "peer_id": peer_id,
            "listen_addrs": [listen_with_peer],
            "external_addr": external_addr,
        }))
    } else {
        HttpResponse::ServiceUnavailable()
            .json(serde_json::json!({"error": "P2P not enabled"}))
    }
}

// ---------------------------------------------------------------------------
// P2P network introspection endpoints (feature-gated)
// ---------------------------------------------------------------------------

#[cfg(feature = "p2p")]
pub async fn get_network_peers(state: web::Data<AppState>) -> impl Responder {
    if let Some(ref client) = state.p2p {
        let stats = client.stats();
        HttpResponse::Ok().json(serde_json::json!({
            "connected_peers": stats.connected_peers,
            "local_peer_id": client.local_peer_id.to_string(),
        }))
    } else {
        HttpResponse::ServiceUnavailable().json(serde_json::json!({"error": "P2P not enabled"}))
    }
}

// ---------------------------------------------------------------------------
// /network/dht/peers — Kademlia discovered-peer log (#1407)
// ---------------------------------------------------------------------------
//
// Kademlia emits `RoutingUpdated` events whenever it learns of a peer or
// refreshes its addresses. The p2p crate accumulates the most recent events
// in a bounded ring buffer; this endpoint exposes that buffer to operators
// so the DHT routing table is no longer a debug-log-only artifact.
#[cfg(feature = "p2p")]
pub async fn get_network_dht_peers(req: HttpRequest, state: web::Data<AppState>) -> impl Responder {
    // Peer IP addresses are sensitive topology info — gate behind Reader (#1829).
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    if let Some(ref client) = state.p2p {
        let peers = client.discovered_peers.snapshot();
        HttpResponse::Ok().json(serde_json::json!({
            "local_peer_id": client.local_peer_id.to_string(),
            "total": peers.len(),
            "peers": peers,
        }))
    } else {
        HttpResponse::ServiceUnavailable().json(serde_json::json!({"error": "P2P not enabled"}))
    }
}

// ---------------------------------------------------------------------------
// /network/peers/sync — per-peer sync rotation surface (#2820)
// ---------------------------------------------------------------------------
//
// The variable that decides sync-rotation eligibility — each peer's advertised
// ChainStatus height — was previously invisible outside node logs: during the
// 2026-08-27 producer-rollout stall every visible endpoint looked healthy
// while rotation was starved by stale advertisements. The p2p event loop now
// publishes a per-peer snapshot (advertised height, advertisement age,
// connection age, in-flight requests, failure/backoff state, and eligibility
// with the disqualifying reason) on its sync tick; this endpoint exposes it.
#[cfg(feature = "p2p")]
pub async fn get_network_peers_sync(
    req: HttpRequest,
    state: web::Data<AppState>,
) -> impl Responder {
    // Peer identity/topology is sensitive — gate behind Reader like
    // /network/dht/peers (#1829).
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    if let Some(ref client) = state.p2p {
        let peers = client.sync_peers.snapshot();
        HttpResponse::Ok().json(serde_json::json!({
            "local_peer_id": client.local_peer_id.to_string(),
            "total": peers.len(),
            "eligible": peers.iter().filter(|p| p.eligible).count(),
            "peers": peers,
        }))
    } else {
        HttpResponse::ServiceUnavailable().json(serde_json::json!({"error": "P2P not enabled"}))
    }
}

// ---------------------------------------------------------------------------
// /network/peers/reputation — accumulated peer-penalty reputation (#1406)
// ---------------------------------------------------------------------------
//
// Gossipsub peer scoring graylists misbehaving peers and surfaces the outcomes
// as `PeerPenalized` events; the P2P event loop accumulates them into a shared
// `PeerReputationTracker`. This endpoint exposes that tracker so operators can
// see per-peer reputation / graylist status — the penalty outcomes are no
// longer a log-only artifact. Worst reputation is listed first.
#[cfg(feature = "p2p")]
pub async fn get_network_peer_reputation(
    req: HttpRequest,
    state: web::Data<AppState>,
    reputation: web::Data<crate::peer_reputation::SharedPeerReputation>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }
    let peers = reputation.read().await.snapshot();
    let graylisted = peers.iter().filter(|entry| entry.graylisted).count();
    HttpResponse::Ok().json(serde_json::json!({
        "total": peers.len(),
        "graylisted": graylisted,
        "peers": peers,
    }))
}

#[cfg(feature = "p2p")]
pub async fn get_network_stats(state: web::Data<AppState>) -> impl Responder {
    if let Some(ref client) = state.p2p {
        let stats = client.stats();
        HttpResponse::Ok().json(serde_json::json!({
            "connected_peers": stats.connected_peers,
            "blocks_received": stats.blocks_received,
            "blocks_published": stats.blocks_published,
            "txs_received": stats.txs_received,
            "txs_published": stats.txs_published,
            "weather_msgs_received": stats.weather_msgs_received,
            "weather_msgs_published": stats.weather_msgs_published,
            "mesh_peers_by_topic": stats.mesh_peers_by_topic,
            "bytes_inbound": stats.bytes_inbound,
            "bytes_outbound": stats.bytes_outbound,
            "sync_local_height": stats.sync_local_height,
            "sync_target_height": stats.sync_target_height,
            "sync_state": stats.sync_state,
            "sync_pending_ranges": stats.sync_pending_ranges,
            "sync_in_flight_ranges": stats.sync_in_flight_ranges,
            "sync_outstanding_ranges": stats.sync_outstanding_ranges,
            "sync_requests_dispatched": stats.sync_requests_dispatched,
            "sync_responses_received": stats.sync_responses_received,
            "sync_transport_failures": stats.sync_transport_failures,
            "sync_ranges_requeued": stats.sync_ranges_requeued,
            "sync_peer_rotations": stats.sync_peer_rotations,
            "sync_last_resort_dispatches": stats.sync_last_resort_dispatches,
            "sync_connection_refresh_attempts": stats.sync_connection_refresh_attempts,
            "sync_inbound_requests": stats.sync_inbound_requests,
            "sync_responses_sent": stats.sync_responses_sent,
            "sync_inbound_failures": stats.sync_inbound_failures,
            "sync_reconnect_status_broadcasts": stats.sync_reconnect_status_broadcasts,
            "outgoing_dial_failures": stats.outgoing_dial_failures,
            "uptime_secs": stats.uptime_secs,
        }))
    } else {
        HttpResponse::ServiceUnavailable().json(serde_json::json!({"error": "P2P not enabled"}))
    }
}

// ---------------------------------------------------------------------------
// Node sync & resource endpoints (#935)
// ---------------------------------------------------------------------------

/// GET /node/sync/status — return sync status derived from chain state.
pub async fn get_node_sync_status(state: web::Data<AppState>) -> impl Responder {
    let blockchain = state.blockchain.read().await;

    let height = blockchain.height;

    let (sync_status, block_age_secs) = if let Some(last_block) = blockchain.last_block() {
        let age_secs = chrono::Utc::now()
            .signed_duration_since(last_block.timestamp)
            .num_seconds()
            .max(0);
        let status = sync_status_from_block_age(height, age_secs);
        (status, age_secs)
    } else {
        ("genesis", 0i64)
    };

    let blocks_in_memory = blockchain.blocks().len();
    drop(blockchain);

    // #1614: the downloader's own view, beside the block-age heuristic. The
    // heuristic cannot tell "syncing" from "halted"; `p2p_sync.state` can
    // (`stalled` / `validation_halted` are terminal — see `/readyz`).
    #[cfg(feature = "p2p")]
    let (peer_count, p2p_sync): (u64, Option<serde_json::Value>) = state
        .p2p
        .as_ref()
        .map(|c| {
            let stats = c.stats();
            (
                stats.connected_peers as u64,
                Some(json!({
                    "state": stats.sync_state,
                    "local_height": stats.sync_local_height,
                    "target_height": stats.sync_target_height,
                })),
            )
        })
        .unwrap_or((0, None));
    #[cfg(not(feature = "p2p"))]
    let (peer_count, p2p_sync): (u64, Option<serde_json::Value>) = (0, None);

    // #1798: surface WAN failover state in sync diagnostics so operators can
    // correlate sync health with primary↔LTE/5G fallback.
    let (wan_state, metered) = {
        let monitor = state.failover_monitor.read().await;
        (monitor.state().to_string(), monitor.is_metered())
    };

    HttpResponse::Ok().json(json!({
        "sync_status": sync_status,
        "chain_height": height,
        "block_age_secs": block_age_secs,
        "blocks_in_memory": blocks_in_memory,
        "peer_count": peer_count,
        "p2p_sync": p2p_sync,
        "wan_state": wan_state,
        "metered": metered,
    }))
}

/// GET /node/network/failover — WAN failover monitor state (#1798).
///
/// Exposes the operational LTE/5G fallback state machine so contributor nodes
/// can assert carrier-failover behavior from this repo. `wan_state` is one of
/// `PrimaryOnline` / `Probing` / `FailoverActive` / `Disconnected`; `metered`
/// is true while on the cellular backup. Node-local operational state — not
/// part of the consensus `state_root`.
pub async fn get_node_network_failover(state: web::Data<AppState>) -> impl Responder {
    let monitor = state.failover_monitor.read().await;
    HttpResponse::Ok().json(json!({
        "wan_state": monitor.state().to_string(),
        "metered": monitor.is_metered(),
        "failover_count": monitor.failover_count,
    }))
}

// Operator visibility policy (#3599), not a consensus timing parameter.
const RESOURCE_CHAIN_BLOCK_INTERVAL_SECS: i64 = 77;

fn resource_chain_status(age_secs: Option<i64>) -> &'static str {
    match age_secs {
        Some(age) if age < 3 * RESOURCE_CHAIN_BLOCK_INTERVAL_SECS => "producing",
        _ => "stalled",
    }
}

/// Newest recorded accepting-block time across stations, not observation time.
/// None means metadata unavailable (including chains before the recording feature).
fn resource_weather_freshness(timestamps: impl Iterator<Item = i64>, now: i64) -> (Option<i64>, Option<i64>) {
    let last = timestamps.max();
    (last, last.map(|at| now.saturating_sub(at).max(0)))
}

/// GET /node/resources — return process resources and chain production freshness.
pub async fn get_node_resources(state: web::Data<AppState>) -> impl Responder {
    let uptime_secs = NODE_PROCESS_START.get_or_init(Instant::now).elapsed().as_secs();
    let blockchain = state.blockchain.read().await;
    let blocks_in_memory = blockchain.blocks().len();
    let accounts = blockchain.state.accounts.len();
    let nfts = blockchain.state.nfts.len();
    // Clamp future timestamps to zero, matching /network/info's age semantics.
    // No tip means unavailable age, never evidence of production.
    let chain_last_block_age_secs = blockchain.last_block().map(|tip| {
        // determinism-ok: response-only operator telemetry; this age never enters consensus validation, persisted state or a state root.
        chrono::Utc::now()
            .signed_duration_since(tip.timestamp)
            .num_seconds()
            .max(0)
    });
    let chain_status = resource_chain_status(chain_last_block_age_secs);
    // Recording is feature-gated: null means unavailable metadata, not no weather.
    // determinism-ok: response-only weather acceptance age; never persisted or used by consensus.
    let now = chrono::Utc::now().timestamp();
    let (weather_last_accepted_block_unix, weather_last_accepted_block_age_secs) =
        resource_weather_freshness(blockchain.state.station_weather_accepted_block_unix.values().copied(), now);
    drop(blockchain);

    let mempool = state.mempool.read().await;
    let pending_tx = mempool.size();
    drop(mempool);

    HttpResponse::Ok().json(json!({
        "chain_last_block_age_secs": chain_last_block_age_secs,
        "chain_status": chain_status,
        "weather_last_accepted_block_unix": weather_last_accepted_block_unix,
        "weather_last_accepted_block_age_secs": weather_last_accepted_block_age_secs,
        "chain_block_interval_secs": RESOURCE_CHAIN_BLOCK_INTERVAL_SECS,
        "uptime_secs": uptime_secs,
        "blocks_in_memory": blocks_in_memory,
        "account_count": accounts,
        "nft_count": nfts,
        "pending_transactions": pending_tx,
    }))
}

// ---------------------------------------------------------------------------
// Contributor capability telemetry endpoint (#1801)
// ---------------------------------------------------------------------------

/// GET /node/capability — contributor hardware/network capability telemetry.
///
/// Returns a snapshot of the node's current hardware and network capabilities
/// as maintained by the WAN failover monitor and any other telemetry
/// publishers.  Before the background monitor is wired up, all fields reflect
/// safe defaults (`wan_state: "PrimaryOnline"`, `is_metered: false`, etc.).
///
/// Intentionally unauthenticated — capability state is observable by the
/// operator's own monitoring stack and does not expose chain-internal secrets.
pub async fn get_node_capability(state: web::Data<AppState>) -> impl Responder {
    let snap = state.node_capability.read().await.clone();
    // Propagate snapshot into NodeCapabilityProfile telemetry fields (#1802).
    // telemetry_lte_active tracks metered-link state (WAN failover to cellular).
    let telemetry_ap_active = snap.ap_mode_enabled;
    let telemetry_lte_active = snap.is_metered;
    let telemetry_mesh_peer_count = snap.mesh_peer_count as u32;
    HttpResponse::Ok().json(json!({
        "wan_state": snap.wan_state,
        "is_metered": snap.is_metered,
        "failover_count": snap.failover_count,
        "mesh_peer_count": snap.mesh_peer_count,
        "ap_mode_enabled": snap.ap_mode_enabled,
        "capability_profile": {
            "telemetry_ap_active": telemetry_ap_active,
            "telemetry_lte_active": telemetry_lte_active,
            "telemetry_mesh_peer_count": telemetry_mesh_peer_count,
        },
    }))
}

// ---------------------------------------------------------------------------
// Partition mode endpoints (#1217)
// ---------------------------------------------------------------------------

/// GET /node/partition-status — return the node's current partition mode.
///
/// Exposed as a read-only diagnostic endpoint so operators and upstream
/// monitoring can see whether the node is currently serving a minority
/// partition (ReadOnly), a middle partition (BlockProductionOnly), or a
/// healthy majority (Normal). The response also reports the capability
/// flags derived from the mode so clients do not have to re-implement the
/// `PartitionMode` decision logic.
///
/// This endpoint is intentionally unauthenticated — it reports observable
/// state that the operator's own mining loop and the network at large
/// need to discover during an incident.
pub async fn get_partition_status(state: web::Data<AppState>) -> impl Responder {
    let mode = *state.partition_mode.read().await;
    HttpResponse::Ok().json(json!({
        "mode": mode.label(),
        "allows_block_production": mode.allows_block_production(),
        "allows_finality_voting": mode.allows_finality_voting(),
        "accepts_mempool_writes": mode.accepts_mempool_writes(),
        "serves_queries": mode.serves_queries(),
    }))
}

/// POST /operator/partition-mode — operator push of a new partition mode.
///
/// Used by the partition orchestrator (outside `core4`) to tell the node
/// its current classification after a detection round, or by an
/// incident-response operator who wants to manually force a mode during
/// recovery. The setter propagates the change to both the `AppState`
/// field and the mempool in lockstep so HTTP and protocol-layer gates
/// agree.
///
/// Requires the [`ApiRole::Operator`] role. The mode is accepted from
/// the request body as one of the `PartitionMode` serde variants
/// (`"Normal"`, `"BlockProductionOnly"`, `"ReadOnly"`).
pub async fn set_partition_mode(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<SetPartitionModeRequest>,
) -> impl Responder {
    use wrfcoin_native_chain::security::api_auth::ApiRole;
    if let Err(resp) = crate::helpers::require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }

    let new_mode = request.mode;
    {
        let mut current = state.partition_mode.write().await;
        *current = new_mode;
    }
    {
        let mut mempool = state.mempool.write().await;
        mempool.set_partition_mode(new_mode);
    }

    log::warn!(
        "operator forced partition mode transition → {} (source: POST /operator/partition-mode)",
        new_mode.label()
    );

    HttpResponse::Ok().json(json!({
        "mode": new_mode.label(),
        "allows_block_production": new_mode.allows_block_production(),
        "accepts_mempool_writes": new_mode.accepts_mempool_writes(),
    }))
}

#[derive(Debug, Deserialize)]
pub struct SetPartitionModeRequest {
    pub mode: wrfcoin_shared_protocol::consensus::partition_policy::PartitionMode,
}

// ---------------------------------------------------------------------------
// GET /network/upgrade — the signed upgrade notice this node trusts (#3095)
// ---------------------------------------------------------------------------
//
// Unauthenticated read, like `/network/info`: it carries no secret, and the
// whole point is that an operator (or a peer, or a monitoring probe) can ask a
// node what it believes about the next activation without holding a token. The
// artifact it returns is governance-signed, so a caller that does not trust
// this node can verify the notice itself (#3094) rather than trusting the
// answer.

/// What a node reports about the upgrade it is heading for.
///
/// `notice` is the verified [`SignedUpgradeNotice`] verbatim — signatures
/// included — so a peer polling this endpoint re-verifies rather than trusting
/// the transport. `blocks_remaining` is `None` exactly when there is no
/// notice.
#[derive(Debug, Serialize, Deserialize)]
pub struct NetworkUpgradeResponse {
    /// `none` | `ready` | `upgrade_required` | `past_deadline`.
    pub status: String,
    /// This binary's `PROTOCOL_VERSION` (#3093).
    pub local_protocol_version: u32,
    pub tip_height: u64,
    /// `activation_height - tip_height`, saturating; `None` with no notice.
    pub blocks_remaining: Option<u64>,
    /// Convenience mirrors of the notice fields a poller keys off, so the
    /// attester need not reach into the signed body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activation_height: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_protocol_version: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_commit: Option<String>,
    /// The verified signed notice, or `null` when none is installed.
    pub notice: Option<wrfcoin_shared_protocol::governance::upgrade_notice::SignedUpgradeNotice>,
    /// #3277 S6: the EFFECTIVE feature schedule — base (genesis, per-chain
    /// bootstrap) plus applied schedule extensions — with each entry's source,
    /// whether this binary implements it, and run-up warnings for the ones it
    /// does not. Independent of `status`, which is about the signed notice.
    pub scheduled: wrfcoin_native_chain::schedule_view::ScheduledView,
    /// #3361 F4: the ADR-003 faucet sink the next block pays, and why.
    /// Block metadata `faucet_rewards_minted_total` is PER-BLOCK, not a
    /// running total.
    pub faucet_sink: wrfcoin_native_chain::schedule_view::FaucetSinkView,
}

pub async fn get_network_upgrade(state: web::Data<AppState>) -> impl Responder {
    use wrfcoin_native_chain::schedule_view::{
        ScheduleViewInputs, build_scheduled_view, describe_faucet_sink,
    };
    use wrfcoin_shared_protocol::crypto::chain_binding as cb;

    // One read of the CANONICAL chain: the tip, its applied extensions and
    // its genesis emission config are taken together, so the view is never a
    // mix of two heights.
    let (tip_height, scheduled, faucet_sink) = {
        let chain = state.blockchain.read().await;
        let tip_height = chain.height;
        let next_height = tip_height.saturating_add(1);
        let base = wrfcoin_native_chain::blockchain::state::State::base_feature_schedule();
        let scheduled = build_scheduled_view(&ScheduleViewInputs {
            base: &base,
            genesis_schedule_raw: cb::try_genesis_activation_schedule_string(),
            extensions: &chain.state.feature_schedule_extensions,
            tip_height,
            known: wrfcoin_shared_protocol::crypto::protocol_support::KNOWN_FEATURES,
            local_protocol_version:
                wrfcoin_shared_protocol::crypto::protocol_support::PROTOCOL_VERSION,
            version_at_next: cb::protocol_version_at(next_height).as_u32(),
            base_provisional: cb::base_feature_schedule_is_provisional(),
            halt_deadline: wrfcoin_native_chain::upgrade::upgrade_deadline(),
        });
        let effective = cb::effective_feature_schedule();
        let faucet_sink = describe_faucet_sink(
            chain.state.genesis_emission_config.as_ref(),
            next_height,
            cb::LEGACY_FAUCET_BINDINGS,
            cb::feature_active,
            |name| effective.activation_height(name),
        );
        (tip_height, scheduled, faucet_sink)
    };
    let signed = wrfcoin_native_chain::upgrade_notice::upgrade_notice();
    let status = wrfcoin_native_chain::upgrade_notice::status_of(
        signed.as_ref().map(|s| &s.notice),
        tip_height,
        wrfcoin_shared_protocol::crypto::protocol_support::PROTOCOL_VERSION,
    );

    HttpResponse::Ok().json(NetworkUpgradeResponse {
        status: status.as_str().to_string(),
        local_protocol_version:
            wrfcoin_shared_protocol::crypto::protocol_support::PROTOCOL_VERSION,
        tip_height,
        blocks_remaining: signed.as_ref().map(|s| {
            wrfcoin_native_chain::upgrade_notice::blocks_remaining(&s.notice, tip_height)
        }),
        activation_height: signed.as_ref().map(|s| s.notice.activation_height),
        min_protocol_version: signed.as_ref().map(|s| s.notice.min_protocol_version),
        release_commit: signed.as_ref().map(|s| s.notice.release_commit.clone()),
        notice: signed,
        scheduled,
        faucet_sink,
    })
}

/// Default and maximum number of recent blocks `/network/producers` scans for
/// per-key activity. Only in-memory blocks are scanned (never storage), so the
/// work is bounded by `MAX_PRODUCER_ACTIVITY_WINDOW` under one read lock.
pub const DEFAULT_PRODUCER_ACTIVITY_WINDOW: u64 = 256;
pub const MAX_PRODUCER_ACTIVITY_WINDOW: u64 = 2048;

#[derive(Debug, Deserialize)]
pub struct ProducersQuery {
    pub window: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct ProducerView {
    pub producer_id: String,
    /// Hex ed25519 key that authenticates this producer's blocks.
    pub public_key: String,
    pub payout_address: String,
    /// Newest height in the scanned window whose block this key signed.
    pub last_produced_height: Option<u64>,
    pub blocks_in_window: u64,
}

#[derive(Debug, Serialize)]
pub struct ProducerActivityWindow {
    /// Requested window after clamping to `1..=MAX_PRODUCER_ACTIVITY_WINDOW`.
    pub requested: u64,
    /// Lowest and highest heights actually scanned (`null` when nothing was).
    pub from_height: Option<u64>,
    pub to_height: Option<u64>,
    pub blocks_scanned: u64,
}

#[derive(Debug, Serialize)]
pub struct NetworkProducersResponse {
    pub tip_height: u64,
    /// The set below authorizes the block at this height (`tip_height + 1`).
    pub authorizes_height: u64,
    /// `epoch` / `effective_height` of the producer-set change in force; `null`
    /// when the chain carries no producer-set history.
    pub set_epoch: Option<u64>,
    pub set_effective_height: Option<u64>,
    pub producer_set_hash: Option<String>,
    pub producers: Vec<ProducerView>,
    pub activity_window: ProducerActivityWindow,
}

/// `GET /network/producers` — read-only view of the authorized producer set
/// (`State::producer_set_history`) plus cheap recent activity per key.
pub async fn get_network_producers(
    req: HttpRequest,
    state: web::Data<AppState>,
    query: web::Query<ProducersQuery>,
) -> impl Responder {
    use wrfcoin_shared_protocol::consensus::producer_set::validate_producer_set_history;
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let requested = query
        .window
        .unwrap_or(DEFAULT_PRODUCER_ACTIVITY_WINDOW)
        .clamp(1, MAX_PRODUCER_ACTIVITY_WINDOW);
    let chain = state.blockchain.read().await;
    let history = &chain.state.producer_set_history;
    if validate_producer_set_history(history).is_err() {
        return envelope_error(
            &req,
            actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
            "PRODUCER_SET_INVALID",
            "producer set history is invalid",
        );
    }
    let tip_height = chain.height;
    let authorizes_height = tip_height.saturating_add(1);
    // Validated history is in strictly-increasing effective_height order, so the
    // change in force is the last one at or below the height it authorizes.
    let active = history.iter().rfind(|c| c.effective_height <= authorizes_height);
    let mut producers: Vec<ProducerView> = active
        .map(|c| {
            c.canonical_members()
                .into_iter()
                .map(|m| ProducerView {
                    producer_id: m.producer_id.clone(),
                    public_key: hex::encode(m.public_key),
                    payout_address: m.payout_address.clone(),
                    last_produced_height: None,
                    blocks_in_window: 0,
                })
                .collect()
        })
        .unwrap_or_default();
    let keys: Vec<[u8; 32]> = active
        .map(|c| c.canonical_members().into_iter().map(|m| m.public_key).collect())
        .unwrap_or_default();
    let mut from_height = None;
    let mut to_height = None;
    let mut blocks_scanned = 0u64;
    for (height, block) in chain.blocks().iter().rev().take(requested as usize) {
        to_height.get_or_insert(*height);
        from_height = Some(*height);
        blocks_scanned += 1;
        let index = keys.iter().position(|k| *k == block.producer_pubkey);
        if let Some(view) = index.and_then(|i| producers.get_mut(i)) {
            view.blocks_in_window += 1;
            view.last_produced_height.get_or_insert(*height);
        }
    }
    HttpResponse::Ok().json(NetworkProducersResponse {
        tip_height,
        authorizes_height,
        set_epoch: active.map(|c| c.epoch),
        set_effective_height: active.map(|c| c.effective_height),
        producer_set_hash: active.map(|c| c.producer_set_hash().to_hex()),
        producers,
        activity_window: ProducerActivityWindow {
            requested,
            from_height,
            to_height,
            blocks_scanned,
        },
    })
}

#[cfg(all(test, feature = "p2p"))]
mod tests {
    use crate::peer_reputation::{PeerReputationTracker, SharedPeerReputation, apply_peer_penalty};
    use libp2p::PeerId;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    // Tests the tracker snapshot ordering in isolation; HTTP-layer auth-gate
    // coverage lives in tests/network_routes.rs where test_state() is available.
    #[actix_web::test]
    async fn peer_reputation_snapshot_orders_worst_first() {
        let tracker: SharedPeerReputation = Arc::new(RwLock::new(PeerReputationTracker::new()));
        let banned = PeerId::random();
        let mild = PeerId::random();
        {
            let mut guard = tracker.write().await;
            let _ = apply_peer_penalty(&mut guard, &banned, "peer_banned", true);
            let _ = apply_peer_penalty(&mut guard, &mild, "invalid_message", false);
        }

        let snapshot = tracker.read().await.snapshot();
        assert_eq!(snapshot.len(), 2);
        // Worst reputation first: the banned peer (reputation -100, graylisted).
        assert_eq!(snapshot[0].peer_id, banned.to_string());
        assert!(snapshot[0].graylisted);
        assert_eq!(snapshot[0].ban_count, 1);
        assert_eq!(snapshot[1].peer_id, mild.to_string());
        assert!(!snapshot[1].graylisted);
    }
}

#[cfg(test)]
mod resource_chain_status_tests {
    use super::{RESOURCE_CHAIN_BLOCK_INTERVAL_SECS, resource_chain_status};

    #[test]
    fn producing_is_strictly_below_three_intervals() {
        assert_eq!(RESOURCE_CHAIN_BLOCK_INTERVAL_SECS, 77);
        assert_eq!(resource_chain_status(Some(0)), "producing");
        assert_eq!(resource_chain_status(Some(230)), "producing");
        assert_eq!(resource_chain_status(Some(231)), "stalled");
        assert_eq!(resource_chain_status(Some(232)), "stalled");
        assert_eq!(resource_chain_status(Some(i64::MAX)), "stalled");
        assert_eq!(resource_chain_status(None), "stalled");
    }
}

#[cfg(test)]
mod resource_weather_tests {
    use super::resource_weather_freshness;

    #[test]
    fn resource_weather_reports_missing_newest_and_stale_acceptance_times() {
        assert_eq!(resource_weather_freshness([].into_iter(), 1000), (None, None));
        assert_eq!(resource_weather_freshness([100, 900, 400].into_iter(), 1000), (Some(900), Some(100)));
        assert_eq!(resource_weather_freshness([100].into_iter(), 1000), (Some(100), Some(900)));
    }

    #[test]
    fn resource_weather_clamps_future_timestamps_and_saturates_age() {
        assert_eq!(resource_weather_freshness([1001].into_iter(), 1000), (Some(1001), Some(0)));
        assert_eq!(resource_weather_freshness([i64::MIN].into_iter(), i64::MAX),
            (Some(i64::MIN), Some(i64::MAX)));
    }
}
