use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(feature = "execution")]
use rustc_hash::FxHashMap;
use tokio::sync::{Mutex, RwLock, mpsc};
use wrfcoin_consensus_engines::{
    BftFinalityGadget, FinalityCertificate, FinalityEvent, FinalityVote, PopConsensusAdapter,
    TypedConsensusManager,
};
use wrfcoin_native_chain::blockchain::block::Block;
use wrfcoin_native_chain::blockchain::chain::{Blockchain, MAX_REORG_DEPTH};
use wrfcoin_native_chain::blockchain::transaction::{
    CommitPayload, RevealPayload, Transaction, TransactionInput, TransactionOutput,
    TransactionType,
};
use wrfcoin_native_chain::crypto::hash::{Hash as CryptoHash, hash_bytes};
use wrfcoin_native_chain::mempool::pool::Mempool;
use wrfcoin_native_chain::p2p_divergence::{
    ChainDivergenceObservation, ChainStateView, LocalCanonicalRoot,
    StateDivergenceRecoveryPolicy, observe_local_canonical_root,
    observe_peer_chain_status_with_history,
};
use wrfcoin_native_chain::storage::Storage;
use wrfcoin_p2p::{
    P2pClient, P2pCommand, P2pEvent,
    sync::{AuthoritativeSyncRejectionAttribution, BlockResponse},
};
use wrfcoin_shared_protocol::consensus::finality::{
    FinalitySignal, FinalitySignalSender, emit_finality_signal,
};
use wrfcoin_shared_protocol::monitoring::CORE_METRICS;
#[cfg(feature = "execution")]
use wrfcoin_shared_protocol::blockchain::transaction::{
    forecast_receipt_has_valid_signature, ForecastArtifactManifest, ForecastVerificationReceipt,
};
use wrfcoin_shared_protocol::weather::schema::{
    canonicalize_weather_payload, classify_weather_payload, WeatherPayloadClassification,
    WeatherPayloadParseOptions,
};

#[cfg(feature = "p2p")]
use crate::da_challenge_producer::DaChallengeRuntime;
#[cfg(feature = "p2p")]
use crate::helpers::{
    broadcast_p2p_chain_status, notify_p2p_chain_advanced, update_p2p_chain_state,
};
use crate::transition_extractor::extract_transitions;

/// Maximum number of out-of-order blocks we will buffer while waiting for
/// missing ancestors to arrive via the block-sync response path. Sized to
/// comfortably cover the worst-case gossipsub cache of recent tips seen
/// during bringup (~100 × safety factor).
const ORPHAN_BUFFER_MAX_ENTRIES: usize = 512;

/// In-memory buffer of blocks received from peers whose height is ahead of
/// our chain by more than one — they're "orphans" with respect to the local
/// view because their parent isn't applied yet. Re-applied automatically
/// after the block-sync backfill path catches up to them.
struct OrphanBlockBuffer {
    /// Blocks keyed by height. Multiple blocks at the same height are
    /// possible if different peers gossip different candidate tips.
    blocks: BTreeMap<u64, Vec<Block>>,
    max_entries: usize,
}

impl OrphanBlockBuffer {
    fn new() -> Self {
        Self {
            blocks: BTreeMap::new(),
            max_entries: ORPHAN_BUFFER_MAX_ENTRIES,
        }
    }

    fn insert(&mut self, block: Block) {
        // Deduplicate by hash at the same height so repeated gossip
        // broadcasts of the same tip don't inflate the buffer.
        let entries = self.blocks.entry(block.height).or_default();
        let new_hash = block.hash.unwrap_or_else(|| block.calculate_hash());
        if entries
            .iter()
            .any(|existing| existing.hash.unwrap_or_else(|| existing.calculate_hash()) == new_hash)
        {
            return;
        }
        entries.push(block);

        // Evict oldest (lowest height) entries if we exceed capacity.
        // Once the real chain catches up those lower-height orphans will no
        // longer be relevant anyway.
        while self.total_count() > self.max_entries {
            if let Some(&oldest) = self.blocks.keys().next() {
                self.blocks.remove(&oldest);
            } else {
                break;
            }
        }
    }

    fn total_count(&self) -> usize {
        self.blocks.values().map(|v| v.len()).sum()
    }

    /// Remove and return all buffered blocks with the given height.
    fn take_height(&mut self, height: u64) -> Vec<Block> {
        self.blocks.remove(&height).unwrap_or_default()
    }

    /// Drop any buffered blocks at or below the given chain height — their
    /// applicability window has passed.
    fn prune_below_or_equal(&mut self, height: u64) {
        self.blocks.retain(|&h, _| h > height);
    }
}

/// In-memory buffer of blocks received from peers that are at-or-below our
/// current height — potential fork candidates. Keyed by block height so we
/// can reconstruct candidate chains when a longer fork is detected.
struct ForkCandidateBuffer {
    /// Blocks keyed by height. Multiple blocks at the same height are
    /// possible (from different forks).
    blocks: BTreeMap<u64, Vec<Block>>,
    /// Maximum number of entries before we start evicting the oldest.
    max_entries: usize,
}

impl ForkCandidateBuffer {
    fn new() -> Self {
        Self {
            blocks: BTreeMap::new(),
            max_entries: MAX_REORG_DEPTH as usize * 2,
        }
    }

    fn insert(&mut self, block: Block) {
        self.blocks.entry(block.height).or_default().push(block);

        // Evict oldest entries if we exceed capacity.
        while self.total_count() > self.max_entries {
            if let Some(&oldest) = self.blocks.keys().next() {
                self.blocks.remove(&oldest);
            } else {
                break;
            }
        }
    }

    fn total_count(&self) -> usize {
        self.blocks.values().map(|v| v.len()).sum()
    }

    /// Try to build a contiguous chain of candidate blocks starting from
    /// `start_height` up to the highest available height. The chain must
    /// be internally consistent (each block's `prev_block_hash` matches
    /// the previous block's hash).
    ///
    /// Returns `None` if no contiguous chain can be formed.
    fn build_chain_from(&self, start_height: u64, anchor_hash: CryptoHash) -> Option<Vec<Block>> {
        let mut chain = Vec::new();
        let mut expected_prev = anchor_hash;

        let mut h = start_height;
        loop {
            let candidates = self.blocks.get(&h)?;
            // Find a block at this height whose prev_block_hash matches.
            let matching = candidates
                .iter()
                .find(|b| b.prev_block_hash == expected_prev)?;
            let blk_hash = matching.hash.unwrap_or_else(|| matching.calculate_hash());
            chain.push(matching.clone());
            expected_prev = blk_hash;
            h += 1;
            // Stop if no blocks at next height.
            if !self.blocks.contains_key(&h) {
                break;
            }
        }

        if chain.is_empty() { None } else { Some(chain) }
    }
}

/// What the serve-admission step decided for one inbound `BlockRequest`.
///
/// Extracted from the event handler so the decision is reachable by tests.
/// Before this existed, a RED control that made
/// [`SyncServeGovernor::admit`](wrfcoin_p2p::SyncServeGovernor::admit) return
/// `Serve { u32::MAX, u64::MAX }` unconditionally — the governor present,
/// called, type-checking, and enforcing NOTHING — left every `launch_api` test
/// green. The unit tests in `sync_limits` failed 9 of 12, so the governor
/// itself was covered; what nothing covered was this binary consulting it.
#[cfg_attr(not(feature = "p2p"), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
enum SyncServeAdmission {
    /// Serve up to `max_blocks`, stopping once `byte_budget` is reached.
    Serve { max_blocks: u32, byte_budget: u64 },
    /// Refuse. `label` is the metric reason and `retry_after` is carried on the
    /// block-sync wire as `retry_after_ms`.
    Refused { label: &'static str, retry_after: std::time::Duration },
}

/// Ask the governor whether this peer may be served now.
///
/// A thin mapping rather than a policy of its own: the policy lives in
/// `SyncServeGovernor`, and this exists so the binary's use of it is testable.
/// Kept free of I/O and metrics so a test needs only a governor and a clock —
/// the caller increments the counter, because a function that mutates a global
/// registry cannot be asserted on twice in one test binary.
#[cfg_attr(not(feature = "p2p"), allow(dead_code))]
fn decide_sync_serve_admission(
    governor: &mut wrfcoin_p2p::SyncServeGovernor,
    peer_key: &str,
    requested: u32,
    now: std::time::Instant,
) -> SyncServeAdmission {
    match governor.admit(peer_key, requested, now) {
        wrfcoin_p2p::ServeDecision::Serve {
            max_blocks,
            byte_budget,
        } => SyncServeAdmission::Serve {
            max_blocks,
            byte_budget,
        },
        wrfcoin_p2p::ServeDecision::Reject(rejection) => SyncServeAdmission::Refused {
            label: rejection.label(),
            retry_after: rejection.retry_after(),
        },
    }
}

/// #2579: global bound on concurrent block-sync serve tasks.
///
/// Admission rate-limits each PEER, but per-peer timing is not a global
/// storage-concurrency bound: with serving off the event loop, up to
/// `peers × (serve_latency / min_request_interval)` blocking reads could
/// otherwise pile up. Four concurrent redb range reads is ample for the
/// ~12-node testnet — a serving node sees a handful of IBD peers, each with
/// at most `MAX_PARALLEL_PEERS` ranges in flight — and a refused serve is
/// answered empty, which the requester already treats as a short response
/// and requeues (the same backpressure shape as the byte budget).
#[cfg_attr(not(feature = "p2p"), allow(dead_code))]
const MAX_CONCURRENT_SYNC_SERVES: usize = 4;

/// How one served (or refused) block-sync request is answered.
///
/// Abstracted from `P2pCommand::RespondToBlockSyncRequest` because libp2p's
/// `InboundRequestId` has a private constructor: no test can fabricate the
/// `P2pEvent` that carries one, which is why the serving lifecycle was only
/// unit-tested at the governor before #2579. The event-loop arm binds this to
/// the real command channel; tests bind it to a channel they can observe.
#[cfg_attr(not(feature = "p2p"), allow(dead_code))]
type SyncServeResponder = Box<
    dyn FnOnce(
            wrfcoin_p2p::sync::BlockResponse,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
        + Send,
>;

/// #2579: the full serve lifecycle for one inbound block-sync request —
/// concurrency permit, admission (which RESERVES the byte budget), the
/// spawned read, settlement, and the response.
///
/// Runs inline on the event loop only up to the spawn: everything before it
/// is a semaphore check and a map lookup, and everything after it — the redb
/// read, settlement, the response send — happens on the spawned task, so a
/// slow disk range can never delay `NewBlock`, `SyncedBlock`, peer penalties,
/// or another peer's request.
///
/// Returns `Some((label, retry_after))` when refused (the caller counts the
/// metric — a function that mutates a global registry cannot be asserted on
    /// twice in one test binary), `None` when a serve task was spawned. The
    /// refusal is answered with an empty, wire-visible refusal response before
    /// returning.
#[cfg_attr(not(feature = "p2p"), allow(dead_code))]
#[allow(clippy::too_many_arguments)]
async fn handle_inbound_block_sync_request(
    sync_serve_governor: &Arc<Mutex<wrfcoin_p2p::SyncServeGovernor>>,
    sync_serve_permits: &Arc<tokio::sync::Semaphore>,
    blockchain: &Arc<RwLock<Blockchain>>,
    storage: &Arc<RwLock<Storage>>,
    peer_key: String,
    start_height: u64,
    requested: u32,
    now: std::time::Instant,
    respond: SyncServeResponder,
) -> Option<(&'static str, std::time::Duration)> {
    let empty = |retry_after_ms| wrfcoin_p2p::sync::BlockResponse {
        blocks: Vec::new(),
        retry_after_ms,
    };

    // Concurrency BEFORE admission: a request refused for lack of a serve slot
    // must not consume the peer's rate/byte allowance — the node is busy, the
    // peer did nothing wrong. `try_acquire` rather than `acquire().await` so a
    // full house refuses cheaply instead of queueing unboundedly (the queue
    // would be the same unbounded-work problem one task-spawn later).
    let Ok(permit) = Arc::clone(sync_serve_permits).try_acquire_owned() else {
        respond(empty(Some(0))).await;
        // A slot frees on no schedule, so zero is the honest wire deadline.
        return Some(("serve_busy", std::time::Duration::ZERO));
    };

    let (max_blocks, byte_budget) = match decide_sync_serve_admission(
        &mut *sync_serve_governor.lock().await,
        &peer_key,
        requested,
        now,
    ) {
        SyncServeAdmission::Serve {
            max_blocks,
            byte_budget,
        } => (max_blocks, byte_budget),
        SyncServeAdmission::Refused { label, retry_after } => {
            // Answer empty rather than buffering the request.
            //
            // The wire-visible retry deadline distinguishes refusal from a
            // legitimate empty range, so the requester defers this exact
            // range instead of busy-looping it. Admission remains a map
            // lookup before any storage read.
            let retry_after_ms = retry_after
                .as_millis()
                .min(u128::from(u64::MAX)) as u64;
            respond(empty(Some(retry_after_ms))).await;
            return Some((label, retry_after));
        },
    };

    // Admission reserved `byte_budget` against the peer's window; from here on
    // every path must settle, or the reservation only frees via the governor's
    // one-window expiry backstop (which also covers this task panicking).
    let serve_chain = Arc::clone(blockchain);
    let serve_storage = Arc::clone(storage);
    let serve_governor = Arc::clone(sync_serve_governor);
    tokio::spawn(async move {
        // Hold the serve slot for the read AND the response send: the bound is
        // on outstanding serve work, not just on blocking-pool occupancy.
        let _permit = permit;
        let (response, bytes_served) = collect_block_sync_response(
            &serve_chain,
            &serve_storage,
            start_height,
            max_blocks,
            byte_budget,
            Some(&peer_key),
        )
        .await;
        // Settle at COMPLETION time, not with the admission timestamp: the
        // charge must age in the window from when the bytes actually moved.
        // Same non-consensus surface as the admission clock read.
        // determinism-ok: node-local serve rate limiting; never enters state, WAL or wire (#2119/#2579)
        let settled_at = std::time::Instant::now();
        serve_governor
            .lock()
            .await
            .settle_served(&peer_key, byte_budget, bytes_served, settled_at);
        respond(response).await;
    });
    None
}

/// Build a `BlockResponse` for an inbound `BlockRequest`, falling through
/// from the in-memory window to persistent storage when in-memory does not
/// cover the requested range (#1664 round 3).
///
/// Before this helper, the inbound-sync handler called only
/// `chain.blocks_range(start, end)`, which reads from the in-memory
/// `BTreeMap` (capped at `WRFCOIN_MAX_IN_MEMORY_BLOCKS`, default 200). A
/// peer requesting an older range — e.g. heights 6101..=6200 while our
/// in-memory window is 6170..=6370 — got either an empty response or a
/// partial response of recent blocks that could not chain onto the
/// requester's tip. Every block in that partial response was treated as
/// an orphan by `handle_response::ingest_block` (`block.height >
/// expected_height`), the range was requeued, and the requester stalled
/// in an infinite retry loop.
///
/// The helper queries storage when the in-memory result does not start
/// at the requested `start` height (either fully empty, or a gap at the
/// low end). Storage uses redb, which holds the full blockchain
/// history, so `load_blocks_range` always returns the persisted blocks
/// for any committed height.
async fn collect_block_sync_response(
    blockchain: &Arc<RwLock<Blockchain>>,
    storage: &Arc<RwLock<Storage>>,
    start: u64,
    count: u32,
    byte_budget: u64,
    peer_label: Option<&str>,
) -> (BlockResponse, u64) {
    let count = count.min(wrfcoin_p2p::sync::MAX_BLOCKS_PER_REQUEST);
    let end = start.saturating_add(u64::from(count.saturating_sub(1)));

    // Try in-memory first — for recent ranges this avoids a redb read and
    // matches the prior fast-path behaviour.
    let in_memory: Vec<Block> = {
        let chain = blockchain.read().await;
        chain
            .blocks_range(start, end)
            .take(count as usize)
            .cloned()
            .collect()
    };

    let need_storage = match in_memory.first() {
        None => true,
        Some(first) => first.height > start,
    };

    let blocks = if need_storage {
        // `redb` range reads decrypt and deserialize synchronously. Clone the
        // read handle while briefly holding the async lock, then move the I/O
        // itself to Tokio's blocking pool. Holding the lock or doing redb I/O
        // on the event-loop worker would let a legal old-range request stall
        // P2P gossip and block application (#2522).
        let storage_read = storage.read().await.clone();
        // #2119 P1: bound the WORK, not just the response. The unbounded loader
        // decrypts and deserializes the whole range before any trimming can run,
        // so a hostile request still bought up to 100 full block decodes even
        // though the reply was trimmed afterwards. This stops at the budget.
        match tokio::task::spawn_blocking(move || {
            storage_read.load_blocks_range_bounded(start, end, byte_budget)
        })
        .await
        .unwrap_or_else(|error| Err(wrfcoin_native_chain::storage::StorageError::Serialization(format!("block-sync read task failed: {error}")))) {
            Ok((mut from_storage, _stored_bytes)) => {
                from_storage.truncate(count as usize);
                from_storage
            },
            Err(err) => {
                if let Some(label) = peer_label {
                    log::warn!(
                        "Failed to load blocks {start}..={end} from storage \
                         while serving {label}: {err}; falling back to in-memory only"
                    );
                } else {
                    log::warn!(
                        "Failed to load blocks {start}..={end} from storage: {err}; \
                         falling back to in-memory only"
                    );
                }
                in_memory
            },
        }
    } else {
        in_memory
    };

    let (blocks, bytes) = truncate_to_byte_budget(blocks, byte_budget);

    // #2119 proportionality signal — Cindy's `serve_verdict` (#2517), fed by the
    // byte total `truncate_to_byte_budget` ALREADY computed.
    //
    // Her original called `bincode::serialize(&blocks)` purely to obtain a length,
    // and her own comment names the cost: "cloning up to 16 MiB of blocks to
    // measure them would be a regression in the one function whose job is bounding
    // served work." Reading the count out of the truncation removes even the
    // serialize — the per-block sizes were computed to enforce the budget, so the
    // metric is free.
    //
    // It now also measures the response actually sent, after truncation, rather
    // than the pre-trim vector. The two checks answer different questions and both
    // are worth having: the byte budget bounds ABSOLUTE cost, this bounds cost
    // RELATIVE to what was asked for. A 1-block request yielding a large response
    // is disproportionate even while under the absolute cap — which is exactly the
    // asymmetry she quantified (12-byte request, 16 MiB ceiling, ~1.4e6x).
    //
    // Refusal stays behind her `enforce` flag, default false. The split is
    // deliberate: serving SHORT is backpressure the requester already handles
    // (`handle_response` requeues the remainder), so it is safe to enforce now;
    // refusing outright on a ratio is the judgement that wants live data first.
    let limits = wrfcoin_p2p::sync::SyncServeLimits::default();
    let verdict = wrfcoin_p2p::sync::serve_verdict(count, bytes as usize, &limits);
    if let wrfcoin_p2p::sync::ServeVerdict::Exceeds { estimated, budget } = verdict {
        log::warn!(
            "block-sync amplification: peer {} requested {count} block(s) from height \
             {start} and would be served {estimated} bytes against a {budget}-byte budget \
             (#2119)",
            peer_label.unwrap_or("<unknown>")
        );
        if verdict.should_refuse(&limits) {
            return (BlockResponse { blocks: Vec::new(), retry_after_ms: None }, 0);
        }
    }

    (BlockResponse { blocks, retry_after_ms: None }, bytes)
}

/// Trim a response to `byte_budget` (#2119), returning the kept blocks and the
/// bytes they serialize to.
///
/// A short response is the protocol's own backpressure: `SyncManager::handle_response`
/// penalises a peer for returning MORE blocks than requested but treats fewer as
/// normal and requeues the unserved remainder. So trimming costs an honest peer a
/// round trip and costs a hostile one its amplification.
///
/// **At least one block is always kept.** If a single block exceeds the budget,
/// serving zero would stall that peer's IBD permanently — it would re-request the
/// same range forever and never advance. One oversized block is still bounded by
/// the codec's `MAX_SYNC_MESSAGE_BYTES`, so the worst case is one large response,
/// not an unbounded one. Pinned by
/// `a_single_oversized_block_is_still_served_so_ibd_cannot_stall_2119`.
fn truncate_to_byte_budget(blocks: Vec<Block>, byte_budget: u64) -> (Vec<Block>, u64) {
    let mut used = 0u64;
    let mut kept = 0usize;
    for block in &blocks {
        // Size without allocating the encoding.
        let size = bincode::serialized_size(block).unwrap_or(u64::MAX);
        if kept > 0 && used.saturating_add(size) > byte_budget {
            break;
        }
        used = used.saturating_add(size);
        kept += 1;
    }
    let mut blocks = blocks;
    blocks.truncate(kept);
    (blocks, used)
}

fn parse_weather_observation_timestamp(payload: &serde_json::Value) -> i64 {
    let extract_timestamp = |value: &serde_json::Value| -> Option<i64> {
        match value {
            serde_json::Value::Number(number) => number.as_i64(),
            serde_json::Value::String(raw) => raw.parse::<i64>().ok(),
            _ => None,
        }
    };

    for key in ["observed_at_unix", "observed_at", "timestamp"] {
        if let Some(value) = payload.get(key).and_then(&extract_timestamp) {
            return value;
        }
    }
    if let Some(observation) = payload.get("observation") {
        for key in ["observed_at_unix", "observed_at", "timestamp"] {
            if let Some(value) = observation.get(key).and_then(&extract_timestamp) {
                return value;
            }
        }
    }

    chrono::Utc::now().timestamp()
}

async fn bridge_weather_payload_to_mempool(
    station_id: &str,
    raw_payload: &[u8],
    mempool: &Arc<RwLock<Mempool>>,
    // Node-local weather identity key (#1969 gossip signer). The bridge signs the
    // commit authorization (#1848) with it so the ingested contribution passes
    // fail-closed mempool admission instead of being silently dropped (#2389).
    // `verify_commit_authorization` is self-certifying (it does not bind the
    // pubkey to `station_id`), and contribution rewards are separately gated by
    // oracle-attestation quorum, so authorizing with the node's own key vouches
    // for the submission without becoming a reward-forgery vector.
    commit_signing_key: &wrfcoin_native_chain::crypto::signature::PrivateKey,
) {
    let payload: serde_json::Value = match serde_json::from_slice(raw_payload) {
        Ok(value) => value,
        Err(error) => {
            log::warn!("Failed to decode weather payload JSON from station {station_id}: {error}");
            return;
        },
    };

    let classification = classify_weather_payload(&payload, WeatherPayloadParseOptions::default());
    match classification {
        WeatherPayloadClassification::NonWeather
        | WeatherPayloadClassification::UnsupportedSchemaVersion
        | WeatherPayloadClassification::InvalidSchemaVersionMarkerType
        | WeatherPayloadClassification::InvalidCanonicalPayload => {
            log::debug!("Ignoring non-weather or invalid weather payload from station {station_id}");
            return;
        },
        WeatherPayloadClassification::CanonicalV1 | WeatherPayloadClassification::LegacyUnversioned => {}
    }

    let canonical_payload = match canonicalize_weather_payload(&payload, WeatherPayloadParseOptions::default()) {
        Ok(Some(canonical_payload)) => canonical_payload,
        Ok(None) => {
            log::warn!("Weather payload classification expected weather data but canonicalization returned empty payload for station {station_id}");
            return;
        },
        Err(error) => {
            log::warn!("Failed to canonicalize weather payload from station {station_id}: {error}");
            return;
        },
    };

    let contribution_type = "contribution".to_string();
    let timestamp = parse_weather_observation_timestamp(&canonical_payload);
    let nonce = hex::encode(
        hash_bytes(
            format!(
                "wrfcoin-weather:{station_id}:{contribution_type}:{timestamp}:{}",
                canonical_payload
            )
            .as_bytes(),
        )
        .as_bytes(),
    );
    let commit_id = Transaction::compute_commit_id(
        &canonical_payload,
        &nonce,
        station_id,
        timestamp,
        &contribution_type,
    );

    let timestamp_dt = chrono::DateTime::from_timestamp(timestamp, 0)
        .unwrap_or_else(chrono::Utc::now);

    let mut commit_payload = CommitPayload {
        commit_id: commit_id.clone(),
        station_id: station_id.to_string(),
        timestamp,
        contribution_type: contribution_type.clone(),
        auth_pubkey: None,
        auth_signature: None,
    };
    // #2389: authorize the commit with the node's weather identity key so it
    // satisfies the #1848 fail-closed sender-auth check at mempool admission.
    // Without this the commit is rejected ("commit sender authorization failed")
    // and the entire P2P weather-ingestion path produces zero transactions.
    commit_payload.sign_authorization(commit_signing_key);
    let reveal_payload = RevealPayload {
        commit_id: commit_id.clone(),
        station_id: station_id.to_string(),
        timestamp,
        contribution_type: contribution_type.clone(),
        data: canonical_payload,
        nonce: nonce.clone(),
        data_cid: None,
        data_hash: None,
        storage_provider: None,
        provenance: None,
        manifest: None,
        verification_receipt: None,
        compute_receipt: None,
        quality_tier: None,
    };

    let commitment_tx = {
        let tx_input_seed =
            hash_bytes(format!("{station_id}:{commit_id}:{timestamp}:commit").as_bytes());
        let tx = Transaction::new(
            vec![TransactionInput {
                txid: tx_input_seed,
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 0,
                address: format!("0x{station_id}"),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx
    };
    let mut commitment_tx = commitment_tx;
    commitment_tx.tx_type = TransactionType::ContributionCommit;
    commitment_tx.timestamp = timestamp_dt;
    commitment_tx.data = serde_json::to_string(&commit_payload).ok();
    commitment_tx.set_id();

    let reveal_tx = {
        let tx_input_seed =
            hash_bytes(format!("{station_id}:{commit_id}:{timestamp}:reveal").as_bytes());
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: tx_input_seed,
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 0,
                address: format!("0x{station_id}"),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.tx_type = TransactionType::ContributionReveal;
        tx.timestamp = timestamp_dt;
        tx.data = serde_json::to_string(&reveal_payload).ok();
        tx.set_id();
        tx
    };

    let mut guard = mempool.write().await;
    if let Err(error) = guard.add_transaction(commitment_tx) {
        log::warn!(
            "Failed to enqueue contribution commit for station {station_id} payload: {error}"
        );
    }
    if let Err(error) = guard.add_transaction(reveal_tx) {
        log::warn!(
            "Failed to enqueue contribution reveal for station {station_id} payload: {error}"
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum P2pBlockSource {
    LiveGossip,
    HistoricalSync,
    BufferedOrphan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum P2pBlockApplyOutcome {
    Applied,
    Deferred,
    Rejected {
        attribution: AuthoritativeSyncRejectionAttribution,
        reason: String,
    },
}

impl P2pBlockApplyOutcome {
    fn peer_invalid(reason: impl Into<String>) -> Self {
        Self::Rejected {
            attribution: AuthoritativeSyncRejectionAttribution::PeerInvalid,
            reason: reason.into(),
        }
    }

    fn local_failure(reason: impl Into<String>) -> Self {
        Self::Rejected {
            attribution: AuthoritativeSyncRejectionAttribution::LocalFailure,
            reason: reason.into(),
        }
    }

    /// #2807: `validate_block == Ok(false)` — a deterministic verdict of
    /// this node's own consensus rules. A forged block and a canonical
    /// block the local rules reject are indistinguishable here, and
    /// penalizing the server graylists every honest peer when the block is
    /// canonical (the 2026-08-16 wedge at block 11,017), so this class is
    /// handled by sync-local rotation plus the corroborated validation
    /// halt instead of external reputation.
    fn rule_rejection(reason: impl Into<String>) -> Self {
        Self::Rejected {
            attribution: AuthoritativeSyncRejectionAttribution::LocalRuleRejection,
            reason: reason.into(),
        }
    }
}

impl P2pBlockSource {
    fn broadcasts_status(self) -> bool {
        matches!(self, Self::LiveGossip)
    }
}

async fn apply_p2p_block(
    block: Block,
    source: P2pBlockSource,
    blockchain: &Arc<RwLock<Blockchain>>,
    storage: &Arc<RwLock<Storage>>,
    consensus: &Arc<TypedConsensusManager<PopConsensusAdapter>>,
    p2p: Option<&P2pClient>,
    fork_buffer: &mut ForkCandidateBuffer,
    orphan_buffer: &mut OrphanBlockBuffer,
) -> P2pBlockApplyOutcome {
    apply_p2p_block_at(
        wrfcoin_shared_protocol::crypto::chain_binding::protocol_version_at(block.height),
        block,
        source,
        blockchain,
        storage,
        consensus,
        p2p,
        fork_buffer,
        orphan_buffer,
        std::convert::identity,
    )
    .await
}

async fn report_authoritative_sync_rejection(
    p2p: Option<&P2pClient>,
    blockchain: &Arc<RwLock<Blockchain>>,
    peer_id: libp2p::PeerId,
    block_height: u64,
    attribution: AuthoritativeSyncRejectionAttribution,
    reason: String,
) {
    let Some(client) = p2p else {
        return;
    };
    let (accepted_height, accepted_tip_hash, accepted_state_root) = {
        let chain = blockchain.read().await;
        (
            chain.height,
            <[u8; 32]>::from(chain.tip),
            chain.state.state_root.0,
        )
    };
    if let Err(error) = client
        .command_tx
        .send(P2pCommand::AuthoritativeSyncRejected {
            peer_id,
            block_height,
            accepted_height,
            accepted_tip_hash,
            accepted_state_root,
            attribution,
            reason,
        })
        .await
    {
        log::warn!(
            "Failed to return authoritative sync rejection for block {block_height} \
             to P2P worker: {error}"
        );
    }
}

/// Version-explicit P2P apply path. Production always calls
/// [`apply_p2p_block`], which resolves the received block's height against the
/// genesis-installed schedule. Keeping the version as an argument here lets
/// the V2 rejection tests execute without mutating process-global state.
#[allow(clippy::too_many_arguments)]
async fn apply_p2p_block_at<F>(
    version: wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion,
    block: Block,
    source: P2pBlockSource,
    blockchain: &Arc<RwLock<Blockchain>>,
    storage: &Arc<RwLock<Storage>>,
    consensus: &Arc<TypedConsensusManager<PopConsensusAdapter>>,
    p2p: Option<&P2pClient>,
    fork_buffer: &mut ForkCandidateBuffer,
    orphan_buffer: &mut OrphanBlockBuffer,
    prepare_persist_state: F,
) -> P2pBlockApplyOutcome
where
    F: FnOnce(
        wrfcoin_native_chain::blockchain::state::State,
    ) -> wrfcoin_native_chain::blockchain::state::State,
{
    apply_p2p_block_with_persist_control_at(
        version,
        block,
        source,
        blockchain,
        storage,
        consensus,
        p2p,
        fork_buffer,
        orphan_buffer,
        prepare_persist_state,
        #[cfg(test)]
        None,
    )
    .await
}

// Per-call test controls: no process-global fault state can affect another import.
#[cfg(test)]
struct PeerPersistTestControl {
    omit_checkpoint: bool,
    barrier: Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>,
}

#[cfg(test)]
struct EventPersistTestControl {
    persist: PeerPersistTestControl,
    version: wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion,
    reports: mpsc::UnboundedSender<String>,
}

/// Consume local failures at each event caller without attributing them to
/// the peer. Tests observe the same contextual diagnostic the caller emits.
fn report_local_block_failure(
    source: P2pBlockSource,
    height: u64,
    hash: wrfcoin_native_chain::crypto::hash::Hash,
    outcome: &P2pBlockApplyOutcome,
    #[cfg(test)] control: Option<&EventPersistTestControl>,
) -> bool {
    if let P2pBlockApplyOutcome::Rejected {
        attribution: AuthoritativeSyncRejectionAttribution::LocalFailure,
        reason,
    } = outcome
    {
        let diagnostic = format!(
            "P2P local import failure source={source:?} height={height} hash={hash}: {reason}"
        );
        log::error!("{diagnostic}");
        #[cfg(test)]
        if let Some(control) = control {
            let _ = control.reports.send(diagnostic);
        }
        return true;
    }
    false
}

#[allow(clippy::too_many_arguments)]
async fn apply_event_block(
    block: Block,
    source: P2pBlockSource,
    blockchain: &Arc<RwLock<Blockchain>>,
    storage: &Arc<RwLock<Storage>>,
    consensus: &Arc<TypedConsensusManager<PopConsensusAdapter>>,
    p2p: Option<&P2pClient>,
    fork_buffer: &mut ForkCandidateBuffer,
    orphan_buffer: &mut OrphanBlockBuffer,
    #[cfg(test)] control: Option<&EventPersistTestControl>,
) -> P2pBlockApplyOutcome {
    #[cfg(test)]
    if let Some(control) = control {
        return apply_p2p_block_with_persist_control_at(
            control.version,
            block,
            source,
            blockchain,
            storage,
            consensus,
            p2p,
            fork_buffer,
            orphan_buffer,
            std::convert::identity,
            Some(&control.persist),
        )
        .await;
    }
    apply_p2p_block(
        block,
        source,
        blockchain,
        storage,
        consensus,
        p2p,
        fork_buffer,
        orphan_buffer,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn apply_p2p_block_with_persist_control_at<F>(
    version: wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion,
    block: Block,
    source: P2pBlockSource,
    blockchain: &Arc<RwLock<Blockchain>>,
    storage: &Arc<RwLock<Storage>>,
    consensus: &Arc<TypedConsensusManager<PopConsensusAdapter>>,
    p2p: Option<&P2pClient>,
    fork_buffer: &mut ForkCandidateBuffer,
    orphan_buffer: &mut OrphanBlockBuffer,
    prepare_persist_state: F,
    #[cfg(test)] persist_control: Option<&PeerPersistTestControl>,
) -> P2pBlockApplyOutcome
where
    F: FnOnce(
        wrfcoin_native_chain::blockchain::state::State,
    ) -> wrfcoin_native_chain::blockchain::state::State,
{
    let expected_hash = block.calculate_hash();
    let block_hash = match block.hash {
        Some(h) if h == expected_hash => h,
        Some(_) => {
            log::warn!(
                "Rejecting P2P block at height {}: hash mismatch",
                block.height
            );
            return P2pBlockApplyOutcome::peer_invalid("hash mismatch");
        },
        None => {
            log::warn!(
                "Rejecting P2P block at height {}: missing hash",
                block.height
            );
            return P2pBlockApplyOutcome::peer_invalid("missing hash");
        },
    };

    // #3093 upgrade deadline: a block at or beyond the height at which the
    // genesis schedule activates a feature this binary does not implement
    // is never applied, buffered or relayed — this node cannot evaluate it.
    // It is a LOCAL failure, not the peer's: upgraded peers are right to
    // build it. The clean halt is requested once the local tip has reached
    // H − 1; a block far ahead of a still-syncing tip is just not applied.
    if let Some(deadline) = wrfcoin_native_chain::upgrade::production_refused_at(block.height) {
        let tip = blockchain.read().await.height;
        if tip.saturating_add(1) >= deadline.height {
            wrfcoin_native_chain::upgrade::request_upgrade_halt(
                &deadline,
                block.height,
                "P2P block apply",
            );
        } else {
            log::warn!(
                "Not applying P2P block at height {} (at or beyond the upgrade deadline {}: \
                 feature {:?}); still syncing to height {} on this binary (#3093)",
                block.height,
                deadline.height,
                deadline.feature,
                deadline.height.saturating_sub(1)
            );
        }
        return P2pBlockApplyOutcome::local_failure(format!(
            "EXIT_UPGRADE_REQUIRED: block {} is at or beyond the upgrade deadline {} (feature {:?})",
            block.height, deadline.height, deadline.feature
        ));
    }

    let mut chain = blockchain.write().await;

    // --- Fork candidate detection ---
    // Buffer an at-or-below-height block, or a next-height block that does not
    // extend our current tip, as a potential fork candidate. The latter is
    // load-bearing: a competing branch becomes longer precisely when its next
    // block arrives at `local_height + 1`; sending that block through ordinary
    // parent validation made the reorg path unreachable (#2085 review).
    let is_next_height_competitor =
        block.height == chain.height.saturating_add(1) && block.prev_block_hash != chain.tip;
    if block.height <= chain.height || is_next_height_competitor {
        let min_fork_height = chain.height.saturating_sub(MAX_REORG_DEPTH);
        if block.height > min_fork_height {
            log::debug!(
                "Buffering P2P block at height {} as fork candidate (local height={})",
                block.height,
                chain.height
            );
            fork_buffer.insert(block);

            // Check if the buffer now contains a fork longer than ours.
            // We need to find the fork point — look for a block in the
            // buffer whose prev_block_hash matches one of our blocks.
            if let Some(reorg_chain) = detect_fork_from_buffer(fork_buffer, &chain) {
                let candidate_tip = reorg_chain.last().map(|b| b.height).unwrap_or(0);
                if candidate_tip > chain.height {
                    log::info!(
                        "Detected longer fork: candidate tip={} vs local={}",
                        candidate_tip,
                        chain.height
                    );
                    drop(chain);
                    try_reorg_inner(
                        version,
                        reorg_chain,
                        blockchain,
                        storage,
                        consensus,
                        p2p,
                        prepare_persist_state,
                    )
                    .await;
                    // #3277 S3: whether the reorg completed or restored the
                    // pre-reorg snapshot, re-derive the effective schedule
                    // from the canonical state it left behind (a branch-only
                    // extension must not outlive its branch).
                    {
                        let chain = blockchain.read().await;
                        wrfcoin_native_chain::upgrade::refresh_schedule_from_canonical_state(
                            &chain.state,
                            "chain reorganization",
                        );
                    }
                    return P2pBlockApplyOutcome::Deferred;
                }
            }
        } else {
            log::debug!(
                "Skipping P2P block at height {} — too deep for reorg (local height={})",
                block.height,
                chain.height
            );
        }
        return P2pBlockApplyOutcome::Deferred;
    }

    if block.height != chain.height + 1 {
        // Block is ahead of our tip by more than one — buffer it as an
        // orphan so it can be re-applied once the block-sync backfill path
        // fills the gap, and nudge the sync manager to dispatch a
        // `BlockRequest` to the peer(s) that advertised a higher tip via
        // `PeerChainStatus` gossip.
        //
        // Without this, a fresh node that hears the current tip via
        // gossipsub silently drops every inbound block and never makes
        // progress (issue #1369).
        let target_height = block.height;
        let local_height = chain.height;
        log::info!(
            "P2P block height {target_height} is ahead of local tip {local_height} — \
             buffering as orphan and kicking sync"
        );
        orphan_buffer.insert(block);
        drop(chain);

        if let Some(client) = p2p {
            // Pass the observed orphan height so the sync manager can
            // bump its `target_height` even if the next `ChainStatus`
            // gossip is still seconds away. Without this hint, the
            // sync manager stalls at the original IBD ceiling because
            // `refill_pending_ranges` early-returns when
            // `target_height <= local_height` (#1664).
            if let Err(err) = client.command_tx.try_send(P2pCommand::KickSync {
                observed_height: Some(target_height),
            }) {
                log::debug!("Failed to send KickSync command: {err}");
            }
        }
        return P2pBlockApplyOutcome::Deferred;
    }

    match consensus.validate_block(&block, &chain).await {
        Ok(true) => {
            log::debug!("P2P block {} passed consensus validation", block.height);
        },
        Ok(false) => {
            log::warn!(
                "Rejecting P2P block at height {}: failed consensus validation",
                block.height
            );
            return P2pBlockApplyOutcome::rule_rejection(
                "failed authoritative consensus validation",
            );
        },
        Err(e) => {
            log::warn!("P2P block {} consensus error: {e}, rejecting", block.height);
            return P2pBlockApplyOutcome::local_failure(format!("consensus error: {e}"));
        },
    }

    // Extract transitions using the current state as a shared reference.
    // No clone needed here — extract_transitions only reads state and
    // chain.state is not mutated until the apply call below (#1979).
    let applied_transitions = extract_transitions(
        &chain.state,
        &block,
        block.height,
        block.timestamp.timestamp(),
    );

    // Apply state transitions BEFORE add_block so that a transition failure
    // cannot leave chain.height / chain.tip / blocks map out of sync with
    // chain.state (fixes #961).  Apply to a working copy so that on failure
    // the live chain state is never partially mutated — no restoration step
    // required (#1979: replaces eager pre-clone/restore with copy-on-commit).
    let mut staged_chain = chain.clone();
    let working_state = &mut staged_chain.state;
    if let Err(e) = working_state
        .apply_transitions_batch_at_height(block.height, applied_transitions.iter().cloned())
    {
        log::error!(
            "P2P block {} state transition failed ({e}) — rejecting block",
            block.height
        );
        return P2pBlockApplyOutcome::local_failure(format!("state transition failed: {e}"));
    }
    // Per-block finalization the producer also runs (deferred-slashing flush +
    // commit/reveal expiry sweep + replay-protection prune). These are
    // height-driven, not encoded as per-tx transitions, so without this the
    // follower accumulates stale pending_commits and its state_root drifts from
    // the producer's at every height (commit-reveal lifecycle parity fix).
    // #2219: the checked variant also verifies the block's committed
    // post-state root against the root this applier actually reached; a
    // mismatch is a rejection, not a latched warning — divergence becomes
    // structural instead of the out-of-band root comparison that took #1776
    // 28 hours to diagnose.
    // state-root-checked-reject: p2p-working-copy-return
    if let Err(e) = working_state.apply_block_finalization_checked_at(version, &block) {
        log::error!(
            "P2P block {} rejected at finalization ({e}) — committed post-state \
             root does not match this node's applied state (#2219)",
            block.height
        );
        return P2pBlockApplyOutcome::local_failure(format!(
            "checked finalization failed: {e}"
        ));
    }

    // #2086: stage chain bookkeeping too (tip, block window, counts and
    // finality). The live chain stays unchanged and its write guard stays held
    // until the block/WAL/checkpoint transaction has durably committed.
    let block_height = block.height;
    if let Err(e) = staged_chain.add_block(block.clone()) {
        return P2pBlockApplyOutcome::local_failure(format!("add block failed: {e}"));
    }
    let checkpoint_state = {
        let st = storage.read().await;
        st.checkpoint_state_bytes(block_height, &staged_chain.state)
    };

    #[cfg(test)]
    let checkpoint_state = if let Some(control) = persist_control {
        if let Some((reached, resume)) = &control.barrier {
            reached.notify_one();
            resume.notified().await;
        }
        if control.omit_checkpoint {
            Ok(None)
        } else {
            checkpoint_state
        }
    } else {
        checkpoint_state
    };

    let tx_count = block.transactions.len();
    let transition_count = applied_transitions.len();
    let persisted = match checkpoint_state {
        Ok(checkpoint_state) => {
            storage
                .write()
                .await
                .persist_block_with_finalized(
                    block.clone(),
                    applied_transitions,
                    checkpoint_state,
                    staged_chain.finalized_height(),
                )
                .await
        },
        Err(e) => Err(e),
    };
    if let Err(e) = persisted {
        // Dropping the candidate restores nothing: no live chain fields or
        // process-wide schedule were changed. The same peer block can retry.
        log::error!(
            "P2P checkpoint persistence failed source={source:?} height={block_height} hash={block_hash}: {e}"
        );
        return P2pBlockApplyOutcome::local_failure(format!("persist block failed: {e}"));
    }

    *chain = staged_chain;
    // Process-wide upgrade inputs become canonical only after durable commit.
    wrfcoin_native_chain::upgrade::reresolve_base_schedule_if_block1(&block);
    wrfcoin_native_chain::upgrade::refresh_schedule_from_canonical_state(
        &chain.state,
        "applied P2P block",
    );
    let current_state_root = chain.state.state_root.0;
    drop(chain);

    if let Some(client) = p2p {
        if source.broadcasts_status() {
            notify_p2p_chain_advanced(client, &block, false, current_state_root);
        } else {
            // Preserve #2760: IBD/orphan imports update sync state without
            // flooding the rate-limited ChainStatus topic.
            update_p2p_chain_state(client, &block, current_state_root);
        }
    }

    log::info!(
        "Accepted P2P block height={block_height} hash={block_hash} txs={tx_count} transitions={transition_count}",
    );
    P2pBlockApplyOutcome::Applied
}

/// Apply any buffered orphan blocks whose parent is now the local tip.
///
/// Called after every successful (and also every attempted) `apply_p2p_block`
/// call so that a chain of orphans that arrived out of order is drained as
/// soon as the missing ancestor is applied.
async fn drain_orphan_buffer(
    blockchain: &Arc<RwLock<Blockchain>>,
    storage: &Arc<RwLock<Storage>>,
    consensus: &Arc<TypedConsensusManager<PopConsensusAdapter>>,
    p2p: Option<&P2pClient>,
    fork_buffer: &mut ForkCandidateBuffer,
    orphan_buffer: &mut OrphanBlockBuffer,
) {
    drain_orphan_buffer_with_control(
        blockchain,
        storage,
        consensus,
        p2p,
        fork_buffer,
        orphan_buffer,
        #[cfg(test)]
        None,
    )
    .await;
}

async fn drain_orphan_buffer_with_control(
    blockchain: &Arc<RwLock<Blockchain>>,
    storage: &Arc<RwLock<Storage>>,
    consensus: &Arc<TypedConsensusManager<PopConsensusAdapter>>,
    p2p: Option<&P2pClient>,
    fork_buffer: &mut ForkCandidateBuffer,
    orphan_buffer: &mut OrphanBlockBuffer,
    #[cfg(test)] control: Option<&EventPersistTestControl>,
) {
    // Cap iterations defensively — the orphan buffer is capped at
    // ORPHAN_BUFFER_MAX_ENTRIES but each apply can only consume one height
    // at a time, so we bound the loop.
    for _ in 0..ORPHAN_BUFFER_MAX_ENTRIES {
        let current_height = {
            let chain = blockchain.read().await;
            chain.height
        };
        orphan_buffer.prune_below_or_equal(current_height);
        let next_height = current_height + 1;
        let candidates = orphan_buffer.take_height(next_height);
        if candidates.is_empty() {
            break;
        }
        let mut applied_any = false;
        let mut candidates = candidates.into_iter();
        while let Some(candidate) = candidates.next() {
            // apply_p2p_block will validate hash/parent/consensus and ignore
            // blocks that don't extend the local tip. If a candidate fails
            // validation (wrong parent hash), it is dropped; the next
            // candidate at the same height — if any — gets a chance.
            let before_height = {
                let chain = blockchain.read().await;
                chain.height
            };
            let height = candidate.height;
            let hash = candidate.hash.unwrap_or_else(|| candidate.calculate_hash());
            let outcome = apply_event_block(
                candidate.clone(),
                P2pBlockSource::BufferedOrphan,
                blockchain,
                storage,
                consensus,
                p2p,
                fork_buffer,
                orphan_buffer,
                #[cfg(test)]
                control,
            )
            .await;
            if report_local_block_failure(
                P2pBlockSource::BufferedOrphan,
                height,
                hash,
                &outcome,
                #[cfg(test)]
                control,
            ) {
                // Keep the failed import and every untried candidate. Retry on
                // the next drain trigger, not in a tight loop against storage.
                orphan_buffer.insert(candidate);
                for pending in candidates {
                    orphan_buffer.insert(pending);
                }
                return;
            }
            let after_height = {
                let chain = blockchain.read().await;
                chain.height
            };
            if after_height > before_height {
                applied_any = true;
                break;
            }
        }
        if !applied_any {
            break;
        }
    }
}

/// Recompute the canonical state root deterministically from materialized
/// state material, identical to [`State::recalculate_state_root`] but without
/// requiring `&mut self`. Used by the #1724 local-recovery path so the live
/// state root is never mutated while we merely *verify* it.
fn recompute_canonical_state_root(material: &[u8]) -> [u8; 32] {
    use sha3::{Digest, Sha3_256};
    let mut hasher = Sha3_256::new();
    hasher.update(material);
    let digest = hasher.finalize();
    let mut root = [0u8; 32];
    root.copy_from_slice(&digest[..32]);
    root
}

/// #1724: after the node makes local forward progress (applying or syncing a
/// block), attempt to clear the `state_diverged` readiness latch using a
/// *locally-verified* canonical root rather than a peer's self-reported root.
///
/// No-op unless `policy` is
/// [`LocallyVerifiedRoot`](StateDivergenceRecoveryPolicy::LocallyVerifiedRoot)
/// and the latch is currently set. Recomputes the canonical state root from the
/// node's own materialized state and compares it to the stored live root;
/// equality (post-#1681 producer/applier/replay parity) is peer-independent
/// proof and clears the latch via [`observe_local_canonical_root`].
async fn try_clear_latch_on_local_root(
    policy: StateDivergenceRecoveryPolicy,
    blockchain: &Arc<RwLock<Blockchain>>,
    state_diverged: &AtomicBool,
) {
    if !policy.requires_local_proof() || !state_diverged.load(Ordering::SeqCst) {
        return;
    }
    let verification = {
        let chain = blockchain.read().await;
        let live_root: [u8; 32] = chain.state.state_root.0;
        let recomputed_root = recompute_canonical_state_root(&chain.state.state_root_material());
        LocalCanonicalRoot {
            recomputed_root,
            live_root,
        }
    };
    match observe_local_canonical_root(verification, state_diverged) {
        ChainDivergenceObservation::Recovered => {
            log::warn!(
                "STATE DIVERGENCE CLEARED: locally-verified canonical root (recompute matches \
                 stored root) after local block progress; node re-converged on its own state, \
                 clearing readiness latch (#1724)",
            );
        }
        ChainDivergenceObservation::LocalRootMismatch => {
            log::error!(
                "INTERNAL STATE INTEGRITY FAILURE: canonical-root recompute disagrees with \
                 stored state root after applying a block; holding readiness latch set (#1724)",
            );
        }
        // AwaitingLocalProof (zero recompute / genesis-only) or NoDivergence
        // (latch cleared concurrently): leave the latch as-is, nothing to report.
        _ => {}
    }
}

/// Walk the fork candidate buffer to find a contiguous chain that forks
/// from our local chain and is longer.
fn detect_fork_from_buffer(buffer: &ForkCandidateBuffer, chain: &Blockchain) -> Option<Vec<Block>> {
    // Try each height in the buffer as a potential fork start.
    for (&height, candidates) in &buffer.blocks {
        for candidate in candidates {
            // Check if this block's parent is in our chain.
            if let Some(local_block) = chain.block_at_height(height.saturating_sub(1)) {
                let local_hash = local_block
                    .hash
                    .unwrap_or_else(|| local_block.calculate_hash());
                if candidate.prev_block_hash == local_hash {
                    // Found a fork point. Build the longest chain from here.
                    if let Some(fork_chain) = buffer.build_chain_from(height, local_hash) {
                        let fork_tip = fork_chain.last().map(|b| b.height).unwrap_or(0);
                        if fork_tip > chain.height {
                            return Some(fork_chain);
                        }
                    }
                }
            }
            // Also check fork from genesis.
            if height == 1 && candidate.prev_block_hash == CryptoHash::default() {
                if let Some(fork_chain) = buffer.build_chain_from(1, CryptoHash::default()) {
                    let fork_tip = fork_chain.last().map(|b| b.height).unwrap_or(0);
                    if fork_tip > chain.height {
                        return Some(fork_chain);
                    }
                }
            }
        }
    }
    None
}

/// Shared implementation for production and exact-version rejection tests.
///
/// `prepare_persist_state` is the identity function in production. Tests use
/// it for one purpose: inject a divergent checkpoint-rebuilt state *after* the
/// in-memory reorg has passed, so the otherwise defense-in-depth persistence
/// commitment check can be exercised at its real call site.
async fn try_reorg_inner<F>(
    version: wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion,
    candidate_blocks: Vec<Block>,
    blockchain: &Arc<RwLock<Blockchain>>,
    storage: &Arc<RwLock<Storage>>,
    consensus: &Arc<TypedConsensusManager<PopConsensusAdapter>>,
    p2p: Option<&P2pClient>,
    prepare_persist_state: F,
) where
    F: FnOnce(
        wrfcoin_native_chain::blockchain::state::State,
    ) -> wrfcoin_native_chain::blockchain::state::State,
{
    let mut chain = blockchain.write().await;

    // Resolve the fork point against both the in-memory window and persisted
    // storage (issue #1277). We take the storage read lock up-front and
    // snapshot the small MAX_REORG_DEPTH-bounded range we may need, rather
    // than acquiring/releasing inside the closure — the closure itself is
    // sync and non-async, and `plan_reorg_with_loader` walks at most
    // MAX_REORG_DEPTH blocks backwards.
    let disk_blocks: std::collections::HashMap<u64, Block> = {
        let st = storage.read().await;
        let search_floor = chain.height.saturating_sub(MAX_REORG_DEPTH);
        let search_ceiling = chain.height;
        match st.load_blocks_range(search_floor, search_ceiling) {
            Ok(blocks) => blocks.into_iter().map(|b| (b.height, b)).collect(),
            Err(e) => {
                log::warn!(
                    "Reorg: failed to preload disk blocks for fork-point fallback: {e}"
                );
                std::collections::HashMap::new()
            },
        }
    };

    let plan = match chain
        .plan_reorg_with_loader(&candidate_blocks, |h| disk_blocks.get(&h).cloned())
    {
        Ok(Some(plan)) => plan,
        Ok(None) => {
            log::debug!("Reorg plan rejected: local chain is already best");
            return;
        },
        Err(e) => {
            log::error!("Failed to plan reorg: {e}");
            return;
        },
    };

    log::info!(
        "REORG planned: fork_height={} reverting={} applying={}",
        plan.fork_height,
        plan.blocks_to_revert,
        plan.blocks_to_apply.len()
    );

    // #3370: FAIL CLOSED on a genesis reorg that would replace this node's
    // canonical block 1. Block 1 already armed this process's first-install-wins
    // inputs (protocol version, activation table, base feature schedule and —
    // once #3277 S5 lands — the schedule-extension authority). Installing a
    // different block 1 here would skip the block-1 hook, and even re-running it
    // could not re-arm those inputs: the node would follow the peers' chain with
    // the old block 1's inputs, then rule-reject (or out-produce, the #1972
    // class) the first block that depends on the new one's. Snapshot join and IBD
    // both arm block 1 correctly, so the operator's recovery is a wipe + rejoin.
    //
    // Nodes on a BFT-finalized chain never reach this: `plan_reorg_with_loader`
    // already refuses any fork below `finalized_height` (#1216), and a fork at
    // height >= 1 never touches block 1.
    if plan.replaces_block_one() {
        let local_block1 = chain
            .block_at_height(1)
            .or_else(|| disk_blocks.get(&1))
            .map(|b| b.hash.unwrap_or_else(|| b.calculate_hash()).to_string())
            .unwrap_or_else(|| "<unavailable>".to_string());
        let candidate_block1 = plan
            .blocks_to_apply
            .first()
            .map(|b| b.hash.unwrap_or_else(|| b.calculate_hash()).to_string())
            .unwrap_or_else(|| "<none>".to_string());
        log::error!(
            "REFUSING genesis reorg (#3370): peers offer a longer chain (tip height {}) with a \
             DIFFERENT block 1 ({candidate_block1}) than this node's canonical block 1 \
             ({local_block1}, local height {}). Block 1 has already armed this process's \
             protocol inputs (version, activation table, base feature schedule, schedule-extension \
             authority), which cannot be swapped under a running node. This node is on a \
             different chain from its peers and will NOT follow them. To join the peers' chain, \
             stop the node, wipe its data directory, and rejoin via snapshot join or IBD.",
            plan.blocks_to_apply.last().map_or(0, |b| b.height),
            chain.height,
        );
        return;
    }

    // Load the nearest checkpoint at or before the fork height and replay
    // state transitions from the checkpoint to the fork height.
    //
    // Moved before validation so that consensus checks use the fork-point
    // state (oracle set, quorum) instead of the current tip state (#972).
    let fork_state = {
        let st = storage.read().await;
        let (cp_height, mut state) = match st.load_checkpoint_at_or_before(plan.fork_height) {
            Ok(Some(pair)) => pair,
            Ok(None) => {
                log::warn!(
                    "Reorg aborted: no checkpoint at or before fork height {}",
                    plan.fork_height
                );
                return;
            },
            Err(e) => {
                log::error!("Reorg aborted: failed to load checkpoint: {e}");
                return;
            },
        };

        // Replay WAL entries from checkpoint to fork height, grouped per block
        // and finalized per height — byte-identical to the storage WAL-replay
        // path (`storage.rs::load_blockchain`) and the producer. Without the
        // per-height `apply_block_finalization` the reconstructed fork-point
        // state (and every durable checkpoint derived from it in the persist
        // loop below) carries un-pruned commit/reveal + deferred-slashing
        // residue, so a restarted node — which rebuilds via the finalized WAL
        // path — reconstructs a divergent `state_root` (#1810, same divergence
        // class as #1776).
        if cp_height < plan.fork_height {
            use wrfcoin_native_chain::blockchain::state::StateTransition;
            let transitions = match st.load_wal_transitions_range(cp_height + 1, plan.fork_height)
            {
                Ok(transitions) => transitions,
                Err(e) => {
                    log::error!("Reorg aborted: failed to load WAL transitions: {e}");
                    return;
                },
            };
            // WAL entries are appended in block order; consecutive entries
            // sharing a height form one block. Apply each block's transitions
            // through the SAME batch entrypoint then finalize the block.
            let mut pending_height: Option<u64> = None;
            let mut block_transitions: Vec<StateTransition> = Vec::new();
            for (h, t) in transitions {
                if pending_height.is_some_and(|ph| ph != h) {
                    let ph = pending_height.expect("checked is_some_and above"); // panic-ok: local Option is checked immediately above before extraction
                    if let Err(e) = state.apply_transitions_batch_at_height(
                        ph,
                        std::mem::take(&mut block_transitions),
                    ) {
                        log::error!("Reorg aborted: WAL replay batch failed at height {ph}: {e}");
                        return;
                    }
                    // Unchecked by design: this reconstructs persisted fork-point
                    // state without a candidate Block or commitment to compare.
                    state.apply_block_finalization(ph); // state-root-unchecked-ok: fork-point-wal-group
                if let Err(e) = state.validate_node_finalized(ph) {
                    log::error!("Reorg aborted: Node settlement replay failed at height {ph}: {e}");
                    return;
                }
                }
                pending_height = Some(h);
                block_transitions.push(t);
            }
            if let Some(ph) = pending_height {
                if let Err(e) = state.apply_transitions_batch_at_height(ph, block_transitions) {
                    log::error!("Reorg aborted: WAL replay batch failed at height {ph}: {e}");
                    return;
                }
                // Same reconstruction-only exception as the grouped WAL path:
                // there is no candidate Block commitment at the fork point.
                state.apply_block_finalization(ph); // state-root-unchecked-ok: fork-point-wal-tail
                if let Err(e) = state.validate_node_finalized(ph) {
                    log::error!("Reorg aborted: Node settlement replay failed at height {ph}: {e}");
                    return;
                }
            }
        }
        state
    };

    // Keep clones of the fork state for validation, execute_reorg and
    // persistence (#989).
    let fork_state_for_validation = fork_state.clone();
    let fork_state_for_reorg = fork_state.clone();
    let fork_state_for_persist = fork_state.clone();

    // Validate the candidate fork sequentially from the fork point.
    //
    // Merely swapping `chain.state` (the old implementation) left
    // `chain.height`/`chain.tip` on the abandoned local tip. The first candidate
    // was therefore rejected by the basic parent check before any reorg could
    // reach the state-root call sites. Build a disposable fork-point view and
    // advance it one block at a time so state-dependent validation for block N
    // sees the state produced by block N-1.
    let mut validation_blocks: std::collections::BTreeMap<u64, Block> = chain
        .blocks()
        .range(..=plan.fork_height)
        .map(|(&height, block)| (height, block.clone()))
        .collect();
    if plan.fork_height > 0 && !validation_blocks.contains_key(&plan.fork_height) {
        if let Some(fork_block) = disk_blocks.get(&plan.fork_height) {
            validation_blocks.insert(plan.fork_height, fork_block.clone());
        }
    }
    let mut validation_chain = Blockchain::from_persisted(
        plan.fork_height,
        plan.fork_hash,
        fork_state_for_validation,
        validation_blocks,
        plan.fork_height,
        chain.finalized_height(),
    );
    for blk in &plan.blocks_to_apply {
        match consensus.validate_block(blk, &validation_chain).await {
            Ok(true) => {},
            Ok(false) => {
                log::warn!(
                    "Reorg aborted: candidate block at height {} failed consensus",
                    blk.height
                );
                return;
            },
            Err(e) => {
                log::warn!(
                    "Reorg aborted: consensus error on block {}: {e}",
                    blk.height
                );
                return;
            },
        }
        let transitions = extract_transitions(
            &validation_chain.state,
            blk,
            blk.height,
            blk.timestamp.timestamp(),
        );
        if let Err(e) =
            validation_chain.apply_state_transitions_batch_at_height(blk.height, transitions)
        {
            log::warn!(
                "Reorg aborted: validation replay failed for block {}: {e}",
                blk.height
            );
            return;
        }
        // Equality is enforced by the real in-memory and persistence call
        // sites below. This unchecked finalization advances only the disposable
        // validation view so the next block sees the correct fork state.
        validation_chain.state.apply_block_finalization(blk.height); // state-root-unchecked-ok: disposable-validation-view
        if let Err(e) = validation_chain.add_block(blk.clone()) {
            log::warn!(
                "Reorg aborted: validation view could not append block {}: {e}",
                blk.height
            );
            return;
        }
    }

    let old_tip = chain.tip;
    let old_height = chain.height;

    // Snapshot the entire chain before execute_reorg so we can restore it if
    // state transitions fail mid-loop (#1025).
    let pre_reorg_snapshot = chain.clone();

    // Execute the reorg on the in-memory chain.
    if let Err(e) = chain.execute_reorg(&plan, fork_state_for_reorg) {
        log::error!("Reorg execution failed: {e}");
        return;
    }

    // Apply state transitions for each new block. If any transition fails,
    // restore the pre-reorg snapshot so the node remains consistent (#1025).
    let mut transition_failed = false;
    for blk in &plan.blocks_to_apply {
        // Use &chain.state directly as the extract_transitions reference — no
        // clone needed here since chain.state is not mutated until the apply
        // call below.  Per-block rollback uses pre_reorg_snapshot, not a
        // per-block save (#1979: removes O(N) State clones per reorg).
        let transitions = extract_transitions(
            &chain.state,
            blk,
            blk.height,
            blk.timestamp.timestamp(),
        );
        if let Err(e) = chain.apply_state_transitions_batch_at_height(blk.height, transitions) {
            log::error!(
                "REORG RECOVERY: state transition failed at height {} (error: {e}). \
                 Restoring chain to pre-reorg state (height={}, tip={}).",
                blk.height,
                old_height,
                old_tip,
            );
            // Restore the chain from the pre-reorg snapshot so that
            // height, tip, blocks, and state are all consistent.
            *chain = pre_reorg_snapshot.clone();
            transition_failed = true;
            break;
        }
        // Same per-block finalization the producer + P2P apply path run, so a
        // re-applied fork yields the identical state_root the producer minted.
        // #2219: checked — a fork block whose committed post-state root does
        // not match the re-applied state is an invalid fork; treat it exactly
        // like a failed transition and restore the pre-reorg snapshot.
        // state-root-checked-reject: reorg-restore-break
        if let Err(e) = chain.state.apply_block_finalization_checked_at(version, blk) {
            log::error!(
                "REORG RECOVERY: post-state-root commitment mismatch at height {} \
                 ({e}). Restoring chain to pre-reorg state (height={}, tip={}).",
                blk.height,
                old_height,
                old_tip,
            );
            *chain = pre_reorg_snapshot.clone();
            transition_failed = true;
            break;
        }
    }

    if transition_failed {
        log::error!(
            "REORG ABORTED: rolled back to height={} tip={}. \
             The candidate fork contained an invalid state transition. \
             fork_height={} blocks_to_revert={} blocks_to_apply={}",
            chain.height,
            chain.tip,
            plan.fork_height,
            plan.blocks_to_revert,
            plan.blocks_to_apply.len(),
        );
        return;
    }

    // Prepare the ENTIRE durable replacement before rolling storage back.
    //
    // The old order rolled storage back first and then checked each rebuilt
    // checkpoint against the block commitment. If this defense-in-depth check
    // ever fired (the #1810 checkpoint/replay divergence class), the function
    // returned with the in-memory chain on the new fork but durable storage
    // truncated to the fork point. That is not rejection; it is the #2056
    // log-and-continue failure shape split across memory and disk.
    //
    // Here, no storage mutation is reachable until every candidate block has
    // produced transitions, passed V2 equality, and serialized its checkpoint.
    // On any failure the pre-reorg in-memory snapshot is restored while storage
    // is still untouched.
    let mut evolving_state = prepare_persist_state(fork_state_for_persist);
    let mut prepared_persistence = Vec::new();
    {
        let st = storage.read().await;
        for blk in &plan.blocks_to_apply {
            let transitions = extract_transitions(
                &evolving_state,
                blk,
                blk.height,
                blk.timestamp.timestamp(),
            );
            if let Err(e) = evolving_state
                .apply_transitions_batch_at_height(blk.height, transitions.iter().cloned())
            {
                log::error!(
                    "Reorg persist preparation rejected: state transition batch failed at \
                     height {}: {e}. Restoring the pre-reorg in-memory chain; durable storage \
                     is untouched.",
                    blk.height
                );
                *chain = pre_reorg_snapshot;
                return;
            }
            // state-root-checked-reject: persist-restore-return
            if let Err(e) = evolving_state.apply_block_finalization_checked_at(version, blk) {
                log::error!(
                    "Reorg persist preparation rejected: post-state-root commitment mismatch at \
                     height {}: {e}. Restoring the pre-reorg in-memory chain; durable storage \
                     is untouched.",
                    blk.height
                );
                *chain = pre_reorg_snapshot;
                return;
            }
            let checkpoint_state = match st.checkpoint_state_bytes(blk.height, &evolving_state) {
                Ok(checkpoint_state) => checkpoint_state,
                Err(e) => {
                    log::error!(
                        "Reorg persist checkpoint preparation failed at height {}: {e}. \
                         Restoring the pre-reorg in-memory chain; durable storage is untouched.",
                        blk.height
                    );
                    *chain = pre_reorg_snapshot;
                    return;
                },
            };
            prepared_persistence.push((blk.clone(), transitions, checkpoint_state));
        }
    }

    let new_tip_height = chain.height;
    let new_tip = chain.tip;
    let current_state_root: [u8; 32] = chain.state.state_root.0;
    let finalized_height = chain.finalized_height();
    drop(chain);

    // Every consensus/state check has passed. Only now mutate durable storage.
    {
        let mut st = storage.write().await;

        if let Err(e) = st.rollback_to_height(plan.fork_height, new_tip).await {
            log::error!("Reorg storage rollback failed: {e}");
            return;
        }

        for (blk, transitions, checkpoint_state) in prepared_persistence {
            let height = blk.height;
            if let Err(e) = st
                .persist_block_with_finalized(
                    blk,
                    transitions,
                    checkpoint_state,
                    finalized_height,
                )
                .await
            {
                log::error!("Reorg persist failed at height {height}: {e}");
                return;
            }
        }
    }

    if let Some(client) = p2p {
        if let Some(tip_block) = candidate_blocks.last() {
            notify_p2p_chain_advanced(client, tip_block, false, current_state_root);
        }
    }

    log::warn!(
        "REORG COMPLETE: depth={} old_tip={} new_tip={} new_height={}",
        plan.blocks_to_revert,
        old_tip,
        new_tip,
        new_tip_height,
    );
}

/// Remove all transactions that were confirmed in the block range
/// `(height_before, current_height]` from the mempool, including their
/// replay-protection keys (#1847).
///
/// This prevents `protocol_replay_keys` from growing without bound when blocks
/// arrive via P2P gossip or block-sync, as the P2P apply path never called
/// `drain_confirmed` on the mempool before this fix.
async fn drain_mempool_for_confirmed_range(
    height_before: u64,
    blockchain: &Arc<RwLock<Blockchain>>,
    mempool: &Arc<RwLock<Mempool>>,
) {
    let confirmed_tx_ids: Vec<CryptoHash> = {
        let chain = blockchain.read().await;
        let height_after = chain.height;
        if height_after <= height_before {
            return;
        }
        ((height_before + 1)..=height_after)
            .flat_map(|h| {
                chain
                    .block_at_height(h)
                    .map(|b| b.transactions.iter().filter_map(|tx| tx.id).collect::<Vec<_>>())
                    .unwrap_or_default()
            })
            .collect()
    };
    if !confirmed_tx_ids.is_empty() {
        let mut mp = mempool.write().await;
        mp.drain_confirmed_block_txs(&confirmed_tx_ids);
        log::debug!(
            "Drained {} confirmed tx(s) from mempool after P2P block apply (#1847)",
            confirmed_tx_ids.len()
        );
    }
}

/// Consumes P2P events and integrates them into the local node state.
///
/// #2884: one shared shape for a peer payload that failed bincode decode after
/// the gossip envelope itself parsed. Counts it (per fixed `kind` label — never
/// peer-controlled data, which would mint unbounded metric series) and names
/// VERSION SKEW as the candidate cause in the warn line.
///
/// Why: the #2727-class audit showed a version-skewed peer fails exactly here —
/// its `Block`/`FinalityVote` layout differs, decode errors (unexpected EOF one
/// direction, invalid enum tag the other), and the handler drops the message.
/// The behaviour stays warn+drop (changing it is a consensus-affecting design
/// decision that belongs in #2884 with the protocol-identifier bump); what
/// changes is that the failure is countable
/// (`wrfcoin_p2p_payload_decode_failed_total{kind=…}`) and its likeliest cause
/// has a name, instead of being one more anonymous warn in a busy log — the
/// 2026-08-28 quorum outage ran 10.5h unalerted because its failure had no
/// name.
fn note_payload_decode_failure(kind: &str, context: &str, error: &dyn std::fmt::Display) {
    CORE_METRICS.inc_p2p_payload_decode_failed(kind);
    log::warn!(
        "P2P {kind} payload failed to decode ({context}): {error} — dropped, not applied. \
         If this recurs, especially from one peer, the likely cause is VERSION SKEW: a \
         peer built from a binary whose {kind} layout differs (bincode is positional — \
         #2884, the #2727 hazard class on the wire). The effect is silently lost {kind} \
         traffic from that peer; compare the two nodes' builds before suspecting the peer."
    );
}

/// - **NewBlock**: deserializes, validates height + hash, runs consensus validation,
///   replays state transitions, adds to chain and persists.
/// - **NewTransactions**: deserializes each and adds to the mempool.
/// - **Peer lifecycle / status**: logged for observability.
/// - **Weather**: currently cached or logged depending on content-ref availability.
/// - **Forecast / compute events**: wired into execution/scheduler in-memory state.
pub async fn run_p2p_event_loop(
    event_rx: mpsc::Receiver<P2pEvent>,
    blockchain: Arc<RwLock<Blockchain>>,
    mempool: Arc<RwLock<Mempool>>,
    storage: Arc<RwLock<Storage>>,
    consensus: Arc<TypedConsensusManager<PopConsensusAdapter>>,
    p2p: Option<P2pClient>,
    da_challenge_runtime: DaChallengeRuntime,
    bft_finality: Option<Arc<RwLock<BftFinalityGadget>>>,
    finality_signal_tx: Option<FinalitySignalSender>,
    #[cfg(feature = "execution")] forecast_task_assignments: Arc<
        RwLock<FxHashMap<String, ForecastArtifactManifest>>,
    >,
    #[cfg(feature = "execution")] forecast_node_heartbeats: Arc<RwLock<FxHashMap<String, i64>>>,
    #[cfg(feature = "execution")] forecast_task_results: Arc<
        RwLock<FxHashMap<String, ForecastVerificationReceipt>>,
    >,
    state_diverged: Arc<AtomicBool>,
    // #1724: how the `state_diverged` readiness latch may be *cleared*. The
    // default (`PeerAgreement`) preserves the #1717 behaviour: a same-height
    // peer that re-confirms our tip + non-zero root clears it. Under
    // `LocallyVerifiedRoot` the peer path can never clear the latch — only a
    // locally-verified canonical root (recomputed deterministically from our own
    // materialized state, post-#1681 producer/applier/replay parity) does, so a
    // colluding same-height peer cannot clear it by echoing a matching tip+root.
    //
    // Injected by the caller (production reads it from the environment via
    // `StateDivergenceRecoveryPolicy::from_env()` at the spawn site in `main.rs`)
    // rather than read here, so the runtime gate is exercisable in tests without
    // mutating process-global env state.
    recovery_policy: StateDivergenceRecoveryPolicy,
    // Shared peer-reputation tracker (#1406): updated here on `PeerPenalized`
    // and read by the `/network/peers/reputation` operator route. Shared (not
    // loop-local) so the route observes the same state the loop maintains.
    peer_reputation: crate::peer_reputation::SharedPeerReputation,
    // #2389: node-local weather identity key used to sign the commit
    // authorization on P2P-ingested weather contributions (see
    // `bridge_weather_payload_to_mempool`). In production this is the node's
    // `weather_gossip_signing_key`.
    commit_signing_key: Arc<wrfcoin_native_chain::crypto::signature::PrivateKey>,
) {
    run_p2p_event_loop_with_persist_control(
        event_rx,
        blockchain,
        mempool,
        storage,
        consensus,
        p2p,
        da_challenge_runtime,
        bft_finality,
        finality_signal_tx,
        #[cfg(feature = "execution")]
        forecast_task_assignments,
        #[cfg(feature = "execution")]
        forecast_node_heartbeats,
        #[cfg(feature = "execution")]
        forecast_task_results,
        state_diverged,
        recovery_policy,
        peer_reputation,
        commit_signing_key,
        #[cfg(test)]
        None,
    )
    .await;
}

async fn run_p2p_event_loop_with_persist_control(
    mut event_rx: mpsc::Receiver<P2pEvent>,
    blockchain: Arc<RwLock<Blockchain>>,
    mempool: Arc<RwLock<Mempool>>,
    storage: Arc<RwLock<Storage>>,
    consensus: Arc<TypedConsensusManager<PopConsensusAdapter>>,
    p2p: Option<P2pClient>,
    da_challenge_runtime: DaChallengeRuntime,
    bft_finality: Option<Arc<RwLock<BftFinalityGadget>>>,
    finality_signal_tx: Option<FinalitySignalSender>,
    #[cfg(feature = "execution")] forecast_task_assignments: Arc<
        RwLock<FxHashMap<String, ForecastArtifactManifest>>,
    >,
    #[cfg(feature = "execution")] forecast_node_heartbeats: Arc<RwLock<FxHashMap<String, i64>>>,
    #[cfg(feature = "execution")] forecast_task_results: Arc<
        RwLock<FxHashMap<String, ForecastVerificationReceipt>>,
    >,
    state_diverged: Arc<AtomicBool>,
    recovery_policy: StateDivergenceRecoveryPolicy,
    peer_reputation: crate::peer_reputation::SharedPeerReputation,
    commit_signing_key: Arc<wrfcoin_native_chain::crypto::signature::PrivateKey>,
    #[cfg(test)] persist_control: Option<EventPersistTestControl>,
) {
    log::info!("P2P event consumer started");
    log::info!("P2P state-divergence recovery policy: {recovery_policy:?}");
    // #2119: serve-side admission control for inbound block-sync requests.
    // Lives beside the fork/orphan buffers because it is the same kind of thing —
    // per-peer state owned by the event loop — and because a governor shared
    // across peers would let one hostile peer deny service to honest ones.
    let sync_serve_governor = Arc::new(Mutex::new(
        wrfcoin_p2p::SyncServeGovernor::new(wrfcoin_p2p::SyncServeQuota::default()),
    ));
    // #2579: bound on concurrent serve tasks — see MAX_CONCURRENT_SYNC_SERVES.
    let sync_serve_permits = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_SYNC_SERVES));
    let mut fork_buffer = ForkCandidateBuffer::new();
    let mut orphan_buffer = OrphanBlockBuffer::new();

    while let Some(event) = event_rx.recv().await {
        match event {
            P2pEvent::ListeningAddress(address) => {
                log::info!("P2P listening on {address}");
            },

            P2pEvent::NewBlock(bytes) => {
                let block: Block = match bincode::deserialize(&bytes) {
                    Ok(b) => b,
                    Err(e) => {
                        note_payload_decode_failure("block", "live gossip", &e);
                        continue;
                    }
                };
                let height_before = blockchain.read().await.height;
                let height = block.height;
                let hash = block.hash.unwrap_or_else(|| block.calculate_hash());
                let outcome = apply_event_block(
                    block,
                    P2pBlockSource::LiveGossip,
                    &blockchain,
                    &storage,
                    &consensus,
                    p2p.as_ref(),
                    &mut fork_buffer,
                    &mut orphan_buffer,
                    #[cfg(test)]
                    persist_control.as_ref(),
                )
                .await;
                report_local_block_failure(
                    P2pBlockSource::LiveGossip,
                    height,
                    hash,
                    &outcome,
                    #[cfg(test)]
                    persist_control.as_ref(),
                );
                drain_orphan_buffer_with_control(
                    &blockchain,
                    &storage,
                    &consensus,
                    p2p.as_ref(),
                    &mut fork_buffer,
                    &mut orphan_buffer,
                    #[cfg(test)]
                    persist_control.as_ref(),
                )
                .await;
                // Drain confirmed transactions from the mempool so that their
                // replay keys don't accumulate forever (#1847).
                drain_mempool_for_confirmed_range(height_before, &blockchain, &mempool).await;
                try_clear_latch_on_local_root(
                    recovery_policy,
                    &blockchain,
                    state_diverged.as_ref(),
                )
                .await;
            }

            P2pEvent::NewTransactions(tx_bytes_vec) => {
                let (signing_keys, height) = {
                    let chain = blockchain.read().await;
                    (chain.state.signing_keys.clone(), chain.height)
                };
                let mut mp = mempool.write().await;
                for tx_bytes in tx_bytes_vec {
                    let tx: Transaction = match bincode::deserialize(&tx_bytes) {
                        Ok(t) => t,
                        Err(e) => {
                            note_payload_decode_failure("transaction", "live gossip", &e);
                            continue;
                        },
                    };
                    // Mempool naturally rejects duplicates.
                    let _ = ingest_gossiped_transaction_at_height(&mut mp, tx, &signing_keys, Some(height));
                }
            },

            P2pEvent::PeerConnected(peer_id) => {
                // #2589: count the connection in AGGREGATE only. Allocating a
                // `peer_id` series here would let an unauthenticated peer cycling
                // libp2p identities grow the metric family without bound, before
                // it has proven anything. The per-peer series is created at
                // admission instead — see the `PeerChainStatus` arm below.
                CORE_METRICS.record_preadmission_peer_connection();
                da_challenge_runtime
                    .record_peer_connected(peer_id.to_string())
                    .await;
                log::info!("P2P peer connected: {peer_id}");
            },

            P2pEvent::PeerDisconnected(peer_id) => {
                let peer_label = peer_id.to_string();
                // #2589: release the per-peer series. `IntCounterVec` has no
                // eviction, so without this an admitted peer's series outlives
                // its connection for the whole process lifetime. Safe for peers
                // that were never admitted — release is a no-op for those.
                CORE_METRICS.release_state_divergence_peer(&peer_label);
                da_challenge_runtime
                    .record_peer_disconnected(&peer_label)
                    .await;
                log::info!("P2P peer disconnected: {peer_id}");
            },

            P2pEvent::PeerChainStatus {
                peer_id,
                height,
                tip_hash,
                state_root,
            } => {
                log::info!(
                    "P2P peer {peer_id} chain status: height={height} tip={} state_root={}",
                    hex::encode(tip_hash),
                    hex::encode(state_root)
                );
                let chain = blockchain.read().await;
                let local_tip: [u8; 32] = chain.tip.into();
                let local_root: [u8; 32] = chain.state.state_root.0;
                let peer_label = peer_id.to_string();
                // #2589: THIS is the admission boundary. A peer reaching here has
                // reported chain status, so it earns a `peer_id` series — created
                // at zero so a healthy admitted peer is visible in /metrics before
                // it ever diverges. Released again on `PeerDisconnected`.
                CORE_METRICS.admit_state_divergence_peer(&peer_label);

                // #2734: when the peer reports from BEHIND our tip (the normal
                // race on a continuously producing chain), resolve OUR own
                // block at the peer's height so full agreement with our chain
                // can clear the divergence latch without waiting for the
                // exact-height coincidence that steady state rarely produces.
                // The hash comes from the block itself; the state root is the
                // post-state root that block committed (#2219). A block outside
                // the in-memory window, an unhashed block, or a malformed
                // commitment yields None — identical to pre-#2734 behavior.
                let local_at_peer_height = if height < chain.height {
                    chain.blocks().get(&height).and_then(|b| {
                        let block_hash: [u8; 32] = (*b.hash.as_ref()?).into();
                        let committed_root = b
                            .metadata
                            .get(wrfcoin_shared_protocol::blockchain::block::STATE_ROOT_METADATA_KEY)
                            .and_then(|s| {
                                let bytes = hex::decode(s).ok()?;
                                <[u8; 32]>::try_from(bytes).ok()
                            })?;
                        Some(ChainStateView {
                            height,
                            tip_hash: block_hash,
                            state_root: committed_root,
                        })
                    })
                } else {
                    None
                };

                match observe_peer_chain_status_with_history(
                    &peer_label,
                    ChainStateView {
                        height,
                        tip_hash,
                        state_root,
                    },
                    ChainStateView {
                        height: chain.height,
                        tip_hash: local_tip,
                        state_root: local_root,
                    },
                    local_at_peer_height,
                    state_diverged.as_ref(),
                    recovery_policy,
                ) {
                    ChainDivergenceObservation::OmittedStateRoot => {
                        // Peer sent an all-zero state root but agrees on tip and height.
                        // This happens when a peer binary predates state-root v6 gossip or
                        // does not yet compute/publish state roots.  It is not evidence of
                        // consensus divergence; downgrade to WARN so operators can still
                        // observe the condition without false-positive ERROR noise.
                        log::warn!(
                            "peer {peer_id} at height {height} omitted state root \
                             (reported all-zero); tip hash matches local; peer likely \
                             does not publish state roots yet",
                        );
                    }
                    ChainDivergenceObservation::Diverged { .. } => {
                        log::error!(
                            "STATE DIVERGENCE DETECTED: peer {peer_id} at height {height} \
                             has tip {} / state_root {} but local is tip {} / state_root {}",
                            hex::encode(tip_hash),
                            hex::encode(state_root),
                            hex::encode(local_tip),
                            hex::encode(local_root)
                        );
                    }
                    ChainDivergenceObservation::Recovered => {
                        // A same-height peer re-confirmed our tip AND a non-zero
                        // state root after we had latched diverged. The node has
                        // re-converged; the divergence latch was cleared so
                        // /readyz can report ready again (#1713).
                        log::warn!(
                            "STATE DIVERGENCE CLEARED: peer {peer_id} at height {height} \
                             now agrees on tip {} / state_root {}; node re-converged, \
                             clearing readiness latch",
                            hex::encode(tip_hash),
                            hex::encode(state_root),
                        );
                    }
                    ChainDivergenceObservation::AwaitingLocalProof => {
                        // #1724 (LocallyVerifiedRoot policy only): a same-height
                        // peer agrees on tip + non-zero root and *would* clear
                        // the latch under the default PeerAgreement policy, but
                        // we refuse to trust the peer's self-reported root.
                        // The latch stays set until our own canonical-root
                        // recompute confirms re-convergence (handled on the
                        // NewBlock path via observe_local_canonical_root).
                        log::info!(
                            "peer {peer_id} at height {height} agrees on tip / state_root \
                             {} but recovery policy is LocallyVerifiedRoot; readiness latch \
                             held pending local canonical-root verification",
                            hex::encode(state_root),
                        );
                    }
                    ChainDivergenceObservation::LocalRootMismatch => {
                        // observe_peer_chain_status never returns this variant;
                        // it can only originate from observe_local_canonical_root.
                        // Match arm kept exhaustive defensively.
                        log::error!(
                            "INTERNAL STATE INTEGRITY FAILURE: local canonical-root recompute \
                             disagrees with stored state root at height {height}",
                        );
                    }
                    ChainDivergenceObservation::NoDivergence => {}
                }

                // Detect fork: peer is ahead or at same height with
                // different tip. The SyncManager will request blocks from
                // this peer, which will flow through apply_p2p_block and
                // trigger reorg if the peer's chain is better.
                if height > chain.height && tip_hash != local_tip {
                    log::warn!(
                        "FORK DETECTED: peer {peer_id} is at height {} (local={}) \
                         with different tip — reorg may be needed",
                        height,
                        chain.height
                    );
                }
            },

            P2pEvent::InboundBlockSyncRequest {
                peer_id,
                request_id,
                request,
            } => {
                // #2119: serve-side admission BEFORE any storage read. Every
                // other ceiling in sync.rs bounds what this node ASKS FOR; this
                // is the one that bounds what a peer can make it DO.
                let peer_key = peer_id.to_string();
                // The client is resolved before admission: with no P2P client
                // there is nobody to answer, so consuming the peer's allowance
                // for an unanswerable request would only skew the accounting.
                // (In production these events originate FROM the p2p task, so
                // the client is always present here.)
                let Some(client) = p2p.clone() else {
                    log::warn!(
                        "Received block sync request from {peer_id} but no P2P client is available"
                    );
                    continue;
                };
                // A monotonic `Instant`, never a wall clock. The value reaches no
                // consensus surface — never applied to state, never written to the
                // WAL, never sent on the wire. It decides only whether THIS node
                // answers THIS peer now, so two nodes reaching different decisions
                // is the intended behaviour rather than a divergence. The governor
                // holds no clock of its own (`admit` takes `now`), which keeps the
                // policy a pure function. (The serve task reads the clock once
                // more when it settles — see `handle_inbound_block_sync_request`.)
                // determinism-ok: node-local serve rate limiting; never enters state, WAL or wire (#2119)
                let now = std::time::Instant::now();
                let respond: SyncServeResponder = Box::new(move |response| {
                    Box::pin(async move {
                        if let Err(err) = client
                            .command_tx
                            .send(P2pCommand::RespondToBlockSyncRequest {
                                request_id,
                                response,
                            })
                            .await
                        {
                            log::warn!("Failed to answer P2P sync request from {peer_id}: {err}");
                        }
                    })
                });
                if let Some((label, retry_after)) = handle_inbound_block_sync_request(
                    &sync_serve_governor,
                    &sync_serve_permits,
                    &blockchain,
                    &storage,
                    peer_key,
                    request.start_height,
                    request.count,
                    now,
                    respond,
                )
                .await
                {
                    // Counted, not just logged. These rejections ARE the
                    // observation in "observe before enforce": the limits
                    // ship with defaults chosen for ~12 nodes and no
                    // measurement behind them, so a reason that exists
                    // only in a debug log is the same as no reason at all.
                    CORE_METRICS.inc_sync_serve_rejected(label);
                    log::debug!(
                        "Block-sync request from {peer_id} refused ({label}) — retry after \
                         {retry_after:?} (#2119)"
                    );
                }
            },

            P2pEvent::SyncedBlock {
                peer_id,
                height,
                block_bytes,
            } => {
                let block: Block = match bincode::deserialize(&block_bytes) {
                    Ok(b) => b,
                    Err(e) => {
                        note_payload_decode_failure(
                            "block",
                            &format!("synced block at height {height} from {peer_id}"),
                            &e,
                        );
                        report_authoritative_sync_rejection(
                            p2p.as_ref(),
                            &blockchain,
                            peer_id,
                            height,
                            AuthoritativeSyncRejectionAttribution::PeerInvalid,
                            format!("failed to deserialize synced block: {e}"),
                        )
                        .await;
                        continue;
                    },
                };
                let height_before = blockchain.read().await.height;
                let outcome = apply_p2p_block(
                    block,
                    P2pBlockSource::HistoricalSync,
                    &blockchain,
                    &storage,
                    &consensus,
                    p2p.as_ref(),
                    &mut fork_buffer,
                    &mut orphan_buffer,
                )
                .await;
                if let P2pBlockApplyOutcome::Rejected {
                    attribution,
                    reason,
                } = outcome
                {
                    report_authoritative_sync_rejection(
                        p2p.as_ref(),
                        &blockchain,
                        peer_id,
                        height,
                        attribution,
                        reason,
                    )
                    .await;
                    continue;
                }
                drain_orphan_buffer(
                    &blockchain,
                    &storage,
                    &consensus,
                    p2p.as_ref(),
                    &mut fork_buffer,
                    &mut orphan_buffer,
                )
                .await;
                // Drain confirmed transactions from the mempool so that their
                // replay keys don't accumulate forever (#1847).
                drain_mempool_for_confirmed_range(
                    height_before,
                    &blockchain,
                    &mempool,
                )
                .await;
                try_clear_latch_on_local_root(
                    recovery_policy,
                    &blockchain,
                    state_diverged.as_ref(),
                )
                .await;
            },

            P2pEvent::SyncCompleted { height } => {
                let completed_tip = {
                    let chain = blockchain.read().await;
                    if chain.height < height {
                        log::warn!(
                            "Ignoring premature P2P sync completion at height {height}: \
                             launch API has applied only through {}",
                            chain.height
                        );
                        None
                    } else {
                        chain
                            .last_block()
                            .cloned()
                            .map(|block| (block, chain.state.state_root.0))
                    }
                };
                if let (Some(client), Some((tip, state_root))) =
                    (p2p.as_ref(), completed_tip)
                {
                    // Exactly one flush after the response that completed IBD.
                    // At current testnet heights the periodic fallback is 300s,
                    // so making the final tip/root visible immediately matters.
                    broadcast_p2p_chain_status(client, &tip, state_root);
                }
            },

            P2pEvent::PeerPenalized {
                peer_id,
                reason,
                banned,
            } => {
                // #1406: accumulate the penalty into shared per-peer reputation
                // (escalating the log once a peer is graylisted). When this
                // penalty is the graylist *transition*, evict the peer: send a
                // `DisconnectPeer` command so the node removes it from kademlia
                // and drops the connection. Issued once per transition (not on
                // every subsequent penalty) so we don't spam the command channel.
                let newly_graylisted = {
                    let mut tracker = peer_reputation.write().await;
                    crate::peer_reputation::apply_peer_penalty(
                        &mut tracker,
                        &peer_id,
                        &reason,
                        banned,
                    )
                };
                if newly_graylisted {
                    CORE_METRICS.inc_peer_graylisted();
                    if let Some(client) = p2p.as_ref() {
                        client.disconnect_peer(peer_id, format!("graylisted: {reason}"));
                    }
                }
            },

            P2pEvent::WeatherData { station_id, data, .. } => {
                bridge_weather_payload_to_mempool(
                    &station_id,
                    &data,
                    &mempool,
                    &commit_signing_key,
                )
                .await;
                log::info!("P2P weather data received from station {station_id}");
            },

            #[allow(unused_variables)]
            P2pEvent::ForecastTaskAssignment {
                task_id, node_id, manifest_bytes, ..
            } => {
                #[cfg(feature = "execution")]
                {
                    let manifest: ForecastArtifactManifest = match bincode::deserialize(&manifest_bytes) {
                        Ok(manifest) => manifest,
                        Err(e) => {
                            note_payload_decode_failure(
                                "forecast_manifest",
                                &format!("ForecastTaskAssignment task={task_id}"),
                                &e,
                            );
                            continue;
                        },
                    };
                    if let Err(err) = manifest.validate() {
                        log::warn!(
                            "Rejecting ForecastTaskAssignment {task_id}: manifest validation failed: {err}"
                        );
                        continue;
                    }

                    let mut assignments = forecast_task_assignments.write().await;
                    assignments.insert(task_id.clone(), manifest);
                    log::debug!(
                        "Stored forecast task assignment: task={task_id} node={node_id}"
                    );
                }
                #[cfg(not(feature = "execution"))]
                {
                    let _ = manifest_bytes;
                    log::debug!("P2P forecast task assignment: task={task_id} node={node_id}");
                }
            },

            P2pEvent::ComputeNodeHeartbeat {
                node_id,
                active_tasks,
                ..
            } => {
                #[cfg(feature = "execution")]
                {
                    let now = match std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                    {
                        Ok(delta) => delta.as_secs() as i64,
                        Err(_) => 0,
                    };
                    let mut heartbeats = forecast_node_heartbeats.write().await;
                    heartbeats.insert(node_id.clone(), now);
                    log::debug!(
                        "Stored forecast compute heartbeat: node={node_id} active_tasks={active_tasks}"
                    );
                }
                #[cfg(not(feature = "execution"))]
                {
                    log::debug!(
                        "P2P compute heartbeat: node={node_id} active_tasks={active_tasks}"
                    );
                }
            },

            #[allow(unused_variables)]
            P2pEvent::ForecastTaskResult {
                task_id,
                node_id,
                receipt_bytes,
            } => {
                #[cfg(feature = "execution")]
                {
                    let receipt: ForecastVerificationReceipt =
                        match bincode::deserialize(&receipt_bytes) {
                            Ok(receipt) => receipt,
                            Err(e) => {
                                note_payload_decode_failure(
                                    "forecast_receipt",
                                    &format!("ForecastTaskResult task={task_id}"),
                                    &e,
                                );
                                continue;
                            },
                        };
                    if let Err(err) = receipt.validate() {
                        log::warn!(
                            "Rejecting ForecastTaskResult {task_id}: receipt validation failed: {err}"
                        );
                        continue;
                    }
                    if !forecast_receipt_has_valid_signature(&receipt) {
                        log::warn!(
                            "Rejecting ForecastTaskResult {task_id}: invalid oracle signature"
                        );
                        continue;
                    }
                    let mut results = forecast_task_results.write().await;
                    results.insert(task_id.clone(), receipt);
                    log::debug!("P2P forecast result accepted: task={task_id} node={node_id}");
                }
                #[cfg(not(feature = "execution"))]
                {
                    let _ = receipt_bytes;
                    log::debug!("P2P forecast result: task={task_id} node={node_id}");
                }
            },

            P2pEvent::WeatherDataV2 {
                station_id,
                data,
                content_ref,
            } => {
                if let Some(content_ref) = content_ref.as_ref() {
                    da_challenge_runtime
                        .cache_content_ref_payload(content_ref, &data, "weather-data-v2")
                        .await;
                }
                bridge_weather_payload_to_mempool(
                    &station_id,
                    &data,
                    &mempool,
                    &commit_signing_key,
                )
                .await;
                log::info!("P2P weather data v2 received from station {station_id}");
            },

            P2pEvent::ForecastTaskResultV2 {
                task_id,
                node_id,
                receipt_bytes,
                content_ref,
            } => {
                #[cfg(feature = "execution")]
                {
                    let receipt: ForecastVerificationReceipt =
                        match bincode::deserialize(&receipt_bytes) {
                            Ok(receipt) => receipt,
                            Err(e) => {
                                note_payload_decode_failure(
                                    "forecast_receipt",
                                    &format!("ForecastTaskResultV2 task={task_id}"),
                                    &e,
                                );
                                continue;
                            },
                        };
                    if let Err(err) = receipt.validate() {
                        log::warn!(
                            "Rejecting ForecastTaskResultV2 {task_id}: receipt validation failed: {err}"
                        );
                        continue;
                    }
                    if !forecast_receipt_has_valid_signature(&receipt) {
                        log::warn!(
                            "Rejecting ForecastTaskResultV2 {task_id}: invalid oracle signature"
                        );
                        continue;
                    }
                    if let Some(content_ref) = content_ref.as_ref() {
                        da_challenge_runtime
                            .cache_content_ref_payload(
                                content_ref,
                                &receipt_bytes,
                                "forecast-task-result-v2",
                            )
                            .await;
                    }
                    let mut results = forecast_task_results.write().await;
                    results.insert(task_id.clone(), receipt);
                    log::debug!(
                        "P2P forecast result v2 accepted: task={task_id} node={node_id}"
                    );
                }
                #[cfg(not(feature = "execution"))]
                {
                    if let Some(content_ref) = content_ref.as_ref() {
                        da_challenge_runtime
                            .cache_content_ref_payload(
                                content_ref,
                                &receipt_bytes,
                                "forecast-task-result-v2",
                            )
                            .await;
                    }
                    log::debug!("P2P forecast result v2: task={task_id} node={node_id}");
                }
            },

            P2pEvent::DaChallenge { peer_id, challenge } => {
                da_challenge_runtime
                    .handle_inbound_challenge(&peer_id.to_string(), challenge.clone(), p2p.as_ref())
                    .await;
                log::info!(
                    "P2P DA challenge received: id={} cid={} from={peer_id}",
                    challenge.challenge_id, challenge.cid
                );
            },

            P2pEvent::DaChallengeResponse { peer_id, response } => {
                da_challenge_runtime
                    .handle_inbound_response(&peer_id.to_string(), response.clone())
                    .await;
                log::info!(
                    "P2P DA challenge response: id={} from={peer_id}",
                    response.challenge_id
                );
            },

            // #1388: a peer broadcast a finality vote on the
            // wrfcoin/finality-votes/1 topic. Decode, hand to the gadget
            // via the cryptographically-verified path, and re-broadcast
            // any newly-produced certificate so peers that missed
            // individual votes still see the finality decision.
            P2pEvent::FinalityVote { peer_id, vote_bytes } => {
                let Some(gadget) = bft_finality.as_ref() else {
                    log::debug!(
                        "P2P FinalityVote from {peer_id} dropped — no BFT gadget attached"
                    );
                    continue;
                };
                let vote: FinalityVote = match bincode::deserialize(&vote_bytes) {
                    Ok(v) => v,
                    Err(e) => {
                        note_payload_decode_failure(
                            "finality_vote",
                            &format!("from {peer_id}"),
                            &e,
                        );
                        continue;
                    }
                };
                let height = vote.block_height;
                let block_hash = vote.block_hash;
                let mut g = gadget.write().await;
                match g.submit_vote_verified(vote) {
                    Ok(event) => {
                        CORE_METRICS.bft_finality_votes_received_total.inc();
                        if matches!(event, FinalityEvent::BlockFinalized { .. }) {
                            CORE_METRICS.bft_finality_certificates_emitted_total.inc();
                            // Snapshot + drop the lock before any awaits so we
                            // don't hold the gadget across the broadcast send.
                            let cert_snapshot = g.last_certificate().cloned();
                            drop(g);
                            if let Some(cert) = cert_snapshot {
                                rebroadcast_finality_certificate(&p2p, &cert);
                                if let Some(ref tx) = finality_signal_tx {
                                    emit_finality_signal(
                                        tx,
                                        FinalitySignal::new(
                                            height,
                                            block_hash,
                                            "bft-gadget-p2p",
                                        ),
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => {
                        log::debug!(
                            "P2P FinalityVote from {peer_id} rejected by gadget: {e}"
                        );
                    }
                }
            },

            // #1388/#1783: a peer broadcast a complete certificate. Certificate
            // gossip is untrusted P2P input, so it must be validated before it
            // can advance the local finality signal channel. The gate below is
            // intentionally fail-closed: no attached gadget, stale height,
            // unknown/noncanonical local block, invalid signature set, or
            // below-quorum vote weight all drop the certificate.
            P2pEvent::FinalityCertificate { peer_id, cert_bytes } => {
                let Some(gadget) = bft_finality.as_ref() else {
                    log::warn!(
                        "P2P FinalityCertificate from {peer_id} dropped — no BFT gadget attached"
                    );
                    continue;
                };
                let cert: FinalityCertificate = match bincode::deserialize(&cert_bytes) {
                    Ok(c) => c,
                    Err(e) => {
                        note_payload_decode_failure(
                            "finality_certificate",
                            &format!("from {peer_id}"),
                            &e,
                        );
                        continue;
                    }
                };
                if let Err(e) = validate_p2p_finality_certificate_against_chain(
                    &blockchain,
                    &cert,
                )
                .await
                {
                    log::warn!(
                        "P2P FinalityCertificate from {peer_id} rejected before gadget validation: {e}"
                    );
                    continue;
                }
                {
                    let g = gadget.read().await;
                    if let Err(e) = g.validate_certificate(&cert) {
                        log::warn!(
                            "P2P FinalityCertificate from {peer_id} rejected by BFT validation: {e}"
                        );
                        continue;
                    }
                }
                log::info!(
                    "P2P FinalityCertificate from {peer_id} accepted: height={} votes={} weight={}/{}",
                    cert.block_height,
                    cert.votes.len(),
                    cert.total_vote_weight,
                    cert.total_validator_weight
                );
                if let Some(ref tx) = finality_signal_tx {
                    emit_finality_signal(
                        tx,
                        FinalitySignal::new(
                            cert.block_height,
                            cert.block_hash,
                            "bft-cert-gossip",
                        ),
                    );
                    CORE_METRICS.bft_finality_certificates_emitted_total.inc();
                }
            },

            // Test-only signal — production binary doesn't need to act on this
            P2pEvent::SubscriptionStateChanged { .. } => {},
        }
    }

    log::warn!("P2P event channel closed, event consumer shutting down");
}

async fn validate_p2p_finality_certificate_against_chain(
    blockchain: &Arc<RwLock<Blockchain>>,
    cert: &FinalityCertificate,
) -> Result<(), String> {
    let chain = blockchain.read().await;

    if cert.block_height <= chain.finalized_height() {
        return Err(format!(
            "non-advancing height {} is not above finalized_height {}",
            cert.block_height,
            chain.finalized_height()
        ));
    }

    let Some(block) = chain.block_at_height(cert.block_height) else {
        return Err(format!(
            "unknown local block at height {}; refusing to finalize unseen block hash {}",
            cert.block_height,
            hex::encode(cert.block_hash)
        ));
    };

    let local_hash: [u8; 32] = block.hash.unwrap_or_else(|| block.calculate_hash()).into();
    if local_hash != cert.block_hash {
        return Err(format!(
            "certificate hash {} does not match local canonical block hash {} at height {}",
            hex::encode(cert.block_hash),
            hex::encode(local_hash),
            cert.block_height
        ));
    }

    Ok(())
}

/// Re-broadcast a freshly-produced [`FinalityCertificate`] so peers that
/// missed individual votes still adopt the finality decision (#1388).
fn rebroadcast_finality_certificate(
    p2p: &Option<P2pClient>,
    cert: &FinalityCertificate,
) {
    let Some(client) = p2p.as_ref() else {
        return;
    };
    let bytes = match bincode::serialize(cert) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("Failed to serialize FinalityCertificate for rebroadcast: {e}");
            return;
        }
    };
    if let Err(e) = client.command_tx.try_send(P2pCommand::BroadcastFinalityCertificate {
        cert_bytes: bytes,
    }) {
        log::warn!("Failed to enqueue FinalityCertificate broadcast: {e}");
    }
}

/// Station registrations declined at the P2P ingress since process start.
static GOSSIP_STATION_REGISTRATIONS_DECLINED: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Owner-ledger transactions declined at the P2P ingress since process start.
static GOSSIP_STATION_OWNER_TXS_DECLINED: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Log the first this-many declines individually.
const DECLINED_LOG_FIRST: u64 = 10;

/// Should the `total`-th decline be logged? The first [`DECLINED_LOG_FIRST`], then
/// only when `total` is a power of two, so the log grows with log2 of the count
/// instead of the count. Pure, so it is testable.
fn should_log_declined_registration(total: u64) -> bool {
    total <= DECLINED_LOG_FIRST || total.is_power_of_two()
}

/// Outcome of offering one gossiped transaction to the local mempool.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum GossipIngest {
    /// Added to the mempool.
    Admitted,
    /// Declined by node admission policy before the mempool saw it.
    RefusedByPolicy(&'static str),
    /// The mempool itself rejected it (duplicate, capacity, validation).
    Rejected,
}

/// Admit one gossiped transaction (the `P2pEvent::NewTransactions` ingress).
#[cfg(test)]
pub(crate) fn ingest_gossiped_transaction(
    mempool: &mut Mempool,
    tx: Transaction,
    signing_keys: &wrfcoin_native_chain::security::key_registry::SigningKeyRegistry,
) -> GossipIngest {
    ingest_gossiped_transaction_at_height(mempool, tx, signing_keys, None)
}

/// The height is obtained from the same canonical chain read as signing keys.
/// Unknown height remains fail-closed; peers cannot select the activation gate.
pub(crate) fn ingest_gossiped_transaction_at_height(
    mempool: &mut Mempool,
    tx: Transaction,
    signing_keys: &wrfcoin_native_chain::security::key_registry::SigningKeyRegistry,
    current_height: Option<u64>,
) -> GossipIngest {
    // Node admission policy (no consensus effect): station registrations are
    // admitted only through the operator-only registrations route, which
    // checks the caller's role and the registration's authorization. They are
    // not taken from the peer-to-peer network. Only a real registration payload
    // is matched, so the other transactions carried under the same type (the
    // owner-ledger kinds) are not affected by THIS rule (see the next one).
    // Role-independent by construction.
    if tx.tx_type == TransactionType::StationRegistration
        && tx.parse_station_registration().is_some()
    {
        let total = GOSSIP_STATION_REGISTRATIONS_DECLINED.fetch_add(1, Ordering::Relaxed) + 1;
        // Rate-limited: a peer must not be able to fill the log by sending
        // many. The counter is exact; only the log line is thinned.
        if should_log_declined_registration(total) {
            log::warn!(
                "P2P station registration not admitted: registrations are accepted only through the operator route (declined so far: {total})"
            );
        }
        return GossipIngest::RefusedByPolicy("station_registration_gossip");
    }
    // The owner-ledger transactions (transfer, owner action, recovery file /
    // cancel / execute) enter a producer only through the state-checked,
    // deduplicating HTTP submit route, never from the peer-to-peer network. Our
    // nodes do not gossip them, so no legitimate path depends on this. Same
    // thinned logging as above; the counter is exact.
    if tx.is_station_owner_ledger_tx() {
        let total = GOSSIP_STATION_OWNER_TXS_DECLINED.fetch_add(1, Ordering::Relaxed) + 1;
        if should_log_declined_registration(total) {
            log::warn!(
                "P2P station owner transaction not admitted: owner transactions are accepted only through the submit route (declined so far: {total})"
            );
        }
        return GossipIngest::RefusedByPolicy("station_owner_tx_gossip");
    }
    match mempool.add_transaction_with_signing_keys_at_height(tx, Some(signing_keys), current_height) {
        Ok(()) => GossipIngest::Admitted,
        Err(e) => {
            log::debug!("P2P transaction rejected by mempool: {e}");
            GossipIngest::Rejected
        },
    }
}

#[cfg(test)]
pub(crate) use tests::{apply_node_receipt_block_for_test, diff_states};

#[cfg(test)]
mod tests {
    mod e1_stage_a {
        include!("../../../tests/support/e1_stage_a_tests.rs");
    }
    use super::{
        AuthoritativeSyncRejectionAttribution, CORE_METRICS, ForkCandidateBuffer,
        OrphanBlockBuffer, P2pBlockApplyOutcome, P2pBlockSource, PeerPersistTestControl,
        StateDivergenceRecoveryPolicy,
        SyncServeAdmission, SyncServeResponder, apply_p2p_block, collect_block_sync_response,
        decide_sync_serve_admission, detect_fork_from_buffer, drain_orphan_buffer,
        apply_p2p_block_at, apply_p2p_block_with_persist_control_at,
        handle_inbound_block_sync_request, run_p2p_event_loop,
        validate_p2p_finality_certificate_against_chain,
    };
    use chrono::TimeZone;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;
    use tokio::sync::{RwLock, mpsc};
    use wrfcoin_consensus_engines::{
        BftFinalityGadget, ConsensusFactory, ConsensusType, FinalityCertificate,
        FinalityValidator, TypedConsensusManager,
    };
    use wrfcoin_native_chain::blockchain::block::Block;
    use wrfcoin_native_chain::blockchain::chain::Blockchain;
    use wrfcoin_native_chain::blockchain::transaction::{Transaction, TransactionType};
    use wrfcoin_native_chain::config::settings::Config;
    use wrfcoin_native_chain::crypto::hash::Hash as CryptoHash;
    use wrfcoin_native_chain::mempool::pool::Mempool;
    use wrfcoin_native_chain::storage::{DEFAULT_CHECKPOINT_INTERVAL, Storage};
    use wrfcoin_p2p::node::handle_sync_response_for_test;
    use wrfcoin_p2p::{P2pClient, P2pCommand, P2pEvent};
    use wrfcoin_shared_protocol::blockchain::block::STATE_ROOT_METADATA_KEY;
    use wrfcoin_shared_protocol::common::types::{Address, Balance};
    use wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion;
    #[cfg(feature = "execution")]
    use rustc_hash::FxHashMap;
    #[cfg(feature = "execution")]
    use wrfcoin_shared_protocol::blockchain::transaction::{
        ForecastArtifactManifest, ForecastVerificationReceipt,
    };

    use crate::da_challenge_producer::DaChallengeRuntime;
    use crate::transition_extractor::extract_transitions;

    #[cfg(feature = "execution")]
    fn test_forecast_state(
    ) -> (
        Arc<RwLock<FxHashMap<String, ForecastArtifactManifest>>>,
        Arc<RwLock<FxHashMap<String, i64>>>,
        Arc<RwLock<FxHashMap<String, ForecastVerificationReceipt>>>,
    ) {
        (
            Arc::new(RwLock::new(FxHashMap::default())),
            Arc::new(RwLock::new(FxHashMap::default())),
            Arc::new(RwLock::new(FxHashMap::default())),
        )
    }

    fn test_address_string(byte: u8) -> String {
        format!("0x{}", hex::encode([byte; 20]))
    }

    /// Serializes the tests that assert an exact delta on the process-global
    /// `CORE_METRICS.state_diverged_total` counter. `cargo test` runs tests in
    /// parallel within one process, so without this guard two such tests can
    /// interleave between one's before/after read and observe a delta of 2
    /// instead of 1. (These tests began compiling only after the p2p-feature
    /// test build was repaired, exposing the latent shared-counter race.)
    static DIVERGENCE_COUNTER_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Serializes tests that assert an exact delta on the process-global
    /// `CORE_METRICS.peers_graylisted_total` counter (#1789). Same rationale as
    /// `DIVERGENCE_COUNTER_GUARD` above.
    static PEER_GRAYLIST_COUNTER_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn test_block(height: u64, prev_hash: CryptoHash, nonce: u64) -> Block {
        let mut block = Block::new(
            1,
            prev_hash,
            vec![Transaction::new_coinbase(
                &test_address_string(height as u8),
                50,
                height,
            )],
            1,
            height,
        );
        block.timestamp = chrono::Utc
            .timestamp_millis_opt(1_700_000_000_000 + height as i64)
            .single()
            .expect("fixture timestamp should be valid");
        // `Transaction::new_coinbase` stamps `Utc::now()`, and chrono serializes the fractional
        // seconds with a variable digit count, so unpinned fixture blocks differ in serialized
        // size by a few bytes (#3442). Pin it like `block.timestamp`.
        for tx in &mut block.transactions {
            tx.timestamp = block.timestamp;
        }
        block.nonce = nonce;
        block
            .metadata
            .insert("rewards_minted_count".to_string(), "1".to_string());
        block.set_hash(block.calculate_hash());
        block
    }

    fn extend_chain(chain: &mut Blockchain, count: u64, nonce_offset: u64) {
        for i in 0..count {
            let height = chain.height + 1;
            let block = test_block(height, chain.tip, nonce_offset + i);
            chain.add_block(block).expect("fixture chain should extend");
        }
    }

    fn test_consensus() -> Arc<TypedConsensusManager<wrfcoin_consensus_engines::PopConsensusAdapter>>
    {
        let pop_config = ConsensusFactory::default_config(ConsensusType::ProofOfPublishing);
        let pop_adapter = ConsensusFactory::create_pop_adapter(&pop_config);
        // These fixtures intentionally model a chain with no pre-seated oracle
        // ceremony. State that posture explicitly so height 1 exercises its
        // intended behavior instead of failing setup under #2523/#2540.
        pop_adapter.declare_no_genesis_oracles_blocking();
        Arc::new(TypedConsensusManager::new(pop_adapter, pop_config))
    }

    async fn test_consensus_with_genesis_oracle_set(
        oracle_set: Vec<String>,
        oracle_quorum: u64,
    ) -> Arc<TypedConsensusManager<wrfcoin_consensus_engines::PopConsensusAdapter>> {
        let pop_config = ConsensusFactory::default_config(ConsensusType::ProofOfPublishing);
        let pop_adapter = ConsensusFactory::create_pop_adapter(&pop_config);
        pop_adapter
            .set_genesis_oracle_declaration(
                oracle_set.join(","),
                oracle_quorum.to_string(),
            )
            .await;
        Arc::new(TypedConsensusManager::new(pop_adapter, pop_config))
    }

    /// Deterministic node weather-identity key for `run_p2p_event_loop` tests
    /// (#2389). Stands in for the production `weather_gossip_signing_key` the
    /// bridge signs commit authorizations with.
    fn test_commit_signing_key()
    -> std::sync::Arc<wrfcoin_native_chain::crypto::signature::PrivateKey> {
        std::sync::Arc::new(seeded_keypair(7).0)
    }

    fn test_storage() -> Arc<RwLock<Storage>> {
        let tmp = std::env::temp_dir().join(format!("wrfcoin-p2p-events-{}.redb", fastrand::u64(..)));
        Arc::new(RwLock::new(
            Storage::open(&tmp, DEFAULT_CHECKPOINT_INTERVAL).expect("test storage should open"),
        ))
    }

    async fn run_test_p2p_event_transport(
        event_rx: mpsc::Receiver<P2pEvent>,
        blockchain: Arc<RwLock<Blockchain>>,
        p2p: P2pClient,
    ) {
        run_p2p_event_loop(
            event_rx,
            blockchain,
            Arc::new(RwLock::new(Mempool::new(&Config::default()))),
            test_storage(),
            test_consensus(),
            Some(p2p),
            DaChallengeRuntime::new(),
            None,
            None,
            #[cfg(feature = "execution")]
            Arc::new(RwLock::new(FxHashMap::default())),
            #[cfg(feature = "execution")]
            Arc::new(RwLock::new(FxHashMap::default())),
            #[cfg(feature = "execution")]
            Arc::new(RwLock::new(FxHashMap::default())),
            Arc::new(AtomicBool::new(false)),
            StateDivergenceRecoveryPolicy::PeerAgreement,
            Arc::new(RwLock::new(
                crate::peer_reputation::PeerReputationTracker::new(),
            )),
            test_commit_signing_key(),
        )
        .await;
    }

    /// Produce blocks through the real stateful producer, then add the V2
    /// commitment using the post-block state the producer actually reached.
    ///
    /// The launch_api test process stays on V1 so unrelated tests cannot be
    /// contaminated by the process-global protocol switch. Reconstructing the
    /// fixture chain with the re-sealed block makes the next block chain onto
    /// the committed predecessor exactly as a V2 producer would.
    async fn committed_fixture_blocks(
        initial_chain: Blockchain,
        count: usize,
        addr_byte: u8,
    ) -> (Blockchain, Vec<Block>) {
        let consensus = test_consensus();
        let chain = Arc::new(RwLock::new(initial_chain));
        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let mut blocks = Vec::with_capacity(count);

        for _ in 0..count {
            let mut block = consensus
                .create_pop_block_with_state(
                    Arc::clone(&chain),
                    Arc::clone(&mempool),
                    &test_address_string(addr_byte),
                )
                .await
                .expect("fixture block should be created by the real stateful producer");

            let rebuilt = {
                let produced = chain.read().await;
                block.metadata.insert(
                    STATE_ROOT_METADATA_KEY.to_string(),
                    produced.state.state_root.to_string(),
                );
                block.set_hash(block.calculate_hash());

                let mut block_window = produced.blocks().clone();
                block_window.insert(block.height, block.clone());
                Blockchain::from_persisted(
                    produced.height,
                    block.hash.expect("re-sealed fixture block has a hash"),
                    produced.state.clone(),
                    block_window,
                    produced.total_block_count(),
                    produced.finalized_height(),
                )
            };
            *chain.write().await = rebuilt;
            blocks.push(block);
        }

        let final_chain = chain.read().await.clone();
        (final_chain, blocks)
    }


    /// A missing checkpoint is a real storage error, deliberately injected only
    /// after authoritative state apply/finalization. It also detects a partial
    /// block/WAL commit followed by a failed checkpoint transaction.
    #[tokio::test]
    async fn peer_persist_failure_keeps_tip_root_and_advertisements_2086() {
        use wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion;
        let initial = Blockchain::default();
        let (expected, blocks) = committed_fixture_blocks(initial.clone(), 1, 206).await;
        let block = blocks[0].clone();
        for source in [
            P2pBlockSource::LiveGossip,
            P2pBlockSource::HistoricalSync,
            P2pBlockSource::BufferedOrphan,
        ] {
            let chain = Arc::new(RwLock::new(initial.clone()));
            let before = chain_mark(&chain).await;
            let dir = tempfile::tempdir().unwrap();
            let storage = Arc::new(RwLock::new(
                Storage::open(&dir.path().join("chain.redb"), 1).unwrap(),
            ));
            storage
                .read()
                .await
                .persist_genesis_state(&initial.state)
                .await
                .unwrap();
            let before_state = chain.read().await.state.clone();
            let before_bytes = storage
                .read()
                .await
                .checkpoint_state_bytes(1, &before_state)
                .unwrap()
                .unwrap();
            let (p2p, mut commands) =
                P2pClient::for_testing_with_commands(libp2p::PeerId::random());
            let consensus = test_consensus();
            let mut forks = ForkCandidateBuffer::new();
            let mut orphans = OrphanBlockBuffer::new();
            let fault = PeerPersistTestControl {
                omit_checkpoint: true,
                barrier: None,
            };
            let outcome = apply_p2p_block_with_persist_control_at(
                SigningProtocolVersion::V2,
                block.clone(),
                source,
                &chain,
                &storage,
                &consensus,
                Some(&p2p),
                &mut forks,
                &mut orphans,
                std::convert::identity,
                Some(&fault),
            )
            .await;
            assert!(
                commands.try_recv().is_err(),
                "failed persistence advertised chain advancement: {source:?}"
            );
            assert!(
                matches!(
                    outcome,
                    P2pBlockApplyOutcome::Rejected {
                        attribution: AuthoritativeSyncRejectionAttribution::LocalFailure,
                        ..
                    }
                ),
                "storage failure must be local rejection: {outcome:?}"
            );
            assert_eq!(
                chain_mark(&chain).await,
                before,
                "failed write changed API-visible chain"
            );
            let after_state = chain.read().await.state.clone();
            let after_bytes = storage
                .read()
                .await
                .checkpoint_state_bytes(1, &after_state)
                .unwrap()
                .unwrap();
            assert_eq!(
                after_bytes, before_bytes,
                "failed write changed compatible checkpoint bytes"
            );
            assert_eq!(
                chain.read().await.total_block_count(),
                initial.total_block_count()
            );
            assert_eq!(
                chain.read().await.in_memory_block_count(),
                initial.in_memory_block_count()
            );
            assert_eq!(
                storage
                    .read()
                    .await
                    .load_state()
                    .unwrap()
                    .map(|chain| chain.height)
                    .unwrap_or(0),
                initial.height,
                "failed checkpoint leaked a durable block/WAL commit"
            );
            let outcome = apply_p2p_block_at(
                SigningProtocolVersion::V2,
                block.clone(),
                source,
                &chain,
                &storage,
                &consensus,
                Some(&p2p),
                &mut forks,
                &mut orphans,
                std::convert::identity,
            )
            .await;
            assert_eq!(
                outcome,
                P2pBlockApplyOutcome::Applied,
                "same block must retry successfully"
            );
            let applied = chain.read().await;
            assert_eq!(applied.height, expected.height);
            assert_eq!(applied.state.state_root, expected.state.state_root);
            let recovered = storage.read().await.load_state().unwrap().unwrap();
            assert_eq!(recovered.height, applied.height);
            assert_eq!(recovered.tip, applied.tip);
            assert_eq!(recovered.state.state_root, applied.state.state_root);
            assert!(
                commands.try_recv().is_ok(),
                "successful retry must update P2P"
            );
        }
    }

    /// API readers and peers cannot observe the candidate while persistence is
    /// paused. Releasing the barrier permits one durable commit and publication.
    #[tokio::test]
    async fn peer_tip_is_not_visible_while_persistence_is_pending_2086() {
        use wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion;
        let initial = Blockchain::default();
        let (_, blocks) = committed_fixture_blocks(initial.clone(), 1, 207).await;
        let chain = Arc::new(RwLock::new(initial));
        let dir = tempfile::tempdir().unwrap();
        let storage = Arc::new(RwLock::new(
            Storage::open(&dir.path().join("chain.redb"), 1).unwrap(),
        ));
        let (p2p, mut commands) = P2pClient::for_testing_with_commands(libp2p::PeerId::random());
        let reached = Arc::new(tokio::sync::Notify::new());
        let resume = Arc::new(tokio::sync::Notify::new());
        let applying = {
            let chain = Arc::clone(&chain);
            let storage = Arc::clone(&storage);
            let reached = Arc::clone(&reached);
            let resume = Arc::clone(&resume);
            tokio::spawn(async move {
                let control = PeerPersistTestControl {
                    omit_checkpoint: false,
                    barrier: Some((reached, resume)),
                };
                apply_p2p_block_with_persist_control_at(
                    SigningProtocolVersion::V2,
                    blocks[0].clone(),
                    P2pBlockSource::LiveGossip,
                    &chain,
                    &storage,
                    &test_consensus(),
                    Some(&p2p),
                    &mut ForkCandidateBuffer::new(),
                    &mut OrphanBlockBuffer::new(),
                    std::convert::identity,
                    Some(&control),
                )
                .await
            })
        };
        tokio::time::timeout(std::time::Duration::from_secs(10), reached.notified())
            .await
            .unwrap();
        let premature_command = commands.try_recv().is_ok();
        let premature_tip =
            tokio::time::timeout(std::time::Duration::from_millis(30), chain.read())
                .await
                .is_ok();
        resume.notify_one();
        assert_eq!(applying.await.unwrap(), P2pBlockApplyOutcome::Applied);
        assert!(!premature_command, "peer saw unpersisted tip");
        assert!(
            !premature_tip,
            "API reader crossed the pre-durability publication boundary"
        );
        assert_eq!(chain.read().await.height, 1);
    }

    fn caller_fault_control_2086() -> (
        super::EventPersistTestControl,
        mpsc::UnboundedReceiver<String>,
    ) {
        let (reports, rx) = mpsc::unbounded_channel();
        (
            super::EventPersistTestControl {
                persist: PeerPersistTestControl {
                    omit_checkpoint: true,
                    barrier: None,
                },
                version: SigningProtocolVersion::V2,
                reports,
            },
            rx,
        )
    }

    /// Drive the actual NewBlock arm, not just the apply helper. Its failure
    /// diagnostic must consume LocalFailure and preserve the storage reason.
    #[tokio::test]
    async fn live_new_block_reports_local_persist_failure_2086() {
        let initial = Blockchain::default();
        let (_, blocks) = committed_fixture_blocks(initial.clone(), 1, 208).await;
        let block = blocks[0].clone();
        let chain = Arc::new(RwLock::new(initial.clone()));
        let dir = tempfile::tempdir().unwrap();
        let storage = Arc::new(RwLock::new(
            Storage::open(&dir.path().join("chain.redb"), 1).unwrap(),
        ));
        storage
            .read()
            .await
            .persist_genesis_state(&initial.state)
            .await
            .unwrap();
        let before_bytes = storage
            .read()
            .await
            .checkpoint_state_bytes(1, &initial.state)
            .unwrap()
            .unwrap();
        let (client, mut commands) = P2pClient::for_testing_with_commands(libp2p::PeerId::random());
        let (event_tx, event_rx) = mpsc::channel(1);
        let (control, mut reports) = caller_fault_control_2086();
        event_tx
            .send(P2pEvent::NewBlock(bincode::serialize(&block).unwrap()))
            .await
            .unwrap();
        drop(event_tx);
        tokio::time::timeout(
            Duration::from_secs(10),
            super::run_p2p_event_loop_with_persist_control(
                event_rx,
                Arc::clone(&chain),
                Arc::new(RwLock::new(Mempool::new(&Config::default()))),
                Arc::clone(&storage),
                test_consensus(),
                Some(client),
                DaChallengeRuntime::new(),
                None,
                None,
                #[cfg(feature = "execution")]
                Arc::new(RwLock::new(FxHashMap::default())),
                #[cfg(feature = "execution")]
                Arc::new(RwLock::new(FxHashMap::default())),
                #[cfg(feature = "execution")]
                Arc::new(RwLock::new(FxHashMap::default())),
                Arc::new(AtomicBool::new(false)),
                StateDivergenceRecoveryPolicy::PeerAgreement,
                Arc::new(RwLock::new(
                    crate::peer_reputation::PeerReputationTracker::new(),
                )),
                test_commit_signing_key(),
                Some(control),
            ),
        )
        .await
        .unwrap();
        let report = reports
            .try_recv()
            .expect("NewBlock caller silently dropped LocalFailure");
        assert!(
            report.contains("LiveGossip")
                && report.contains("height=1")
                && report.contains(&block.hash.unwrap().to_string())
                && report.contains("persist block failed"),
            "{report}"
        );
        assert!(
            commands.try_recv().is_err(),
            "failed live import advertised advancement"
        );
        let failed = chain.read().await;
        assert_eq!(failed.height, initial.height);
        assert_eq!(
            storage
                .read()
                .await
                .checkpoint_state_bytes(1, &failed.state)
                .unwrap()
                .unwrap(),
            before_bytes
        );
    }

    /// A local write failure must retain both the failed orphan and untried
    /// candidates. A later drain retries successfully, without a busy retry.
    #[tokio::test]
    async fn orphan_drain_reports_and_requeues_local_persist_failure_2086() {
        let initial = Blockchain::default();
        let (_, blocks) = committed_fixture_blocks(initial.clone(), 1, 209).await;
        let (_, alternative) = committed_fixture_blocks(initial.clone(), 1, 210).await;
        let chain = Arc::new(RwLock::new(initial.clone()));
        let dir = tempfile::tempdir().unwrap();
        let storage = Arc::new(RwLock::new(
            Storage::open(&dir.path().join("chain.redb"), 1).unwrap(),
        ));
        storage
            .read()
            .await
            .persist_genesis_state(&initial.state)
            .await
            .unwrap();
        let before_bytes = storage
            .read()
            .await
            .checkpoint_state_bytes(1, &initial.state)
            .unwrap()
            .unwrap();
        let (client, mut commands) = P2pClient::for_testing_with_commands(libp2p::PeerId::random());
        let (mut control, mut reports) = caller_fault_control_2086();
        let mut forks = ForkCandidateBuffer::new();
        let mut orphans = OrphanBlockBuffer::new();
        orphans.insert(blocks[0].clone());
        orphans.insert(alternative[0].clone());
        super::drain_orphan_buffer_with_control(
            &chain,
            &storage,
            &test_consensus(),
            Some(&client),
            &mut forks,
            &mut orphans,
            Some(&control),
        )
        .await;
        assert_eq!(
            orphans.total_count(),
            2,
            "local failure dropped buffered orphan candidates"
        );
        assert!(orphans.blocks[&1].iter().any(|b| b.hash == blocks[0].hash));
        assert!(orphans.blocks[&1]
            .iter()
            .any(|b| b.hash == alternative[0].hash));
        let report = reports
            .try_recv()
            .expect("orphan caller silently dropped LocalFailure");
        assert!(
            report.contains("BufferedOrphan")
                && report.contains("height=1")
                && report.contains(&blocks[0].hash.unwrap().to_string())
                && report.contains("persist block failed"),
            "{report}"
        );
        assert!(
            reports.try_recv().is_err(),
            "drain must stop after local failure rather than busy retry"
        );
        assert!(commands.try_recv().is_err());
        let failed = chain.read().await;
        assert_eq!(failed.height, initial.height);
        assert_eq!(
            storage
                .read()
                .await
                .checkpoint_state_bytes(1, &failed.state)
                .unwrap()
                .unwrap(),
            before_bytes
        );
        drop(failed);
        control.persist.omit_checkpoint = false;
        super::drain_orphan_buffer_with_control(
            &chain,
            &storage,
            &test_consensus(),
            Some(&client),
            &mut forks,
            &mut orphans,
            Some(&control),
        )
        .await;
        assert_eq!(
            chain.read().await.height,
            1,
            "later orphan drain failed to retry"
        );
        assert_eq!(orphans.total_count(), 0);
        assert!(commands.try_recv().is_ok());
        assert!(reports.try_recv().is_err());
    }

    fn corrupt_state_root_commitment(block: &Block) -> Block {
        let mut tampered = block.clone();
        tampered.metadata.insert(
            STATE_ROOT_METADATA_KEY.to_string(),
            "abababababababababababababababababababababababababababababababab".to_string(),
        );
        tampered.set_hash(tampered.calculate_hash());
        tampered
    }

    #[derive(Debug, PartialEq, Eq)]
    struct ChainMark {
        height: u64,
        tip: CryptoHash,
        state_root: String,
    }

    async fn chain_mark(chain: &Arc<RwLock<Blockchain>>) -> ChainMark {
        let chain = chain.read().await;
        ChainMark {
            height: chain.height,
            tip: chain.tip,
            state_root: chain.state.state_root.to_string(),
        }
    }

    async fn feed_committed_block_at_v2(
        block: Block,
        chain: &Arc<RwLock<Blockchain>>,
        storage: &Arc<RwLock<Storage>>,
    ) {
        let consensus = test_consensus();
        let mut fork_buffer = ForkCandidateBuffer::new();
        let mut orphan_buffer = OrphanBlockBuffer::new();
        apply_p2p_block_at(
            SigningProtocolVersion::V2,
            block,
            P2pBlockSource::HistoricalSync,
            chain,
            storage,
            &consensus,
            None,
            &mut fork_buffer,
            &mut orphan_buffer,
            std::convert::identity,
        )
        .await;
    }

    async fn feed_committed_fork_at_v2<F>(
        blocks: Vec<Block>,
        chain: &Arc<RwLock<Blockchain>>,
        storage: &Arc<RwLock<Storage>>,
        prepare_persist_state: F,
    ) where
        F: FnOnce(
            wrfcoin_native_chain::blockchain::state::State,
        ) -> wrfcoin_native_chain::blockchain::state::State,
    {
        assert!(!blocks.is_empty(), "fork fixture must contain at least one block");
        let consensus = test_consensus();
        let mut fork_buffer = ForkCandidateBuffer::new();
        let mut orphan_buffer = OrphanBlockBuffer::new();
        let last = blocks.len() - 1;

        for block in blocks.iter().take(last).cloned() {
            apply_p2p_block_at(
                SigningProtocolVersion::V2,
                block,
                P2pBlockSource::HistoricalSync,
                chain,
                storage,
                &consensus,
                None,
                &mut fork_buffer,
                &mut orphan_buffer,
                std::convert::identity,
            )
            .await;
        }
        apply_p2p_block_at(
            SigningProtocolVersion::V2,
            blocks[last].clone(),
            P2pBlockSource::HistoricalSync,
            chain,
            storage,
            &consensus,
            None,
            &mut fork_buffer,
            &mut orphan_buffer,
            prepare_persist_state,
        )
        .await;
    }

    /// Local height 2 and a competing height-3 fork sharing block 1.
    ///
    /// `try_reorg` rebuilds from the fork point's durable checkpoint, so the
    /// fixture forces a checkpoint at shared block 1 before extending the local
    /// branch. The normal interval stays large so block 2 does not replace and
    /// prune that fork-point checkpoint.
    async fn committed_reorg_fixture(
    ) -> (Arc<RwLock<Blockchain>>, Arc<RwLock<Storage>>, Vec<Block>) {
        let (common_chain, common_blocks) =
            committed_fixture_blocks(Blockchain::default(), 1, 0x41).await;
        let common = common_blocks[0].clone();

        let (local_producer, local_blocks) =
            committed_fixture_blocks(common_chain.clone(), 1, 0x41).await;
        let (_, fork_blocks) = committed_fixture_blocks(common_chain, 2, 0x42).await;

        let chain = Arc::new(RwLock::new(Blockchain::default()));
        let storage = test_storage();
        feed_committed_block_at_v2(common, &chain, &storage).await;
        let common_state = chain.read().await.state.clone();
        storage
            .read()
            .await
            .force_checkpoint(&common_state, 1)
            .await
            .expect("fixture fork point must have a durable checkpoint");
        feed_committed_block_at_v2(local_blocks[0].clone(), &chain, &storage).await;

        assert_eq!(
            chain.read().await.height,
            local_producer.height,
            "fixture control: the local branch must be fully accepted before reorg"
        );
        (chain, storage, fork_blocks)
    }

    // ---------------------------------------------------------------------
    // #2085 — execute every V2 checked-finalization call site.
    // ---------------------------------------------------------------------

    #[tokio::test]
    async fn v2_p2p_apply_accepts_the_same_block_with_the_correct_commitment_2085() {
        let (_, blocks) = committed_fixture_blocks(Blockchain::default(), 1, 0x41).await;
        let chain = Arc::new(RwLock::new(Blockchain::default()));
        let storage = test_storage();

        feed_committed_block_at_v2(blocks[0].clone(), &chain, &storage).await;

        assert_eq!(
            chain.read().await.height,
            1,
            "positive control: the exact fixture used by the negative test must reach and pass \
             the live P2P apply path"
        );
    }

    /// Regression for #2760: initial block download used to publish one
    /// `ChainStatus` for every imported block. A fast new node therefore sent
    /// more than the receiver's 64 statuses / 10 seconds allowance and was
    /// graylisted despite serving only valid history. Exercise the production
    /// apply call site for more than one receiver rate window and prove every
    /// block still advances the P2P sync manager without putting an outbound
    /// status broadcast on the command channel.
    #[tokio::test]
    #[serial_test::serial(upgrade_deadline)]
    async fn p2p_ibd_apply_updates_local_state_without_broadcasting_chain_status_2760() {
        use libp2p::PeerId;
        use wrfcoin_p2p::{P2pClient, P2pCommand};

        const IMPORTED_BLOCKS: usize = 65;
        let (_, blocks) =
            committed_fixture_blocks(Blockchain::default(), IMPORTED_BLOCKS, 0x41).await;
        let expected_height = blocks.last().expect("fixture has a tip").height;
        let expected_tip: [u8; 32] = blocks
            .last()
            .and_then(|block| block.hash)
            .expect("fixture tip has a hash")
            .into();
        let chain = Arc::new(RwLock::new(Blockchain::default()));
        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let (event_tx, event_rx) = mpsc::channel(IMPORTED_BLOCKS + 1);
        let (p2p, mut command_rx) = P2pClient::for_testing_with_commands(PeerId::random());
        let command_collector = tokio::spawn(async move {
            let mut local_updates = 0;
            let mut status_broadcasts = 0;
            let mut last_update = None;
            while let Some(command) = command_rx.recv().await {
                match command {
                    P2pCommand::UpdateLocalChainState {
                        height,
                        tip_hash,
                        state_root,
                    } => {
                        local_updates += 1;
                        last_update = Some((height, tip_hash, state_root));
                    },
                    P2pCommand::BroadcastChainStatus { .. } => status_broadcasts += 1,
                    other => panic!("IBD apply emitted unexpected P2P command: {other:?}"),
                }
            }
            (local_updates, status_broadcasts, last_update)
        });
        let event_loop = tokio::spawn(run_p2p_event_loop(
            event_rx,
            Arc::clone(&chain),
            mempool,
            test_storage(),
            test_consensus(),
            Some(p2p),
            DaChallengeRuntime::new(),
            None,
            None,
            #[cfg(feature = "execution")]
            Arc::new(tokio::sync::RwLock::new(rustc_hash::FxHashMap::default())),
            #[cfg(feature = "execution")]
            Arc::new(tokio::sync::RwLock::new(rustc_hash::FxHashMap::default())),
            #[cfg(feature = "execution")]
            Arc::new(tokio::sync::RwLock::new(rustc_hash::FxHashMap::default())),
            Arc::new(AtomicBool::new(false)),
            StateDivergenceRecoveryPolicy::PeerAgreement,
            std::sync::Arc::new(tokio::sync::RwLock::new(
                crate::peer_reputation::PeerReputationTracker::new(),
            )),
            test_commit_signing_key(),
        ));

        for block in blocks {
            event_tx
                .send(P2pEvent::SyncedBlock {
                    peer_id: libp2p::PeerId::random(),
                    height: block.height,
                    block_bytes: bincode::serialize(&block).expect("fixture block serializes"),
                })
                .await
                .expect("synced block event must enqueue");
        }
        drop(event_tx);
        event_loop.await.expect("event loop should exit cleanly");

        let expected_state_root = chain.read().await.state.state_root.0;
        let (local_updates, status_broadcasts, last_update) = command_collector
            .await
            .expect("command collector must not panic");

        assert_eq!(
            local_updates, IMPORTED_BLOCKS,
            "every accepted IBD block must advance the P2P sync manager"
        );
        assert_eq!(
            status_broadcasts, 0,
            "65 IBD imports must not send even one rate-limited status"
        );
        assert_eq!(
            last_update,
            Some((expected_height, expected_tip, expected_state_root)),
            "the final local update must carry the imported tip and state root"
        );
    }

    /// #1614 — the manual "desktop peer completes IBD past h=0" checklist as an
    /// automated join-path test. Two REAL swarms in one process: a seed
    /// (`P2pNode` advertising a producer-built chain, served by the production
    /// `run_p2p_event_loop` responder) and an empty joiner (`P2pNode` + the same
    /// production loop as its authoritative applier). Nothing between them is
    /// faked: the joiner learns the seed's height from `ChainStatus`, plans block
    /// requests, the seed serves from its window, and the joiner applies every
    /// block through consensus validation. Pass = the joiner's height leaves 0,
    /// reaches the seed tip, both agree on tip hash and state root, and the
    /// downloader never reports `stalled` (#1482) or `validation_halted` (#2807).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn desktop_peer_completes_ibd_past_genesis_against_a_seed_1614() {
        use std::time::{Duration, Instant};
        use wrfcoin_p2p::{InitialChainState, P2pCommand, P2pConfig, P2pNode};
        use wrfcoin_shared_protocol::crypto::chain_binding::{
            ActivationTable, SigningProtocolVersion, install_activation_table,
        };

        // A real swarm refuses to start with no signing version / schedule
        // installed (#2275, #1396). This test binary runs V1 everywhere
        // (`committed_fixture_blocks` seals under V1), so declare exactly that.
        // First-install-wins; the result is asserted rather than assumed.
        assert_eq!(
            SigningProtocolVersion::install_active(SigningProtocolVersion::V1),
            SigningProtocolVersion::V1,
            "another install won with a different version — broken test binary"
        );
        assert_eq!(
            // secret-leak-ok: test; `SigningProtocolVersion` is an enum type name
            install_activation_table(ActivationTable::default()).version_at(0),
            SigningProtocolVersion::V1
        );

        const SEED_HEIGHT: usize = 30;
        // Shared non-placeholder identity so the genesis handshake retains the
        // connection. Derived at runtime rather than written as a hex literal so
        // the secret scanners have nothing high-entropy to flag in source.
        let test_genesis_hash = hex::encode(
            wrfcoin_shared_protocol::crypto::hash::hash_string("wrfcoin-join-path-test-genesis-1614")
                .as_bytes(),
        );
        let (seed_chain, blocks) =
            committed_fixture_blocks(Blockchain::default(), SEED_HEIGHT, 0x51).await;
        let seed_tip = seed_chain.tip;
        assert_eq!(seed_chain.height, SEED_HEIGHT as u64); // secret-leak-ok: test; seed = seed node; compares block heights
        assert_eq!(blocks.len(), SEED_HEIGHT); // secret-leak-ok: test; `SEED_HEIGHT` is a block-height constant

        let seed_port = {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind an ephemeral port");
            listener.local_addr().expect("local addr").port()
        };
        let seed_addr = format!("/ip4/127.0.0.1/tcp/{seed_port}"); // secret-leak-ok: test; `seed_port` is the seed node's TCP port
        let seed_cfg = P2pConfig {
            listen_addresses: vec![seed_addr.parse().expect("seed listen addr")],
            genesis_hash: test_genesis_hash.clone(),
            ..P2pConfig::default()
        };
        let seed_handle = P2pNode::start_with_initial_chain_state(
            seed_cfg,
            InitialChainState {
                height: seed_chain.height,
                tip_hash: seed_tip.into(),
                state_root: seed_chain.state.state_root.0,
            },
        )
        .await
        .expect("seed swarm starts");
        let (seed_client, seed_events) = seed_handle.into_parts();
        let seed_chain = Arc::new(RwLock::new(seed_chain));
        let seed_loop = tokio::spawn(run_test_p2p_event_transport(
            seed_events,
            Arc::clone(&seed_chain),
            seed_client.clone(),
        ));

        let joiner_cfg = P2pConfig {
            listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().expect("joiner listen addr")],
            bootstrap_peers: vec![seed_addr.parse().expect("bootstrap addr")],
            genesis_hash: test_genesis_hash.clone(),
            ..P2pConfig::default()
        };
        let joiner_handle =
            P2pNode::start_with_initial_chain_state(joiner_cfg, InitialChainState::default())
                .await
                .expect("joiner swarm starts");
        let (joiner_client, joiner_events) = joiner_handle.into_parts();
        let joiner_chain = Arc::new(RwLock::new(Blockchain::default()));
        let joiner_loop = tokio::spawn(run_test_p2p_event_transport(
            joiner_events,
            Arc::clone(&joiner_chain),
            joiner_client.clone(),
        ));

        // Step 2 of the checklist: watch the height leave 0 and reach the tip.
        let deadline = Instant::now() + Duration::from_secs(120);
        let mut left_genesis = false;
        loop {
            let height = joiner_chain.read().await.height;
            left_genesis |= height > 0;
            if height >= SEED_HEIGHT as u64 {
                break;
            }
            let stats = joiner_client.stats();
            assert_ne!(
                stats.sync_state, "stalled",
                "joiner hit the orphan-seed halt (#1482) at height {height}: {stats:?}"
            );
            assert_ne!(
                stats.sync_state, "validation_halted",
                "joiner hit a rule halt (#2807) at height {height}: {stats:?}"
            );
            assert!(
                // secret-leak-ok: test; `SEED_HEIGHT` is a block-height constant
                Instant::now() < deadline,
                "joiner stuck at height {height} of {SEED_HEIGHT}; downloader: {stats:?}"
            );
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        assert!(left_genesis, "height must leave 0 — the #1614 symptom");

        // Step 3: both nodes agree.
        {
            let joiner = joiner_chain.read().await;
            let seed = seed_chain.read().await;
            assert_eq!(joiner.height, seed.height, "heights must match at the tip"); // secret-leak-ok: test; seed = seed node; compares heights
            assert_eq!(
                joiner.tip, seed_tip,
                "both nodes must agree on the tip hash"
            ); // secret-leak-ok: test; `seed_tip` is the seed node's public tip hash
            assert_eq!(
                // secret-leak-ok: test; seed = seed node; public chain data
                joiner.state.state_root, seed.state.state_root,
                "a joiner that replayed every block must reach the seed's state root"
            );
        }
        // The downloader learns of each applied block through an async
        // `UpdateLocalChainState` command and `stats()` is a cached snapshot, so
        // the observation may trail the authoritative chain by a tick under
        // load (it did, once, in the full parallel suite): poll, bounded.
        let observe_deadline = Instant::now() + Duration::from_secs(30);
        let final_stats = loop {
            let stats = joiner_client.stats();
            if stats.sync_local_height >= SEED_HEIGHT as u64 {
                break stats;
            }
            assert!(
                Instant::now() < observe_deadline,
                "the downloader must observe every applied block: {stats:?}"
            );
            tokio::time::sleep(Duration::from_millis(250)).await;
        };
        assert!(
            !matches!(final_stats.sync_state.as_str(), "stalled" | "validation_halted"),
            "a completed join must not end in a terminal halt: {final_stats:?}"
        );

        let _ = joiner_client.command_tx.send(P2pCommand::Shutdown).await;
        let _ = seed_client.command_tx.send(P2pCommand::Shutdown).await;
        let _ = tokio::time::timeout(Duration::from_secs(10), async {
            let _ = joiner_loop.await;
        })
        .await;
        let _ = tokio::time::timeout(Duration::from_secs(10), async {
            let _ = seed_loop.await;
        })
        .await;
    }

    #[tokio::test]
    async fn p2p_live_gossip_apply_preserves_immediate_chain_status_2760() {
        use libp2p::PeerId;
        use wrfcoin_p2p::{P2pClient, P2pCommand};

        let (_, mut blocks) = committed_fixture_blocks(Blockchain::default(), 1, 0x41).await;
        let block = blocks.pop().expect("fixture block");
        let chain = Arc::new(RwLock::new(Blockchain::default()));
        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let (event_tx, event_rx) = mpsc::channel(4);
        let (p2p, mut command_rx) = P2pClient::for_testing_with_commands(PeerId::random());
        let event_loop = tokio::spawn(run_p2p_event_loop(
            event_rx,
            Arc::clone(&chain),
            mempool,
            test_storage(),
            test_consensus(),
            Some(p2p),
            DaChallengeRuntime::new(),
            None,
            None,
            #[cfg(feature = "execution")]
            Arc::new(tokio::sync::RwLock::new(rustc_hash::FxHashMap::default())),
            #[cfg(feature = "execution")]
            Arc::new(tokio::sync::RwLock::new(rustc_hash::FxHashMap::default())),
            #[cfg(feature = "execution")]
            Arc::new(tokio::sync::RwLock::new(rustc_hash::FxHashMap::default())),
            Arc::new(AtomicBool::new(false)),
            StateDivergenceRecoveryPolicy::PeerAgreement,
            std::sync::Arc::new(tokio::sync::RwLock::new(
                crate::peer_reputation::PeerReputationTracker::new(),
            )),
            test_commit_signing_key(),
        ));

        event_tx
            .send(P2pEvent::NewBlock(
                bincode::serialize(&block).expect("fixture block serializes"),
            ))
            .await
            .expect("live block event must enqueue");

        assert!(
            matches!(
                tokio::time::timeout(Duration::from_secs(5), command_rx.recv()).await,
                Ok(Some(P2pCommand::UpdateLocalChainState { .. }))
            ),
            "live gossip must still advance the sync manager"
        );
        assert!(
            matches!(
                tokio::time::timeout(Duration::from_secs(5), command_rx.recv()).await,
                Ok(Some(P2pCommand::BroadcastChainStatus { .. }))
            ),
            "live gossip must retain its immediate divergence-detection status"
        );
        assert!(command_rx.try_recv().is_err(), "exactly two commands expected");
        drop(event_tx);
        event_loop.await.expect("event loop should exit cleanly");
    }

    #[tokio::test]
    async fn p2p_sync_completion_flushes_final_status_immediately_2760() {
        use libp2p::PeerId;
        use wrfcoin_p2p::{P2pClient, P2pCommand};

        let mut initial_chain = Blockchain::default();
        extend_chain(&mut initial_chain, 1, 0x2760);
        let expected_height = initial_chain.height;
        let expected_tip: [u8; 32] = initial_chain.tip.into();
        let expected_state_root = initial_chain.state.state_root.0;
        let blockchain = Arc::new(RwLock::new(initial_chain));
        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let (event_tx, event_rx) = mpsc::channel(8);
        let (p2p, mut command_rx) = P2pClient::for_testing_with_commands(PeerId::random());

        let event_loop = tokio::spawn(run_p2p_event_loop(
            event_rx,
            Arc::clone(&blockchain),
            mempool,
            test_storage(),
            test_consensus(),
            Some(p2p),
            DaChallengeRuntime::new(),
            None,
            None,
            #[cfg(feature = "execution")]
            Arc::new(tokio::sync::RwLock::new(rustc_hash::FxHashMap::default())),
            #[cfg(feature = "execution")]
            Arc::new(tokio::sync::RwLock::new(rustc_hash::FxHashMap::default())),
            #[cfg(feature = "execution")]
            Arc::new(tokio::sync::RwLock::new(rustc_hash::FxHashMap::default())),
            Arc::new(AtomicBool::new(false)),
            StateDivergenceRecoveryPolicy::PeerAgreement,
            std::sync::Arc::new(tokio::sync::RwLock::new(
                crate::peer_reputation::PeerReputationTracker::new(),
            )),
            test_commit_signing_key(),
        ));

        event_tx
            .send(P2pEvent::SyncCompleted {
                height: expected_height,
            })
            .await
            .expect("completion event must enqueue");

        let command = tokio::time::timeout(Duration::from_secs(5), command_rx.recv())
            .await
            .expect("completion status must be immediate")
            .expect("command channel must remain open");
        match command {
            P2pCommand::BroadcastChainStatus {
                height,
                tip_hash,
                state_root,
            } => {
                assert_eq!(height, expected_height);
                assert_eq!(tip_hash, expected_tip);
                assert_eq!(state_root, expected_state_root);
            },
            other => panic!("expected final BroadcastChainStatus, got {other:?}"),
        }

        drop(event_tx);
        event_loop.await.expect("event loop should exit cleanly");
    }

    #[tokio::test]
    async fn v2_p2p_apply_rejects_a_resealed_wrong_commitment_2085() {
        let (_, blocks) = committed_fixture_blocks(Blockchain::default(), 1, 0x41).await;
        let tampered = corrupt_state_root_commitment(&blocks[0]);
        let chain = Arc::new(RwLock::new(Blockchain::default()));
        let storage = test_storage();
        let before = chain_mark(&chain).await;

        feed_committed_block_at_v2(tampered, &chain, &storage).await;

        assert_eq!(
            chain_mark(&chain).await,
            before,
            "p2p_events apply call site must drop the working copy on a V2 root mismatch; \
             height, tip, and live state root must all remain untouched (#2085/#2056)"
        );
        assert!(
            storage
                .read()
                .await
                .load_blocks_range(1, 1)
                .expect("storage query")
                .is_empty(),
            "a rejected P2P block must not be persisted"
        );
    }

    #[tokio::test]
    #[serial_test::serial(upgrade_deadline)]
    async fn v2_reorg_accepts_the_same_fork_when_both_checked_sites_pass_2085() {
        let (chain, storage, fork) = committed_reorg_fixture().await;
        let before = chain_mark(&chain).await;

        feed_committed_fork_at_v2(
            fork,
            &chain,
            &storage,
            std::convert::identity,
        )
        .await;

        let after = chain_mark(&chain).await;
        assert_eq!(
            after.height, 3,
            "positive control: the clean longer fork must pass both in-memory and durable \
             checked-finalization sites (before={before:?}, after={after:?})"
        );
        assert_eq!(
            storage
                .read()
                .await
                .load_blocks_range(3, 3)
                .expect("storage query")
                .len(),
            1,
            "positive control: the accepted fork tip must be durable"
        );
    }

    /// A signed non-member candidate must be rejected by the real reorg
    /// validation loop before `execute_reorg` can replace the local chain.
    /// The first candidate block is signed by the seeded member and both
    /// candidate state-root commitments are re-sealed, so the only semantic
    /// difference in the final candidate is its producer identity.
    #[tokio::test]
    #[serial_test::serial(upgrade_deadline)]
    async fn v2_reorg_rejects_unauthorized_producer_before_execute_reorg_2603() {
        use rand::SeedableRng;
        use wrfcoin_native_chain::crypto::signature::{PrivateKey, PublicKey};
        use wrfcoin_shared_protocol::consensus::block_producer_auth::sign_block_producer;
        use wrfcoin_shared_protocol::consensus::producer_set::{
            ProducerMember, ProducerSetChange,
        };
        use wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity;

        let (chain, storage, mut fork) = committed_reorg_fixture().await;

        let member_key = PrivateKey::generate(&mut rand::rngs::StdRng::seed_from_u64(2603_7701));
        let member_public_key = PublicKey::from_private_key(&member_key);
        let member_bytes: [u8; 32] = member_public_key.as_bytes().try_into().unwrap();
        let member = ProducerMember {
            producer_id: "reorg-member".to_string(),
            payout_address: test_address_string(0x42),
            public_key: member_bytes,
        };

        let outsider_key =
            PrivateKey::generate(&mut rand::rngs::StdRng::seed_from_u64(2603_7702));

        // The reorg validator reconstructs its fork-point state from this
        // durable checkpoint. Seed the same authority snapshot it will read,
        // rather than mutating only the live chain state.
        let mut fork_state = {
            let storage = storage.read().await;
            storage
                .load_checkpoint_at_or_before(1)
                .expect("fork-point checkpoint must load")
                .expect("fork-point checkpoint must exist")
                .1
        };
        fork_state
            .producer_set_history
            .push(ProducerSetChange::new(0, 0, vec![member]));
        fork_state.recalculate_state_root();
        storage
            .read()
            .await
            .force_checkpoint(&fork_state, 1)
            .await
            .expect("seeded fork-point checkpoint must persist");

        // Re-seal the candidate state roots after adding the producer set to
        // the fork-point snapshot. Producer authentication is then the only
        // reason the final candidate should fail.
        let mut expected_state = fork_state;
        for index in 0..fork.len() {
            if index > 0 {
                fork[index].prev_block_hash = fork[index - 1]
                    .hash
                    .expect("preceding candidate must have a hash");
            }

            let transitions = extract_transitions(
                &expected_state,
                &fork[index],
                fork[index].height,
                fork[index].timestamp.timestamp(),
            );
            expected_state
                .apply_transitions_batch_at_height(fork[index].height, transitions)
                .expect("candidate transitions must apply to the fork-point state");
            expected_state.apply_block_finalization(fork[index].height);
            fork[index].metadata.insert(
                STATE_ROOT_METADATA_KEY.to_string(),
                expected_state.state_root.to_string(),
            );

            let signer = if index + 1 == fork.len() {
                &outsider_key
            } else {
                &member_key
            };
            let public_key = PublicKey::from_private_key(signer);
            fork[index].producer_pubkey = public_key.as_bytes().try_into().unwrap();
            let hash = fork[index].calculate_hash();
            fork[index].set_hash(hash);
            let attestation = sign_block_producer(
                &fork[index],
                signer,
                &ChainIdentity::active(),
                SigningProtocolVersion::V2,
            );
            fork[index].producer_sig = attestation.producer_sig.to_vec();
        }

        let before = chain_mark(&chain).await;
        feed_committed_fork_at_v2(
            fork,
            &chain,
            &storage,
            std::convert::identity,
        )
        .await;

        assert_eq!(
            chain_mark(&chain).await,
            before,
            "an unauthorized reorg candidate must be rejected before execute_reorg"
        );
        let durable = storage
            .read()
            .await
            .load_blocks_range(2, 3)
            .expect("storage query");
        assert_eq!(durable.len(), 1, "the rejected fork tip must not be persisted");
        assert_eq!(
            durable[0].hash,
            Some(before.tip),
            "durable storage must remain on the old local tip"
        );
    }

    #[tokio::test]
    #[serial_test::serial(upgrade_deadline)]
    async fn v2_reorg_in_memory_site_restores_the_old_chain_on_wrong_commitment_2085() {
        let (chain, storage, mut fork) = committed_reorg_fixture().await;
        let last = fork.len() - 1;
        fork[last] = corrupt_state_root_commitment(&fork[last]);
        let before = chain_mark(&chain).await;

        feed_committed_fork_at_v2(
            fork,
            &chain,
            &storage,
            |_| {
                panic!(
                    "an in-memory commitment mismatch must reject before durable preparation starts"
                )
            },
        )
        .await;

        assert_eq!(
            chain_mark(&chain).await,
            before,
            "the in-memory reorg call site must restore the complete pre-reorg snapshot on a \
             resealed but wrong V2 commitment"
        );
        assert!(
            storage
                .read()
                .await
                .load_blocks_range(3, 3)
                .expect("storage query")
                .is_empty(),
            "a fork rejected in memory must never reach durable persistence"
        );
    }

    #[tokio::test]
    #[serial_test::serial(upgrade_deadline)]
    async fn v2_reorg_persist_site_rejects_before_storage_rollback_2085() {
        let (chain, storage, fork) = committed_reorg_fixture().await;
        let before = chain_mark(&chain).await;

        feed_committed_fork_at_v2(
            fork,
            &chain,
            &storage,
            |mut checkpoint_rebuilt_state| {
                // Fault injection at the exact #1810 seam: in-memory replay has
                // already passed, but the independently checkpoint-rebuilt
                // persistence state contains one extra consensus account.
                checkpoint_rebuilt_state
                    .create_account(Address([0xdd; 20]), Balance::new(0));
                checkpoint_rebuilt_state
            },
        )
        .await;

        assert_eq!(
            chain_mark(&chain).await,
            before,
            "a persistence-side V2 mismatch must restore the pre-reorg in-memory chain"
        );
        let durable = storage
            .read()
            .await
            .load_blocks_range(2, 3)
            .expect("storage query");
        assert_eq!(
            durable.len(),
            1,
            "the old height-2 block must remain and no height-3 fork tip may be written"
        );
        assert_eq!(
            durable[0].hash,
            Some(before.tip),
            "durable storage must remain on the exact old tip; logging and returning after \
             rollback would leave it truncated at the fork point (#2056 class)"
        );
    }

    // ---------------------------------------------------------------------
    // #3370 — a genesis reorg (fork_height 0) must not swap block 1 under a
    // running process whose block-1 inputs are already armed.
    // ---------------------------------------------------------------------

    /// Local node: a self-mined block 1 (NOT the peers' block 1), no BFT
    /// finality. Peers: a longer chain from genesis with a different block 1.
    /// Returns the local chain, its storage, and the peers' fork (heights 1..=2).
    async fn genesis_fork_fixture_3370(
    ) -> (Arc<RwLock<Blockchain>>, Arc<RwLock<Storage>>, Vec<Block>) {
        let (_, local_blocks) = committed_fixture_blocks(Blockchain::default(), 1, 0x41).await;
        let (_, fork_blocks) = committed_fixture_blocks(Blockchain::default(), 2, 0x42).await;
        assert_ne!(
            local_blocks[0].hash, fork_blocks[0].hash,
            "fixture control: the two block 1s must differ"
        );

        let chain = Arc::new(RwLock::new(Blockchain::default()));
        let storage = test_storage();
        // Every fresh node writes the height-0 genesis anchor before its first
        // block (`main.rs`, #2501). It is the checkpoint a genesis reorg rebuilds
        // its fork-point state from; without it the reorg would abort for an
        // unrelated reason and this test could not see the refusal.
        let genesis_state = chain.read().await.state.clone();
        storage
            .read()
            .await
            .persist_genesis_state(&genesis_state)
            .await
            .expect("fixture must write the production genesis anchor");
        feed_committed_block_at_v2(local_blocks[0].clone(), &chain, &storage).await;
        {
            let c = chain.read().await;
            assert_eq!(c.height, 1, "fixture control: local block 1 must be accepted");
            assert_eq!(c.finalized_height(), 0, "fixture control: no BFT finality yet");
        }
        (chain, storage, fork_blocks)
    }

    /// Process-wide, first-install-wins inputs block 1 arms. A refused genesis
    /// reorg must leave every one of them exactly as it found them.
    fn armed_inputs_3370() -> (bool, String, Option<wrfcoin_native_chain::upgrade::UpgradeDeadline>) {
        (
            wrfcoin_shared_protocol::crypto::chain_binding::base_feature_schedule_is_provisional(),
            format!(
                "{:?}",
                wrfcoin_shared_protocol::crypto::chain_binding::effective_feature_schedule()
            ),
            wrfcoin_native_chain::upgrade::upgrade_deadline(),
        )
    }

    #[tokio::test]
    #[serial_test::serial(upgrade_deadline)]
    async fn genesis_reorg_replacing_an_armed_block_one_is_refused_3370() {
        let (chain, storage, fork) = genesis_fork_fixture_3370().await;
        let before = chain_mark(&chain).await;
        let local_block1 = chain.read().await.block_at_height(1).cloned();
        let inputs_before = armed_inputs_3370();

        feed_committed_fork_at_v2(fork.clone(), &chain, &storage, std::convert::identity).await;

        assert_eq!(
            chain_mark(&chain).await,
            before,
            "a fork_height-0 reorg must be refused: swapping block 1 under a running process \
             skips the block-1 hook (no bootstrap, no authority) (#3370)"
        );
        assert_eq!(
            chain.read().await.block_at_height(1).map(|b| b.hash),
            local_block1.map(|b| b.hash),
            "the local block 1 must stay canonical in memory"
        );
        let durable = storage.read().await.load_blocks_range(1, 2).expect("storage query");
        assert_eq!(durable.len(), 1, "no fork block may be written: {durable:?}");
        assert_eq!(durable[0].hash, Some(before.tip), "durable block 1 must be the local one");
        assert_ne!(durable[0].hash, fork[0].hash);
        assert_eq!(armed_inputs_3370(), inputs_before, "armed block-1 inputs must be unchanged");
    }

    /// This guard inventories `p2p_events.rs` only. The finalization call class
    /// also has separately audited callers in `proof_of_publishing.rs` and
    /// `storage.rs`; adding a new applier there requires its own classification.
    #[test]
    fn every_production_state_root_finalization_call_is_classified_2085() {
        let source = include_str!("p2p_events.rs");
        let production = source
            .split_once("#[cfg(test)]\nmod tests")
            .expect("p2p_events.rs must retain a distinct test module")
            .0;
        let lines = production.lines().collect::<Vec<_>>();
        let calls = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| {
                let code = line.trim_start();
                !code.starts_with("//")
                    && code.contains(".apply_block_finalization")
            })
            .collect::<Vec<_>>();

        assert_eq!(
            calls.len(),
            6,
            "production finalization call-site count changed; classify every new site as a \
             checked rejection or a specifically reasoned reconstruction-only exception:\n{}",
            calls
                .iter()
                .map(|(index, line)| format!("  {}: {}", index + 1, line.trim()))
                .collect::<Vec<_>>()
                .join("\n")
        );

        for (index, line) in &calls {
            let context_start = index.saturating_sub(2);
            let context = lines[context_start..=*index].join("\n");
            if line.contains(".apply_block_finalization_checked") {
                assert!(
                    context.contains("state-root-checked-reject:"),
                    "checked finalization at line {} lacks a named rejection contract",
                    index + 1
                );
            } else {
                assert!(
                    line.contains("state-root-unchecked-ok:"),
                    "unchecked finalization at line {} is not an allow-listed reconstruction \
                     site: {}",
                    index + 1,
                    line.trim()
                );
            }
        }

        let checked_rejections: &[(&str, &[&str])] = &[
            // The rejection path returns a local_failure OUTCOME (it stopped
            // being a bare `return;` when apply grew a typed outcome) — the
            // contract is still "checked failure aborts the apply".
            ("p2p-working-copy-return", &["return P2pBlockApplyOutcome::local_failure("]),
            (
                "reorg-restore-break",
                &["*chain = pre_reorg_snapshot.clone();", "break;"],
            ),
            (
                "persist-restore-return",
                &["*chain = pre_reorg_snapshot;", "return;"],
            ),
        ];
        for (marker, required_tokens) in checked_rejections {
            let matching = lines
                .iter()
                .enumerate()
                .filter(|(_, line)| line.contains(marker))
                .collect::<Vec<_>>();
            assert_eq!(
                matching.len(),
                1,
                "checked rejection marker {marker:?} must occur exactly once"
            );
            let marker_line = matching[0].0;
            let snippet = lines[marker_line..(marker_line + 25).min(lines.len())].join("\n");
            for token in *required_tokens {
                assert!(
                    // secret-leak-ok: test; `token` is a source-text control-flow token
                    snippet.contains(token),
                    "checked rejection {marker:?} no longer contains required control-flow token \
                     {token:?}"
                );
            }
        }

        for marker in [
            "fork-point-wal-group",
            "fork-point-wal-tail",
            "disposable-validation-view",
        ] {
            assert_eq!(
                production.matches(marker).count(),
                1,
                "unchecked reconstruction marker {marker:?} must occur exactly once"
            );
        }
    }

    fn seeded_keypair(seed: u64) -> (wrfcoin_native_chain::crypto::signature::PrivateKey, String) {
        use rand::SeedableRng;
        use wrfcoin_native_chain::crypto::signature::{PrivateKey, PublicKey};

        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let private_key = PrivateKey::generate(&mut rng);
        let public_key = PublicKey::from_private_key(&private_key).to_string();
        (private_key, public_key)
    }

    fn oracle_attestation_tx(
        oracle_id: &str,
        signer: &wrfcoin_native_chain::crypto::signature::PrivateKey,
        tag: &str,
        timestamp: i64,
    ) -> Transaction {
        use wrfcoin_native_chain::blockchain::transaction::{
            OracleAttestationPayload, TransactionInput, TransactionOutput,
        };
        use wrfcoin_native_chain::crypto::hash::hash_string;

        let mut payload = OracleAttestationPayload {
            commit_id: "a".repeat(64),
            oracle_id: oracle_id.to_string(),
            approved: true,
            signature: None,
            confidence: Some(0.95),
            timestamp,
            nonce: Some(format!("p2p-{tag}")),
        };
        payload.signature = Some(signer.sign(payload.signing_message().as_bytes()).to_string());

        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_string(&format!("input-{tag}")),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 1,
                address: format!("wrf-{tag}"),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.tx_type = TransactionType::OracleAttestation;
        tx.data = Some(serde_json::to_string(&payload).unwrap());
        tx.set_id();
        tx
    }

    fn test_bft_gadget() -> Arc<RwLock<BftFinalityGadget>> {
        Arc::new(RwLock::new(BftFinalityGadget::new(vec![
            FinalityValidator {
                id: "v0".to_string(),
                stake_weight: 100,
                public_key: None,
            },
            FinalityValidator {
                id: "v1".to_string(),
                stake_weight: 100,
                public_key: None,
            },
            FinalityValidator {
                id: "v2".to_string(),
                stake_weight: 100,
                public_key: None,
            },
        ])))
    }

    fn unsigned_finality_certificate(block_height: u64, block_hash: [u8; 32]) -> FinalityCertificate {
        FinalityCertificate {
            block_hash,
            block_height,
            votes: Vec::new(),
            total_vote_weight: 0,
            total_validator_weight: 300,
        }
    }

    #[test]
    fn orphan_block_buffer_deduplicates_hashes_and_evicts_oldest_height() {
        let mut buffer = OrphanBlockBuffer {
            blocks: Default::default(),
            max_entries: 2,
        };

        let block_5 = test_block(5, CryptoHash::hash_bytes(b"parent-5"), 5);
        let block_6 = test_block(6, CryptoHash::hash_bytes(b"parent-6"), 6);
        let block_7 = test_block(7, CryptoHash::hash_bytes(b"parent-7"), 7);

        buffer.insert(block_5.clone());
        buffer.insert(block_5);
        buffer.insert(block_6.clone());
        buffer.insert(block_7.clone());

        assert_eq!(buffer.total_count(), 2, "duplicate hashes should not count twice");
        assert!(
            buffer.take_height(5).is_empty(),
            "oldest buffered height should be evicted once capacity is exceeded"
        );
        let height_6 = buffer.take_height(6);
        let height_7 = buffer.take_height(7);
        assert_eq!(height_6.len(), 1);
        assert_eq!(height_7.len(), 1);
        assert_eq!(height_6[0].hash, block_6.hash);
        assert_eq!(height_7[0].hash, block_7.hash);
    }

    #[tokio::test]
    async fn run_p2p_event_loop_accepts_registered_operator_key_attestation() {
        use wrfcoin_native_chain::blockchain::state::StateTransition;
        use wrfcoin_native_chain::security::key_registry::SigningRole;

        let (_oracle_identity_sk, oracle_id) = seeded_keypair(731);
        let (operator_sk, operator_public_key) = seeded_keypair(732);
        let activated_at = 1_700_000_000;
        let mut chain = Blockchain::default();
        chain
            .apply_state_transition(StateTransition::SetOracleConfig {
                oracle_set: vec![oracle_id.clone()],
                oracle_quorum: 1,
            })
            .expect("oracle config should initialize");
        chain
            .apply_state_transition(StateTransition::RegisterSigningKeyEpoch {
                identity: oracle_id.clone(),
                role: SigningRole::Oracle,
                epoch: 1,
                public_key: operator_public_key,
                activated_at,
            })
            .expect("operator signing key should register");

        let blockchain = Arc::new(RwLock::new(chain));
        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let storage = test_storage();
        let consensus = test_consensus();
        let (event_tx, event_rx) = mpsc::channel(8);
        #[cfg(feature = "execution")]
        let (forecast_task_assignments, forecast_node_heartbeats, forecast_task_results) =
            test_forecast_state();
        let event_loop = tokio::spawn(run_p2p_event_loop(
            event_rx,
            Arc::clone(&blockchain),
            Arc::clone(&mempool),
            storage,
            consensus,
            None,
            DaChallengeRuntime::new(),
            None,
            None,
            #[cfg(feature = "execution")]
            forecast_task_assignments,
            #[cfg(feature = "execution")]
            forecast_node_heartbeats,
            #[cfg(feature = "execution")]
            forecast_task_results,
            Arc::new(AtomicBool::new(false)),
            StateDivergenceRecoveryPolicy::PeerAgreement,
            std::sync::Arc::new(tokio::sync::RwLock::new(
                crate::peer_reputation::PeerReputationTracker::new(),
            )),
            test_commit_signing_key(),
        ));

        let tx = oracle_attestation_tx(&oracle_id, &operator_sk, "operator-key", activated_at + 1);
        event_tx
            .send(P2pEvent::NewTransactions(vec![
                bincode::serialize(&tx).expect("transaction should serialize"),
            ]))
            .await
            .expect("event receiver should accept transaction gossip");
        drop(event_tx);
        event_loop.await.expect("event loop should exit after channel close");

        assert_eq!(
            mempool.read().await.size(),
            1,
            "P2P admission must accept registered operator-key oracle attestations"
        );
    }

    #[tokio::test]
    async fn run_p2p_event_loop_graylists_and_disconnects_a_banned_peer() {
        // #1406 follow-up end-to-end: a `PeerPenalized{banned}` event flowing
        // through the real event loop must (1) graylist the peer in the shared
        // tracker the operator route reads, (2) issue exactly one
        // `DisconnectPeer` command (the kademlia-removal wiring), and
        // (3) increment `CORE_METRICS.peers_graylisted_total` (#1789).
        use libp2p::PeerId;
        use wrfcoin_p2p::{P2pClient, P2pCommand};

        // Serialize against other tests that read CORE_METRICS.peers_graylisted_total.
        let _guard = PEER_GRAYLIST_COUNTER_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let graylisted_before = CORE_METRICS.peers_graylisted_total.get();

        let blockchain = Arc::new(RwLock::new(Blockchain::default()));
        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let storage = test_storage();
        let consensus = test_consensus();
        let (event_tx, event_rx) = mpsc::channel(8);

        let (p2p_client, mut command_rx) =
            P2pClient::for_testing_with_commands(PeerId::random());
        let reputation = std::sync::Arc::new(tokio::sync::RwLock::new(
            crate::peer_reputation::PeerReputationTracker::new(),
        ));

        let event_loop = tokio::spawn(run_p2p_event_loop(
            event_rx,
            Arc::clone(&blockchain),
            Arc::clone(&mempool),
            storage,
            consensus,
            Some(p2p_client),
            DaChallengeRuntime::new(),
            None,
            None,
            // The three forecast maps only exist when the `execution` feature
            // is on; these tests predate them (#2266 round — test code did not
            // compile under execution+p2p).
            #[cfg(feature = "execution")]
            Arc::new(tokio::sync::RwLock::new(rustc_hash::FxHashMap::default())),
            #[cfg(feature = "execution")]
            Arc::new(tokio::sync::RwLock::new(rustc_hash::FxHashMap::default())),
            #[cfg(feature = "execution")]
            Arc::new(tokio::sync::RwLock::new(rustc_hash::FxHashMap::default())),
            Arc::new(AtomicBool::new(false)),
            StateDivergenceRecoveryPolicy::PeerAgreement,
            reputation.clone(),
            test_commit_signing_key(),
        ));

        let victim = PeerId::random();
        event_tx
            .send(P2pEvent::PeerPenalized {
                peer_id: victim,
                reason: "invalid_message_flood".to_string(),
                banned: true,
            })
            .await
            .expect("event receiver should accept the penalty event");

        let command = tokio::time::timeout(std::time::Duration::from_secs(5), command_rx.recv())
            .await
            .expect("a DisconnectPeer command must be issued within the timeout")
            .expect("command channel must stay open");
        match command {
            P2pCommand::DisconnectPeer { peer_id, reason } => {
                assert_eq!(peer_id, victim, "must disconnect the penalized peer");
                assert!(
                    reason.contains("graylisted"),
                    "disconnect reason should mark the graylist, got: {reason}"
                );
            },
            other => panic!("expected DisconnectPeer, got {other:?}"),
        }

        drop(event_tx);
        event_loop.await.expect("event loop should exit after channel close");

        assert!(
            reputation.read().await.is_graylisted(&victim),
            "the shared tracker the operator route reads must record the peer graylisted"
        );
        assert_eq!(
            CORE_METRICS.peers_graylisted_total.get(),
            graylisted_before + 1,
            "CORE_METRICS.peers_graylisted_total must increment by 1 on graylist transition (#1789)"
        );
    }

    #[tokio::test]
    async fn run_p2p_event_loop_weather_data_bridges_to_contribution_txs() {
        let blockchain = Arc::new(RwLock::new(Blockchain::default()));
        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let storage = test_storage();
        let consensus = test_consensus();
        let (event_tx, event_rx) = mpsc::channel(8);
        let reputation = std::sync::Arc::new(tokio::sync::RwLock::new(
            crate::peer_reputation::PeerReputationTracker::new(),
        ));
        let event_loop = tokio::spawn(run_p2p_event_loop(
            event_rx,
            Arc::clone(&blockchain),
            mempool.clone(),
            storage,
            consensus,
            None,
            DaChallengeRuntime::new(),
            None,
            None,
            #[cfg(feature = "execution")]
            Arc::new(tokio::sync::RwLock::new(rustc_hash::FxHashMap::default())),
            #[cfg(feature = "execution")]
            Arc::new(tokio::sync::RwLock::new(rustc_hash::FxHashMap::default())),
            #[cfg(feature = "execution")]
            Arc::new(tokio::sync::RwLock::new(rustc_hash::FxHashMap::default())),
            Arc::new(AtomicBool::new(false)),
            StateDivergenceRecoveryPolicy::PeerAgreement,
            reputation,
            test_commit_signing_key(),
        ));

        let weather_payload = serde_json::json!({
            "schema_version": "wrf.weather.observation.v1",
            "station_id": "station-001",
            "observed_at_unix": 1_700_000_000_i64,
            "observation": {
                "temperature_c": 19.5,
            },
        });

        event_tx
            .send(P2pEvent::WeatherData {
                station_id: "station-001".to_string(),
                data: serde_json::to_vec(&weather_payload).expect("weather payload should serialize"),
                station_pubkey: "pubkey".to_string(),
            })
            .await
            .expect("event receiver should accept weather gossip");
        drop(event_tx);
        event_loop
            .await
            .expect("event loop should exit cleanly after channel close");

        let commits = mempool.read().await.get_transactions_by_type(
            wrfcoin_native_chain::blockchain::transaction::TransactionType::ContributionCommit,
            10,
        );
        let reveals = mempool.read().await.get_transactions_by_type(
            wrfcoin_native_chain::blockchain::transaction::TransactionType::ContributionReveal,
            10,
        );
        assert_eq!(commits.len(), 1, "WeatherData should create one contribution commit");
        assert_eq!(reveals.len(), 1, "WeatherData should create one contribution reveal");

        let commit_payload: serde_json::Value = serde_json::from_str(
            commits[0]
                .data
                .as_deref()
                .expect("commit transaction should include a payload"),
        )
        .expect("commit payload should be valid JSON");
        let reveal_payload: serde_json::Value = serde_json::from_str(
            reveals[0]
                .data
                .as_deref()
                .expect("reveal transaction should include a payload"),
        )
        .expect("reveal payload should be valid JSON");
        assert_eq!(
            commit_payload["station_id"],
            serde_json::Value::String("station-001".to_string())
        );
        assert_eq!(
            commit_payload["contribution_type"],
            serde_json::Value::String("contribution".to_string())
        );
        assert_eq!(commit_payload["commit_id"], reveal_payload["commit_id"]);

        // #2389 RED-on-revert: the bridged commit must carry a valid #1848
        // sender authorization. If `bridge_weather_payload_to_mempool` stops
        // signing the payload, the commit is rejected at admission and never
        // reaches the mempool, so `commits` is empty and the length assertions
        // above already fail — but assert the authorization explicitly so the
        // intent (a *signed, admissible* commit) is unambiguous.
        assert!(
            commit_payload["auth_pubkey"].is_string(),
            "bridged commit must be signed (auth_pubkey present): {commit_payload}"
        );
        assert!(
            commit_payload["auth_signature"].is_string(),
            "bridged commit must be signed (auth_signature present): {commit_payload}"
        );
        assert!(
            commits[0].verify_commit_authorization(),
            "bridged commit authorization must verify so it passes #1848 fail-closed \
             mempool admission (RED if the bridge's sign_authorization call is reverted)"
        );
    }

    #[tokio::test]
    async fn run_p2p_event_loop_weather_data_v2_bridges_to_contribution_txs() {
        let blockchain = Arc::new(RwLock::new(Blockchain::default()));
        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let storage = test_storage();
        let consensus = test_consensus();
        let (event_tx, event_rx) = mpsc::channel(8);
        let reputation = std::sync::Arc::new(tokio::sync::RwLock::new(
            crate::peer_reputation::PeerReputationTracker::new(),
        ));
        let event_loop = tokio::spawn(run_p2p_event_loop(
            event_rx,
            Arc::clone(&blockchain),
            mempool.clone(),
            storage,
            consensus,
            None,
            DaChallengeRuntime::new(),
            None,
            None,
            #[cfg(feature = "execution")]
            Arc::new(tokio::sync::RwLock::new(rustc_hash::FxHashMap::default())),
            #[cfg(feature = "execution")]
            Arc::new(tokio::sync::RwLock::new(rustc_hash::FxHashMap::default())),
            #[cfg(feature = "execution")]
            Arc::new(tokio::sync::RwLock::new(rustc_hash::FxHashMap::default())),
            Arc::new(AtomicBool::new(false)),
            StateDivergenceRecoveryPolicy::PeerAgreement,
            reputation,
            test_commit_signing_key(),
        ));

        let weather_payload = serde_json::json!({
            "schema_version": "wrf.weather.observation.v1",
            "station_id": "station-002",
            "observed_at_unix": 1_700_000_001_i64,
            "observation": {
                "temperature_c": 20.1,
            },
        });

        event_tx
            .send(P2pEvent::WeatherDataV2 {
                station_id: "station-002".to_string(),
                data: serde_json::to_vec(&weather_payload).expect("weather payload should serialize"),
                content_ref: None,
            })
            .await
            .expect("event receiver should accept weather v2 gossip");
        drop(event_tx);
        event_loop
            .await
            .expect("event loop should exit cleanly after channel close");

        let commits = mempool.read().await.get_transactions_by_type(
            wrfcoin_native_chain::blockchain::transaction::TransactionType::ContributionCommit,
            10,
        );
        let reveals = mempool.read().await.get_transactions_by_type(
            wrfcoin_native_chain::blockchain::transaction::TransactionType::ContributionReveal,
            10,
        );
        assert_eq!(commits.len(), 1, "WeatherDataV2 should create one contribution commit");
        assert_eq!(reveals.len(), 1, "WeatherDataV2 should create one contribution reveal");

        let commit_payload: serde_json::Value = serde_json::from_str(
            commits[0]
                .data
                .as_deref()
                .expect("commit transaction should include a payload"),
        )
        .expect("commit payload should be valid JSON");
        let reveal_payload: serde_json::Value = serde_json::from_str(
            reveals[0]
                .data
                .as_deref()
                .expect("reveal transaction should include a payload"),
        )
        .expect("reveal payload should be valid JSON");
        assert_eq!(
            commit_payload["station_id"],
            serde_json::Value::String("station-002".to_string())
        );
        assert_eq!(
            commit_payload["contribution_type"],
            serde_json::Value::String("contribution".to_string())
        );
        assert_eq!(commit_payload["commit_id"], reveal_payload["commit_id"]);

        // #2389 RED-on-revert: the bridged commit must carry a valid #1848
        // sender authorization. If `bridge_weather_payload_to_mempool` stops
        // signing the payload, the commit is rejected at admission and never
        // reaches the mempool, so `commits` is empty and the length assertions
        // above already fail — but assert the authorization explicitly so the
        // intent (a *signed, admissible* commit) is unambiguous.
        assert!(
            commit_payload["auth_pubkey"].is_string(),
            "bridged commit must be signed (auth_pubkey present): {commit_payload}"
        );
        assert!(
            commit_payload["auth_signature"].is_string(),
            "bridged commit must be signed (auth_signature present): {commit_payload}"
        );
        assert!(
            commits[0].verify_commit_authorization(),
            "bridged commit authorization must verify so it passes #1848 fail-closed \
             mempool admission (RED if the bridge's sign_authorization call is reverted)"
        );
    }

    #[tokio::test]
    async fn p2p_finality_certificate_for_unknown_block_does_not_advance_finalized_height() {
        use libp2p::PeerId;

        let blockchain = Arc::new(RwLock::new(Blockchain::default()));
        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let storage = test_storage();
        let consensus = test_consensus();
        let bft_finality = test_bft_gadget();
        let (finality_signal_tx, finality_signal_rx) =
            wrfcoin_shared_protocol::consensus::finality::finality_signal_channel();
        let (event_tx, event_rx) = mpsc::channel(8);
        let reputation = std::sync::Arc::new(tokio::sync::RwLock::new(
            crate::peer_reputation::PeerReputationTracker::new(),
        ));

        let finality_loop = tokio::spawn(crate::finality_events::run_finality_event_loop(
            finality_signal_rx,
            Arc::clone(&blockchain),
            None,
            // Pre-existing p2p test-harness gap (unrelated to mobile#215): a
            // `finality_invariant` param was added to `run_finality_event_loop`
            // but not propagated to this `--features p2p` test helper, so the
            // feature's test build did not compile. A fresh handle suffices here
            // — this harness exercises gamification-payout block parity, not the
            // finality latch.
            wrfcoin_shared_protocol::invariants::finality::SharedFinalityInvariant::new(),
        ));
        let event_loop = tokio::spawn(run_p2p_event_loop(
            event_rx,
            Arc::clone(&blockchain),
            mempool,
            storage,
            consensus,
            None,
            DaChallengeRuntime::new(),
            Some(bft_finality),
            Some(finality_signal_tx),
            #[cfg(feature = "execution")]
            Arc::new(tokio::sync::RwLock::new(rustc_hash::FxHashMap::default())),
            #[cfg(feature = "execution")]
            Arc::new(tokio::sync::RwLock::new(rustc_hash::FxHashMap::default())),
            #[cfg(feature = "execution")]
            Arc::new(tokio::sync::RwLock::new(rustc_hash::FxHashMap::default())),
            Arc::new(AtomicBool::new(false)),
            StateDivergenceRecoveryPolicy::PeerAgreement,
            reputation,
            test_commit_signing_key(),
        ));

        let forged = unsigned_finality_certificate(9, [0x99; 32]);
        event_tx
            .send(P2pEvent::FinalityCertificate {
                peer_id: PeerId::random(),
                cert_bytes: bincode::serialize(&forged).expect("certificate should serialize"),
            })
            .await
            .expect("event receiver should accept certificate gossip");
        drop(event_tx);

        event_loop.await.expect("event loop should shut down cleanly");
        finality_loop
            .await
            .expect("finality loop should shut down after sender drop");

        assert_eq!(
            blockchain.read().await.finalized_height(),
            0,
            "unknown-block P2P FinalityCertificate must not advance finalized_height"
        );
    }

    #[tokio::test]
    async fn p2p_finality_certificate_requires_strictly_advancing_height() {
        let mut chain = Blockchain::default();
        extend_chain(&mut chain, 1, 600);
        let block_hash: [u8; 32] = chain
            .block_at_height(1)
            .and_then(|block| block.hash)
            .expect("fixture block should exist")
            .into();
        chain.set_finalized_height(1);
        let blockchain = Arc::new(RwLock::new(chain));
        let cert = unsigned_finality_certificate(1, block_hash);

        let err = validate_p2p_finality_certificate_against_chain(&blockchain, &cert)
            .await
            .expect_err("stale certificate height must be rejected");
        assert!(
            err.contains("non-advancing height"),
            "expected height-monotonicity rejection, got {err}"
        );
    }

    /// #2884: a skew-shaped block payload — the pre-producer-auth layout the
    /// #2727-class audit demonstrated (9 fields, no `producer_pubkey` /
    /// `producer_sig`; the shape a `5007ce747`-era binary actually gossips) —
    /// must be COUNTED under
    /// `wrfcoin_p2p_payload_decode_failed_total{kind="block"}`, so version
    /// skew is diagnosable from /metrics instead of only from an anonymous
    /// warn line in a busy log. Behaviour stays drop-and-continue: the chain
    /// must not move.
    #[tokio::test]
    async fn skew_shaped_block_payload_is_counted_not_silent() {
        /// Byte-shape mirror of `Block` as of 5007ce747 (bincode encodes by
        /// field shape and order, not names).
        #[derive(serde::Serialize)]
        struct PreProducerAuthBlock {
            version: u32,
            prev_block_hash: wrfcoin_shared_protocol::crypto::hash::Hash,
            transactions: Vec<wrfcoin_shared_protocol::blockchain::transaction::Transaction>,
            difficulty: u32,
            height: u64,
            nonce: u64,
            timestamp: chrono::DateTime<chrono::Utc>,
            metadata: rustc_hash::FxHashMap<String, String>,
            hash: Option<wrfcoin_shared_protocol::crypto::hash::Hash>,
        }
        let july_block = PreProducerAuthBlock {
            version: 3,
            prev_block_hash: wrfcoin_shared_protocol::crypto::hash::Hash::default(),
            transactions: Vec::new(),
            difficulty: 1,
            height: 47476,
            nonce: 7,
            timestamp: chrono::Utc::now(),
            metadata: rustc_hash::FxHashMap::default(),
            hash: Some(wrfcoin_shared_protocol::crypto::hash::Hash::default()),
        };
        let skew_bytes = bincode::serialize(&july_block).expect("mirror block serializes");
        // Control: the fixture really is the skew shape — it must NOT decode
        // as the current Block (otherwise this test would count nothing).
        assert!(
            bincode::deserialize::<Block>(&skew_bytes).is_err(),
            "fixture must be undecodable under the current Block layout"
        );

        let blockchain = Arc::new(RwLock::new(Blockchain::default()));
        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let (event_tx, event_rx) = mpsc::channel(8);

        let before = CORE_METRICS.p2p_payload_decode_failed_count("block");

        let event_loop = tokio::spawn(run_p2p_event_loop(
            event_rx,
            Arc::clone(&blockchain),
            mempool,
            test_storage(),
            test_consensus(),
            None,
            DaChallengeRuntime::new(),
            None,
            None,
            #[cfg(feature = "execution")]
            Arc::new(tokio::sync::RwLock::new(rustc_hash::FxHashMap::default())),
            #[cfg(feature = "execution")]
            Arc::new(tokio::sync::RwLock::new(rustc_hash::FxHashMap::default())),
            #[cfg(feature = "execution")]
            Arc::new(tokio::sync::RwLock::new(rustc_hash::FxHashMap::default())),
            Arc::new(AtomicBool::new(false)),
            StateDivergenceRecoveryPolicy::PeerAgreement,
            std::sync::Arc::new(tokio::sync::RwLock::new(
                crate::peer_reputation::PeerReputationTracker::new(),
            )),
            test_commit_signing_key(),
        ));

        event_tx
            .send(P2pEvent::NewBlock(skew_bytes))
            .await
            .expect("event receiver should accept the skew-shaped block");
        drop(event_tx);
        event_loop
            .await
            .expect("event loop should exit cleanly after channel close");

        let after = CORE_METRICS.p2p_payload_decode_failed_count("block");
        assert!(
            after > before,
            "a skew-shaped block decode failure must increment \
             wrfcoin_p2p_payload_decode_failed_total{{kind=\"block\"}} \
             (before={before}, after={after}) — an uncounted drop is the \
             10.5h-unalerted failure shape (#2884)"
        );
        assert_eq!(
            blockchain.read().await.height,
            0,
            "the skew-shaped block must be dropped, never applied"
        );
    }

    #[cfg(feature = "execution")]
    #[tokio::test]
    async fn run_p2p_event_loop_rejects_invalid_forecast_task_result_without_side_effects() {
        let blockchain = Arc::new(RwLock::new(Blockchain::default()));
        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let storage = test_storage();
        let consensus = test_consensus();
        let (event_tx, event_rx) = mpsc::channel(8);
        let (forecast_task_assignments, forecast_node_heartbeats, forecast_task_results) =
            test_forecast_state();
        let reputation = std::sync::Arc::new(tokio::sync::RwLock::new(
            crate::peer_reputation::PeerReputationTracker::new(),
        ));

        let event_loop = tokio::spawn(run_p2p_event_loop(
            event_rx,
            blockchain,
            mempool,
            storage,
            consensus,
            None,
            DaChallengeRuntime::new(),
            None,
            None,
            forecast_task_assignments.clone(),
            forecast_node_heartbeats.clone(),
            forecast_task_results.clone(),
            Arc::new(AtomicBool::new(false)),
            StateDivergenceRecoveryPolicy::PeerAgreement,
            reputation,
            test_commit_signing_key(),
        ));

        let invalid_receipt_bytes = bincode::serialize(&"invalid-receipt")
            .expect("invalid payload fixture should serialize");
        event_tx
            .send(P2pEvent::ForecastTaskResult {
                task_id: "bad-task".to_string(),
                node_id: "bad-node".to_string(),
                receipt_bytes: invalid_receipt_bytes,
            })
            .await
            .expect("event receiver should accept invalid forecast result");
        drop(event_tx);

        event_loop
            .await
            .expect("event loop should exit cleanly after channel close");

        assert_eq!(
            forecast_task_assignments.read().await.len(),
            0,
            "invalid payload must not mutate forecast assignments"
        );
        assert_eq!(
            forecast_node_heartbeats.read().await.len(),
            0,
            "invalid payload must not mutate forecast heartbeats"
        );
        assert_eq!(
            forecast_task_results.read().await.len(),
            0,
            "invalid payload must not mutate forecast task results"
        );
    }

    #[test]
    fn fork_candidate_buffer_build_chain_from_requires_contiguous_prev_hashes() {
        let anchor_hash = CryptoHash::hash_bytes(b"anchor");
        let block_1 = test_block(1, anchor_hash, 11);
        let block_1_hash = block_1.hash.expect("fixture block should have hash");

        let mut buffer = ForkCandidateBuffer {
            blocks: Default::default(),
            max_entries: 8,
        };
        buffer.insert(block_1.clone());
        buffer.insert(test_block(2, CryptoHash::hash_bytes(b"wrong-parent"), 12));

        assert!(
            buffer.build_chain_from(1, anchor_hash).is_none(),
            "a missing parent link at the next height must reject the candidate chain"
        );

        buffer.insert(test_block(2, block_1_hash, 13));
        let chain = buffer
            .build_chain_from(1, anchor_hash)
            .expect("contiguous links should build a candidate chain");
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[0].height, 1);
        assert_eq!(chain[1].height, 2);
        assert_eq!(chain[1].prev_block_hash, block_1_hash);
    }

    #[test]
    fn detect_fork_from_buffer_accepts_genesis_anchored_longer_fork() {
        let mut local_chain = Blockchain::default();
        extend_chain(&mut local_chain, 2, 100);

        let mut candidate_buffer = ForkCandidateBuffer {
            blocks: Default::default(),
            max_entries: 8,
        };
        let candidate_1 = test_block(1, CryptoHash::default(), 201);
        let candidate_1_hash = candidate_1.hash.expect("fixture block should have hash");
        let candidate_2 = test_block(2, candidate_1_hash, 202);
        let candidate_2_hash = candidate_2.hash.expect("fixture block should have hash");
        let candidate_3 = test_block(3, candidate_2_hash, 203);

        candidate_buffer.insert(candidate_1.clone());
        candidate_buffer.insert(candidate_2.clone());
        candidate_buffer.insert(candidate_3.clone());

        let detected = detect_fork_from_buffer(&candidate_buffer, &local_chain)
            .expect("genesis-anchored longer fork should be detected");

        assert_eq!(detected.len(), 3);
        assert_eq!(detected[0].height, 1);
        assert_eq!(detected[0].prev_block_hash, CryptoHash::default());
        assert_eq!(detected.last().map(|b| b.height), Some(3));
    }

    #[tokio::test]
    #[serial_test::serial(upgrade_deadline)]
    async fn run_p2p_event_loop_replays_buffered_orphan_after_synced_parent() {
        let blockchain = Arc::new(RwLock::new(Blockchain::default()));
        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let storage = test_storage();
        let consensus = test_consensus();
        let fixture_consensus = test_consensus();
        let (event_tx, event_rx) = mpsc::channel(8);

        let fixture_chain = Arc::new(RwLock::new(Blockchain::default()));
        let fixture_mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let block_1 = fixture_consensus
            .create_pop_block_with_state(
                Arc::clone(&fixture_chain),
                Arc::clone(&fixture_mempool),
                &test_address_string(0x31),
            )
            .await
            .expect("fixture block 1 should be created by stateful consensus");
        let block_1_hash = block_1.hash.expect("fixture block should have hash");
        let block_2 = fixture_consensus
            .create_pop_block_with_state(
                Arc::clone(&fixture_chain),
                Arc::clone(&fixture_mempool),
                &test_address_string(0x32),
            )
            .await
            .expect("fixture block 2 should be created by stateful consensus");
        let block_2_hash = block_2.hash.expect("fixture block should have hash");

        let event_loop = tokio::spawn(run_p2p_event_loop(
            event_rx,
            Arc::clone(&blockchain),
            mempool,
            Arc::clone(&storage),
            consensus,
            None,
            DaChallengeRuntime::new(),
            None,
            None,
            #[cfg(feature = "execution")]
            test_forecast_state().0,
            #[cfg(feature = "execution")]
            test_forecast_state().1,
            #[cfg(feature = "execution")]
            test_forecast_state().2,
            Arc::new(AtomicBool::new(false)),
            StateDivergenceRecoveryPolicy::PeerAgreement,
            std::sync::Arc::new(tokio::sync::RwLock::new(
                crate::peer_reputation::PeerReputationTracker::new(),
            )),
            test_commit_signing_key(),
        ));

        event_tx
            .send(P2pEvent::NewBlock(
                bincode::serialize(&block_2).expect("orphan block should serialize"),
            ))
            .await
            .expect("event receiver should accept orphan block");
        event_tx
            .send(P2pEvent::SyncedBlock {
                peer_id: libp2p::PeerId::random(),
                height: 1,
                block_bytes: bincode::serialize(&block_1)
                    .expect("synced parent block should serialize"),
            })
            .await
            .expect("event receiver should accept synced parent");
        drop(event_tx);

        event_loop
            .await
            .expect("event loop should shut down cleanly once sender is dropped");

        let chain = blockchain.read().await;
        assert_eq!(chain.height, 2, "synced parent should trigger orphan drain");
        assert_eq!(chain.tip, block_2_hash);
        assert_eq!(
            chain.block_at_height(1).and_then(|block| block.hash),
            Some(block_1_hash)
        );
        assert_eq!(
            chain.block_at_height(2).and_then(|block| block.hash),
            Some(block_2_hash)
        );
    }

    /// #3093: with an upgrade deadline at H = 2, block 1 applies and block 2
    /// is refused — not applied, tip stays at 1 — and the clean halt is
    /// requested.
    ///
    /// Runs in its own process (re-exec child). The deadline and the halt
    /// flag are process-wide, and `serial_test` serialises this test only
    /// against tests sharing its key: every OTHER test that mines or applies
    /// a block at height >= 2 while the deadline is installed was refused
    /// (build container run 36385632660 on core4#3364: 14 unrelated failures, all
    /// "EXIT_UPGRADE_REQUIRED … deadline 2 (feature \"future_feature\")").
    #[tokio::test]
    async fn upgrade_deadline_refuses_the_block_at_the_height_and_requests_the_halt_3093() {
        const CHILD: &str = "WRFCOIN_UPGRADE_DEADLINE_3093_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "p2p_events::tests::upgrade_deadline_refuses_the_block_at_the_height_and_requests_the_halt_3093",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .status()
                .expect("re-exec child");
            assert!(status.success(), "isolated #3093 deadline child failed");
            return;
        }
        use wrfcoin_native_chain::upgrade::{
            halt_requested, install_upgrade_deadline, reset_for_tests, UpgradeDeadline,
        };
        let blockchain = Arc::new(RwLock::new(Blockchain::default()));
        let storage = test_storage();
        let consensus = test_consensus();
        let fixture_consensus = test_consensus();
        let fixture_chain = Arc::new(RwLock::new(Blockchain::default()));
        let fixture_mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let mut fork_buffer = ForkCandidateBuffer::new();
        let mut orphan_buffer = OrphanBlockBuffer::new();

        let block_1 = fixture_consensus
            .create_pop_block_with_state(
                Arc::clone(&fixture_chain),
                Arc::clone(&fixture_mempool),
                &test_address_string(0x51),
            )
            .await
            .expect("fixture block 1");
        let block_2 = fixture_consensus
            .create_pop_block_with_state(
                Arc::clone(&fixture_chain),
                Arc::clone(&fixture_mempool),
                &test_address_string(0x52),
            )
            .await
            .expect("fixture block 2");
        let block_1_hash = block_1.hash.expect("hash");

        reset_for_tests();
        install_upgrade_deadline(Some(UpgradeDeadline {
            feature: "future_feature".into(),
            height: 2,
            running_protocol_version: 2,
        }));

        let outcome = apply_p2p_block(
            block_1,
            P2pBlockSource::HistoricalSync,
            &blockchain,
            &storage,
            &consensus,
            None,
            &mut fork_buffer,
            &mut orphan_buffer,
        )
        .await;
        assert!(matches!(outcome, P2pBlockApplyOutcome::Applied), "block 1 (< H) applies");
        assert!(!halt_requested(), "no halt before the deadline");

        let outcome = apply_p2p_block(
            block_2,
            P2pBlockSource::HistoricalSync,
            &blockchain,
            &storage,
            &consensus,
            None,
            &mut fork_buffer,
            &mut orphan_buffer,
        )
        .await;
        match outcome {
            P2pBlockApplyOutcome::Rejected { attribution, reason } => {
                assert_eq!(attribution, AuthoritativeSyncRejectionAttribution::LocalFailure);
                assert!(reason.contains("EXIT_UPGRADE_REQUIRED"), "{reason}");
            },
            other => panic!("block 2 (== H) must be refused, got {other:?}"),
        }
        let chain = blockchain.read().await;
        assert_eq!(chain.height, 1, "tip stays at H - 1");
        assert_eq!(chain.tip, block_1_hash);
        assert_eq!(orphan_buffer.total_count(), 0, "never buffered");
        assert!(halt_requested(), "the clean halt is requested at the deadline");
        drop(chain);
        reset_for_tests();
    }

    #[tokio::test]
    async fn drain_orphan_buffer_skips_invalid_candidate_and_applies_next_same_height() {
        let blockchain = Arc::new(RwLock::new(Blockchain::default()));
        let storage = test_storage();
        let consensus = test_consensus();
        let fixture_consensus = test_consensus();
        let fixture_chain = Arc::new(RwLock::new(Blockchain::default()));
        let fixture_mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let mut fork_buffer = ForkCandidateBuffer::new();
        let mut orphan_buffer = OrphanBlockBuffer::new();

        let block_1 = fixture_consensus
            .create_pop_block_with_state(
                Arc::clone(&fixture_chain),
                Arc::clone(&fixture_mempool),
                &test_address_string(0x41),
            )
            .await
            .expect("fixture block 1 should be created by stateful consensus");
        let valid_block_2 = fixture_consensus
            .create_pop_block_with_state(
                Arc::clone(&fixture_chain),
                Arc::clone(&fixture_mempool),
                &test_address_string(0x42),
            )
            .await
            .expect("fixture block 2 should be created by stateful consensus");
        let valid_block_2_hash = valid_block_2.hash.expect("fixture block should have hash");

        apply_p2p_block(
            block_1,
            P2pBlockSource::HistoricalSync,
            &blockchain,
            &storage,
            &consensus,
            None,
            &mut fork_buffer,
            &mut orphan_buffer,
        )
        .await;

        let mut invalid_block_2 = valid_block_2.clone();
        invalid_block_2.prev_block_hash = CryptoHash::hash_bytes(b"wrong-parent");
        invalid_block_2.set_hash(invalid_block_2.calculate_hash());

        orphan_buffer.insert(invalid_block_2);
        orphan_buffer.insert(valid_block_2);

        drain_orphan_buffer(
            &blockchain,
            &storage,
            &consensus,
            None,
            &mut fork_buffer,
            &mut orphan_buffer,
        )
        .await;

        let chain = blockchain.read().await;
        assert_eq!(
            chain.height, 2,
            "drain_orphan_buffer should keep trying same-height candidates after a wrong-parent block"
        );
        assert_eq!(chain.tip, valid_block_2_hash);
        assert!(
            orphan_buffer.take_height(2).is_empty(),
            "height 2 candidates should be drained after the valid candidate applies"
        );
    }

    #[tokio::test]
    async fn peer_chain_status_zero_root_with_matching_tip_does_not_set_state_diverged() {
        use libp2p::PeerId;

        let _counter_guard = DIVERGENCE_COUNTER_GUARD.lock().unwrap_or_else(|e| e.into_inner());

        let blockchain = Arc::new(RwLock::new(Blockchain::default()));
        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let storage = test_storage();
        let consensus = test_consensus();
        let fixture_consensus = test_consensus();
        let fixture_chain = Arc::new(RwLock::new(Blockchain::default()));
        let fixture_mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));

        let block_1 = fixture_consensus
            .create_pop_block_with_state(
                Arc::clone(&fixture_chain),
                Arc::clone(&fixture_mempool),
                &test_address_string(0x51),
            )
            .await
            .expect("fixture block should be created");
        let block_1_hash = block_1.hash.expect("fixture block should have hash");

        let state_diverged = Arc::new(AtomicBool::new(false));
        let divergence_counter_before = CORE_METRICS.state_diverged_total.get();
        let (event_tx, event_rx) = mpsc::channel(8);

        let event_loop = tokio::spawn(run_p2p_event_loop(
            event_rx,
            Arc::clone(&blockchain),
            mempool,
            storage,
            consensus,
            None,
            DaChallengeRuntime::new(),
            None,
            None,
            #[cfg(feature = "execution")]
            test_forecast_state().0,
            #[cfg(feature = "execution")]
            test_forecast_state().1,
            #[cfg(feature = "execution")]
            test_forecast_state().2,
            Arc::clone(&state_diverged),
            StateDivergenceRecoveryPolicy::PeerAgreement,
            std::sync::Arc::new(tokio::sync::RwLock::new(
                crate::peer_reputation::PeerReputationTracker::new(),
            )),
            test_commit_signing_key(),
        ));

        event_tx
            .send(P2pEvent::SyncedBlock {
                peer_id: libp2p::PeerId::random(),
                height: 1,
                block_bytes: bincode::serialize(&block_1).expect("block should serialize"),
            })
            .await
            .expect("event receiver should accept synced block");

        // Peer agrees on height and tip but sends an all-zero state root.
        // Guard must emit WARN rather than ERROR and must not set state_diverged.
        event_tx
            .send(P2pEvent::PeerChainStatus {
                peer_id: PeerId::random(),
                height: 1,
                tip_hash: block_1_hash.into(),
                state_root: [0u8; 32],
            })
            .await
            .expect("event receiver should accept peer chain status");

        drop(event_tx);
        event_loop.await.expect("event loop should shut down cleanly");

        assert!(
            !state_diverged.load(Ordering::SeqCst),
            "zero state root with matching tip/height must not set state_diverged"
        );
        assert_eq!(
            CORE_METRICS.state_diverged_total.get(),
            divergence_counter_before,
            "matching tip + zero-root compatibility warning must not bump divergence counter"
        );
    }

    #[tokio::test]
    async fn peer_chain_status_nonzero_root_mismatch_sets_state_diverged() {
        use libp2p::PeerId;

        let _counter_guard = DIVERGENCE_COUNTER_GUARD.lock().unwrap_or_else(|e| e.into_inner());

        let blockchain = Arc::new(RwLock::new(Blockchain::default()));
        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let storage = test_storage();
        let consensus = test_consensus();
        let fixture_consensus = test_consensus();
        let fixture_chain = Arc::new(RwLock::new(Blockchain::default()));
        let fixture_mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));

        let block_1 = fixture_consensus
            .create_pop_block_with_state(
                Arc::clone(&fixture_chain),
                Arc::clone(&fixture_mempool),
                &test_address_string(0x61),
            )
            .await
            .expect("fixture block should be created");
        let block_1_hash = block_1.hash.expect("fixture block should have hash");

        let state_diverged = Arc::new(AtomicBool::new(false));
        let divergence_counter_before = CORE_METRICS.state_diverged_total.get();
        let (event_tx, event_rx) = mpsc::channel(8);

        let event_loop = tokio::spawn(run_p2p_event_loop(
            event_rx,
            Arc::clone(&blockchain),
            mempool,
            storage,
            consensus,
            None,
            DaChallengeRuntime::new(),
            None,
            None,
            #[cfg(feature = "execution")]
            test_forecast_state().0,
            #[cfg(feature = "execution")]
            test_forecast_state().1,
            #[cfg(feature = "execution")]
            test_forecast_state().2,
            Arc::clone(&state_diverged),
            StateDivergenceRecoveryPolicy::PeerAgreement,
            std::sync::Arc::new(tokio::sync::RwLock::new(
                crate::peer_reputation::PeerReputationTracker::new(),
            )),
            test_commit_signing_key(),
        ));

        event_tx
            .send(P2pEvent::SyncedBlock {
                peer_id: libp2p::PeerId::random(),
                height: 1,
                block_bytes: bincode::serialize(&block_1).expect("block should serialize"),
            })
            .await
            .expect("event receiver should accept synced block");

        // Peer sends a non-zero but wrong state root; real divergence must ERROR and set flag.
        event_tx
            .send(P2pEvent::PeerChainStatus {
                peer_id: PeerId::random(),
                height: 1,
                tip_hash: block_1_hash.into(),
                state_root: [0xde; 32],
            })
            .await
            .expect("event receiver should accept peer chain status");
        event_tx
            .send(P2pEvent::PeerChainStatus {
                peer_id: PeerId::random(),
                height: 1,
                tip_hash: block_1_hash.into(),
                state_root: [0xad; 32],
            })
            .await
            .expect("event receiver should accept repeated peer chain status");

        drop(event_tx);
        event_loop.await.expect("event loop should shut down cleanly");

        assert!(
            state_diverged.load(Ordering::SeqCst),
            "non-zero mismatched state root must set state_diverged"
        );
        assert_eq!(
            CORE_METRICS.state_diverged_total.get(),
            divergence_counter_before + 1,
            "repeated same-incident peer reports must bump divergence counter once"
        );
    }

    #[tokio::test]
    async fn peer_chain_status_same_height_different_tip_sets_state_diverged() {
        use libp2p::PeerId;

        let _counter_guard = DIVERGENCE_COUNTER_GUARD.lock().unwrap_or_else(|e| e.into_inner());

        let blockchain = Arc::new(RwLock::new(Blockchain::default()));
        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let storage = test_storage();
        let consensus = test_consensus();
        let fixture_consensus = test_consensus();
        let fixture_chain = Arc::new(RwLock::new(Blockchain::default()));
        let fixture_mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));

        let block_1 = fixture_consensus
            .create_pop_block_with_state(
                Arc::clone(&fixture_chain),
                Arc::clone(&fixture_mempool),
                &test_address_string(0x71),
            )
            .await
            .expect("fixture block should be created");

        let state_diverged = Arc::new(AtomicBool::new(false));
        let divergence_counter_before = CORE_METRICS.state_diverged_total.get();
        let (event_tx, event_rx) = mpsc::channel(8);

        let event_loop = tokio::spawn(run_p2p_event_loop(
            event_rx,
            Arc::clone(&blockchain),
            mempool,
            storage,
            consensus,
            None,
            DaChallengeRuntime::new(),
            None,
            None,
            #[cfg(feature = "execution")]
            test_forecast_state().0,
            #[cfg(feature = "execution")]
            test_forecast_state().1,
            #[cfg(feature = "execution")]
            test_forecast_state().2,
            Arc::clone(&state_diverged),
            StateDivergenceRecoveryPolicy::PeerAgreement,
            std::sync::Arc::new(tokio::sync::RwLock::new(
                crate::peer_reputation::PeerReputationTracker::new(),
            )),
            test_commit_signing_key(),
        ));

        event_tx
            .send(P2pEvent::SyncedBlock {
                peer_id: libp2p::PeerId::random(),
                height: 1,
                block_bytes: bincode::serialize(&block_1).expect("block should serialize"),
            })
            .await
            .expect("event receiver should accept synced block");

        event_tx
            .send(P2pEvent::PeerChainStatus {
                peer_id: PeerId::random(),
                height: 1,
                tip_hash: [0x7b; 32],
                state_root: [0u8; 32],
            })
            .await
            .expect("event receiver should accept peer chain status");

        drop(event_tx);
        event_loop.await.expect("event loop should shut down cleanly");

        assert!(
            state_diverged.load(Ordering::SeqCst),
            "same-height different tip must set state_diverged even when peer omits state root"
        );
        assert_eq!(
            CORE_METRICS.state_diverged_total.get(),
            divergence_counter_before + 1,
            "same-height different tip must bump divergence counter"
        );
    }

    // ---- #2119 serve-side byte budget --------------------------------------

    /// Build `n` chained blocks in memory and return the chain.
    fn chain_of(n: u64) -> Blockchain {
        let mut chain = Blockchain::with_capacity(n as usize + 1);
        for height in 1..=n {
            let block = test_block(height, chain.tip, height);
            chain.add_block(block).expect("extend chain");
        }
        chain
    }

    /// THE amplification bound: a request admitted for 100 blocks is trimmed to
    /// THIS BINARY consults the governor and honours a refusal (#2119).
    ///
    /// Closes a gap found by a RED control on the reconciliation PR (#2536).
    /// Forcing `SyncServeGovernor::admit` to return
    /// `Serve { max_blocks: u32::MAX, byte_budget: u64::MAX }` unconditionally
    /// — governor present, called, type-checking, enforcing NOTHING — failed
    /// 9 of 12 `sync_limits` unit tests and left EVERY `launch_api` test green.
    /// The governor was covered; this binary's use of it was not, so the rate
    /// limiter could have been silently disabled by a refactor, a bad merge, or
    /// the `SyncServeLimits` -> `SyncServeQuota` rename this PR performs.
    ///
    /// The assertions are ordered deliberately: FIRST that a refusal is even
    /// reachable, THEN its label. A test that only checked the label would pass
    /// against a governor that never refuses, because it would never reach it.
    #[::core::prelude::v1::test]
    fn this_binary_honours_a_governor_refusal_2119() {
        use std::time::{Duration, Instant};

        // Deliberately tight so one request cannot fit: the point is to reach a
        // refusal, not to model a realistic ceiling.
        let quota = wrfcoin_p2p::SyncServeQuota {
            max_blocks_per_request: 8,
            max_response_bytes: 4_096,
            max_bytes_per_peer_per_window: 4_096,
            bandwidth_window: Duration::from_secs(60),
            min_request_interval: Duration::from_secs(30),
            max_tracked_peers: 16,
        };
        let mut governor = wrfcoin_p2p::SyncServeGovernor::new(quota);
        let now = Instant::now();

        // First request is admitted, and the binary receives real ceilings
        // rather than the caller's unbounded ask.
        let first = decide_sync_serve_admission(&mut governor, "peer-a", 1_000, now);
        let SyncServeAdmission::Serve {
            max_blocks,
            byte_budget,
        } = first
        else {
            panic!("first request must be admitted, got {first:?}");
        };
        assert!(
            max_blocks <= 8 && byte_budget <= 4_096,
            "the binary must receive the governor's ceilings, not the request's \
             ask — got max_blocks={max_blocks} byte_budget={byte_budget}. If the \
             governor is neutered these are u32::MAX/u64::MAX (#2536)"
        );

        // An immediate second request is inside `min_request_interval`.
        let second = decide_sync_serve_admission(&mut governor, "peer-a", 1_000, now);
        let SyncServeAdmission::Refused { label, retry_after } = second else {
            panic!(
                "a second request inside min_request_interval must be REFUSED — \
                 got {second:?}. This is the assertion that fails when the \
                 governor is present but enforcing nothing (#2536)."
            );
        };
        assert!(
            !label.is_empty(),
            "the refusal must carry a metric label; it is what \
             inc_sync_serve_rejected records"
        );
        assert!(
            retry_after > Duration::ZERO,
            "a refusal must state when to retry, even though the wire cannot \
             carry it — it is the operator-visible half"
        );
    }

    /// Storage seeded with blocks 1..=30 while the in-memory window covers
    /// only 21..=30 — so a request at height 1 MUST take the storage path,
    /// which is where the #2579 lifecycle tests hold serves pending (via the
    /// storage RwLock's write half). Same construction as the #1664
    /// fall-through test above.
    async fn seeded_storage_and_chain(
        storage_path: &std::path::Path,
    ) -> (Arc<RwLock<Storage>>, Arc<RwLock<Blockchain>>) {
        use wrfcoin_native_chain::blockchain::state::StateTransition;

        let storage = Arc::new(RwLock::new(
            Storage::open(storage_path, DEFAULT_CHECKPOINT_INTERVAL).expect("test storage opens"),
        ));
        let mut chain = Blockchain::with_capacity(10);
        for height in 1..=30u64 {
            let block = test_block(height, chain.tip, height);
            chain.add_block(block.clone()).expect("extend chain");
            let mut st = storage.write().await;
            let checkpoint_bytes = st
                .checkpoint_state_bytes(block.height, &chain.state)
                .expect("checkpoint state encodes");
            st.persist_block(block, vec![] as Vec<StateTransition>, checkpoint_bytes)
                .await
                .expect("persist block");
        }
        assert!(chain.block_at_height(1).is_none(), "height 1 must live only in storage");
        (storage, Arc::new(RwLock::new(chain)))
    }

    /// Responder that reports `(tag, response)` onto a channel, standing in
    /// for the command-channel send the event-loop arm binds (libp2p's
    /// `InboundRequestId` is unconstructible from tests — see
    /// `SyncServeResponder`).
    fn tagged_responder(
        tag: u32,
        tx: &mpsc::Sender<(u32, wrfcoin_p2p::sync::BlockResponse)>,
    ) -> SyncServeResponder {
        let tx = tx.clone();
        Box::new(move |response| {
            Box::pin(async move {
                let _ = tx.send((tag, response)).await;
            })
        })
    }

    /// #2579 — THE lifecycle control both reviews asked for: hold one serve
    /// pending, advance past the request interval, prove the second request is
    /// refused because the first serve's byte budget is still RESERVED, then
    /// prove settlement returns the unused allowance.
    ///
    /// This drives `handle_inbound_block_sync_request` — the function the
    /// event-loop arm binds to the command channel — through the real
    /// admission → reservation → spawned-read → settle → respond sequence.
    /// The hold is the storage lock's write half: the spawned serve blocks
    /// acquiring the read half, the same shape as a slow redb range read but
    /// with no timing dependence. That both later calls return decisions (and
    /// the refusal is ANSWERED) while the first serve is still pending is the
    /// liveness half of #2522: admission never waits on serving.
    ///
    /// RED control (documented in the PR): with the reservation removed from
    /// `SyncServeGovernor::admit`, the second call is admitted and the
    /// `bandwidth_exhausted` assertion below fails — which is exactly the
    /// serve-quota bypass the #2579 reviews flagged.
    #[tokio::test]
    async fn sync_serve_budget_is_reserved_while_a_serve_is_in_flight_2579() {
        let storage_path = std::env::temp_dir()
            .join(format!("wrfcoin-2579-lifecycle-{}.redb", fastrand::u64(..)));
        let (storage, blockchain) = seeded_storage_and_chain(&storage_path).await;

        // One response budget IS the whole per-peer window, so a single serve
        // in flight must exhaust the allowance until it settles. The absolute
        // numbers are generous relative to the ~30 small test blocks, so the
        // ONLY thing that can refuse the second request is the reservation.
        let governor = Arc::new(tokio::sync::Mutex::new(wrfcoin_p2p::SyncServeGovernor::new(
            wrfcoin_p2p::SyncServeQuota {
                max_blocks_per_request: 100,
                max_response_bytes: 64_000,
                max_bytes_per_peer_per_window: 64_000,
                bandwidth_window: Duration::from_secs(60),
                min_request_interval: Duration::from_millis(1),
                max_tracked_peers: 16,
            },
        )));
        // Two slots, so the second request reaches the GOVERNOR — the
        // concurrency bound has its own isolated test below.
        let permits = Arc::new(tokio::sync::Semaphore::new(2));
        let (resp_tx, mut resp_rx) = mpsc::channel::<(u32, wrfcoin_p2p::sync::BlockResponse)>(8);

        // HOLD: every serve's storage read now blocks until this guard drops.
        let hold = storage.write().await;

        let first = handle_inbound_block_sync_request(
            &governor,
            &permits,
            &blockchain,
            &storage,
            "peer-a".to_string(),
            1,
            5,
            std::time::Instant::now(),
            tagged_responder(1, &resp_tx),
        )
        .await;
        assert!(first.is_none(), "first request must be admitted and spawned, got {first:?}");

        // Past the rate interval; the first serve is still pending on the hold.
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert!(
            resp_rx.try_recv().is_err(),
            "nothing may be answered while the hold is in place — the serve \
             must actually be pending for this control to mean anything"
        );

        let second = handle_inbound_block_sync_request(
            &governor,
            &permits,
            &blockchain,
            &storage,
            "peer-a".to_string(),
            1,
            5,
            std::time::Instant::now(),
            tagged_responder(2, &resp_tx),
        )
        .await;
        let Some((label, _retry_after)) = second else {
            panic!(
                "the second request must be refused while the first serve's \
                 budget is reserved in flight — admitting it is the #2579 \
                 serve-quota bypass"
            );
        };
        assert_eq!(label, "bandwidth_exhausted");

        // The refusal is answered empty IMMEDIATELY, while the first serve is
        // still held — refusals never wait on in-flight serving.
        let (tag, refused) = tokio::time::timeout(Duration::from_secs(10), resp_rx.recv())
            .await
            .expect("the refusal answer must not require the hold to release")
            .expect("responder channel open");
        assert_eq!(tag, 2, "the response arriving under the hold must be the refusal");
        assert!(refused.blocks.is_empty(), "a refusal answers empty");

        // RELEASE: the pending serve completes, settles, and responds.
        drop(hold);
        let (tag, served) = tokio::time::timeout(Duration::from_secs(10), resp_rx.recv())
            .await
            .expect("released serve must complete")
            .expect("responder channel open");
        assert_eq!(tag, 1);
        assert_eq!(served.blocks.len(), 5, "the held serve must deliver its range");
        assert_eq!(served.blocks.first().map(|b| b.height), Some(1));

        // Settlement reconciled the 64_000-byte reservation down to the actual
        // serialized size of 5 small test blocks, so allowance is back. A
        // reservation that stuck (or a settle that never ran) refuses this.
        tokio::time::sleep(Duration::from_millis(5)).await;
        let third = handle_inbound_block_sync_request(
            &governor,
            &permits,
            &blockchain,
            &storage,
            "peer-a".to_string(),
            1,
            5,
            std::time::Instant::now(),
            tagged_responder(3, &resp_tx),
        )
        .await;
        assert!(
            third.is_none(),
            "after settlement only the ACTUAL bytes stay charged, so this \
             admission must succeed, got {third:?}"
        );
        let (tag, third_response) = tokio::time::timeout(Duration::from_secs(10), resp_rx.recv())
            .await
            .expect("third serve must complete")
            .expect("responder channel open");
        assert_eq!(tag, 3);
        assert_eq!(third_response.blocks.len(), 5);

        let _ = std::fs::remove_file(&storage_path);
    }

    /// #2579 — the concurrency half: in-flight serve TASKS are bounded by the
    /// permit pool, independent of any per-peer budget. Three DIFFERENT peers,
    /// a deliberately generous governor, one serve slot: the only thing that
    /// can refuse here is the global bound.
    ///
    /// RED control (documented in the PR): with the `try_acquire_owned` gate
    /// removed from `handle_inbound_block_sync_request`, the second request
    /// spawns a second pending task and the `serve_busy` assertion fails —
    /// the unbounded-`tokio::spawn` gap from the #2579 reviews.
    #[tokio::test]
    async fn sync_serve_concurrency_is_bounded_by_the_permit_pool_2579() {
        let storage_path = std::env::temp_dir()
            .join(format!("wrfcoin-2579-permits-{}.redb", fastrand::u64(..)));
        let (storage, blockchain) = seeded_storage_and_chain(&storage_path).await;

        let governor = Arc::new(tokio::sync::Mutex::new(wrfcoin_p2p::SyncServeGovernor::new(
            wrfcoin_p2p::SyncServeQuota {
                min_request_interval: Duration::from_millis(1),
                ..wrfcoin_p2p::SyncServeQuota::default()
            },
        )));
        let permits = Arc::new(tokio::sync::Semaphore::new(1));
        let (resp_tx, mut resp_rx) = mpsc::channel::<(u32, wrfcoin_p2p::sync::BlockResponse)>(8);

        let hold = storage.write().await;

        let first = handle_inbound_block_sync_request(
            &governor,
            &permits,
            &blockchain,
            &storage,
            "peer-a".to_string(),
            1,
            5,
            std::time::Instant::now(),
            tagged_responder(1, &resp_tx),
        )
        .await;
        assert!(first.is_none(), "first request takes the only slot, got {first:?}");

        tokio::time::sleep(Duration::from_millis(5)).await;
        let second = handle_inbound_block_sync_request(
            &governor,
            &permits,
            &blockchain,
            &storage,
            "peer-b".to_string(),
            1,
            5,
            std::time::Instant::now(),
            tagged_responder(2, &resp_tx),
        )
        .await;
        let Some((label, _)) = second else {
            panic!(
                "a second serve with every slot occupied must be refused — \
                 spawning it anyway is the unbounded-concurrency gap: {second:?}"
            );
        };
        assert_eq!(label, "serve_busy");

        let (tag, refused) = tokio::time::timeout(Duration::from_secs(10), resp_rx.recv())
            .await
            .expect("the busy refusal must be answered while the slot is held")
            .expect("responder channel open");
        assert_eq!(tag, 2);
        assert!(refused.blocks.is_empty(), "a busy refusal answers empty");

        drop(hold);
        let (tag, served) = tokio::time::timeout(Duration::from_secs(10), resp_rx.recv())
            .await
            .expect("released serve must complete")
            .expect("responder channel open");
        assert_eq!(tag, 1);
        assert_eq!(served.blocks.len(), 5);

        // The completed serve returned its permit: a new peer is admitted.
        tokio::time::sleep(Duration::from_millis(5)).await;
        let third = handle_inbound_block_sync_request(
            &governor,
            &permits,
            &blockchain,
            &storage,
            "peer-c".to_string(),
            1,
            5,
            std::time::Instant::now(),
            tagged_responder(3, &resp_tx),
        )
        .await;
        assert!(third.is_none(), "a freed slot must admit new serves, got {third:?}");
        let (tag, third_response) = tokio::time::timeout(Duration::from_secs(10), resp_rx.recv())
            .await
            .expect("third serve must complete")
            .expect("responder channel open");
        assert_eq!(tag, 3);
        assert_eq!(third_response.blocks.len(), 5);

        let _ = std::fs::remove_file(&storage_path);
    }

    /// whatever the byte budget affords, and the caller learns the real cost.
    ///
    /// Before #2119 the only serve-side ceiling was `count.min(100)`; 100 blocks
    /// were read, cloned and handed to the codec, and the 16 MiB
    /// `MAX_SYNC_MESSAGE_BYTES` write cap then errored AFTER the work was paid
    /// for. A ceiling that fires after the cost is an error report, not a limit.
    #[tokio::test]
    async fn byte_budget_trims_the_response_and_reports_actual_bytes_2119() {
        let storage_path = std::env::temp_dir()
            .join(format!("wrfcoin-2119-budget-{}.redb", fastrand::u64(..)));
        let storage = Arc::new(RwLock::new(
            Storage::open(&storage_path, DEFAULT_CHECKPOINT_INTERVAL).expect("storage opens"),
        ));
        let chain = chain_of(20);
        let one_block = bincode::serialized_size(
            chain.block_at_height(1).expect("height 1 present"),
        )
        .expect("block serializes");
        let blockchain = Arc::new(RwLock::new(chain));

        // Budget for ~3 blocks out of the 20 requested.
        let budget = one_block * 3;
        let (response, bytes) =
            collect_block_sync_response(&blockchain, &storage, 1, 20, budget, Some("peer")).await;

        assert!(
            response.blocks.len() < 20,
            "the byte budget must bind before the block count: got all 20 back"
        );
        assert_eq!(
            response.blocks.len(),
            3,
            "a budget of exactly 3 block-sizes must yield 3 blocks"
        );
        assert!(
            bytes <= budget,
            "reported bytes {bytes} must not exceed the budget {budget}"
        );
        assert_eq!(
            bytes,
            one_block * 3,
            "reported bytes must be what was actually serialized, not the budget"
        );
        // Short responses are the protocol's own backpressure, and contiguous
        // from the requested start, so the requester can requeue the remainder.
        assert_eq!(response.blocks[0].height, 1);
        assert_eq!(response.blocks[2].height, 3);

        let _ = std::fs::remove_file(&storage_path);
    }

    /// #3442: fixture blocks must not depend on the wall clock. `byte_budget_trims_...` asserts
    /// `bytes == 3 * size(block 1)`, which only holds when every fixture block serializes to
    /// the same size, so the coinbase timestamp must be pinned (no `Utc::now()` in a fixture).
    #[::core::prelude::v1::test]
    fn fixture_blocks_do_not_depend_on_the_wall_clock_3442() {
        let chain = chain_of(3);
        for height in 1..=3u64 {
            let block = chain.block_at_height(height).expect("height present");
            for tx in &block.transactions {
                assert_eq!(
                    tx.timestamp, block.timestamp,
                    "fixture transaction timestamp at height {height} is wall-clock, not pinned"
                );
            }
        }
        let sizes: Vec<u64> = (1..=3u64)
            .map(|h| bincode::serialized_size(chain.block_at_height(h).unwrap()).unwrap())
            .collect();
        assert!(
            sizes.windows(2).all(|w| w[0] == w[1]),
            "fixture blocks serialize to different sizes: {sizes:?}"
        );
    }

    /// Liveness rule: a single block larger than the whole budget is STILL
    /// served.
    ///
    /// Serving zero here would stall that peer permanently — it would requeue
    /// the same range forever and never advance, which is a self-inflicted
    /// denial of service against honest peers rather than a defence. One
    /// oversized block remains bounded by the codec's `MAX_SYNC_MESSAGE_BYTES`.
    #[tokio::test]
    async fn a_single_oversized_block_is_still_served_so_ibd_cannot_stall_2119() {
        let storage_path = std::env::temp_dir()
            .join(format!("wrfcoin-2119-oversized-{}.redb", fastrand::u64(..)));
        let storage = Arc::new(RwLock::new(
            Storage::open(&storage_path, DEFAULT_CHECKPOINT_INTERVAL).expect("storage opens"),
        ));
        let blockchain = Arc::new(RwLock::new(chain_of(5)));

        // A budget of 1 byte is smaller than any real block.
        let (response, bytes) =
            collect_block_sync_response(&blockchain, &storage, 1, 5, 1, Some("peer")).await;

        assert_eq!(
            response.blocks.len(),
            1,
            "exactly one block must be served even when it exceeds the budget — \
             serving zero would stall the peer's IBD forever"
        );
        assert!(
            bytes > 1,
            "and the overshoot must be reported honestly ({bytes} bytes) so the \
             peer is charged for what it actually cost"
        );

        let _ = std::fs::remove_file(&storage_path);
    }

    /// A budget large enough for everything must not change behaviour — the
    /// limit has to be invisible on the honest path.
    #[tokio::test]
    async fn a_generous_budget_serves_the_whole_requested_range_2119() {
        let storage_path = std::env::temp_dir()
            .join(format!("wrfcoin-2119-generous-{}.redb", fastrand::u64(..)));
        let storage = Arc::new(RwLock::new(
            Storage::open(&storage_path, DEFAULT_CHECKPOINT_INTERVAL).expect("storage opens"),
        ));
        let blockchain = Arc::new(RwLock::new(chain_of(10)));

        let (response, _) =
            collect_block_sync_response(&blockchain, &storage, 1, 10, u64::MAX, Some("peer")).await;
        assert_eq!(
            response.blocks.len(),
            10,
            "the default-sized honest request must be unaffected by the budget"
        );

        let _ = std::fs::remove_file(&storage_path);
    }

    /// Regression test for #1664 round 3. When a node's in-memory window
    /// has rolled past the requested range (e.g. local in-memory is
    /// 6170..=6370 but a peer asks for 6101..=6200), the legacy
    /// in-memory-only path returned EITHER an empty response OR a
    /// partial response of recent blocks that could not chain onto the
    /// peer's tip. The fix falls through to `Storage::load_blocks_range`
    /// when the in-memory result does not start at the requested
    /// `start_height`.
    #[tokio::test]
    async fn collect_block_sync_response_falls_through_to_storage_when_in_memory_missing() {
        use wrfcoin_native_chain::blockchain::state::StateTransition;

        let storage_path = std::env::temp_dir()
            .join(format!("wrfcoin-bs-fallthrough-{}.redb", fastrand::u64(..)));

        // Build a chain of 30 blocks and persist all of them to storage.
        // The in-memory `Blockchain` is sized to only 10 blocks, so after
        // persisting all 30 the in-memory window covers 21..=30 and
        // blocks 1..=20 live ONLY in storage.
        let storage = Arc::new(RwLock::new(
            Storage::open(&storage_path, DEFAULT_CHECKPOINT_INTERVAL)
                .expect("test storage opens"),
        ));
        let mut chain = Blockchain::with_capacity(10);

        for height in 1..=30u64 {
            let block = test_block(height, chain.tip, height);
            chain.add_block(block.clone()).expect("extend chain");
            let mut st = storage.write().await;
            let checkpoint_bytes = st
                .checkpoint_state_bytes(block.height, &chain.state)
                .expect("checkpoint state encodes");
            st.persist_block(block, vec![] as Vec<StateTransition>, checkpoint_bytes)
                .await
                .expect("persist block");
        }

        // Confirm the in-memory window does NOT cover heights 5..=10.
        assert!(chain.block_at_height(5).is_none(), "height 5 evicted from in-memory");
        assert!(chain.block_at_height(10).is_none(), "height 10 evicted from in-memory");
        assert!(chain.block_at_height(21).is_some(), "height 21 still in-memory");

        let blockchain = Arc::new(RwLock::new(chain));

        // BEFORE THE FIX: a peer requesting heights 5..=10 from us
        // would get either an empty response or a partial slice starting
        // at 21 (whatever the in-memory iterator yielded), neither of
        // which can chain onto the peer's tip at height 4.
        //
        // AFTER THE FIX: the helper detects in-memory misses the
        // requested start, falls through to storage, and returns the
        // exact requested range from redb.
        let (response, _) = collect_block_sync_response(&blockchain, &storage, 5, 6, u64::MAX, Some("test-peer")).await;
        assert_eq!(
            response.blocks.len(),
            6,
            "must return 6 blocks for range 5..=10 from storage"
        );
        assert_eq!(response.blocks.first().unwrap().height, 5);
        assert_eq!(response.blocks.last().unwrap().height, 10);

        // Sanity: in-memory-only path (recent range) is still fast and
        // correct. Range 25..=30 is entirely in-memory.
        let (response_recent, _) =
            collect_block_sync_response(&blockchain, &storage, 25, 6, u64::MAX, Some("test-peer"))
                .await;
        assert_eq!(response_recent.blocks.len(), 6);
        assert_eq!(response_recent.blocks.first().unwrap().height, 25);
        assert_eq!(response_recent.blocks.last().unwrap().height, 30);

        // Sanity: a request that spans the storage/in-memory boundary
        // returns the full range. The fix triggers because in-memory
        // starts at 21 (not the requested start of 18), so we go to
        // storage which returns 18..=25 contiguously.
        let (response_span, _) =
            collect_block_sync_response(&blockchain, &storage, 18, 8, u64::MAX, Some("test-peer"))
                .await;
        assert_eq!(response_span.blocks.len(), 8);
        assert_eq!(response_span.blocks.first().unwrap().height, 18);
        assert_eq!(response_span.blocks.last().unwrap().height, 25);

        let _ = std::fs::remove_file(&storage_path);
    }

    // ---------------------------------------------------------------------
    // #1682 — producer/applier block-apply parity.
    //
    // The block PRODUCER path (`create_pop_block_with_state` ->
    // `ProofOfPublishing::create_next_block` ->
    // `apply_launch_state_transitions_on_state` + `process_contribution_rewards`)
    // and the APPLIER path (`apply_p2p_block` -> `extract_transitions` +
    // `apply_state_transition_no_root`) are two independent "apply block ->
    // state" implementations. When they disagree on any mutation that feeds the
    // consensus root, `state_root` forks between a producing node (node host) and a
    // syncing peer even though both have the same blocks (#1680/#1682).
    //
    // These tests reproduce — and now guard against — that fork IN-PROCESS (no
    // node, no redeploy). They ENUMERATE every divergent `State` field in a
    // single run via a serde field diff PLUS a byte-level diff of the
    // `state_root` pre-image, so any future regression names exactly which field
    // (or which stale-root computation) diverged.
    //
    // Two scenarios are covered:
    //   * coinbase-only — exercises the empty-`attested_commit_ids` early-return
    //     in `process_contribution_rewards` (the common case; this is the path
    //     where the producer previously stored a STALE root, #1683).
    //   * attested-reward — a contribution commit+reveal that reaches oracle
    //     quorum, so the NON-empty reward path runs and quality_policy_events /
    //     audit_events are recorded on the producer (these are telemetry,
    //     excluded from the root by #1681; this scenario guards the #1681+#1683
    //     interaction on the path that actually records them).
    //
    // Run with:
    //   cargo test -p wrfcoin-native-chain --bin launch_api --features p2p \
    //     producer_applier_block_apply_parity -- --nocapture
    // ---------------------------------------------------------------------

    /// Serialize one public `State` field to JSON for a name-tagged diff.
    /// `#[serde(skip)]` fields (zk_verifier, slashing engines, circuit breaker,
    /// deferred triggers) are not consensus state and are not compared.
    fn field_json<T: serde::Serialize>(value: &T) -> String {
        serde_json::to_string(value).unwrap_or_else(|e| format!("<unserializable: {e}>"))
    }

    fn state_diff_fields(
        producer: &wrfcoin_native_chain::blockchain::state::State,
        applier: &wrfcoin_native_chain::blockchain::state::State,
    ) -> Vec<(&'static str, String, String)> {
        use wrfcoin_native_chain::blockchain::state::State;

        // (name, producer-json, applier-json) for each public field.
        // `accessor` is a closure returning the JSON for a given State.
        macro_rules! field {
            ($name:literal, $get:expr) => {{
                let get: fn(&State) -> String = $get;
                ($name, get(producer), get(applier))
            }};
        }

        vec![
            field!("accounts", |s| {
                // Sort accounts so insertion-order differences don't show as diffs.
                let mut v: Vec<_> = s.accounts.iter().collect();
                v.sort_by_key(|(a, _)| a.0);
                field_json(&v)
            }),
            field!("nfts", |s| field_json(&s.nfts)),
            field!("pending_commits", |s| field_json(&s.pending_commits)),
            field!("pending_reveals", |s| field_json(&s.pending_reveals)),
            field!("oracle_set", |s| {
                let mut v: Vec<_> = s.oracle_set.iter().collect();
                v.sort();
                field_json(&v)
            }),
            field!("oracle_quorum", |s| field_json(&s.oracle_quorum)),
            field!("attestations", |s| field_json(&s.attestations)),
            field!("privacy_commitments", |s| field_json(&s.privacy_commitments)),
            field!("privacy_spent_key_images", |s| field_json(
                &s.privacy_spent_key_images
            )),
            field!("privacy_spent_nullifiers", |s| field_json(
                &s.privacy_spent_nullifiers
            )),
            field!("privacy_disclosure_ids", |s| field_json(
                &s.privacy_disclosure_ids
            )),
            field!("spent_oracle_nonces", |s| field_json(&s.spent_oracle_nonces)),
            field!("reward_claim_key_images", |s| field_json(
                &s.reward_claim_key_images
            )),
            field!("contribution_reward_records", |s| field_json(
                &s.contribution_reward_records
            )),
            field!("emission_window_start", |s| field_json(&s.emission_window_start)),
            field!("emission_window_spent", |s| field_json(&s.emission_window_spent)),
            field!("finalized_contribution_rewards", |s| field_json(
                &s.finalized_contribution_rewards
            )),
            field!("feeless_worker_epoch_counts", |s| field_json(
                &s.feeless_worker_epoch_counts
            )),
            field!("low_quality_streaks", |s| field_json(&s.low_quality_streaks)),
            field!("quality_policy_events", |s| field_json(
                &s.quality_policy_events
            )),
            field!("audit_events", |s| field_json(&s.audit_events)),
            field!("quality_policy_event_archive", |s| field_json(
                &s.quality_policy_event_archive
            )),
            field!("archived_quality_policy_event_count", |s| field_json(
                &s.archived_quality_policy_event_count
            )),
            field!("mining_pools", |s| field_json(&s.mining_pools)),
            field!("producer_set_history", |s| field_json(&s.producer_set_history)),
            field!("genesis_emission_config", |s| field_json(&s.genesis_emission_config)),
            field!("last_producer_settlement", |s| field_json(&s.last_producer_settlement)),
            // #2172 parity confirmation: real P2P application plus replay of
            // its persisted WAL rows keep the feature-gated map empty.
            field!("account_recovery", |s| field_json(&s.account_recovery)),
            field!("session_keys", |s| field_json(&s.session_keys)),
            field!("authorization_declarations", |s| field_json(&s.authorization_declarations)),
            field!("paymasters", |s| field_json(&s.paymasters)),
            field!("feature_schedule_extensions", |s| field_json(
                &s.feature_schedule_extensions
            )),
            field!("station_owner_ledger", |s| field_json(
                &s.station_owner_ledger
            )),
            field!("station_weather_accepted_block_unix", |s| field_json(
                &s.station_weather_accepted_block_unix
            )),
            field!("signing_keys", |s| field_json(&s.signing_keys)),
            field!("calibration_attestations", |s| field_json(
                &s.calibration_attestations
            )),
            field!("station_registry", |s| field_json(&s.station_registry)),
            field!("station_counts", |s| field_json(&s.station_counts)),
            field!("recent_weather_observations", |s| field_json(
                &s.recent_weather_observations
            )),
            field!("spatial_scores", |s| field_json(&s.spatial_scores)),
            field!("oracle_local_accuracy", |s| field_json(
                &s.oracle_local_accuracy
            )),
            field!("oracle_registry", |s| field_json(s.oracle_registry())),
            field!("zk_verified_proofs", |s| field_json(&s.zk_verified_proofs)),
            field!("slashing_reason_counts", |s| field_json(
                &s.slashing_reason_counts
            )),
            field!("slashing_evidence_seen", |s| field_json(
                &s.slashing_evidence_seen
            )),
            field!("gamification_claims_seen", |s| field_json(
                &s.gamification_claims_seen
            )),
            field!("governance", |s| field_json(&s.governance)),
            // Lives in the private `commit_reveal_state` but IS hashed into the
            // state_root (the `cal_bindings` section). Exposed via accessor so
            // this consensus-relevant map shows up in the diff (#1682).
            field!("calibration_cid_bindings", |s| field_json(
                s.calibration_cid_bindings()
            )),
            // ADR-003 attester settlement (#2846), added to the guardrail in
            // #2891 AFTER parity was established — not before: see
            // `producer_extractor_and_wal_agree_on_attester_settlement_fields_2891`
            // (transition_extractor), the `attester_settlement_*_2846`
            // producer-path scenarios, and supply-bound obligation (f).
            field!("attester_settlement_due", |s| field_json(
                &s.attester_settlement_due
            )),
            field!("attester_settlements", |s| field_json(
                &s.attester_settlements
            )),
            // Array-keyed maps cannot serialize as JSON objects. Compare the
            // exact canonical registry body plus its durable Option marker.
            field!("node_bond_registry", |s| field_json(
                &s.node_bond_registry.as_ref().map(|r| r.canonical_body())
            )),
            field!("node_fraud", |s| field_json(
                &s.node_fraud.as_ref().map(|r| r.canonical_body())
            )),
            field!("node_settlement", |s| field_json(
                &s.node_settlement.as_ref().map(|r| r.canonical_body())
            )),
            field!("node_receipt_registry", |s| field_json(
                &s.node_receipt_registry.as_ref().map(|r| r.canonical_body())
            )),
        ]
    }

    fn diff_state_field_names() -> Vec<&'static str> {
        let empty = wrfcoin_native_chain::blockchain::state::State::new();
        state_diff_fields(&empty, &empty)
            .into_iter()
            .map(|(name, _, _)| name)
            .collect()
    }

    /// Compare every consensus-relevant public field of two `State`s and return
    /// the names of those that differ, printing a short diff for each.
    pub(crate) fn diff_states(
        producer: &wrfcoin_native_chain::blockchain::state::State,
        applier: &wrfcoin_native_chain::blockchain::state::State,
    ) -> Vec<String> {
        let pairs = state_diff_fields(producer, applier);

        let mut divergent = Vec::new();
        for (name, prod, appl) in pairs {
            if prod != appl {
                divergent.push(name.to_string());
                let prod_trunc: String = prod.chars().take(400).collect();
                let appl_trunc: String = appl.chars().take(400).collect();
                eprintln!("  DIVERGENT FIELD: {name}");
                eprintln!("    producer: {prod_trunc}");
                eprintln!("    applier : {appl_trunc}");
            }
        }
        divergent
    }

    // Pure signing/setup helpers from the existing S2 synthetic fixture. No
    // process-global feature changes run outside the isolated child below.
    mod node_bond_parity_fixture {
        use wrfcoin_shared_protocol::{
            blockchain::state::{State, StateTransition},
            common::types::{Address, Balance},
            consensus::{node_bond_registry::*, node_share_contracts::*},
            crypto::{
                chain_binding::{ChainIdentity, SigningProtocolVersion},
                domain_signing::{DomainSeparated, sign_domain_for},
                signature::{PrivateKey, PublicKey},
            },
            governance::schedule_extension::{
                AppliedExtension, ScheduleEntry, install_schedule_extension_authority_for_tests,
            },
        };
        pub const A: u64 = 3;
        fn key(byte: u8) -> PrivateKey {
            PrivateKey::from_bytes(&[byte; 32]).unwrap()
        }
        fn pk(byte: u8) -> [u8; 32] {
            PublicKey::from_private_key(&key(byte))
                .as_bytes()
                .try_into()
                .unwrap()
        }
        pub fn address(byte: u8) -> Address {
            Address::from_public_key(&PublicKey::from_private_key(&key(byte)))
        }
        fn signature<T: DomainSeparated>(subject: &T, byte: u8) -> Vec<u8> {
            sign_domain_for(
                &key(byte),
                subject,
                &ChainIdentity::new("777", "testnet"),
                SigningProtocolVersion::V2,
            )
            .as_bytes()
            .to_vec()
        }
        pub fn auth(
            owner: u8,
            id: u8,
            nonce: u64,
            action: NodeBondAction,
            epoch: u64,
        ) -> NodeBondLifecycle {
            let a = NodeBondAuthorization {
                genesis_hash: [7; 32],
                deployment_id: [id; 32],
                owner_account: address(owner),
                escrow_account: node_bond_escrow_address(&[id; 32]),
                payout_account: address(owner),
                server_key: pk(owner + 1),
                consumer_key: pk(owner + 2),
                service_alias: format!("node-{id}"),
                action,
                amount_units: if action == NodeBondAction::Deposit
                    || action == NodeBondAction::Withdraw
                {
                    NODE_BOND_UNITS
                } else {
                    0
                },
                expected_nonce: nonce,
                effective_epoch: epoch,
            };
            let online = matches!(action, NodeBondAction::Deposit | NodeBondAction::Update);
            let digest = node_contract_digest(&a, &ChainIdentity::new("777", "testnet"));
            let signed = SignedNodeBondAuthorization {
                owner_signature: signature(&a, owner),
                server_signature: if online {
                    signature(
                        &NodeBondPossession {
                            authorization_preimage_hash: digest,
                            role: NodeBondKeyRole::Server,
                        },
                        owner + 1,
                    )
                } else {
                    vec![]
                },
                consumer_signature: if online {
                    signature(
                        &NodeBondPossession {
                            authorization_preimage_hash: digest,
                            role: NodeBondKeyRole::Consumer,
                        },
                        owner + 2,
                    )
                } else {
                    vec![]
                },
                authorization: a.clone(),
            };
            NodeBondLifecycle {
                signed,
                owner_key: pk(owner),
                governance_key: online.then(|| pk(99)),
                governance_signature: if online { signature(&a, 99) } else { vec![] },
                payout_key: online.then(|| pk(owner)),
                payout_signature: if online { signature(&a, owner) } else { vec![] },
            }
        }
        pub fn state() -> State {
            // Identical synthetic schedule for every fixture, including parallel tests.
            use wrfcoin_shared_protocol::crypto::chain_binding::{
                FeatureSchedule, install_feature_schedule,
            };
            install_feature_schedule(
                FeatureSchedule::from_entries(vec![
                    ("node_share_settlement".into(), A),
                    ("account_authorization_dispatch".into(), A),
                ])
                .unwrap(),
            );
            install_schedule_extension_authority_for_tests(
                "777",
                "testnet",
                &hex::encode([7; 32]),
                vec![hex::encode(pk(99))],
            );
            let mut s = State::new();
            // Persist the synthetic chain identity before activation, as a real
            // genesis/checkpoint does; ambient signing identity is not authority.
            s.genesis_emission_config = Some(wrfcoin_shared_protocol::crypto::chain_binding::GenesisEmissionConfig {
                chain_id: wrfcoin_shared_protocol::governance::schedule_extension::schedule_extension_authority().unwrap().context().chain_id.to_string(),
                faucet_address: None,
             ..Default::default() });
            s.feature_schedule_extensions.push(AppliedExtension {
                sequence: 0,
                carried_at_height: 0,
                entries: vec![ScheduleEntry::new("node_share_settlement", A)],
                version_steps: vec![],
                min_protocol_version: 2,
                release_commit: "test-only-explicit-schedule".into(),
                signers: vec![hex::encode(pk(99))],
            });
            s.create_account(address(10), Balance::new(u128::from(NODE_BOND_UNITS) * 3));
            s.create_account(address(20), Balance::new(u128::from(NODE_BOND_UNITS) * 3));
            s.apply_transition(StateTransition::BeginNodeServiceBlock {
                block_height: A,
                parent_hash: [2; 32],
            })
            .unwrap();
            s
        }
        pub fn transaction(
            p: &NodeBondLifecycle,
            fee: u64,
        ) -> wrfcoin_shared_protocol::blockchain::transaction::Transaction {
            use wrfcoin_shared_protocol::blockchain::transaction::{
                Transaction, TransactionInput, TransactionOutput, TransactionType,
            };
            let a = &p.signed.authorization;
            let mut tx = Transaction::new(
                vec![TransactionInput {
                    txid: wrfcoin_shared_protocol::crypto::hash::hash_bytes(
                        b"synthetic-node-input",
                    ),
                    vout: 0,
                    signature: None,
                    public_key: Some(hex::encode(p.owner_key)),
                    coinbase_data: None,
                }],
                vec![TransactionOutput {
                    address: a.owner_account.to_string(),
                    amount: 0,
                    script: "OP_TRUE".into(),
                    nft_data: None,
                }],
            );
            tx.tx_type = TransactionType::NodeBondLifecycle;
            tx.sender_address = a.owner_account;
            tx.nonce = a.expected_nonce;
            tx.fee = fee;
            tx.data = Some(serde_json::to_string(p).unwrap());
            tx.set_id();
            tx
        }
    }

    #[test]
    fn node_lifecycle_gossip_uses_canonical_height_and_fails_closed() {
        const CHILD: &str = "WRFCOIN_NODE_GOSSIP_S6_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "p2p_events::tests::node_lifecycle_gossip_uses_canonical_height_and_fails_closed", "--nocapture"])
                .env(CHILD, "1").env("WRFCOIN_CHAIN_ID", "777").env("WRFCOIN_NETWORK_ID", "testnet")
                .status().unwrap();
            assert!(status.success(), "gossip control child must execute and pass");
            return;
        }
        use node_bond_parity_fixture as f;
        use super::{GossipIngest, ingest_gossiped_transaction_at_height};
        use wrfcoin_shared_protocol::consensus::node_share_contracts::NodeBondAction;
        let source = include_str!("p2p_events.rs");
        let ingress = source.split("P2pEvent::NewTransactions(tx_bytes_vec) => {").nth(1).unwrap().split("P2pEvent::PeerConnected").next().unwrap();
        assert!(ingress.contains("ingest_gossiped_transaction_at_height(&mut mp, tx, &signing_keys, Some(height))"), "actual P2P ingress must pass canonical height to the exercised helper");
        let _state = f::state();
        let tx = f::transaction(&f::auth(10, 1, 0, NodeBondAction::Deposit, 1), 1_000_000);
        let keys = wrfcoin_native_chain::security::key_registry::SigningKeyRegistry::default();
        let mut unknown = Mempool::new(&Config::default());
        assert_eq!(ingest_gossiped_transaction_at_height(&mut unknown, tx.clone(), &keys, None), GossipIngest::Rejected, "unknown height must not bypass activation");
        let mut off = Mempool::new(&Config::default());
        assert_eq!(ingest_gossiped_transaction_at_height(&mut off, tx.clone(), &keys, Some(f::A - 1)), GossipIngest::Rejected, "pre-A gossip must be refused");
        let mut on = Mempool::new(&Config::default());
        assert_eq!(ingest_gossiped_transaction_at_height(&mut on, tx.clone(), &keys, Some(f::A)), GossipIngest::Admitted, "funded signed lifecycle gossip at A must be admitted");
        let mut forged = tx;
        let mut payload = wrfcoin_shared_protocol::consensus::node_bond_registry::NodeBondLifecycle::parse(&forged).unwrap();
        payload.signed.owner_signature[0] ^= 1;
        forged.data = Some(serde_json::to_string(&payload).unwrap());
        forged.set_id();
        let mut invalid = Mempool::new(&Config::default());
        assert_eq!(ingest_gossiped_transaction_at_height(&mut invalid, forged, &keys, Some(f::A)), GossipIngest::Rejected, "canonical height cannot authorize forged gossip");
    }

    /// #3591: a block carrying a node-bond Deposit AND a low-quality attested
    /// ContributionReveal. Feature off, the extractor's streak replay derives no
    /// streak transitions for it (today's behaviour). Feature on, the replay
    /// begins the node-service block first, the Deposit block derives the same
    /// streak transitions as an identical block without the Deposit, and
    /// producer, P2P applier, independent extraction and persisted WAL replay
    /// agree.
    ///
    /// Each mode runs in its own child process: the feature schedule is
    /// process-wide.
    #[test]
    fn streak_replay_node_service_3591() {
        const CHILD: &str = "WRFCOIN_STREAK_REPLAY_3591_CHILD";
        const NAME: &str = "p2p_events::tests::streak_replay_node_service_3591";
        let Some(mode) = std::env::var_os(CHILD) else {
            for mode in ["off", "on"] {
                let out = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", NAME, "--nocapture"])
                    .env(CHILD, mode)
                    .env("WRFCOIN_CHAIN_ID", "777")
                    .env("WRFCOIN_NETWORK_ID", "testnet")
                    .output()
                    .expect("isolated #3591 child");
                let stdout = String::from_utf8_lossy(&out.stdout);
                assert!(
                    out.status.success() && stdout.contains("1 passed"),
                    "#3591 child ({mode}) failed:\n{stdout}\n{}",
                    String::from_utf8_lossy(&out.stderr)
                );
            }
            return;
        };
        let feature_on = mode == "on";
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                use node_bond_parity_fixture as f;
                use wrfcoin_shared_protocol::crypto::chain_binding::{
                    ConsensusFeature, feature_active, refresh_feature_schedule_extensions,
                };
                let _schedule = f::state(); // installs the synthetic node-share schedule
                if feature_on {
                    refresh_feature_schedule_extensions(vec![(
                        "streak_replay_node_service".to_string(),
                        f::A,
                    )]);
                }
                assert_eq!(
                    feature_active(ConsensusFeature::StreakReplayNodeService, f::A + 1),
                    feature_on
                );
                let with_deposit = mine_low_quality_block_3591(true, feature_on).await;
                let without_deposit = mine_low_quality_block_3591(false, feature_on).await;
                let streaks = |t: &[StateTransition3591]| -> Vec<(String, Option<u32>)> {
                    t.iter()
                        .filter_map(|t| match t {
                            StateTransition3591::SetLowQualityStreak { address, streak } => {
                                Some((address.to_string(), *streak))
                            },
                            _ => None,
                        })
                        .collect()
                };
                // The control block (no node-service tx) derives the streak in both modes.
                assert!(
                    !streaks(&without_deposit.extracted).is_empty(),
                    "control: a low-quality attested reveal derives a streak transition"
                );
                assert!(without_deposit.applier_agrees, "control: producer and applier agree");
                assert_eq!(
                    without_deposit.producer_streak,
                    Some(1),
                    "control: the producer counted one low-quality reveal"
                );
                if feature_on {
                    assert_eq!(with_deposit.producer_streak, Some(1), "gate on: producer streak");
                    assert_eq!(
                        streaks(&with_deposit.extracted),
                        streaks(&without_deposit.extracted),
                        "gate on: the Deposit block derives the same streak transitions"
                    );
                    assert!(with_deposit.applier_agrees, "gate on: producer and applier agree");
                    assert!(with_deposit.extractor_agrees, "gate on: extractor root agrees");
                    assert!(with_deposit.wal_agrees, "gate on: WAL replay root agrees");
                } else {
                    assert!(
                        streaks(&with_deposit.extracted).is_empty(),
                        "gate off: today's replay derives nothing on a node-service block"
                    );
                }
            });
    }

    use wrfcoin_native_chain::blockchain::state::StateTransition as StateTransition3591;

    struct LowQualityBlock3591 {
        extracted: Vec<StateTransition3591>,
        producer_streak: Option<u32>,
        applier_agrees: bool,
        extractor_agrees: bool,
        wal_agrees: bool,
    }

    /// Producer at `A`, then block `A + 1` with a low-quality (0.20) attested
    /// reveal, optionally plus a signed node-bond Deposit.
    async fn mine_low_quality_block_3591(with_deposit: bool, feature_on: bool) -> LowQualityBlock3591 {
        use node_bond_parity_fixture as f;
        use wrfcoin_native_chain::blockchain::transaction::{
            CommitPayload, OracleAttestationPayload, RevealPayload, TransactionInput,
            TransactionOutput,
        };
        use wrfcoin_native_chain::crypto::hash::hash_string;
        use wrfcoin_native_chain::crypto::signature::{PrivateKey, PublicKey};
        use wrfcoin_shared_protocol::common::types::{Address, Balance};
        use wrfcoin_shared_protocol::consensus::node_share_contracts::NodeBondAction;
        use wrfcoin_shared_protocol::station_registry::{
            ContributionProvenanceClass, DataQualityTier, StationLocation, WeatherStation,
        };
        fn oracle(seed: u64) -> (PrivateKey, String) {
            use rand::SeedableRng;
            let sk = PrivateKey::generate(&mut rand::rngs::StdRng::seed_from_u64(seed));
            let id = PublicKey::from_private_key(&sk).to_string();
            (sk, id)
        }
        let (o1_sk, o1_id) = oracle(901);
        let (o2_sk, o2_id) = oracle(902);
        let recipient = Address([0x77; 20]);
        let station_id = "station-3591";

        let mut initial = f::state();
        initial.node_bond_registry = None;
        initial.node_receipt_registry = None;
        initial.node_settlement = None;
        initial.node_fraud = None;
        if feature_on {
            // Scheduled the way an applied #3277 extension schedules it: the
            // node refreshes the process overlay from these state entries on
            // every applied block, so a test-only overlay would be wiped.
            initial.feature_schedule_extensions[0].entries.push(
                wrfcoin_shared_protocol::governance::schedule_extension::ScheduleEntry::new(
                    "streak_replay_node_service",
                    f::A,
                ),
            );
        }
        initial
            .apply_transition(StateTransition3591::SetOracleConfig {
                oracle_set: vec![o1_id.clone(), o2_id.clone()],
                oracle_quorum: 2,
            })
            .unwrap();
        initial.create_account(recipient, Balance::new(0));
        initial
            .register_weather_station(WeatherStation {
                station_id: station_id.to_string(),
                owner_address: recipient.to_string(),
                location: StationLocation {
                    latitude: 10.5,
                    longitude: 20.5,
                    elevation_meters: Some(220.0),
                },
                hardware_model: "parity-fixture".to_string(),
                calibration_date: None,
                is_active: true,
                reputation_score: 50,
                data_quality_tier: DataQualityTier::Gold,
                provenance_class: ContributionProvenanceClass::Real,
                ring_public_key: None,
            })
            .unwrap();
        initial.recalculate_state_root();
        let mut genesis = Blockchain::default();
        extend_chain(&mut genesis, f::A - 1, 3591);
        genesis.state = initial.clone();
        let producer = Arc::new(RwLock::new(genesis.clone()));
        let applier = Arc::new(RwLock::new(genesis));
        let consensus = test_consensus();
        let peer_consensus = test_consensus();
        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(RwLock::new(
            Storage::open(&dir.path().join("streak-3591.redb"), 10_000).unwrap(),
        ));
        store.write().await.force_checkpoint(&initial, f::A - 1).await.unwrap();
        let mut fork = ForkCandidateBuffer::new();
        let mut orphan = OrphanBlockBuffer::new();
        let mut extracted_state = initial.clone();
        let mut wal = initial.clone();

        let mut out = LowQualityBlock3591 {
            extracted: Vec::new(),
            producer_streak: None,
            applier_agrees: false,
            extractor_agrees: false,
            wal_agrees: false,
        };
        for height in [f::A, f::A + 1] {
            if height == f::A + 1 {
                let timestamp: i64 = chrono::Utc::now().timestamp() - 5;
                let metrics = serde_json::json!({"quality": 0.20, "volume": 1.0, "uniqueness": 1.0});
                let nonce = "nonce-3591".to_string();
                let commit_id = Transaction::compute_commit_id(
                    &metrics, &nonce, station_id, timestamp, "contribution",
                );
                let tx_ts = chrono::DateTime::from_timestamp(timestamp, 0).unwrap();
                let mut commit_payload = CommitPayload {
                    commit_id: commit_id.clone(),
                    station_id: station_id.to_string(),
                    timestamp,
                    contribution_type: "contribution".to_string(),
                    auth_pubkey: None,
                    auth_signature: None,
                };
                commit_payload.sign_authorization(&oracle(903).0);
                let reveal_payload = RevealPayload {
                    commit_id: commit_id.clone(),
                    station_id: station_id.to_string(),
                    timestamp,
                    contribution_type: "contribution".to_string(),
                    data: metrics,
                    nonce,
                    data_cid: None,
                    data_hash: None,
                    storage_provider: None,
                    provenance: None,
                    manifest: None,
                    verification_receipt: None,
                    compute_receipt: None,
                    quality_tier: None,
                };
                let mk = |tx_type: TransactionType, tag: &str, data: String| {
                    let mut tx = Transaction::new(
                        vec![TransactionInput {
                            txid: hash_string(&format!("in-3591-{tag}")),
                            vout: 0,
                            signature: None,
                            public_key: None,
                            coinbase_data: None,
                        }],
                        vec![TransactionOutput {
                            amount: 1,
                            address: format!("0x{}", hex::encode(recipient.0)),
                            script: "OP_TRUE".to_string(),
                            nft_data: None,
                        }],
                    );
                    tx.tx_type = tx_type;
                    tx.timestamp = tx_ts;
                    tx.data = Some(data);
                    tx.set_id();
                    tx
                };
                let commit_tx = mk(
                    TransactionType::ContributionCommit,
                    "commit",
                    serde_json::to_string(&commit_payload).unwrap(),
                );
                let mut reveal_tx = mk(
                    TransactionType::ContributionReveal,
                    "reveal",
                    serde_json::to_string(&reveal_payload).unwrap(),
                );
                reveal_tx.sender_address = recipient;
                reveal_tx.fee = 1;
                reveal_tx.set_id();
                let attestation = |sk: &PrivateKey, oracle_id: &str, tag: &str| {
                    let mut payload = OracleAttestationPayload {
                        commit_id: commit_id.clone(),
                        oracle_id: oracle_id.to_string(),
                        approved: true,
                        signature: None,
                        confidence: Some(1.0),
                        timestamp,
                        nonce: Some(format!("nonce-3591-{tag}")),
                    };
                    payload.signature = Some(sk.sign(payload.signing_message().as_bytes()).to_string());
                    mk(TransactionType::OracleAttestation, tag, serde_json::to_string(&payload).unwrap())
                };
                let mut keys = wrfcoin_shared_protocol::security::key_registry::SigningKeyRegistry::default();
                for id in [&o1_id, &o2_id] {
                    keys.ensure_legacy_identity_key(
                        id,
                        wrfcoin_shared_protocol::security::key_registry::SigningRole::Oracle,
                        0,
                    )
                    .unwrap();
                }
                let mut mp = mempool.write().await;
                for tx in [
                    commit_tx,
                    reveal_tx,
                    attestation(&o1_sk, &o1_id, "att1"),
                    attestation(&o2_sk, &o2_id, "att2"),
                ] {
                    mp.add_transaction_with_signing_keys(tx, Some(&keys)).expect("pop tx admitted");
                }
                if with_deposit {
                    let deposit = f::transaction(&f::auth(10, 1, 0, NodeBondAction::Deposit, 1), 100_000);
                    mp.add_transaction_with_signing_keys_at_height(deposit, None, Some(height - 1))
                        .expect("signed Deposit admitted");
                }
            }
            let before = extracted_state.clone();
            let block = consensus
                .create_pop_block_with_state(
                    Arc::clone(&producer),
                    Arc::clone(&mempool),
                    &test_address_string(0x61),
                )
                .await
                .unwrap_or_else(|e| panic!("producer height {height}: {e}"));
            assert_eq!(block.height, height);
            if height == f::A + 1 {
                let count = |ty: TransactionType| block.transactions.iter().filter(|t| t.tx_type == ty).count();
                assert_eq!(count(TransactionType::ContributionReveal), 1, "low-quality reveal mined");
                assert_eq!(count(TransactionType::NodeBondLifecycle), usize::from(with_deposit), "Deposit mined");
            }
            let wire: Block = bincode::deserialize(&bincode::serialize(&block).unwrap()).unwrap();
            apply_p2p_block(
                wire,
                P2pBlockSource::HistoricalSync,
                &applier,
                &store,
                &peer_consensus,
                None,
                &mut fork,
                &mut orphan,
            )
            .await;
            let p = producer.read().await;
            let a = applier.read().await;
            let inputs = extract_transitions(&before, &block, height, block.timestamp.timestamp());
            extracted_state
                .apply_transitions_batch_at_height(height, inputs.clone())
                .unwrap();
            extracted_state.apply_block_finalization_checked(&block).unwrap();
            let rows = store.read().await.load_wal_transitions_range(height, height).unwrap();
            let wal_ok = if rows.is_empty() {
                false
            } else {
                wal.apply_transitions_batch_at_height(height, rows.into_iter().map(|(_, t)| t)).unwrap();
                wal.apply_block_finalization_checked(&block).is_ok() && wal.state_root == p.state.state_root
            };
            let applier_ok = a.height == height && a.state.state_root == p.state.state_root;
            if height == f::A {
                assert!(applier_ok && wal_ok, "the empty activation block agrees everywhere");
                assert_eq!(extracted_state.state_root, p.state.state_root);
            } else {
                out = LowQualityBlock3591 {
                    extracted: inputs,
                    producer_streak: p.state.low_quality_streaks.get(&recipient).copied(),
                    applier_agrees: applier_ok,
                    extractor_agrees: extracted_state.state_root == p.state.state_root,
                    wal_agrees: wal_ok,
                };
            }
        }
        out
    }

    /// Cross the durable None -> Some checkpoint marker, a real signed bond
    /// deposit, next-epoch frozen eligibility and signed exit through mining,
    /// actual P2P apply, independent extraction and persisted WAL replay.
    #[test]
    fn producer_applier_and_wal_agree_on_node_bond_lifecycle_1682() {
        const CHILD: &str = "WRFCOIN_NODE_BOND_PARITY_1682_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "p2p_events::tests::producer_applier_and_wal_agree_on_node_bond_lifecycle_1682",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .env("WRFCOIN_CHAIN_ID", "777")
                .env("WRFCOIN_NETWORK_ID", "testnet")
                .status()
                .expect("isolated node-bond parity child");
            assert!(
                status.success(),
                "node-bond producer/applier parity child failed"
            );
            return;
        }
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                use node_bond_parity_fixture as f;
                use wrfcoin_native_chain::blockchain::state::State;
                use wrfcoin_shared_protocol::consensus::{
                    node_bond_registry::*, node_share_contracts::*,
                };
                let mut initial = f::state();
                // Seed a pre-activation historical checkpoint; this is an
                // activation/replay test, not a new genesis ceremony. The
                // producer itself must create the durable Some marker at A.
                initial.node_bond_registry = None;
                initial.node_receipt_registry = None;
                initial.node_settlement = None;
                initial.node_fraud = None;
                initial.recalculate_state_root();
                let mut genesis = Blockchain::default();
                extend_chain(&mut genesis, f::A - 1, 1682);
                genesis.state = initial.clone();
                assert!(genesis.state.node_bond_registry.is_none());
                let producer = Arc::new(RwLock::new(genesis.clone()));
                let applier = Arc::new(RwLock::new(genesis));
                let consensus = test_consensus();
                let peer_consensus = test_consensus();
                let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
                let dir = tempfile::tempdir().unwrap();
                let store = Arc::new(RwLock::new(
                    Storage::open(&dir.path().join("parity.redb"), 10_000).unwrap(),
                ));
                store
                    .write()
                    .await
                    .force_checkpoint(&initial, f::A - 1)
                    .await
                    .unwrap();
                let mut fork = ForkCandidateBuffer::new();
                let mut orphan = OrphanBlockBuffer::new();
                let mut extracted = initial.clone();
                let mut wal = initial.clone();
                let boundary = f::A + NODE_EPOCH_BLOCKS;
                let mut carried = 0;
                for height in f::A..=boundary + 1 {
                    let action = if height == f::A + 1 {
                        Some((NodeBondAction::Deposit, 0, 1))
                    } else if height == boundary + 1 {
                        Some((NodeBondAction::Exit, 1, 2))
                    } else {
                        None
                    };
                    if let Some((action, nonce, epoch)) = action {
                        let tx = f::transaction(&f::auth(10, 1, nonce, action, epoch), 100_000);
                        mempool
                            .write()
                            .await
                            .add_transaction_with_signing_keys_at_height(tx, None, Some(height - 1))
                            .expect("signed lifecycle admitted");
                    }
                    let block = consensus
                        .create_pop_block_with_state(
                            Arc::clone(&producer),
                            Arc::clone(&mempool),
                            &test_address_string(0x61),
                        )
                        .await
                        .unwrap_or_else(|e| panic!("producer height {height}: {e}"));
                    assert_eq!(block.height, height);
                    if action.is_some() {
                        assert_eq!(
                            block
                                .transactions
                                .iter()
                                .filter(|t| t.tx_type == TransactionType::NodeBondLifecycle)
                                .count(),
                            1,
                            "lifecycle actually mined"
                        );
                        carried += 1;
                    }
                    let wire: Block =
                        bincode::deserialize(&bincode::serialize(&block).unwrap()).unwrap();
                    apply_p2p_block(
                        wire,
                        P2pBlockSource::HistoricalSync,
                        &applier,
                        &store,
                        &peer_consensus,
                        None,
                        &mut fork,
                        &mut orphan,
                    )
                    .await;
                    let p = producer.read().await;
                    let a = applier.read().await;
                    assert_eq!(a.height, height, "peer accepts actual producer block");
                    assert_eq!(
                        p.state.state_root, a.state.state_root,
                        "P2P root at {height}"
                    );
                    assert_eq!(
                        p.state.node_bond_registry, a.state.node_bond_registry,
                        "P2P registry at {height}"
                    );
                    assert!(
                        diff_states(&p.state, &a.state).is_empty(),
                        "all producer/applier material agrees"
                    );
                    let inputs = extract_transitions(
                        &extracted,
                        &block,
                        height,
                        block.timestamp.timestamp(),
                    );
                    validate_block_inputs(&block, &inputs, height >= f::A).unwrap();
                    extracted
                        .apply_transitions_batch_at_height(height, inputs)
                        .unwrap();
                    extracted.apply_block_finalization_checked(&block).unwrap();
                    assert_eq!(
                        extracted.state_root, p.state.state_root,
                        "extractor root at {height}"
                    );
                    assert_eq!(extracted.node_bond_registry, p.state.node_bond_registry);
                    let rows = store
                        .read()
                        .await
                        .load_wal_transitions_range(height, height)
                        .unwrap();
                    wal.apply_transitions_batch_at_height(height, rows.into_iter().map(|(_, t)| t))
                        .unwrap();
                    wal.apply_block_finalization_checked(&block).unwrap();
                    assert_eq!(
                        wal.state_root, p.state.state_root,
                        "persisted WAL root at {height}"
                    );
                    assert_eq!(wal.node_bond_registry, p.state.node_bond_registry);
                    if height < f::A {
                        assert!(p.state.node_bond_registry.is_none());
                    } else {
                        let registry = p.state.node_bond_registry.as_ref().unwrap();
                        assert_eq!(registry.block_height, height);
                        if height == f::A {
                            assert!(registry.registrations.is_empty(), "durable empty activation marker");
                        } else {
                            assert_eq!(registry.registrations[&[1; 32]].principal_units, NODE_BOND_UNITS);
                        }
                        if height == boundary {
                            assert_eq!(registry.frozen_epochs[&1].bindings.len(), 1);
                            assert_eq!(
                                registry.frozen_epochs[&1].parent_hash,
                                *block.prev_block_hash.as_bytes()
                            );
                        }
                    }
                    // Ensure the name-tagged diff reports BOTH the durable
                    // marker and a changed populated map, not merely a list pin.
                    if height == f::A || height == boundary {
                        let mut changed = p.state.clone();
                        if height == f::A {
                            changed.node_bond_registry = None;
                        } else {
                            changed
                                .node_bond_registry
                                .as_mut()
                                .unwrap()
                                .frozen_epochs
                                .get_mut(&1)
                                .unwrap()
                                .bindings
                                .clear();
                        }
                        changed.recalculate_state_root();
                        assert_ne!(changed.state_root_material(), p.state.state_root_material());
                        assert_eq!(diff_states(&p.state, &changed), vec!["node_bond_registry"]);
                    }
                }
                assert_eq!(carried, 2);
                assert_eq!(
                    wal.node_bond_registry.as_ref().unwrap().registrations[&[1; 32]]
                        .exit_effective_height,
                    Some(f::A + 2 * NODE_EPOCH_BLOCKS)
                );
                let loaded = store.read().await.load_state().unwrap().unwrap();
                assert_eq!(
                    loaded.state.state_root, wal.state_root,
                    "checkpoint plus persisted WAL restart"
                );
                assert_eq!(loaded.state.node_bond_registry, wal.node_bond_registry);
            });
    }

    pub(crate) async fn apply_node_receipt_block_for_test(before: Blockchain, block: &Block) -> wrfcoin_native_chain::blockchain::state::State {
        let height = before.height;
        let storage = test_storage();
        storage.write().await.force_checkpoint(&before.state, height).await.unwrap();
        let chain = Arc::new(RwLock::new(before));
        let wire: Block = bincode::deserialize(&bincode::serialize(block).unwrap()).unwrap();
        apply_p2p_block(wire, P2pBlockSource::HistoricalSync, &chain, &storage, &test_consensus(), None, &mut ForkCandidateBuffer::new(), &mut OrphanBlockBuffer::new()).await;
        assert_eq!(chain.read().await.height, block.height, "actual peer accepts Node receipt block");
        let loaded = storage.read().await.load_state();
        assert!(loaded.is_ok(), "included receipts must persist replayable Node WAL");
        let replayed = loaded.unwrap().unwrap().state;
        let applied = chain.read().await.state.clone();
        assert_eq!(replayed.state_root, applied.state_root, "actual peer's persisted WAL restarts");
        applied
    }

    #[test]
    fn producer_applier_diff_covers_state_root_material_fields_1682() {
        use std::collections::BTreeSet;
        use wrfcoin_native_chain::blockchain::state::State;

        let root_fields = State::state_root_material_field_names();
        let unique_root_fields = root_fields.iter().copied().collect::<BTreeSet<_>>();
        assert_eq!(
            unique_root_fields.len(),
            root_fields.len(),
            "state_root_material field names must be unique"
        );

        let diff_fields = diff_state_field_names()
            .into_iter()
            .collect::<BTreeSet<_>>();
        let missing = root_fields
            .iter()
            .copied()
            .filter(|name| !diff_fields.contains(name))
            .collect::<Vec<_>>();

        assert!(
            missing.is_empty(),
            "Every state_root_material section must be included in the #1682 \
             producer/applier state diff guardrail; missing {missing:?}"
        );
    }

    /// Diff two post-apply chains, print the enumeration, and assert the #1682
    /// invariants: ledger (`accounts`) parity and full `state_root` parity.
    /// `label` distinguishes the scenario in `--nocapture` output.
    async fn assert_block_apply_parity(
        label: &str,
        producer_chain: &Arc<RwLock<Blockchain>>,
        applier_chain: &Arc<RwLock<Blockchain>>,
    ) {
        let producer = producer_chain.read().await;
        let applier = applier_chain.read().await;

        // Both must have actually applied block 1.
        assert_eq!(producer.height, 1, "[{label}] producer must be at height 1");
        assert_eq!(
            applier.height, 1,
            "[{label}] applier must accept block 1 (height 0 means apply_p2p_block \
             rejected it — investigate consensus validation before reading the field diff)"
        );
        assert_eq!(
            producer.tip, applier.tip,
            "[{label}] tips must match (same block)"
        );

        eprintln!("\n===== #1682 PRODUCER/APPLIER STATE DIFF [{label}] (height 1) =====");
        eprintln!("producer state_root    : {}", producer.state.state_root);
        eprintln!("applier  state_root    : {}", applier.state.state_root);
        eprintln!("producer accounts.len(): {}", producer.state.accounts.len());
        eprintln!("applier  accounts.len(): {}", applier.state.accounts.len());
        eprintln!("producer attestations  : {}", producer.state.attestations.len());
        eprintln!("producer qpe / audit   : {} / {}",
            producer.state.quality_policy_events.len(),
            producer.state.audit_events.len());
        eprintln!("applier  qpe / audit   : {} / {}",
            applier.state.quality_policy_events.len(),
            applier.state.audit_events.len());
        eprintln!("--- field-by-field diff (empty list below == only the root hash itself differs) ---");

        let divergent = diff_states(&producer.state, &applier.state);

        if divergent.is_empty() {
            eprintln!("  (no public field diverges — divergence, if any, is in a #[serde(skip)]/private field)");
        } else {
            eprintln!("DIVERGENT FIELDS ({}): {:?}", divergent.len(), divergent);
        }

        // Byte-level bisect of the state_root pre-image. This catches divergences
        // invisible to the serde field diff (private/`#[serde(skip)]` fields, or
        // float/NaN bit-pattern differences that JSON renders identically).
        let prod_material = producer.state.state_root_material();
        let appl_material = applier.state.state_root_material();
        eprintln!(
            "--- state_root pre-image: producer {} bytes, applier {} bytes ---",
            prod_material.len(),
            appl_material.len()
        );
        if prod_material != appl_material {
            let first_diff = prod_material
                .iter()
                .zip(appl_material.iter())
                .position(|(a, b)| a != b)
                .unwrap_or(prod_material.len().min(appl_material.len()));
            let lo = first_diff.saturating_sub(24);
            let hi_p = (first_diff + 24).min(prod_material.len());
            let hi_a = (first_diff + 24).min(appl_material.len());
            eprintln!("  FIRST PRE-IMAGE DIFF at byte offset {first_diff}");
            eprintln!(
                "    producer[{lo}..{hi_p}] = {}",
                hex::encode(&prod_material[lo..hi_p])
            );
            eprintln!(
                "    applier [{lo}..{hi_a}] = {}",
                hex::encode(&appl_material[lo..hi_a])
            );
            // Show ASCII around the offset — section labels in the pre-image are
            // string keys (station ids, commit ids, etc.) which are human-readable.
            let ascii: String = prod_material[lo..hi_p]
                .iter()
                .map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { '.' })
                .collect();
            eprintln!("    producer ascii        = {ascii}");
        } else {
            // Pre-images are byte-identical. If the stored roots STILL differ,
            // one stored `state_root` is stale (computed before this block's
            // final state mutation, never recomputed). Recompute from the shared
            // pre-image to learn which stored root is authoritative — this is how
            // #1683's "producer stores a stale root" root cause was found.
            use sha3::{Digest, Sha3_256};
            let mut hasher = Sha3_256::new();
            hasher.update(&prod_material);
            let recomputed = hasher.finalize();
            let recomputed_hex = hex::encode(&recomputed[..32]);
            let prod_ok = hex::encode(producer.state.state_root.0) == recomputed_hex;
            let appl_ok = hex::encode(applier.state.state_root.0) == recomputed_hex;
            eprintln!("  pre-image is BYTE-IDENTICAL across producer/applier.");
            eprintln!("  recomputed root from shared pre-image: {recomputed_hex}");
            eprintln!("    producer stored matches recompute: {prod_ok}");
            eprintln!("    applier  stored matches recompute: {appl_ok}");
            if !prod_ok || !appl_ok {
                eprintln!(
                    "  => a stored state_root is STALE (the side printing `false` \
                     mutated state after its last recalculate_state_root())."
                );
            }
        }
        eprintln!("========================================================\n");

        // Ledger parity is the #1681 guarantee — the `accounts` field above is
        // the ledger; if it appears in `divergent` the coinbase/reward credit
        // regressed. Assert it independently so a ledger regression is
        // distinguishable from a non-account divergence.
        assert!(
            !divergent.contains(&"accounts".to_string()),
            "[{label}] LEDGER (accounts) diverged — #1681 coinbase/reward credit regressed"
        );

        // The headline #1682 invariant. With the #1683 stale-root fix this holds
        // for both scenarios; the diff above names any regressing field/section.
        assert_eq!(
            producer.state.state_root, applier.state.state_root,
            "[{label}] PRODUCER/APPLIER state_root diverged (#1682). Divergent fields: {divergent:?}"
        );
    }

    /// Apply `block` (the producer's block) to a fresh applier chain via the
    /// P2P apply path, after seeding the applier genesis identically via
    /// `seed_genesis` (mirrors a syncing peer that shares the same genesis).
    async fn apply_block_to_fresh_applier(
        block: &Block,
        seed_genesis: impl Fn(&mut Blockchain),
    ) -> Arc<RwLock<Blockchain>> {
        // Round-trip through bincode to mirror exactly what arrives over P2P.
        let block_bytes = bincode::serialize(block).expect("block serializes");
        let block_applied: Block =
            bincode::deserialize(&block_bytes).expect("block deserializes");

        let mut applier_genesis = Blockchain::default();
        seed_genesis(&mut applier_genesis);
        let applier_chain = Arc::new(RwLock::new(applier_genesis));
        let applier_storage = test_storage();
        let applier_consensus = test_consensus();
        let mut fork_buffer = ForkCandidateBuffer::new();
        let mut orphan_buffer = OrphanBlockBuffer::new();

        apply_p2p_block(
            block_applied,
            P2pBlockSource::HistoricalSync,
            &applier_chain,
            &applier_storage,
            &applier_consensus,
            None,
            &mut fork_buffer,
            &mut orphan_buffer,
        )
        .await;

        applier_chain
    }

    #[tokio::test]
    async fn producer_applier_block_apply_parity_coinbase_only() {
        // Coinbase-only block: `attested_commit_ids` is empty so
        // `process_contribution_rewards` takes the early-return path. This is
        // the path where the producer used to store a stale root (#1683).
        let producer_consensus = test_consensus();
        let producer_chain = Arc::new(RwLock::new(Blockchain::default()));
        let producer_mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));

        let block_1 = producer_consensus
            .create_pop_block_with_state(
                Arc::clone(&producer_chain),
                Arc::clone(&producer_mempool),
                &test_address_string(0x61),
            )
            .await
            .expect("producer should mine coinbase-only block via create_next_block");

        // Genesis is plain default for both sides in this scenario.
        let applier_chain = apply_block_to_fresh_applier(&block_1, |_| {}).await;

        assert_block_apply_parity("coinbase-only", &producer_chain, &applier_chain).await;
    }

    /// #2168/#2170/#2171/#2172 P2P/WAL parity while these features have no
    /// transaction-extraction path.
    ///
    /// `ApplyAccountRecovery`, `ApplySessionKeyAction`, and
    /// `ApplyPaymasterAction` are valid shared
    /// transitions, but no transaction currently causes `extract_transitions`
    /// to construct any of them. The honest lifecycle is consequently EMPTY:
    /// the producer, a second syncing P2P applier, and a third State replaying
    /// that applier's persisted WAL must retain the empty maps and the same
    /// state root. Once extraction exists, a populated-map lifecycle parity
    /// test is a scheduling blocker: do not schedule any of these features first.
    ///
    /// #3277: `feature_schedule_extensions` stays empty here because this
    /// block carries no extension transaction; the POPULATED-list parity the
    /// S2 ruling required is
    /// `producer_applier_and_wal_carry_a_populated_schedule_extension_across_activation_3277`
    /// (S4).
    #[tokio::test]
    async fn producer_applier_and_wal_keep_aa_maps_empty_2168_2170_2171_2172() {
        let producer_consensus = test_consensus();
        let producer_chain = Arc::new(RwLock::new(Blockchain::default()));
        let producer_mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let block = producer_consensus
            .create_pop_block_with_state(
                Arc::clone(&producer_chain),
                Arc::clone(&producer_mempool),
                &test_address_string(0x62),
            )
            .await
            .expect("producer must mine the parity block");

        let (
            producer_root,
            producer_recovery,
            producer_session_keys,
            producer_declarations,
            producer_paymasters,
            producer_extensions,
        ) = {
            let producer = producer_chain.read().await;
            assert!(
                producer.state.account_recovery.is_empty(),
                "vacuity guard: no recovery transaction extraction exists yet"
            );
            assert!(
                producer.state.session_keys.is_empty(),
                "vacuity guard: no session-key transaction extraction exists yet"
            );
            assert!(producer.state.authorization_declarations.is_empty(), "vacuity guard: no declaration transaction extraction exists yet");
            assert!(
                producer.state.paymasters.is_empty(),
                "vacuity guard: no paymaster transaction extraction exists yet"
            );
            assert!(
                producer.state.feature_schedule_extensions.is_empty(),
                "no extension transaction in this block (populated parity: the #3277 S4 test)"
            );
            (
                producer.state.state_root,
                producer.state.account_recovery.clone(),
                producer.state.session_keys.clone(),
                producer.state.authorization_declarations.clone(),
                producer.state.paymasters.clone(),
                producer.state.feature_schedule_extensions.clone(),
            )
        };

        // The same reconstruction used by the P2P applier below, retained so
        // the WAL leg can replay exactly the transitions the applier persists.
        let applier_seed = wrfcoin_native_chain::blockchain::state::State::new();
        let applier_transitions = extract_transitions(
            &applier_seed,
            &block,
            block.height,
            block.timestamp.timestamp(),
        );

        let storage_path = std::env::temp_dir().join(format!(
            "wrfcoin-p2p-account-recovery-2172-{}.redb",
            fastrand::u64(..)
        ));
        let _ = std::fs::remove_file(&storage_path);
        let applier_storage = Arc::new(RwLock::new(
            Storage::open(&storage_path, DEFAULT_CHECKPOINT_INTERVAL)
                .expect("applier storage opens"),
        ));
        let applier_chain = Arc::new(RwLock::new(Blockchain::default()));
        let applier_consensus = test_consensus();
        let mut fork_buffer = ForkCandidateBuffer::new();
        let mut orphan_buffer = OrphanBlockBuffer::new();
        let wire_block: Block = bincode::deserialize(
            &bincode::serialize(&block).expect("block serializes for P2P"),
        )
        .expect("block deserializes for P2P");

        apply_p2p_block(
            wire_block,
            P2pBlockSource::HistoricalSync,
            &applier_chain,
            &applier_storage,
            &applier_consensus,
            None,
            &mut fork_buffer,
            &mut orphan_buffer,
        )
        .await;

        {
            let applier = applier_chain.read().await;
            assert_eq!(applier.height, block.height, "applier must accept the block");
            assert_eq!(
                applier.state.state_root, producer_root,
                "P2P applier state_root must match the producer"
            );
            assert_eq!(
                applier.state.account_recovery, producer_recovery,
                "P2P applier must retain the producer recovery map"
            );
            assert!(applier.state.account_recovery.is_empty());
            assert_eq!(
                applier.state.session_keys, producer_session_keys,
                "P2P applier must retain the producer session-key map"
            );
            assert!(applier.state.session_keys.is_empty());
            assert_eq!(applier.state.authorization_declarations, producer_declarations);
            assert!(applier.state.authorization_declarations.is_empty());
            assert_eq!(applier.state.paymasters, producer_paymasters);
            assert!(applier.state.paymasters.is_empty());
            assert_eq!(applier.state.feature_schedule_extensions, producer_extensions);
            assert!(applier.state.feature_schedule_extensions.is_empty());
        }

        // APPLIER LEG — a distinct State applies the extracted transitions via
        // the same batch entrypoint that `apply_p2p_block` uses.
        let mut extracted_applier = applier_seed;
        extracted_applier
            .apply_transitions_batch_at_height(block.height, applier_transitions.clone())
            .expect("extracted P2P transitions must apply");
        extracted_applier
            .apply_block_finalization_checked_at(
                wrfcoin_shared_protocol::crypto::chain_binding::protocol_version_at(block.height),
                &block,
            )
            .expect("extracted P2P state must match the block commitment");
        assert_eq!(
            extracted_applier.state_root, producer_root,
            "extracted P2P applier state_root must match the producer"
        );
        assert_eq!(
            extracted_applier.account_recovery, producer_recovery,
            "extracted P2P applier must retain the producer recovery map"
        );
        assert_eq!(
            extracted_applier.session_keys, producer_session_keys,
            "extracted P2P applier must retain the producer session-key map"
        );
        assert_eq!(extracted_applier.authorization_declarations, producer_declarations);
        assert_eq!(extracted_applier.paymasters, producer_paymasters);
        assert!(extracted_applier.paymasters.is_empty());
        assert_eq!(extracted_applier.feature_schedule_extensions, producer_extensions);
        assert!(extracted_applier.feature_schedule_extensions.is_empty());

        // WAL LEG — read the rows that the real `apply_p2p_block` just wrote,
        // then replay them into a THIRD State through the shared batch path.
        let replayed = applier_storage
            .read()
            .await
            .load_wal_transitions_range(block.height, block.height)
            .expect("P2P-written WAL transitions must load");
        assert_eq!(
            replayed.len(),
            applier_transitions.len(),
            "vacuity guard: WAL must contain every transition the P2P applier extracted"
        );
        let mut wal = wrfcoin_native_chain::blockchain::state::State::new();
        wal.apply_transitions_batch_at_height(
            block.height,
            replayed.into_iter().map(|(_, transition)| transition),
        )
        .expect("P2P-written WAL batch must apply");
        wal.apply_block_finalization_checked_at(
            wrfcoin_shared_protocol::crypto::chain_binding::protocol_version_at(block.height),
            &block,
        )
        .expect("WAL-replayed state must match the block commitment");
        assert_eq!(wal.state_root, producer_root, "WAL replay state_root must match the producer");
        assert_eq!(
            wal.account_recovery, producer_recovery,
            "WAL replay must retain the producer recovery map"
        );
        assert!(wal.account_recovery.is_empty());
        assert_eq!(
            wal.session_keys, producer_session_keys,
            "WAL replay must retain the producer session-key map"
        );
        assert!(wal.session_keys.is_empty());
        assert_eq!(wal.authorization_declarations, producer_declarations);
        assert!(wal.authorization_declarations.is_empty());
        assert_eq!(wal.paymasters, producer_paymasters);
        assert!(wal.paymasters.is_empty());
        assert_eq!(wal.feature_schedule_extensions, producer_extensions);
        assert!(wal.feature_schedule_extensions.is_empty());
        drop(applier_chain);
        drop(applier_storage);
        let _ = std::fs::remove_file(&storage_path);
    }

    /// #3277 S4 test obligation shared by the extension child tests: the V1
    /// gate and the authority, installed as a node installs them at boot (the
    /// base schedule from block 1, the D1 pin), here as test values. First
    /// install wins; each child is its own process.
    fn install_schedule_extension_gate_3277(h_enable: u64) -> wrfcoin_shared_protocol::crypto::signature::PrivateKey {
        use rand::SeedableRng;
        use wrfcoin_shared_protocol::crypto::chain_binding::{
            install_activation_table, install_feature_schedule, resolve_activation_schedule,
            resolve_feature_schedule, SigningProtocolVersion, GENESIS_ACTIVATION_SCHEDULE_KEY,
        };
        use wrfcoin_shared_protocol::governance::schedule_extension::{
            install_schedule_extension_authority_for_tests, SCHEDULE_EXTENSION_FEATURE,
        };
        let governance = wrfcoin_shared_protocol::crypto::signature::PrivateKey::generate(
            &mut rand::rngs::StdRng::seed_from_u64(3277),
        );
        let raw = format!("0:v1,{h_enable}:{SCHEDULE_EXTENSION_FEATURE}");
        let pairs = vec![(GENESIS_ACTIVATION_SCHEDULE_KEY, raw.as_str())];
        let table = resolve_activation_schedule(pairs.clone(), SigningProtocolVersion::V1)
            .expect("activation table");
        let features = resolve_feature_schedule(pairs, &table).expect("feature schedule");
        SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
        install_activation_table(table);
        install_feature_schedule(features);
        install_schedule_extension_authority_for_tests("777", "testnet", &"ab".repeat(32), vec![wrfcoin_shared_protocol::crypto::signature::PublicKey::from_private_key(&governance)
                .to_hex()
                .to_ascii_lowercase()]);
        governance
    }

    fn signed_schedule_extension_3277(
        governance: &wrfcoin_shared_protocol::crypto::signature::PrivateKey,
        entries: &[(&str, u64)],
    ) -> wrfcoin_shared_protocol::governance::schedule_extension::SignedFeatureScheduleExtension {
        use wrfcoin_shared_protocol::governance::schedule_extension::{
            sign_feature_schedule_extension_with_key, FeatureScheduleExtension, ScheduleEntry,
            SignedFeatureScheduleExtension,
        };
        let extension = FeatureScheduleExtension {
            chain_id: "777".into(),
            network_id: "testnet".into(),
            genesis_hash: "ab".repeat(32),
            sequence: 0,
            entries: entries.iter().map(|(n, h)| ScheduleEntry::new(*n, *h)).collect(),
            version_steps: Vec::new(),
            min_protocol_version: 3,
            release_commit: "s4-test-release".into(),
        };
        let signatures = vec![sign_feature_schedule_extension_with_key(&extension, governance)
            .expect("governance signs a non-held extension")];
        SignedFeatureScheduleExtension { extension, signatures }
    }

    /// #3277 S4 — the ruling's populated-list producer/applier/WAL parity
    /// test, plus IBD and restart ACROSS the activation (design T1 end to end
    /// with a mined block; review-s2 "S4 test obligation").
    ///
    /// A REAL producer (`create_pop_block_with_state`) mines a governance
    /// signed extension out of its mempool into block 1, scheduling
    /// `verify_unit_pricing` at `1 + MIN_RUNUP`, then mines on past that
    /// activation. A syncing node applies every block through the real
    /// `apply_p2p_block` (`HistoricalSync`, the IBD source): at EVERY height
    /// its `state_root` and `feature_schedule_extensions` equal the
    /// producer's, and `feature_active` flips exactly at the activation. The
    /// WAL leg reopens the applier's storage and replays it (checkpoint
    /// anchor + WAL tail) under the refreshing observer, reaching the
    /// producer's root and the same effective schedule.
    ///
    /// The gate is a test base schedule standing in for the block-1-keyed
    /// bootstrap (`LEGACY_CHAIN_BOOTSTRAP`, S5), which only the live chain's
    /// block-1 hash selects. `schedule_extension` is a known feature from S5
    /// on, so S3's refresh inside `apply_p2p_block` installs no deadline for
    /// the gate itself; the unknown-feature halt is pinned by
    /// `a_peer_block_carrying_an_unknown_feature_is_valid_and_halts_cleanly_3277`.
    ///
    /// The producer runs on a fixed clock in the past so ~2000 blocks carry
    /// past timestamps, as a real IBD does (a live clock would outrun the
    /// applier's 15 s future-drift bound). Own process (re-exec child): the
    /// schedule, authority and overlay are process-wide.
    #[tokio::test]
    async fn producer_applier_and_wal_carry_a_populated_schedule_extension_across_activation_3277() {
        const CHILD: &str = "WRFCOIN_SCHEDULE_EXTENSION_PARITY_3277_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "p2p_events::tests::producer_applier_and_wal_carry_a_populated_schedule_extension_across_activation_3277",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .status()
                .expect("re-exec child");
            assert!(status.success(), "isolated schedule-extension parity child failed");
            return;
        }
        use wrfcoin_shared_protocol::consensus::proof_of_publishing::{Clock, ProofOfPublishing};
        use wrfcoin_shared_protocol::crypto::chain_binding::{
            feature_active, feature_schedule_extension_entries, ConsensusFeature,
        };
        use wrfcoin_shared_protocol::governance::schedule_extension::MIN_RUNUP_BLOCKS;

        const H_ENABLE: u64 = 1;
        const CARRY: u64 = 1;
        const ACTIVATION: u64 = CARRY + MIN_RUNUP_BLOCKS;
        const TIP: u64 = ACTIVATION + 1;
        let governance = install_schedule_extension_gate_3277(H_ENABLE);
        let feature = ConsensusFeature::VerifyUnitPricing;
        let signed = signed_schedule_extension_3277(&governance, &[(feature.name(), ACTIVATION)]);

        struct PastClock(chrono::DateTime<chrono::Utc>);
        impl Clock for PastClock {
            fn now(&self) -> chrono::DateTime<chrono::Utc> {
                self.0
            }
        }
        let pop_config = ConsensusFactory::default_config(ConsensusType::ProofOfPublishing);
        let mut pop = ProofOfPublishing::new(
            pop_config.difficulty.unwrap_or(1) as u32,
            pop_config.target_block_time,
            pop_config
                .algorithm_params
                .get("min_publication_score")
                .and_then(|v| v.as_f64())
                .map(|v| (v * 1_000_000.0).round() as u64)
                .unwrap_or(600_000),
            pop_config.max_transactions_per_block as usize,
        );
        // Block h is stamped start + h - 1: the tip lands ~400 s in the past,
        // and the extension tx (stamped now) sits inside the 3600 s
        // submission window of block 1.
        let start = chrono::Utc::now() - chrono::Duration::seconds((TIP + 400) as i64);
        pop.set_clock(Arc::new(PastClock(start)));
        let producer_adapter = wrfcoin_consensus_engines::PopConsensusAdapter::new(pop);
        producer_adapter.declare_no_genesis_oracles_blocking();
        let producer_consensus = Arc::new(TypedConsensusManager::new(producer_adapter, pop_config));
        let producer_chain = Arc::new(RwLock::new(Blockchain::default()));
        let producer_mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        producer_mempool
            .write()
            .await
            .add_transaction(Transaction::new_feature_schedule_extension(&signed))
            .expect("the mempool admits a schedulable, governance-signed extension");

        let storage_path = std::env::temp_dir().join(format!(
            "wrfcoin-p2p-schedule-extension-3277-{}.redb",
            fastrand::u64(..)
        ));
        let _ = std::fs::remove_file(&storage_path);
        let applier_storage = Arc::new(RwLock::new(
            Storage::open(&storage_path, DEFAULT_CHECKPOINT_INTERVAL).expect("applier storage opens"),
        ));
        let applier_chain = Arc::new(RwLock::new(Blockchain::default()));
        let applier_consensus = test_consensus();
        let mut fork_buffer = ForkCandidateBuffer::new();
        let mut orphan_buffer = OrphanBlockBuffer::new();

        for height in 1..=TIP {
            let block = producer_consensus
                .create_pop_block_with_state(
                    Arc::clone(&producer_chain),
                    Arc::clone(&producer_mempool),
                    &test_address_string(0x63),
                )
                .await
                .unwrap_or_else(|e| panic!("producer mines height {height}: {e}"));
            assert_eq!(block.height, height);
            let carried = block
                .transactions
                .iter()
                .filter(|t| t.tx_type == TransactionType::FeatureScheduleExtension)
                .count();
            assert_eq!(carried, usize::from(height == CARRY), "carried only at {CARRY}");
            // The mining route's refresh from the producer's canonical state.
            wrfcoin_native_chain::upgrade::refresh_schedule_from_canonical_state(
                &producer_chain.read().await.state,
                "test producer",
            );
            assert_eq!(
                wrfcoin_native_chain::upgrade::upgrade_deadline(),
                None,
                "a known gate and a known feature install no clean-halt deadline"
            );
            let wire: Block = bincode::deserialize(&bincode::serialize(&block).expect("encode"))
                .expect("decode");
            apply_p2p_block(
                wire,
                P2pBlockSource::HistoricalSync,
                &applier_chain,
                &applier_storage,
                &applier_consensus,
                None,
                &mut fork_buffer,
                &mut orphan_buffer,
            )
            .await;

            let producer = producer_chain.read().await;
            let applier = applier_chain.read().await;
            assert_eq!(applier.height, height, "the syncing node must accept height {height}");
            assert_eq!(
                applier.state.state_root, producer.state.state_root,
                "state_root diverged at {height}"
            );
            assert_eq!(
                applier.state.feature_schedule_extensions, producer.state.feature_schedule_extensions,
                "applied extensions diverged at {height}"
            );
            assert_eq!(
                feature_active(feature, height),
                height >= ACTIVATION,
                "{feature} at {height}"
            );
            drop((producer, applier));
            if height == CARRY {
                // The rows the real `apply_p2p_block` persisted carry the
                // extension transition (read now: a later checkpoint compacts
                // them).
                let wal_rows = applier_storage
                    .read()
                    .await
                    .load_wal_transitions_range(CARRY, CARRY)
                    .expect("WAL rows load");
                assert!(
                    wal_rows.iter().any(|(_, t)| matches!(
                        t,
                        wrfcoin_native_chain::blockchain::state::StateTransition::ApplyFeatureScheduleExtension { .. }
                    )),
                    "the WAL must persist the extension transition: {wal_rows:?}"
                );
            }
        }

        let producer_root = producer_chain.read().await.state.state_root;
        let producer_extensions = producer_chain.read().await.state.feature_schedule_extensions.clone();
        assert_eq!(producer_extensions.len(), 1, "vacuity: the list is POPULATED");
        assert_eq!(producer_extensions[0].carried_at_height, CARRY);

        // WAL leg: a restart replays checkpoint + tail under the refreshing
        // observer to the producer's root and schedule.
        drop(applier_chain);
        drop(applier_storage);
        wrfcoin_shared_protocol::crypto::chain_binding::refresh_feature_schedule_extensions(Vec::new());
        let reopened = Storage::open(&storage_path, DEFAULT_CHECKPOINT_INTERVAL).expect("reopen");
        let restored = reopened
            .load_state_observing_replay(|state| state.refresh_effective_feature_schedule())
            .expect("WAL replay")
            .expect("a persisted chain");
        assert_eq!(restored.height, TIP);
        assert_eq!(restored.state.state_root, producer_root, "WAL replay root");
        assert_eq!(restored.state.feature_schedule_extensions, producer_extensions);
        assert_eq!(
            feature_schedule_extension_entries(),
            vec![(feature.name().to_string(), ACTIVATION)],
            "the restored node answers from the same effective schedule"
        );
        drop(reopened);
        let _ = std::fs::remove_file(&storage_path);
    }

    /// #3277 S4, T2 end to end: a block from a NEWER producer carrying an
    /// extension for a feature this binary lacks is VALID here (names are
    /// not consensus) — the syncing node accepts it, holds the entry, and
    /// derives the clean-halt deadline at that feature's activation.
    ///
    /// The block is assembled with `create_block`, which takes the
    /// transactions as given: a newer binary's producer admits a name this
    /// one's policy refuses. The deadline asserted is the one the real
    /// `apply_p2p_block` refresh installed; the boot-refused mechanics are S3's
    /// `schedule_extension_halt_3277`.
    #[tokio::test]
    async fn a_peer_block_carrying_an_unknown_feature_is_valid_and_halts_cleanly_3277() {
        const CHILD: &str = "WRFCOIN_SCHEDULE_EXTENSION_UNKNOWN_3277_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "p2p_events::tests::a_peer_block_carrying_an_unknown_feature_is_valid_and_halts_cleanly_3277",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .status()
                .expect("re-exec child");
            assert!(status.success(), "isolated unknown-feature child failed");
            return;
        }
        use wrfcoin_consensus_engines::ConsensusEngine;
        use wrfcoin_native_chain::upgrade::{production_refused_at, upgrade_deadline};
        use wrfcoin_shared_protocol::governance::schedule_extension::MIN_RUNUP_BLOCKS;

        const B4_FEATURE: &str = "a_b4_feature_this_binary_lacks";
        let activation = 1 + MIN_RUNUP_BLOCKS;
        let governance = install_schedule_extension_gate_3277(1);
        let signed = signed_schedule_extension_3277(&governance, &[(B4_FEATURE, activation)]);
        let tx = Transaction::new_feature_schedule_extension(&signed);
        assert!(
            Mempool::new(&Config::default()).add_transaction(tx.clone()).is_err(),
            "this binary's own policy refuses to admit it"
        );

        let b4_chain = Blockchain::default();
        let b4_adapter = ConsensusFactory::create_pop_adapter(
            &ConsensusFactory::default_config(ConsensusType::ProofOfPublishing),
        );
        b4_adapter.declare_no_genesis_oracles_blocking();
        let coinbase = Transaction::new_coinbase(&test_address_string(0x64), 50, 1);
        let block = b4_adapter
            .create_block(vec![coinbase, tx], "", &test_address_string(0x64), &b4_chain)
            .await
            .expect("a newer producer assembles the block");

        let applier_chain = Arc::new(RwLock::new(Blockchain::default()));
        let applier_storage = test_storage();
        let applier_consensus = test_consensus();
        let mut fork_buffer = ForkCandidateBuffer::new();
        let mut orphan_buffer = OrphanBlockBuffer::new();
        wrfcoin_native_chain::upgrade::reset_for_tests();
        apply_p2p_block(
            block,
            P2pBlockSource::LiveGossip,
            &applier_chain,
            &applier_storage,
            &applier_consensus,
            None,
            &mut fork_buffer,
            &mut orphan_buffer,
        )
        .await;
        let applier = applier_chain.read().await;
        assert_eq!(applier.height, 1, "the block is valid on this binary");
        assert_eq!(
            applier.state.feature_schedule_extension_entries(),
            vec![(B4_FEATURE.to_string(), activation)]
        );
        let deadline = upgrade_deadline().expect("the applied extension installs the clean halt");
        assert_eq!(deadline.feature, B4_FEATURE, "the gate itself is known (S5): only B4's feature");
        assert_eq!(deadline.height, activation);
        assert!(production_refused_at(activation - 1).is_none(), "runs through activation - 1");
        assert!(production_refused_at(activation).is_some(), "refuses the activation height");
    }

    /// #3270 producer / P2P applier / extracted-transitions / WAL-replay parity
    /// for `station_weather_accepted_block_unix` across its POPULATED
    /// lifecycle: `ConsensusFeature::WeatherSkewBlockTimeAllowance` is scheduled from
    /// genesis, the producer mines a quorum-attested WEATHER reveal, and every
    /// replay path must record the same accepting-block HEADER timestamp for
    /// the station and reach the producer's state_root. The feature schedule
    /// is process-global, so the body runs in a re-executed child.
    #[tokio::test]
    async fn producer_applier_and_wal_record_the_same_weather_block_time_3270() {
        const CHILD: &str = "WRFCOIN_3270_PARITY_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "p2p_events::tests::producer_applier_and_wal_record_the_same_weather_block_time_3270",
                    "--exact",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env(CHILD, "1")
                .status()
                .expect("re-exec child");
            assert!(status.success(), "isolated #3270 parity child failed");
            return;
        }
        use wrfcoin_shared_protocol::crypto::chain_binding::{
            feature_active, install_feature_schedule, ConsensusFeature, FeatureSchedule,
        };
        install_feature_schedule(
            FeatureSchedule::from_entries(vec![(
                ConsensusFeature::WeatherSkewBlockTimeAllowance.name().to_string(),
                0,
            )])
            .expect("schedule"),
        );

        let scenario = mine_attested_reward_block_with_reveal_data(
            1.0,
            wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass::Real,
            Arc::new(|_| {}),
            0,
            Some(serde_json::json!({
                "temperature": 12.0,
                "humidity": 70.0,
                "pressure": 990.0,
                "dewpoint": 6.8
            })),
        )
        .await;
        let block = scenario.block_1.clone();
        assert!(feature_active(ConsensusFeature::WeatherSkewBlockTimeAllowance, block.height));
        assert!(
            block
                .transactions
                .iter()
                .any(|tx| tx.tx_type == TransactionType::ContributionReveal),
            "vacuity guard: the weather reveal must be in the block"
        );

        let (producer_root, producer_map) = {
            let producer = scenario.producer_chain.read().await;
            (
                producer.state.state_root,
                producer.state.station_weather_accepted_block_unix.clone(),
            )
        };
        assert_eq!(
            producer_map.get("station-attest"),
            Some(&block.timestamp.timestamp()),
            "vacuity guard: the producer records the reveal block's HEADER timestamp"
        );

        // P2P APPLIER LEG (real `apply_p2p_block`, persisted storage for the WAL leg).
        let storage_path = std::env::temp_dir().join(format!(
            "wrfcoin-p2p-weather-block-time-3270-{}.redb",
            fastrand::u64(..)
        ));
        let _ = std::fs::remove_file(&storage_path);
        let applier_storage = Arc::new(RwLock::new(
            Storage::open(&storage_path, DEFAULT_CHECKPOINT_INTERVAL)
                .expect("applier storage opens"),
        ));
        let mut applier_genesis = Blockchain::default();
        (scenario.seed_genesis)(&mut applier_genesis);
        let applier_seed = applier_genesis.state.clone();
        let applier_chain = Arc::new(RwLock::new(applier_genesis));
        let applier_consensus = test_consensus();
        let mut fork_buffer = ForkCandidateBuffer::new();
        let mut orphan_buffer = OrphanBlockBuffer::new();
        let wire_block: Block = bincode::deserialize(
            &bincode::serialize(&block).expect("block serializes for P2P"),
        )
        .expect("block deserializes for P2P");
        apply_p2p_block(
            wire_block,
            P2pBlockSource::HistoricalSync,
            &applier_chain,
            &applier_storage,
            &applier_consensus,
            None,
            &mut fork_buffer,
            &mut orphan_buffer,
        )
        .await;
        {
            let applier = applier_chain.read().await;
            assert_eq!(applier.height, block.height, "applier must accept the block");
            assert_eq!(applier.state.station_weather_accepted_block_unix, producer_map);
            assert_eq!(
                applier.state.state_root, producer_root,
                "P2P applier state_root must match the producer"
            );
        }

        // EXTRACTED-TRANSITIONS LEG.
        let applier_transitions = extract_transitions(
            &applier_seed,
            &block,
            block.height,
            block.timestamp.timestamp(),
        );
        let mut extracted = applier_seed.clone();
        extracted
            .apply_transitions_batch_at_height(block.height, applier_transitions.clone())
            .expect("extracted transitions apply");
        assert_eq!(extracted.station_weather_accepted_block_unix, producer_map);

        // WAL LEG: replay the rows the real `apply_p2p_block` persisted.
        let replayed = applier_storage
            .read()
            .await
            .load_wal_transitions_range(block.height, block.height)
            .expect("P2P-written WAL transitions must load");
        assert!(
            replayed.iter().any(|(_, t)| matches!(
                t,
                wrfcoin_native_chain::blockchain::state::StateTransition::ApplyReveal {
                    block_timestamp, ..
                } if *block_timestamp == block.timestamp.timestamp()
            )),
            "the WAL row must carry the reveal block's header timestamp"
        );
        let mut wal = applier_seed;
        wal.apply_transitions_batch_at_height(
            block.height,
            replayed.into_iter().map(|(_, transition)| transition),
        )
        .expect("P2P-written WAL batch must apply");
        assert_eq!(
            wal.station_weather_accepted_block_unix, producer_map,
            "WAL replay must reproduce the producer's accepting-block map"
        );
        drop(applier_chain);
        drop(applier_storage);
        let _ = std::fs::remove_file(&storage_path);
    }

    /// #3565: apply a mined scenario block through the real `apply_p2p_block`,
    /// the extracted-transitions batch and the persisted WAL rows, and require
    /// every leg to reach the producer's `state_root` (each leg re-checks the
    /// block's root commitment through `apply_block_finalization_checked_at`).
    async fn assert_producer_applier_wal_root_parity_3565(scenario: &AttestedRewardScenario, label: &str) {
        assert_producer_applier_wal_root_parity_at_base_3565(scenario, label, 0).await
    }

    /// [`assert_producer_applier_wal_root_parity_3565`] for a producer that sat at
    /// `base_height` with the fixture's deterministic tip.
    async fn assert_producer_applier_wal_root_parity_at_base_3565(
        scenario: &AttestedRewardScenario,
        label: &str,
        base_height: u64,
    ) {
        let block = scenario.block_1.clone();
        let producer_root = scenario.producer_chain.read().await.state.state_root;

        let storage_path = std::env::temp_dir().join(format!(
            "wrfcoin-p2p-device-limit-3565-{label}-{}.redb",
            fastrand::u64(..)
        ));
        let _ = std::fs::remove_file(&storage_path);
        let applier_storage = Arc::new(RwLock::new(
            // A checkpoint interval no fixture height reaches, so the block's
            // transitions stay in the WAL rather than being folded into a
            // checkpoint (h3000 is a multiple of the default 100).
            Storage::open(&storage_path, u64::MAX)
                .expect("applier storage opens"),
        ));
        let mut applier_genesis = Blockchain::default();
        (scenario.seed_genesis)(&mut applier_genesis);
        if base_height > 0 {
            applier_genesis.height = base_height;
            applier_genesis.tip =
                wrfcoin_native_chain::crypto::hash::hash_string(&format!("base-tip-{base_height}"));
        }
        let applier_seed = applier_genesis.state.clone();
        let applier_chain = Arc::new(RwLock::new(applier_genesis));
        let applier_consensus = test_consensus();
        let mut fork_buffer = ForkCandidateBuffer::new();
        let mut orphan_buffer = OrphanBlockBuffer::new();
        let wire_block: Block = bincode::deserialize(
            &bincode::serialize(&block).expect("block serializes for P2P"),
        )
        .expect("block deserializes for P2P");
        apply_p2p_block(
            wire_block,
            P2pBlockSource::HistoricalSync,
            &applier_chain,
            &applier_storage,
            &applier_consensus,
            None,
            &mut fork_buffer,
            &mut orphan_buffer,
        )
        .await;
        {
            let applier = applier_chain.read().await;
            assert_eq!(applier.height, block.height, "{label}: applier must accept the block");
            assert_eq!(
                applier.state.state_root, producer_root,
                "{label}: P2P applier state_root must match the producer"
            );
        }

        let version = wrfcoin_shared_protocol::crypto::chain_binding::protocol_version_at(block.height);
        let applier_transitions =
            extract_transitions(&applier_seed, &block, block.height, block.timestamp.timestamp());
        let mut extracted = applier_seed.clone();
        extracted
            .apply_transitions_batch_at_height(block.height, applier_transitions.clone())
            .expect("extracted transitions apply");
        extracted
            .apply_block_finalization_checked_at(version, &block)
            .expect("extracted state must match the block commitment");
        assert_eq!(extracted.state_root, producer_root, "{label}: extracted state_root");

        let replayed = applier_storage
            .read()
            .await
            .load_wal_transitions_range(block.height, block.height)
            .expect("P2P-written WAL transitions must load");
        assert_eq!(
            replayed.len(),
            applier_transitions.len(),
            "{label}: vacuity guard: WAL must contain every extracted transition"
        );
        let mut wal = applier_seed;
        wal.apply_transitions_batch_at_height(
            block.height,
            replayed.into_iter().map(|(_, transition)| transition),
        )
        .expect("P2P-written WAL batch must apply");
        wal.apply_block_finalization_checked_at(version, &block)
            .expect("WAL-replayed state must match the block commitment");
        assert_eq!(wal.state_root, producer_root, "{label}: WAL replay state_root");
        drop(applier_chain);
        drop(applier_storage);
        let _ = std::fs::remove_file(&storage_path);
    }

    fn count_3565(block: &Block, tx_type: TransactionType) -> usize {
        block.transactions.iter().filter(|tx| tx.tx_type == tx_type).count()
    }

    /// The single contribution reward the block records, and the age of its
    /// reading at the block (header timestamp minus the reveal's committed
    /// timestamp).
    fn reward_and_age_3565(block: &Block) -> (u128, i64) {
        let entries: Vec<wrfcoin_native_chain::blockchain::state::ContributionRewardRecord> =
            serde_json::from_str(block.metadata.get("reward_entries").expect("reward_entries"))
                .expect("reward_entries JSON");
        assert_eq!(entries.len(), 1, "vacuity guard: exactly one contribution decision");
        let reveal = block
            .transactions
            .iter()
            .find(|tx| tx.tx_type == TransactionType::ContributionReveal)
            .and_then(|tx| tx.parse_reveal_payload())
            .expect("the block carries the reveal");
        (entries[0].amount.0, block.timestamp.timestamp() - reveal.timestamp)
    }

    fn reexec_child_3565(test: &str, child: &str) -> bool {
        if std::env::var_os(child).is_some() {
            return false;
        }
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([test, "--exact", "--nocapture", "--test-threads=1"])
            .env(child, "1")
            .status()
            .expect("re-exec child");
        assert!(status.success(), "isolated #3565 child {test} failed");
        true
    }

    /// #3565 gate ON (scheduled from height 1, isolated child: the schedule is
    /// process-global). Two quorum-attested reveals from ONE station are staged
    /// in the mempool; the real producer keeps exactly one and both commits.
    /// Producer, P2P applier, extracted transitions and WAL replay agree on the
    /// root. A 1805 s old reading is paid `late_data_multiplier_bps(age)` of the
    /// fresh reading's reward, exactly.
    #[tokio::test]
    async fn device_submission_limit_keeps_one_reveal_per_station_with_parity_3565() {
        if reexec_child_3565(
            "p2p_events::tests::device_submission_limit_keeps_one_reveal_per_station_with_parity_3565",
            "WRFCOIN_3565_ON_CHILD",
        ) {
            return;
        }
        use wrfcoin_shared_protocol::consensus::proof_of_publishing::late_data_multiplier_bps;
        use wrfcoin_shared_protocol::crypto::chain_binding::{
            feature_active, install_feature_schedule, ConsensusFeature, FeatureSchedule,
        };
        use wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass;
        install_feature_schedule(
            FeatureSchedule::from_entries(vec![(
                ConsensusFeature::DeviceSubmissionLimit.name().to_string(),
                1,
            )])
            .expect("schedule"),
        );

        // Owner-signed commits: from activation a commit must be authorized by
        // the station's registered owner key (#3565 pre-activation fix (a)).
        let two = mine_attested_reward_block_with_options(
            1.0, ContributionProvenanceClass::Real, Arc::new(|_| {}), 0, None, 2, 5, true, &[], &[],
        )
        .await;
        assert!(feature_active(ConsensusFeature::DeviceSubmissionLimit, two.block_1.height));
        assert_eq!(count_3565(&two.block_1, TransactionType::ContributionCommit), 2, "vacuity guard: both commits mined");
        assert_eq!(
            count_3565(&two.block_1, TransactionType::ContributionReveal),
            1,
            "gate ON: one reveal per station per block"
        );
        assert_producer_applier_wal_root_parity_3565(&two, "on-two-reveals").await;
        let (fresh_amount, fresh_age) = reward_and_age_3565(&two.block_1);
        assert!(fresh_age <= 600, "fixture: the fresh reading is inside full pay ({fresh_age}s)");
        assert!(fresh_amount > 0, "vacuity guard: the fresh reading is paid");

        let late = mine_attested_reward_block_with_options(
            1.0, ContributionProvenanceClass::Real, Arc::new(|_| {}), 0, None, 1, 1805, true, &[], &[],
        )
        .await;
        assert_producer_applier_wal_root_parity_3565(&late, "on-late").await;
        let (late_amount, late_age) = reward_and_age_3565(&late.block_1);
        let multiplier = late_data_multiplier_bps(late_age);
        assert!(multiplier > 5_000 && multiplier < 10_000, "fixture lands on the slope ({late_age}s -> {multiplier})");
        assert_eq!(late_amount, fresh_amount * multiplier / 10_000, "gate ON: late data is paid less, on the curve");
    }

    /// #3565 gate OFF control (isolated child, nothing scheduled): the same
    /// two-reveal mempool mines BOTH reveals, as on main, and an 1805 s old
    /// reading is paid exactly what a fresh one is. Roots agree on every leg.
    #[tokio::test]
    async fn device_submission_limit_unscheduled_is_unchanged_3565() {
        if reexec_child_3565(
            "p2p_events::tests::device_submission_limit_unscheduled_is_unchanged_3565",
            "WRFCOIN_3565_OFF_CHILD",
        ) {
            return;
        }
        use wrfcoin_shared_protocol::crypto::chain_binding::{feature_active, ConsensusFeature};
        use wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass;

        let two = mine_attested_reward_block_with_reveals(
            1.0, ContributionProvenanceClass::Real, Arc::new(|_| {}), 0, None, 2, 5,
        )
        .await;
        assert!(!feature_active(ConsensusFeature::DeviceSubmissionLimit, two.block_1.height));
        assert_eq!(
            count_3565(&two.block_1, TransactionType::ContributionReveal),
            2,
            "gate OFF: both same-station reveals are mined, as on main"
        );
        assert_producer_applier_wal_root_parity_3565(&two, "off-two-reveals").await;

        let fresh = mine_attested_reward_block_with_reveals(
            1.0, ContributionProvenanceClass::Real, Arc::new(|_| {}), 0, None, 1, 5,
        )
        .await;
        let late = mine_attested_reward_block_with_reveals(
            1.0, ContributionProvenanceClass::Real, Arc::new(|_| {}), 0, None, 1, 1805,
        )
        .await;
        assert_producer_applier_wal_root_parity_3565(&late, "off-late").await;
        assert_eq!(
            reward_and_age_3565(&late.block_1).0,
            reward_and_age_3565(&fresh.block_1).0,
            "gate OFF: no late-data reduction"
        );
    }

    fn reveal_ids_3565(block: &Block) -> Vec<(CryptoHash, String)> {
        block
            .transactions
            .iter()
            .filter(|tx| tx.tx_type == TransactionType::ContributionReveal)
            .map(|tx| (tx.id.expect("mined tx has an id"), tx.parse_reveal_payload().expect("reveal").nonce))
            .collect()
    }

    /// The reveals of `scenario`'s mempool that are still pending after mining.
    async fn pending_reveal_nonces_3565(scenario: &AttestedRewardScenario) -> Vec<String> {
        let mempool = scenario.producer_mempool.read().await;
        let mut nonces: Vec<String> = mempool
            .iter_transactions()
            .filter(|tx| tx.tx_type == TransactionType::ContributionReveal)
            .filter_map(|tx| tx.parse_reveal_payload())
            .map(|payload| payload.nonce)
            .collect();
        nonces.sort();
        nonces
    }

    /// #3565 pre-activation fixes (b) and (a), gate ON from height 1 (isolated child).
    /// (b) Delay, don't drop: of two owner-signed same-station reveals the
    /// producer mines ONE and the other stays PENDING in the mempool for a later
    /// block; roots agree on every leg. (a) Owner binding: when the commit is
    /// signed by a key that is not the station's registered owner, the commit is
    /// refused and evicted, so its reveal cannot occupy the station's slot.
    #[tokio::test]
    async fn device_limit_defers_the_extra_reveal_and_binds_commits_to_the_owner_3565() {
        if reexec_child_3565(
            "p2p_events::tests::device_limit_defers_the_extra_reveal_and_binds_commits_to_the_owner_3565",
            "WRFCOIN_3565_DEFER_CHILD",
        ) {
            return;
        }
        use wrfcoin_shared_protocol::crypto::chain_binding::{
            install_feature_schedule, ConsensusFeature, FeatureSchedule,
        };
        use wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass;
        install_feature_schedule(
            FeatureSchedule::from_entries(vec![(ConsensusFeature::DeviceSubmissionLimit.name().to_string(), 1)])
                .expect("schedule"),
        );

        let two = mine_attested_reward_block_with_options(
            1.0, ContributionProvenanceClass::Real, Arc::new(|_| {}), 0, None, 2, 5, true, &[], &[],
        )
        .await;
        assert_eq!(count_3565(&two.block_1, TransactionType::ContributionCommit), 2, "owner-signed commits are mined");
        let mined = reveal_ids_3565(&two.block_1);
        assert_eq!(mined.len(), 1, "one reveal per station per block");
        let pending = pending_reveal_nonces_3565(&two).await;
        assert_eq!(pending.len(), 1, "(b) the extra reveal is deferred, not evicted");
        assert_ne!(pending[0], mined[0].1, "the pending reveal is the one not mined");
        assert_producer_applier_wal_root_parity_3565(&two, "defer-two").await;

        let not_owner = mine_attested_reward_block_with_options(
            1.0, ContributionProvenanceClass::Real, Arc::new(|_| {}), 0, None, 1, 5, false, &[], &[],
        )
        .await;
        assert_eq!(
            count_3565(&not_owner.block_1, TransactionType::ContributionCommit),
            0,
            "(a) a commit not signed by the registered owner key is refused"
        );
        assert_eq!(count_3565(&not_owner.block_1, TransactionType::ContributionReveal), 0, "(a) so its reveal cannot occupy the slot");
        assert_producer_applier_wal_root_parity_3565(&not_owner, "not-owner").await;
    }

    /// #3565 (b) with the #2777 fence: gate ON, block at h3000 (the fence's
    /// activation height). The FIRST same-station reveal has no attestations and
    /// is held back for lack of quorum; the second has quorum. The producer must
    /// mine the second, not lose both, and keep the first pending. Roots agree.
    #[tokio::test]
    async fn device_limit_quorum_held_first_reveal_does_not_block_its_sibling_3565() {
        if reexec_child_3565(
            "p2p_events::tests::device_limit_quorum_held_first_reveal_does_not_block_its_sibling_3565",
            "WRFCOIN_3565_QUORUM_CHILD",
        ) {
            return;
        }
        use wrfcoin_shared_protocol::consensus::proof_of_publishing::RICH_AMBIENT_REWARD_ACTIVATION_HEIGHT;
        use wrfcoin_shared_protocol::crypto::chain_binding::{
            install_feature_schedule, ConsensusFeature, FeatureSchedule,
        };
        use wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass;
        install_feature_schedule(
            FeatureSchedule::from_entries(vec![(ConsensusFeature::DeviceSubmissionLimit.name().to_string(), 1)])
                .expect("schedule"),
        );
        let base = RICH_AMBIENT_REWARD_ACTIVATION_HEIGHT - 1;
        let scenario = mine_attested_reward_block_with_options(
            1.0, ContributionProvenanceClass::Real, Arc::new(|_| {}), base, None, 2, 5, true, &[0], &[],
        )
        .await;
        assert_eq!(scenario.block_1.height, RICH_AMBIENT_REWARD_ACTIVATION_HEIGHT, "fixture: the #2777 fence is active");
        let mined = reveal_ids_3565(&scenario.block_1);
        assert_eq!(mined.len(), 1, "the quorum-ready sibling is mined");
        assert_eq!(mined[0].1, "nonce-attest-1", "it is the attested (second) reveal");
        assert_eq!(pending_reveal_nonces_3565(&scenario).await, vec!["nonce-attest".to_string()], "the quorum-held first reveal stays pending");
        assert_producer_applier_wal_root_parity_at_base_3565(&scenario, "quorum-held", base).await;
    }

    /// #3565 review B1, end to end through the real producer, gate ON. An
    /// attacker stages a `ContributionCommit` + `ContributionReveal` for the
    /// victim's registered station whose payloads say `contribution_type:
    /// "forecast"`, signed by a NON-owner key, beside the owner's normal pair.
    /// The forecast-typed contribution commit must not be mined (before the fix
    /// the `State` forecast exemption let it apply), and the owner's reveal is
    /// the one that takes the station's slot. Roots agree on every leg.
    #[tokio::test]
    async fn device_limit_refuses_a_forecast_typed_contribution_commit_3565() {
        if reexec_child_3565(
            "p2p_events::tests::device_limit_refuses_a_forecast_typed_contribution_commit_3565",
            "WRFCOIN_3565_TYPE_CHILD",
        ) {
            return;
        }
        use wrfcoin_shared_protocol::crypto::chain_binding::{
            install_feature_schedule, ConsensusFeature, FeatureSchedule,
        };
        use wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass;
        install_feature_schedule(
            FeatureSchedule::from_entries(vec![(ConsensusFeature::DeviceSubmissionLimit.name().to_string(), 1)])
                .expect("schedule"),
        );
        let scenario = mine_attested_reward_block_with_options(
            1.0, ContributionProvenanceClass::Real, Arc::new(|_| {}), 0, None, 2, 5, true, &[], &[0],
        )
        .await;
        let forecast_typed_commits = scenario
            .block_1
            .transactions
            .iter()
            .filter(|tx| tx.tx_type == TransactionType::ContributionCommit)
            .filter_map(|tx| tx.parse_commit_payload())
            .filter(|payload| payload.contribution_type == "forecast")
            .count();
        assert_eq!(forecast_typed_commits, 0, "B1: a forecast-typed ContributionCommit is refused");
        assert_eq!(count_3565(&scenario.block_1, TransactionType::ContributionCommit), 1, "the owner's commit is mined");
        let mined = reveal_ids_3565(&scenario.block_1);
        assert_eq!(mined.len(), 1);
        assert_eq!(mined[0].1, "nonce-attest-1", "the owner's reveal takes the slot");
        assert_producer_applier_wal_root_parity_3565(&scenario, "forecast-typed").await;
    }

    /// COMMIT-REVEAL LIFECYCLE expiry parity. The producer runs a per-block
    /// `prune_commit_reveal_expired` sweep (now via the shared
    /// `State::apply_block_finalization`); the applier replay path previously
    /// skipped it, so a follower replaying the same `ContributionCommit`
    /// accumulated a stale `pending_commit` that never expired and its
    /// `state_root` drifted from the producer's at every height from the first
    /// expiry onward (the live multi-node divergence: same tip, different
    /// state_root). The existing single-block parity tests never crossed an
    /// expiry boundary, so they couldn't catch this.
    ///
    /// At genesis the reveal window is 3 blocks, so a commit at height 1 expires
    /// at height 4 and is pruned at height 5. This mines the commit then empty
    /// blocks across that boundary, applying each to a fresh applier, and
    /// asserts `state_root` + `pending_commits` parity at EVERY height. FAILS
    /// pre-fix (applier never prunes → counts and root diverge at height 5),
    /// PASSES post-fix.
    /// #3345 / #3347 POPULATED owner-ledger lifecycle parity (the #1682
    /// demand before `station_owner_ledger` is pinned). With
    /// `station_owner_consent` and `station_owner_actions` scheduled from
    /// height 1 (in an isolated child process — the schedule is
    /// process-global), the producer mines, one owner step per block:
    ///
    /// 1. a governance registration of the station (owner A);
    /// 2. A reports it stolen (nonce 0) → frozen;
    /// 3. A tries to transfer to B (nonce 1) → REFUSED inert (frozen);
    /// 4. A unfreezes (nonce 1);
    /// 5. A transfers to B, countersigned (nonce 2) → owner B;
    /// 6. governance files a recovery B → C, C accepts (nonce 3) → pending;
    /// 7. B cancels it inside the notice window (nonce 3) → cleared;
    /// 8. B self-deactivates (nonce 4).
    ///
    /// Each block goes to a SEPARATE P2P applier (`apply_p2p_block`), to a
    /// third State through `extract_transitions` + the shared batch +
    /// finalization, and finally the applier's persisted WAL is replayed into
    /// a fourth State. `state_root` and the ledger must agree at EVERY
    /// height. The recovery EXECUTE (valid only from filed + 34,000) cannot
    /// be mined here in reasonable time; it uses the same shared constructor
    /// and apply arm, and is proven at the State level in
    /// `shared-protocol/tests/station_owner_ledger_3345_3347.rs`.
    #[test]
    fn producer_applier_and_wal_agree_on_a_populated_owner_ledger_3345_3347() {
        const CHILD: &str = "WRFCOIN_OWNER_LEDGER_PARITY_3345_CHILD";
        const GOVERNANCE_ENV: &str = "WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS";
        use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
        let governance_key = PrivateKey::from_bytes(&[0x60; 32]).expect("seed");
        if std::env::var_os(CHILD).is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "p2p_events::tests::producer_applier_and_wal_agree_on_a_populated_owner_ledger_3345_3347",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .env(GOVERNANCE_ENV, PublicKey::from_private_key(&governance_key).to_hex())
                .status()
                .expect("re-exec child");
            assert!(
                status.success(),
                "isolated owner-ledger parity child failed"
            );
            return;
        }
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(owner_ledger_parity_child(governance_key));
    }

    async fn owner_ledger_parity_child(
        governance_key: wrfcoin_shared_protocol::crypto::signature::PrivateKey,
    ) {
        use wrfcoin_native_chain::blockchain::state::State;
        use wrfcoin_native_chain::blockchain::transaction::{TransactionInput, TransactionOutput};
        use wrfcoin_native_chain::crypto::hash::hash_string;
        use wrfcoin_shared_protocol::blockchain::station_owner::{
            StationOwnerActionPayload, StationOwnerRecoveryCancelPayload,
            StationOwnerRecoveryFilingPayload, StationOwnerTransferPayload,
        };
        use wrfcoin_shared_protocol::blockchain::transaction::{
            StationOwnerActionKind, StationRegistrationPayload,
        };
        use wrfcoin_shared_protocol::crypto::chain_binding::{
            ChainIdentity, ConsensusFeature, FeatureSchedule, feature_active,
            install_feature_schedule,
        };
        use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
        use wrfcoin_shared_protocol::station_registry::{
            ContributionProvenanceClass, DataQualityTier,
        };

        install_feature_schedule(
            FeatureSchedule::from_entries(vec![
                (ConsensusFeature::StationOwnerConsent.name().to_string(), 1),
                (ConsensusFeature::StationOwnerActions.name().to_string(), 1),
            ])
            .expect("schedule"),
        );
        assert!(feature_active(ConsensusFeature::StationOwnerConsent, 1));
        assert!(feature_active(ConsensusFeature::StationOwnerActions, 1));

        let identity = ChainIdentity::active();
        let (chain_id, network_id) = (identity.chain_id.clone(), identity.network_id.clone());
        let key = |b: u8| PrivateKey::from_bytes(&[b; 32]).expect("seed");
        let (a_key, b_key, c_key) = (key(0xA1), key(0xB2), key(0xC3));
        let addr = |k: &PrivateKey| PublicKey::from_private_key(k).to_address();
        let pk = |k: &PrivateKey| PublicKey::from_private_key(k).to_hex();
        let (a, b, c) = (addr(&a_key), addr(&b_key), addr(&c_key));
        let station_id = "ws5000-ledger-parity";
        let expires = 5_000u64;

        let wrap = |tag: &str, data: String| {
            let mut tx = Transaction::new(
                vec![TransactionInput {
                    txid: hash_string(&format!("in-ledger-parity-{tag}")),
                    vout: 0,
                    signature: None,
                    public_key: None,
                    coinbase_data: None,
                }],
                vec![TransactionOutput {
                    amount: 1,
                    address: test_address_string(0x77),
                    script: "OP_TRUE".to_string(),
                    nft_data: None,
                }],
            );
            tx.tx_type = TransactionType::StationRegistration;
            tx.data = Some(data);
            tx.set_id();
            tx
        };
        let action = |kind: StationOwnerActionKind, owner_key: &PrivateKey, nonce: u64| {
            let mut p = StationOwnerActionPayload::new(
                station_id,
                addr(owner_key),
                pk(owner_key),
                kind,
                nonce,
                expires,
            );
            p.sign_with_domain(&chain_id, &network_id, owner_key);
            serde_json::to_string(&p).unwrap()
        };
        let transfer = |nonce: u64| {
            let mut p = StationOwnerTransferPayload::new(
                station_id,
                a.clone(),
                pk(&a_key),
                b.clone(),
                pk(&b_key),
                nonce,
                expires,
            );
            p.sign_as_owner_with_domain(&chain_id, &network_id, &a_key);
            p.countersign_with_domain(&chain_id, &network_id, &b_key);
            serde_json::to_string(&p).unwrap()
        };
        let registration = {
            let mut p = StationRegistrationPayload::new(
                station_id,
                &a,
                46.7867,
                -92.1005,
                "Ambient WS-5000",
                DataQualityTier::Gold,
                ContributionProvenanceClass::Real,
            );
            p.sign_ownership(&a_key);
            p.sign(&governance_key);
            serde_json::to_string(&p).unwrap()
        };
        let filing = {
            let mut p = StationOwnerRecoveryFilingPayload::new(
                station_id,
                b.clone(),
                c.clone(),
                pk(&c_key),
                3,
            );
            p.sign_as_governance_with_domain(&chain_id, &network_id, &governance_key);
            p.accept_with_domain(&chain_id, &network_id, &c_key);
            serde_json::to_string(&p).unwrap()
        };
        let cancel = {
            let mut p =
                StationOwnerRecoveryCancelPayload::new(station_id, b.clone(), pk(&b_key), 3);
            p.sign_with_domain(&chain_id, &network_id, &b_key);
            serde_json::to_string(&p).unwrap()
        };
        // (tag, tx data, expected owner, expected nonce, frozen, deactivated, pending)
        let steps: Vec<(&str, String, &str, u64, bool, bool, bool)> = vec![
            ("register", registration, &a, 0, false, false, false),
            (
                "report-stolen",
                action(StationOwnerActionKind::ReportStolen, &a_key, 0),
                &a,
                1,
                true,
                false,
                false,
            ),
            (
                "transfer-while-frozen",
                transfer(1),
                &a,
                1,
                true,
                false,
                false,
            ),
            (
                "unfreeze",
                action(StationOwnerActionKind::Unfreeze, &a_key, 1),
                &a,
                2,
                false,
                false,
                false,
            ),
            ("transfer", transfer(2), &b, 3, false, false, false),
            ("recovery-file", filing, &b, 3, false, false, true),
            ("recovery-cancel", cancel, &b, 4, false, false, false),
            (
                "deactivate",
                action(StationOwnerActionKind::Deactivate, &b_key, 4),
                &b,
                5,
                false,
                true,
                false,
            ),
        ];

        let producer_consensus = test_consensus();
        let producer_chain = Arc::new(RwLock::new(Blockchain::default()));
        let producer_mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let storage_path = std::env::temp_dir().join(format!(
            "wrfcoin-p2p-owner-ledger-3345-{}.redb",
            fastrand::u64(..)
        ));
        let _ = std::fs::remove_file(&storage_path);
        let applier_storage = Arc::new(RwLock::new(
            Storage::open(&storage_path, DEFAULT_CHECKPOINT_INTERVAL).expect("applier storage"),
        ));
        let applier_chain = Arc::new(RwLock::new(Blockchain::default()));
        let applier_consensus = test_consensus();
        let mut fork_buffer = ForkCandidateBuffer::new();
        let mut orphan_buffer = OrphanBlockBuffer::new();
        let mut extracted = State::new();
        let mut blocks = Vec::new();
        let mut transitions_seen = 0usize;

        for (tag, data, owner, nonce, frozen, deactivated, pending) in steps {
            producer_mempool
                .write()
                .await
                .add_transaction(wrap(tag, data))
                .unwrap_or_else(|e| panic!("mempool admits {tag}: {e}"));
            let block = producer_consensus
                .create_pop_block_with_state(
                    Arc::clone(&producer_chain),
                    Arc::clone(&producer_mempool),
                    &test_address_string(0x63),
                )
                .await
                .unwrap_or_else(|e| panic!("producer mines {tag}: {e}"));
            let height = block.height;

            let (p_root, p_ledger, p_owner) = {
                let p = producer_chain.read().await;
                (
                    p.state.state_root,
                    p.state.station_owner_ledger.clone(),
                    p.state
                        .station_registry
                        .get(station_id)
                        .map(|s| s.owner_address.clone()),
                )
            };
            assert_eq!(p_owner.as_deref(), Some(owner), "[{tag}] producer owner");
            let entry = p_ledger.get(station_id).cloned().unwrap_or_default();
            assert_eq!(entry.owner_nonce, nonce, "[{tag}] producer nonce");
            assert_eq!(entry.frozen.is_some(), frozen, "[{tag}] producer frozen");
            assert_eq!(
                entry.owner_deactivated, deactivated,
                "[{tag}] producer deactivated"
            );
            assert_eq!(
                entry.pending_recovery.is_some(),
                pending,
                "[{tag}] producer pending"
            );

            // P2P applier.
            let wire: Block =
                bincode::deserialize(&bincode::serialize(&block).expect("ser")).expect("de");
            apply_p2p_block(
                wire,
                P2pBlockSource::HistoricalSync,
                &applier_chain,
                &applier_storage,
                &applier_consensus,
                None,
                &mut fork_buffer,
                &mut orphan_buffer,
            )
            .await;
            {
                let applier = applier_chain.read().await;
                assert_eq!(applier.height, height, "[{tag}] applier accepts the block");
                assert_eq!(
                    applier.state.state_root, p_root,
                    "[{tag}] applier state_root"
                );
                assert_eq!(
                    applier.state.station_owner_ledger, p_ledger,
                    "[{tag}] applier ledger"
                );
            }

            // Extract → apply-only leg.
            let transitions =
                extract_transitions(&extracted, &block, height, block.timestamp.timestamp());
            transitions_seen += transitions
                .iter()
                .filter(|t| {
                    matches!(
                        t,
                        wrfcoin_native_chain::blockchain::state::StateTransition::ApplyStationOwnerTransfer { .. }
                            | wrfcoin_native_chain::blockchain::state::StateTransition::ApplyStationOwnerAction { .. }
                            | wrfcoin_native_chain::blockchain::state::StateTransition::ApplyStationOwnerRecoveryFiling { .. }
                            | wrfcoin_native_chain::blockchain::state::StateTransition::ApplyStationOwnerRecoveryCancel { .. }
                    )
                })
                .count();
            extracted
                .apply_transitions_batch_at_height(height, transitions)
                .unwrap_or_else(|e| panic!("[{tag}] extracted transitions apply: {e}"));
            extracted
                .apply_block_finalization_checked_at(
                    wrfcoin_shared_protocol::crypto::chain_binding::protocol_version_at(height),
                    &block,
                )
                .unwrap_or_else(|e| panic!("[{tag}] extracted state matches commitment: {e}"));
            assert_eq!(extracted.state_root, p_root, "[{tag}] extracted state_root");
            assert_eq!(
                extracted.station_owner_ledger, p_ledger,
                "[{tag}] extracted ledger"
            );
            blocks.push(block);
        }
        assert_eq!(
            transitions_seen, 7,
            "vacuity guard: every owner step was extracted"
        );
        let producer_root = producer_chain.read().await.state.state_root;
        let producer_ledger = producer_chain
            .read()
            .await
            .state
            .station_owner_ledger
            .clone();
        assert!(
            !producer_ledger.is_empty(),
            "vacuity guard: the ledger is populated"
        );

        // WAL leg: replay what the real P2P applier persisted.
        let tip = blocks.last().expect("blocks").height;
        let replayed = applier_storage
            .read()
            .await
            .load_wal_transitions_range(1, tip)
            .expect("WAL loads");
        let mut wal = State::new();
        for block in &blocks {
            let rows: Vec<_> = replayed
                .iter()
                .filter(|(h, _)| *h == block.height)
                .map(|(_, t)| t.clone())
                .collect();
            wal.apply_transitions_batch_at_height(block.height, rows)
                .expect("WAL batch applies");
            wal.apply_block_finalization_checked_at(
                wrfcoin_shared_protocol::crypto::chain_binding::protocol_version_at(block.height),
                block,
            )
            .expect("WAL-replayed state matches the block commitment");
        }
        assert_eq!(wal.state_root, producer_root, "WAL replay state_root");
        assert_eq!(
            wal.station_owner_ledger, producer_ledger,
            "WAL replay ledger"
        );
        drop(applier_chain);
        drop(applier_storage);
        let _ = std::fs::remove_file(&storage_path);
    }

    /// COMMIT-REVEAL LIFECYCLE expiry parity. The producer runs a per-block
    /// `prune_commit_reveal_expired` sweep (now via the shared
    /// `State::apply_block_finalization`); the applier replay path previously
    /// skipped it, so a follower replaying the same `ContributionCommit`
    /// accumulated a stale `pending_commit` that never expired and its
    /// `state_root` drifted from the producer's at every height from the first
    /// expiry onward (the live multi-node divergence: same tip, different
    /// state_root). The existing single-block parity tests never crossed an
    /// expiry boundary, so they couldn't catch this.
    ///
    /// At genesis the reveal window is 3 blocks, so a commit at height 1 expires
    /// at height 4 and is pruned at height 5. This mines the commit then empty
    /// blocks across that boundary, applying each to a fresh applier, and
    /// asserts `state_root` + `pending_commits` parity at EVERY height. FAILS
    /// pre-fix (applier never prunes → counts and root diverge at height 5),
    /// PASSES post-fix.
    #[tokio::test]
    async fn producer_applier_commit_reveal_lifecycle_expiry_parity() {
        use wrfcoin_native_chain::blockchain::transaction::{
            CommitPayload, TransactionInput, TransactionOutput,
        };
        use wrfcoin_native_chain::crypto::hash::hash_string;
        use wrfcoin_native_chain::crypto::signature::PrivateKey;

        let station_id = "lifecycle-station";
        let contribution_type = "contribution";
        let timestamp = chrono::Utc::now().timestamp() - 5;
        let metrics =
            serde_json::json!({"quality": 1.0, "volume": 1.0, "uniqueness": 1.0});
        let nonce = "nonce-lifecycle".to_string();
        let commit_id = Transaction::compute_commit_id(
            &metrics,
            &nonce,
            station_id,
            timestamp,
            contribution_type,
        );
        let tx_ts = chrono::DateTime::from_timestamp(timestamp, 0).expect("ts");
        let mut commit_payload = CommitPayload {
            commit_id,
            station_id: station_id.to_string(),
            timestamp,
            contribution_type: contribution_type.to_string(),
            auth_pubkey: None,
            auth_signature: None,
        };
        // #1848 fail-closes commit admission: sign the authorization so the
        // commit is admitted (mirrors the collector/backend signer).
        {
            use rand::SeedableRng;
            let mut rng = rand::rngs::StdRng::seed_from_u64(9903);
            commit_payload.sign_authorization(&PrivateKey::generate(&mut rng));
        }
        let mut commit_tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_string("in-lifecycle-commit"),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 1,
                address: test_address_string(0x99),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        commit_tx.tx_type = TransactionType::ContributionCommit;
        commit_tx.timestamp = tx_ts;
        commit_tx.data = Some(serde_json::to_string(&commit_payload).unwrap());
        commit_tx.set_id();

        // Producer chain with the commit queued for block 1.
        let producer_consensus = test_consensus();
        let producer_chain = Arc::new(RwLock::new(Blockchain::default()));
        let producer_mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        producer_mempool
            .write()
            .await
            .add_transaction(commit_tx)
            .expect("mempool admits contribution commit");

        // Fresh applier chain + P2P apply infra.
        let applier_chain = Arc::new(RwLock::new(Blockchain::default()));
        let applier_storage = test_storage();
        let applier_consensus = test_consensus();
        let mut fork_buffer = ForkCandidateBuffer::new();
        let mut orphan_buffer = OrphanBlockBuffer::new();

        let mut commit_was_present = false;
        let mut commit_was_pruned = false;
        for round in 0..10u64 {
            let block = producer_consensus
                .create_pop_block_with_state(
                    Arc::clone(&producer_chain),
                    Arc::clone(&producer_mempool),
                    &test_address_string(0x61),
                )
                .await
                .unwrap_or_else(|e| panic!("producer mine round {round}: {e}"));
            let height = block.height;

            // Apply the IDENTICAL serialized bytes on the applier (P2P path).
            let bytes = bincode::serialize(&block).expect("block serializes");
            let block_applied: Block =
                bincode::deserialize(&bytes).expect("block deserializes");
            apply_p2p_block(
                block_applied,
                P2pBlockSource::HistoricalSync,
                &applier_chain,
                &applier_storage,
                &applier_consensus,
                None,
                &mut fork_buffer,
                &mut orphan_buffer,
            )
            .await;

            let (p_root, p_commits, a_root, a_commits, a_height) = {
                let p = producer_chain.read().await;
                let a = applier_chain.read().await;
                (
                    p.state.state_root,
                    p.state.pending_commits.len(),
                    a.state.state_root,
                    a.state.pending_commits.len(),
                    a.height,
                )
            };
            assert_eq!(
                a_height, height,
                "[lifecycle h{height}] applier must accept block (height 0 = rejected)"
            );
            assert_eq!(
                p_root, a_root,
                "[lifecycle h{height}] producer/applier state_root diverged \
                 (producer pending_commits={p_commits}, applier pending_commits={a_commits})"
            );
            assert_eq!(
                p_commits, a_commits,
                "[lifecycle h{height}] pending_commits count diverged"
            );
            if p_commits >= 1 {
                commit_was_present = true;
            }
            if commit_was_present && p_commits == 0 {
                commit_was_pruned = true;
                break;
            }
        }
        assert!(
            commit_was_present,
            "the ContributionCommit must have registered into pending_commits"
        );
        assert!(
            commit_was_pruned,
            "producer must have expiry-pruned the commit within 10 blocks \
             (genesis reveal window is 3) — and the applier must have stayed in parity"
        );
    }

    /// STATIC GUARD against the commit-reveal lifecycle divergence class: every
    /// field hashed into the consensus `state_root` MUST be reproducible on a
    /// SYNCING node — reconstructed from block data by `extract_transitions`,
    /// applied by the shared per-block `State::apply_block_finalization`, or
    /// seeded by the deterministic genesis bootstrap. If a hashed field is none
    /// of these, a follower silently forks from the producer at the first block
    /// that touches it (exactly the `pending_commits` expiry bug).
    ///
    /// This pins the state_root field set so ADDING/REMOVING a hashed field is a
    /// deliberate act. If this test fails after you changed `state_root`
    /// material: do NOT just update the list — first confirm the applier
    /// reproduces the field (extract_transitions / apply_block_finalization /
    /// deterministic genesis) and add a producer/applier parity test that
    /// crosses that field's lifecycle. Otherwise multi-node nodes diverge.
    #[test]
    fn state_root_material_fields_are_pinned_for_applier_parity() {
        use std::collections::BTreeSet;
        let actual: BTreeSet<&str> =
            wrfcoin_native_chain::blockchain::state::State::state_root_material_field_names()
                .iter()
                .copied()
                .collect();
        let expected: BTreeSet<&str> = [
            "accounts",
            "nfts",
            "pending_commits",
            "pending_reveals",
            "contribution_reward_records",
            "emission_window_start",
            "emission_window_spent",
            "finalized_contribution_rewards",
            "calibration_cid_bindings",
            "low_quality_streaks",
            "oracle_set",
            "oracle_quorum",
            "attestations",
            "privacy_commitments",
            "privacy_spent_key_images",
            "privacy_spent_nullifiers",
            "privacy_disclosure_ids",
            "spent_oracle_nonces",
            "reward_claim_key_images",
            "slashing_evidence_seen",
            "gamification_claims_seen",
            "feeless_worker_epoch_counts",
            "signing_keys",
            "calibration_attestations",
            "station_registry",
            "station_counts",
            "oracle_local_accuracy",
            "governance",
            "spatial_scores",
            "recent_weather_observations",
            "mining_pools",
            "producer_set_history",
            "genesis_emission_config",
            "last_producer_settlement",
            // #2172 parity confirmation: `producer_applier_and_wal_keep_account_recovery_empty_2172`
            // exercises real P2P application and its persisted WAL replay.
            "account_recovery",
            // #2170 parity confirmation: the combined recovery/session-key
            // test exercises real P2P application and persisted WAL replay.
            "session_keys",
            "authorization_declarations",
            // #2171 parity confirmation: the combined default-off AA test
            // exercises real P2P application and persisted WAL replay.
            "paymasters",
            // #3277 S2: same combined default-off test, empty lifecycle (no
            // extraction until S4, whose populated-list parity test gates
            // any submission).
            "feature_schedule_extensions",
            // #3270 parity confirmation:
            // `producer_applier_and_wal_record_the_same_weather_block_time_3270`
            // crosses the POPULATED lifecycle (feature scheduled, weather
            // reveal mined) through real P2P apply, extracted transitions and
            // persisted WAL replay.
            "station_weather_accepted_block_unix",
            "station_owner_ledger",
            // ADR-003 attester settlement (#2846). Pinned in #2891 only after
            // the demanded parity confirmation existed: the settlement-crossing
            // producer/extractor/WAL test in transition_extractor, the
            // `attester_settlement_*_2846` producer-path scenarios, and
            // supply-bound obligation (f).
            "attester_settlement_due",
            "attester_settlements",
            // Populated deposit, epoch freeze, exit and durable marker parity:
            // producer_applier_and_wal_agree_on_node_bond_lifecycle_1682.
            "node_bond_registry",
            // Mounted real-install/receipt/mining/P2P/extractor proof:
            // actual_bonded_receiver_install_endorse_archive_submit_and_extractor_parity.
            "node_receipt_registry",
            "node_settlement",
            "node_fraud",
        ]
        .into_iter()
        .collect();
        assert_eq!(
            actual, expected,
            "state_root_material field set changed. A field hashed into the \
             consensus state_root MUST be reconstructible on a syncing applier. \
             Before updating this list, confirm the applier reproduces the \
             new/removed field (extract_transitions / apply_block_finalization / \
             deterministic genesis) AND add a producer/applier parity test that \
             crosses its lifecycle — otherwise multi-node nodes silently fork \
             (commit-reveal lifecycle bug class)."
        );
    }

    #[test]
    fn apply_block_finalization_decays_oracle_reputation_1883() {
        use wrfcoin_native_chain::blockchain::state::{State, StateTransition};
        use wrfcoin_shared_protocol::security::key_registry::SigningRole;

        let (oracle_sk, oracle_id) = seeded_keypair(1883);
        let mut state = State::default();
        state
            .apply_transition(StateTransition::SetOracleConfig {
                oracle_set: vec![oracle_id.clone()],
                oracle_quorum: 1,
            })
            .expect("seed oracle config");
        state
            .apply_transition(StateTransition::RegisterSigningKeyEpoch {
                identity: oracle_id.clone(),
                role: SigningRole::Oracle,
                epoch: 1,
                public_key: oracle_id.clone(),
                activated_at: 0,
            })
            .expect("seed oracle signing key");

        for timestamp in 0i64..5 {
            let tx = oracle_attestation_tx(
                &oracle_id,
                &oracle_sk,
                &format!("finalization-decay-{timestamp}"),
                timestamp,
            );
            let payload = tx
                .parse_oracle_attestation_payload()
                .expect("attestation payload parses");
            state
                .apply_transition(StateTransition::RecordOracleAttestation {
                    payload,
                    current_block: timestamp as u64,
                })
                .expect("attestation records reputation signal");
        }

        let before = state.oracle_registry().reputation_score(&oracle_id);
        state.apply_block_finalization(61);
        let after = state.oracle_registry().reputation_score(&oracle_id);

        assert!(
            after < before,
            "shared block finalization must decay oracle reputation so producer, \
             P2P apply, reorg replay, and WAL replay execute the same height-driven \
             mutation (#1883); before={before}, after={after}"
        );
    }

    /// #1731 GUARDRAIL — producer/applier parity for a governance Transfer.
    ///
    /// A block containing a transparent, from-authorized Transfer is mined
    /// on producer chain A, then the IDENTICAL serialized bytes are applied on
    /// a fresh applier B seeded with the same genesis. The producer applies the
    /// transfer exactly once (via `ProcessGovernanceTransaction` →
    /// `apply_transparent_transfer`). Before the fix, the applier's
    /// `extract_transitions` ALSO emitted a standalone `BalanceTransfer` for the
    /// same tx, double-debiting the sender — the batch apply then failed
    /// (`InsufficientSpendableBalance` / `InvalidNonce`), `apply_p2p_block`
    /// rejected the block, and the applier stayed at height 0 while the producer
    /// advanced — a silent fork. This test FAILS without the fix (applier never
    /// reaches height 1) and PASSES with it (state_root parity + single debit).
    #[tokio::test]
    async fn producer_applier_block_apply_parity_governance_transfer() {
        use rand::SeedableRng;
        use wrfcoin_native_chain::blockchain::transaction::{
            transparent_transfer_authorization_message, TransactionInput, TransactionOutput,
        };
        use wrfcoin_native_chain::crypto::hash::hash_string;
        use wrfcoin_native_chain::crypto::signature::{PrivateKey, PublicKey};
        use wrfcoin_shared_protocol::common::types::{Address, Balance};

        // #1727/#1755/#1786: a transparent transfer's debit authority is the
        // ed25519 authorization (`auth_pubkey` + `auth_signature`) from the key
        // of the `from` account being debited. Derive `sender` from a real
        // keypair so the authorization binds correctly.
        let mut rng = rand::rngs::StdRng::seed_from_u64(0x1731_1758);
        let sender_sk = PrivateKey::generate(&mut rng);
        let sender_pk = PublicKey::from_private_key(&sender_sk);
        let sender = Address::from_public_key(&sender_pk);
        let recipient = Address([0x66; 20]);
        let sender_hex = format!("0x{}", hex::encode(sender.0));
        let recipient_hex = format!("0x{}", hex::encode(recipient.0));
        let amount = 1_000u64;
        let nonce = 0u64;

        // Shared genesis: fund the sender identically on producer and applier.
        let seed_genesis: Box<dyn Fn(&mut Blockchain) + Send + Sync> =
            Box::new(move |chain: &mut Blockchain| {
                chain.state.create_account(sender, Balance::new(1_000_000));
                chain.state.recalculate_state_root();
            });

        // Sign the canonical authorization message with the from-account key.
        let auth_message =
            transparent_transfer_authorization_message(&sender_hex, &recipient_hex, amount, nonce);
        let auth_signature = sender_sk.sign(&auth_message).to_string();
        let auth_pubkey = sender_pk.to_string();

        // System-authorized transparent transfer: typed transparent payload +
        // a single null-signature input + verifiable from-account authorization.
        let payload = serde_json::json!({
            "from": sender_hex,
            "to": recipient_hex,
            "amount": amount,
            "transparent_opt_out": true,
            "auth_pubkey": auth_pubkey,
            "auth_signature": auth_signature,
        });
        let mut transfer_tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_string("gov-transfer-input-1731"),
                vout: 0,
                signature: None,
                public_key: Some(sender_hex.clone()),
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 0,
                address: sender_hex.clone(),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        transfer_tx.tx_type = TransactionType::Transfer;
        transfer_tx.sender_address = sender;
        transfer_tx.nonce = nonce;
        transfer_tx.data = Some(serde_json::to_string(&payload).unwrap());
        transfer_tx.set_id();
        assert!(
            transfer_tx.verify_transparent_transfer_authorization(),
            "test transfer must carry from-bound transparent authorization so verify_block accepts it"
        );

        let mut producer_genesis = Blockchain::default();
        seed_genesis(&mut producer_genesis);
        let producer_chain = Arc::new(RwLock::new(producer_genesis));
        let producer_mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        {
            let mut mp = producer_mempool.write().await;
            mp.add_transaction(transfer_tx)
                .expect("mempool admits system transparent transfer");
        }

        let producer_consensus = test_consensus();
        let block_1 = producer_consensus
            .create_pop_block_with_state(
                Arc::clone(&producer_chain),
                Arc::clone(&producer_mempool),
                &test_address_string(0x63),
            )
            .await
            .expect("producer should mine a block containing the governance transfer");

        assert!(
            block_1
                .transactions
                .iter()
                .any(|t| t.tx_type == TransactionType::Transfer),
            "mined block must actually contain the Transfer tx (else the test \
             would vacuously pass); got tx types {:?}",
            block_1
                .transactions
                .iter()
                .map(|t| t.tx_type)
                .collect::<Vec<_>>()
        );

        let applier_chain = apply_block_to_fresh_applier(&block_1, &seed_genesis).await;

        // Headline parity assertion (height 1 on both + identical state_root).
        assert_block_apply_parity("governance-transfer", &producer_chain, &applier_chain)
            .await;

        // Explicit single-debit ledger check on BOTH sides.
        {
            let producer = producer_chain.read().await;
            let applier = applier_chain.read().await;
            assert_eq!(
                producer.state.get_account(&sender).map(|a| a.spendable_balance),
                Some(Balance::new(999_000)),
                "producer sender must be debited exactly once (1_000_000 - 1_000)"
            );
            assert_eq!(
                applier.state.get_account(&sender).map(|a| a.spendable_balance),
                Some(Balance::new(999_000)),
                "applier sender must be debited exactly once — NOT twice (#1731)"
            );
            assert_eq!(
                applier.state.get_account(&recipient).map(|a| a.spendable_balance),
                Some(Balance::new(1_000)),
                "recipient must be credited exactly once"
            );
        }
    }

    // ========================================================================
    // #1758 — producer/applier parity for the 4 privacy tx types that #1731
    // de-duplicated in `extract_transitions` (ConfidentialTransfer,
    // ShieldedDeposit, ShieldedWithdrawal, RewardClaim). Each is mined on
    // producer A (applied once, via `ProcessGovernanceTransaction`) and the
    // identical bytes applied to a fresh applier B; we assert state_root parity
    // and correct SINGLE application. These confirm each privacy type routes
    // exclusively through the shared governance sink (the producer's only
    // application path — `launch_transition_for_transaction` returns None for
    // them), so the #1731 de-dup keeps producer == applier == WAL replay.
    //
    // The applier's `validate_block` runs the ZK pre-pass; the default
    // `RejectAllZkVerifier` would reject any range-proof-bearing privacy tx, so
    // the fresh applier here is given an `AlwaysOkZkVerifier`. The PRODUCER mine
    // path (`create_next_block`) does NOT ZK-verify, so it needs no override.
    // ========================================================================

    /// Test-only ZK verifier that accepts every proof. Lets the applier's
    /// `verify_block` ZK pre-pass admit privacy txs whose range proofs are
    /// placeholder hex (state-application validation still runs in full — only
    /// the consensus-layer ZK gate is stubbed). Mirrors the `AlwaysValid`
    /// verifiers already used in shared-protocol state tests.
    #[derive(Debug)]
    struct AlwaysOkZkVerifier;

    impl wrfcoin_shared_protocol::consensus::zk_verification::ZkVerifier for AlwaysOkZkVerifier {
        fn verify_range_proof(&self, _: &str, _: &str) -> Result<bool, String> {
            Ok(true)
        }
        fn verify_confidential_transfer(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: u64,
            _: usize,
            _: usize,
        ) -> Result<bool, String> {
            Ok(true)
        }
        fn verify_selective_disclosure(&self, _: &str, _: &str) -> Result<bool, String> {
            Ok(true)
        }
        fn verify_weather_model(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: u64,
            _: u64,
        ) -> Result<bool, String> {
            Ok(true)
        }
        fn verify_batch_proof(&self, _: &str, _: &str) -> Result<bool, String> {
            Ok(true)
        }
    }

    /// Build an applier consensus whose ZK pre-pass accepts placeholder proofs.
    async fn permissive_applier_consensus()
    -> Arc<TypedConsensusManager<wrfcoin_consensus_engines::PopConsensusAdapter>> {
        let pop_config = ConsensusFactory::default_config(ConsensusType::ProofOfPublishing);
        let pop_adapter = ConsensusFactory::create_pop_adapter(&pop_config);
        pop_adapter
            .set_zk_verifier(Arc::new(AlwaysOkZkVerifier))
            .await;
        Arc::new(TypedConsensusManager::new(pop_adapter, pop_config))
    }

    /// Like `apply_block_to_fresh_applier` but the applier consensus accepts ZK
    /// proofs (needed for privacy txs whose `verify_block` ZK pre-pass would
    /// otherwise reject placeholder range proofs).
    async fn apply_block_to_fresh_applier_zk_ok(
        block: &Block,
        seed_genesis: &(dyn Fn(&mut Blockchain) + Send + Sync),
    ) -> Arc<RwLock<Blockchain>> {
        let block_bytes = bincode::serialize(block).expect("block serializes");
        let block_applied: Block =
            bincode::deserialize(&block_bytes).expect("block deserializes");

        let mut applier_genesis = Blockchain::default();
        seed_genesis(&mut applier_genesis);
        let applier_chain = Arc::new(RwLock::new(applier_genesis));
        let applier_storage = test_storage();
        let applier_consensus = permissive_applier_consensus().await;
        let mut fork_buffer = ForkCandidateBuffer::new();
        let mut orphan_buffer = OrphanBlockBuffer::new();

        apply_p2p_block(
            block_applied,
            P2pBlockSource::HistoricalSync,
            &applier_chain,
            &applier_storage,
            &applier_consensus,
            None,
            &mut fork_buffer,
            &mut orphan_buffer,
        )
        .await;

        applier_chain
    }

    /// Mine an ordinary block, or assemble a historical privacy block outside
    /// admission policy (Refs #3408), then apply identical bytes to an applier B
    /// (seeded identically, ZK-accepting). Asserts the tx is actually included
    /// and that producer/applier reach height 1 with identical state_root.
    /// Returns `(producer_chain, applier_chain)` for per-test ledger checks.
    async fn mine_and_apply_protocol_tx_parity(
        label: &str,
        seed_genesis: &(dyn Fn(&mut Blockchain) + Send + Sync),
        protocol_tx: Transaction,
    ) -> (Arc<RwLock<Blockchain>>, Arc<RwLock<Blockchain>>) {
        let mut producer_genesis = Blockchain::default();
        seed_genesis(&mut producer_genesis);
        let producer_chain = Arc::new(RwLock::new(producer_genesis));
        let producer_mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let tx_id = protocol_tx.id;
        let historical_privacy = matches!(
            protocol_tx.tx_type,
            TransactionType::ConfidentialTransfer | TransactionType::RewardClaim
        );
        {
            let mut mp = producer_mempool.write().await;
            if historical_privacy {
                mp.add_transaction(protocol_tx.clone())
                    .expect_err("local policy refuses privacy admission, not historical blocks");
            } else {
                mp.add_transaction(protocol_tx.clone())
                    .unwrap_or_else(|e| panic!("[{label}] mempool must admit protocol tx: {e}"));
            }
        }

        let producer_consensus = test_consensus();
        let mut block_1 = producer_consensus
            .create_pop_block_with_state(
                Arc::clone(&producer_chain),
                Arc::clone(&producer_mempool),
                &test_address_string(0x6a),
            )
            .await
            .unwrap_or_else(|e| panic!("[{label}] producer must mine block: {e}"));

        if historical_privacy {
            // These two fixtures move no transparent value and have no oracle
            // effects, so their governance transition commutes with the empty
            // block's reward/finalization. Retain the old incoming-block parity
            // coverage without providing a production mempool bypass.
            let mut producer = producer_chain.write().await;
            producer.state.apply_transition(
                wrfcoin_shared_protocol::blockchain::state::StateTransition::ProcessGovernanceTransaction {
                    tx: protocol_tx.clone(),
                    current_block: block_1.height,
                },
            ).expect("historical privacy transition");
            producer.state.recalculate_state_root();
            block_1.transactions.push(protocol_tx);
            if block_1.metadata.contains_key("state_root") {
                block_1.metadata.insert(
                    "state_root".into(),
                    producer.state.state_root.to_string(),
                );
            }
            let hash = block_1.calculate_hash();
            block_1.set_hash(hash);
            // Install the historical fixture through the existing replay sink
            // so its block index and tip also correspond to these exact bytes.
            let mut replayed = Blockchain::default();
            seed_genesis(&mut replayed);
            wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing::new(1, 60, 0, 10)
                .replay_received_block_for_test_support(&mut replayed, &mut block_1)
                .expect("historical privacy replay");
            assert_eq!(replayed.state.state_root, producer.state.state_root);
            *producer = replayed;
        }

        assert!(
            block_1.transactions.iter().any(|t| t.id == tx_id),
            "[{label}] mined block must contain the protocol tx (else the parity \
             assertion would be vacuous); tx types = {:?}",
            block_1.transactions.iter().map(|t| t.tx_type).collect::<Vec<_>>()
        );

        let applier_chain = apply_block_to_fresh_applier_zk_ok(&block_1, seed_genesis).await;
        assert_block_apply_parity(label, &producer_chain, &applier_chain).await;
        (producer_chain, applier_chain)
    }

    /// #1762: assert a now-disabled shielded tx (`ShieldedDeposit` /
    /// `ShieldedWithdrawal`) is REJECTED at BOTH enforcement points that define
    /// producer/applier parity under the disabled surface:
    ///   * producer-side **mempool admission** — so the producer can never pack
    ///     a block carrying it; and
    ///   * applier-side **block validation** (`verify_block`) — so a forged block
    ///     carrying it is rejected by every syncing peer, even with a ZK verifier
    ///     that accepts the placeholder range proofs.
    ///
    /// This is the #1758 parity property adapted to #1762/#1765, which disable
    /// the shielded surface at the consensus layer pending a real
    /// value-backing/ownership proof. (Pre-#1762 these tests asserted the txs
    /// APPLIED and mutated balances; that behavior is now a rejected forgery.)
    async fn assert_disabled_shielded_tx_rejected(label: &str, tx: Transaction) {
        use rustc_hash::FxHashSet;
        use wrfcoin_shared_protocol::blockchain::state::PopLivenessPolicy;
        use wrfcoin_shared_protocol::consensus::proof_of_publishing::{
            ProofOfPublishing, adjusted_block_reward,
        };

        // (a) Producer-side: mempool admission rejects it outright, so the
        // producer never includes it in a block.
        let mut mempool = Mempool::new(&Config::default());
        assert!(
            mempool.add_transaction(tx.clone()).is_err(),
            "[{label}] mempool must reject the disabled shielded tx at admission (#1762)"
        );

        // (b) Applier-side: a block carrying it is rejected by verify_block. We
        // give the verifier an accepting ZK verifier (AlwaysOk) and a height-0
        // useful-work activation so the rejection is attributable to the #1762
        // disabled-surface gate, not the ZK pre-pass or liveness.
        let height = 1u64;
        let mut block_txs = vec![Transaction::new_coinbase(
            "sd-1762-miner",
            adjusted_block_reward(height),
            height,
        )];
        block_txs.push(tx);

        let mut pop = ProofOfPublishing::new(1, 60, 0, 10);
        pop.set_pop_liveness_policy(PopLivenessPolicy {
            useful_work_activation_height: 0,
        })
        .expect("liveness policy");
        pop.set_zk_verifier(Arc::new(AlwaysOkZkVerifier));
        let score = pop.calculate_block_publishing_score(&block_txs, height);
        let mut block = Block::new(3, CryptoHash::default(), block_txs, 1, height);
        block
            .metadata
            .insert("publishing_score".to_string(), format!("{score}"));

        let accepted = pop
            .verify_block(&block, &FxHashSet::default(), 2, &[])
            .await
            .unwrap_or_else(|e| panic!("[{label}] verify_block returned Err: {e}"));
        assert!(
            !accepted,
            "[{label}] verify_block must REJECT a block carrying the disabled \
             shielded tx (#1762)"
        );
    }

    /// #1758 + #1762 — ShieldedDeposit is disabled at the consensus layer.
    /// Producer/applier parity under the disabled surface means neither side
    /// processes it: mempool admission AND block validation both reject.
    #[tokio::test]
    async fn producer_applier_block_apply_parity_shielded_deposit() {
        use wrfcoin_native_chain::blockchain::transaction::{
            TransactionInput, TransactionOutput,
        };
        use wrfcoin_native_chain::crypto::hash::hash_string;
        use wrfcoin_shared_protocol::common::types::Address;

        let depositor = Address([0x55; 20]);
        let depositor_hex = format!("0x{}", hex::encode(depositor.0));
        let commitment = "cd".repeat(40); // 40 bytes hex (>= 32-byte min)

        let payload = serde_json::json!({
            "shielded_commitment": commitment,
            "range_proof": "ab".repeat(20),
            "transparent_amount": 5_000u64,
        });
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_string("shielded-deposit-input-1758"),
                vout: 0,
                signature: None,
                public_key: Some(depositor_hex.clone()),
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 0,
                address: depositor_hex.clone(),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.tx_type = TransactionType::ShieldedDeposit;
        tx.sender_address = depositor;
        tx.nonce = 0;
        tx.data = Some(serde_json::to_string(&payload).unwrap());
        tx.set_id();

        assert_disabled_shielded_tx_rejected("shielded-deposit", tx).await;
    }

    /// #1758 — ConfidentialTransfer producer/applier parity. A Standard-level
    /// confidential transfer registers an output commitment and advances the
    /// sender nonce; it must apply exactly once through the governance sink.
    #[tokio::test]
    async fn producer_applier_block_apply_parity_confidential_transfer() {
        use wrfcoin_native_chain::blockchain::transaction::{
            TransactionInput, TransactionOutput,
        };
        use wrfcoin_native_chain::crypto::hash::hash_string;
        use wrfcoin_shared_protocol::common::types::{Address, Balance};

        let sender = Address([0x56; 20]);
        let sender_hex = format!("0x{}", hex::encode(sender.0));
        let commitment = "ef".repeat(40); // >= 32-byte min

        let seed: Box<dyn Fn(&mut Blockchain) + Send + Sync> = Box::new(move |chain: &mut Blockchain| {
            chain.state.create_account(sender, Balance::new(1_000_000));
            chain.state.recalculate_state_root();
        });

        let payload = serde_json::json!({
            "output_commitments": [commitment],
            "range_proof": "12".repeat(20),
            "stealth_data": null,
            "ring_signature": null,
            "key_images": [],
            "encrypted_payload": null,
            "privacy_level": "Standard",
            "selective_disclosures": [],
            "signed_message": null,
        });
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_string("confidential-input-1758"),
                vout: 0,
                signature: None,
                public_key: Some(sender_hex.clone()),
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 0,
                address: sender_hex.clone(),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.tx_type = TransactionType::ConfidentialTransfer;
        tx.sender_address = sender;
        tx.nonce = 0;
        tx.data = Some(serde_json::to_string(&payload).unwrap());
        tx.set_id();

        let (producer_chain, applier_chain) =
            mine_and_apply_protocol_tx_parity("confidential-transfer", &seed, tx).await;

        let producer = producer_chain.read().await;
        let applier = applier_chain.read().await;
        assert!(
            producer.state.privacy_commitments.contains(&commitment),
            "producer must register the confidential output commitment"
        );
        assert!(
            applier.state.privacy_commitments.contains(&commitment),
            "applier must register the confidential output commitment exactly as producer"
        );
        // Sender nonce advanced once (no double-apply).
        assert_eq!(
            producer.state.get_account(&sender).map(|a| a.nonce),
            applier.state.get_account(&sender).map(|a| a.nonce),
            "sender nonce must match across producer/applier"
        );
    }

    /// #1758 + #1762 — ShieldedWithdrawal is disabled at the consensus layer.
    /// The pre-#1762 withdrawal credited an arbitrary transparent recipient an
    /// arbitrary amount with `authorization_proof` unverified (an unbacked-mint
    /// vector), so the whole surface is now rejected at mempool admission AND
    /// block validation pending a real value-backing/ownership proof. (Pre-#1762
    /// this asserted the withdrawal applied + credited the recipient once.)
    #[tokio::test]
    async fn producer_applier_block_apply_parity_shielded_withdrawal() {
        use wrfcoin_native_chain::blockchain::transaction::{
            TransactionInput, TransactionOutput,
        };
        use wrfcoin_native_chain::crypto::hash::hash_string;
        use wrfcoin_shared_protocol::common::types::Address;

        let recipient = Address([0x58; 20]);
        let recipient_hex = format!("0x{}", hex::encode(recipient.0));
        let commitment = "9a".repeat(40);
        let nullifier = "7b".repeat(20); // >= 16-byte min

        let payload = serde_json::json!({
            "nullifier": nullifier,
            "shielded_commitment": commitment,
            "range_proof": "cd".repeat(20),
            "transparent_recipient": recipient_hex,
            "transparent_amount": 2_000u64,
            "authorization_proof": "ef".repeat(20),
        });
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_string("shielded-withdrawal-input-1758"),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 0,
                address: recipient_hex.clone(),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.tx_type = TransactionType::ShieldedWithdrawal;
        tx.nonce = 0;
        tx.data = Some(serde_json::to_string(&payload).unwrap());
        tx.set_id();

        assert_disabled_shielded_tx_rejected("shielded-withdrawal", tx).await;
    }

    /// #1758 — RewardClaim producer/applier parity. The anonymized claim is
    /// authorized by a real MLSAG ring signature over active ring-key stations
    /// (replicating the shared-protocol `setup_reward_claim` fixture). On apply
    /// it records the epoch key image and registers the amount commitment
    /// exactly once. Stations + the signature are generated once and seeded
    /// identically into both genesis states so the ring members resolve on the
    /// applier too. (`wrfcoin-privacy` is a dev-dependency so this runs under
    /// `--features p2p` without the optional `privacy` feature.)
    #[tokio::test]
    async fn producer_applier_block_apply_parity_reward_claim() {
        use rand::RngCore;
        use wrfcoin_native_chain::blockchain::transaction::{
            REWARD_CLAIM_EPOCH_BLOCKS, RewardClaimPayload,
        };
        use wrfcoin_shared_protocol::station_registry::{
            ContributionProvenanceClass, DataQualityTier, StationLocation, WeatherStation,
        };

        fn random_keypair() -> ([u8; 32], [u8; 32]) {
            let mut sk_wide = [0u8; 64];
            rand::rngs::OsRng.fill_bytes(&mut sk_wide);
            let scalar = wrfcoin_privacy::Scalar::from_bytes_mod_order_wide(&sk_wide);
            let pk = wrfcoin_privacy::scalar_to_public_key(&scalar);
            (scalar.to_bytes(), pk)
        }

        // Block 1 → epoch 0 (REWARD_CLAIM_EPOCH_BLOCKS = 1000).
        let epoch = 1u64 / REWARD_CLAIM_EPOCH_BLOCKS;
        assert_eq!(epoch, 0, "block-1 reward claim must fall in epoch 0");

        // 8 ring stations (>= REWARD_CLAIM_MIN_RING_SIZE = 7): signer + 7 decoys.
        let keypairs: Vec<([u8; 32], [u8; 32])> = (0..8).map(|_| random_keypair()).collect();
        let stations: Vec<WeatherStation> = keypairs
            .iter()
            .enumerate()
            .map(|(i, (_sk, pk))| WeatherStation {
                station_id: format!("ring-station-{i}"),
                owner_address: format!("owner_ring_{i}"),
                location: StationLocation {
                    latitude: 0.0,
                    longitude: 0.0,
                    elevation_meters: None,
                },
                hardware_model: "test".to_string(),
                calibration_date: None,
                is_active: true,
                reputation_score: 50,
                data_quality_tier: DataQualityTier::default(),
                provenance_class: ContributionProvenanceClass::Real,
                ring_public_key: Some(hex::encode(pk)),
            })
            .collect();

        // Build the MLSAG signature over the canonical reward-claim message.
        let (signer_sk, _signer_pk) = keypairs[0];
        let decoys: Vec<wrfcoin_privacy::RingMember> = keypairs[1..]
            .iter()
            .map(|(_, pk)| wrfcoin_privacy::RingMember { public_key: *pk })
            .collect();
        let key_image = wrfcoin_privacy::KeyImage::generate(&signer_sk).expect("key image");
        let commitment = "ab".repeat(32); // 32-byte commitment hex
        let key_image_hex = hex::encode(key_image.to_bytes());
        let mut payload = RewardClaimPayload {
            epoch,
            ring_signature: String::new(),
            key_image: key_image_hex.clone(),
            amount_commitment: commitment.clone(),
            range_proof: "ff".repeat(32),
            destination: "wrf1destination".to_string(),
        };
        let sig = wrfcoin_privacy::MLSAGSignature::sign(
            &payload.authorization_message(),
            &signer_sk,
            &decoys,
        )
        .expect("sign reward claim");
        payload.ring_signature = hex::encode(serde_json::to_vec(&sig).unwrap());

        let stations_for_seed = stations.clone();
        let seed: Box<dyn Fn(&mut Blockchain) + Send + Sync> = Box::new(move |chain: &mut Blockchain| {
            for station in &stations_for_seed {
                chain
                    .state
                    .station_registry
                    .register(station.clone())
                    .expect("register ring station");
            }
            chain.state.recalculate_state_root();
        });

        let mut tx = Transaction::new(
            vec![wrfcoin_native_chain::blockchain::transaction::TransactionInput {
                txid: wrfcoin_native_chain::crypto::hash::hash_string("reward-claim-input-1758"),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![wrfcoin_native_chain::blockchain::transaction::TransactionOutput {
                amount: 0,
                address: "0x00000000000000000000000000000000000000ff".to_string(),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.tx_type = TransactionType::RewardClaim;
        tx.nonce = 0;
        tx.data = Some(serde_json::to_string(&payload).unwrap());
        tx.set_id();

        let (producer_chain, applier_chain) =
            mine_and_apply_protocol_tx_parity("reward-claim", &seed, tx).await;

        let producer = producer_chain.read().await;
        let applier = applier_chain.read().await;
        let ki_entry = format!("{epoch}:{key_image_hex}");
        assert!(
            producer.state.reward_claim_key_images.contains(&ki_entry),
            "producer must record the reward-claim key image"
        );
        assert!(
            applier.state.reward_claim_key_images.contains(&ki_entry),
            "applier must record the reward-claim key image exactly as producer"
        );
        assert!(
            producer.state.privacy_commitments.contains(&commitment)
                && applier.state.privacy_commitments.contains(&commitment),
            "both sides must register the claim's amount commitment exactly once"
        );
    }

    /// mobile#215 — end-to-end producer/applier parity for an authorized
    /// gamification payout. A `Reward` tx carrying a `GamificationClaimPayload`
    /// is mined into block 1 on the producer and the identical block bytes are
    /// applied to a fresh applier. Both must reach byte-identical `state_root`,
    /// credit the recipient exactly once, and record the `claim_id` in the
    /// consensus dedup set — the property that keeps a payout from forking the
    /// chain (#1811 class).
    #[tokio::test]
    async fn producer_applier_block_apply_parity_gamification_payout() {
        let recipient_str = test_address_string(0x33);
        let recipient: wrfcoin_shared_protocol::common::types::Address =
            recipient_str.parse().expect("valid recipient address");

        // #2081 review: the payout must carry a valid operator authorization or
        // `verify_block` rejects the block. Install a fixed operator key into the
        // consensus allowlist (deterministic + set-only) and sign with it. The
        // write itself is serialized on the process-env lock.
        let tx = {
            use rand::SeedableRng;
            use wrfcoin_shared_protocol::crypto::domain_signing::GAMIFICATION_OPERATOR_PUBKEYS_ENV;
            use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
            // Takes THE process-env lock. Writing the same value from a fixed
            // seed does NOT make this safe: `setenv` mutates the `environ`
            // array itself, so concurrent calls can reallocate and free it
            // under a reader regardless of the value written. That is a
            // libc-level data race, and why Rust 2024 marks `set_var` unsafe.
            let _env_guard = crate::helpers::test_env::lock();
            let mut rng = rand::rngs::StdRng::seed_from_u64(215_215);
            let operator = PrivateKey::generate(&mut rng);
            std::env::set_var(
                GAMIFICATION_OPERATOR_PUBKEYS_ENV,
                PublicKey::from_private_key(&operator).to_hex(),
            );
            Transaction::new_gamification_reward(&recipient_str, 500, "claim-parity", &operator)
        };

        let seed: Box<dyn Fn(&mut Blockchain) + Send + Sync> = Box::new(|_chain| {});

        // Mines on producer, applies identical bytes to a fresh applier, and
        // asserts the tx was included AND producer.state_root == applier.state_root.
        let (producer_chain, applier_chain) =
            mine_and_apply_protocol_tx_parity("gamification-payout", &seed, tx).await;

        let producer = producer_chain.read().await;
        let applier = applier_chain.read().await;

        for (label, chain) in [("producer", &*producer), ("applier", &*applier)] {
            let acct = chain
                .state
                .get_account(&recipient)
                .unwrap_or_else(|| panic!("[{label}] recipient must be credited"));
            assert_eq!(acct.balance.0, 500, "[{label}] payout amount credited once");
            assert!(
                chain.state.gamification_claims_seen.contains("claim-parity"),
                "[{label}] claim id must be recorded in the consensus dedup set"
            );
        }
    }

    /// #2055 Option A: an authorized on-chain oracle registration mined into a
    /// block applies IDENTICALLY on the producer and a fresh applier —
    /// producer.state_root == applier.state_root — and lands in `OracleRegistry`
    /// on both (the registration is gossiped/extracted like any other tx, unlike
    /// the out-of-band admin route). This is the fork-safety the feature exists
    /// for.
    /// See the `_env_guard` comment in the body: the lock must span the mine,
    /// so the guard is held across awaits by design.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn producer_applier_block_apply_parity_oracle_registration() {
        // Oracle ids must be valid ed25519 public keys (the config apply derives
        // a legacy signing key from each), so use a real keypair as the operator
        // identity and its signing key.
        use rand::SeedableRng;
        use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
        let oracle_key = PrivateKey::generate(&mut rand::rngs::StdRng::seed_from_u64(2055_777));
        let identity = PublicKey::from_private_key(&oracle_key).to_hex();
        let signing_pubkey = identity.clone();

        // The guard must outlive the MINE, not just the construction (#2055/S4).
        //
        // The allowlist is read at VERIFICATION time — the producer self-screen
        // calls `verify_oracle_registration_authorization` while building the
        // block — so releasing the lock after signing leaves a window in which
        // another test installs its own governance key, this registration fails
        // to authorize, the #1730 self-screen drops it, and the harness fails on
        // "mined block must contain the protocol tx". That is a flake, and it
        // was latent here until a second oracle test with a different governance
        // seed existed to lose the race against.
        let _env_guard = crate::helpers::test_env::lock();
        let tx = {
            use wrfcoin_shared_protocol::crypto::domain_signing::ORACLE_GOVERNANCE_PUBKEYS_ENV;
            // Fixed governance key (seed 2055_215). Every oracle test installs
            // the same value, but that is NOT what makes this safe — `setenv`
            // mutates the `environ` array, so concurrent writes race at the
            // libc level whatever the value. Hence THE process-env lock.
            let gov = PrivateKey::generate(&mut rand::rngs::StdRng::seed_from_u64(2055_215));
            std::env::set_var(
                ORACLE_GOVERNANCE_PUBKEYS_ENV,
                PublicKey::from_private_key(&gov).to_hex(),
            );
            Transaction::new_oracle_registration(
                vec![identity.clone()],
                1,
                &identity,
                &signing_pubkey,
                2,
                1_700_000_000,
                &gov,
            )
        };

        let seed: Box<dyn Fn(&mut Blockchain) + Send + Sync> = Box::new(|_chain| {});
        let producer_chain = Arc::new(RwLock::new(Blockchain::default()));
        let producer_mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let tx_id = tx.id;
        producer_mempool
            .write()
            .await
            .add_transaction(tx)
            .expect("oracle-registration mempool must admit the protocol tx");

        let producer_consensus =
            test_consensus_with_genesis_oracle_set(vec![identity.clone()], 1).await;
        let block_1 = producer_consensus
            .create_pop_block_with_state(
                Arc::clone(&producer_chain),
                Arc::clone(&producer_mempool),
                &test_address_string(0x6a),
            )
            .await
            .expect("oracle-registration producer must mine block 1");
        assert!(
            block_1.transactions.iter().any(|candidate| candidate.id == tx_id),
            "oracle-registration block must include its protocol transaction"
        );

        let applier_chain = apply_block_to_fresh_applier_zk_ok(&block_1, &seed).await;
        assert_block_apply_parity("oracle-registration", &producer_chain, &applier_chain).await;

        let producer = producer_chain.read().await;
        let applier = applier_chain.read().await;

        // state_root parity is asserted inside the harness; additionally confirm
        // the registration landed in OracleRegistry on BOTH sides.
        for (label, chain) in [("producer", &*producer), ("applier", &*applier)] {
            assert!(
                chain.state.oracle_set.contains(&identity),
                "[{label}] oracle identity must be in the oracle_set after registration"
            );
            assert_eq!(
                chain.state.oracle_quorum, 1,
                "[{label}] oracle quorum must be applied"
            );
        }
    }

    /// Result of mining an attested-reward block on a producer chain. Carries
    /// everything needed to apply the same block to a fresh applier with an
    /// identically-seeded genesis.
    struct AttestedRewardScenario {
        producer_chain: Arc<RwLock<Blockchain>>,
        /// The producer's mempool after mining (#3565: deferred reveals stay here).
        producer_mempool: Arc<RwLock<Mempool>>,
        block_1: Block,
        recipient: wrfcoin_shared_protocol::common::types::Address,
        /// Re-seed an applier genesis exactly like the producer's.
        seed_genesis: Box<dyn Fn(&mut Blockchain) + Send + Sync>,
    }

    /// Build + mine a single block containing a coinbase, a contribution
    /// commit+reveal at the given `quality`, and two signed oracle attestations
    /// that reach quorum (so the NON-empty `process_contribution_rewards` path
    /// runs). The oracle config + reward recipient account are seeded into
    /// genesis (shared by producer and applier, mirroring a real network's
    /// shared genesis). Mirrors the shared-protocol test helpers
    /// `build_commit_and_reveal` / `signed_attestation_tx`.
    async fn mine_attested_reward_block(quality: f64) -> AttestedRewardScenario {
        mine_attested_reward_block_with_genesis(quality, Arc::new(|_| {})).await
    }

    async fn mine_attested_reward_block_with_genesis(
        quality: f64,
        configure_genesis: Arc<dyn Fn(&mut Blockchain) + Send + Sync>,
    ) -> AttestedRewardScenario {
        mine_attested_reward_block_with_provenance_and_genesis(
            quality,
            wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass::Real,
            configure_genesis,
        )
        .await
    }

    async fn mine_attested_reward_block_with_provenance_and_genesis(
        quality: f64,
        provenance_class: wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass,
        configure_genesis: Arc<dyn Fn(&mut Blockchain) + Send + Sync>,
    ) -> AttestedRewardScenario {
        mine_attested_reward_block_at_base_height(quality, provenance_class, configure_genesis, 0)
            .await
    }

    /// Same scenario, but the producer chain sits at `base_height` (empty
    /// window, deterministic tip) so the mined block lands at
    /// `base_height + 1` — used to build the attested-reward shape AT the
    /// #2777 / RICH_AMBIENT activation fence with the real producer (#2806).
    /// `base_height == 0` is the historical genesis shape.
    async fn mine_attested_reward_block_at_base_height(
        quality: f64,
        provenance_class: wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass,
        configure_genesis: Arc<dyn Fn(&mut Blockchain) + Send + Sync>,
        base_height: u64,
    ) -> AttestedRewardScenario {
        mine_attested_reward_block_with_reveal_data(
            quality,
            provenance_class,
            configure_genesis,
            base_height,
            None,
        )
        .await
    }

    /// [`mine_attested_reward_block_at_base_height`] with an optional reveal
    /// `data` override (#3270: a WEATHER payload, so the reveal records the
    /// station's last accepted reading). `None` is the historical metrics
    /// payload, byte for byte.
    async fn mine_attested_reward_block_with_reveal_data(
        quality: f64,
        provenance_class: wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass,
        configure_genesis: Arc<dyn Fn(&mut Blockchain) + Send + Sync>,
        base_height: u64,
        reveal_data: Option<serde_json::Value>,
    ) -> AttestedRewardScenario {
        mine_attested_reward_block_with_reveals(
            quality,
            provenance_class,
            configure_genesis,
            base_height,
            reveal_data,
            1,
            5,
        )
        .await
    }

    /// [`mine_attested_reward_block_with_reveal_data`] with `reveal_count`
    /// quorum-attested commit/reveal pairs from the SAME station staged in one
    /// mempool (#3565), each stamped `reveal_age_secs` before now. `(1, 5)` is
    /// the historical single-reveal scenario, byte for byte (same nonce, tags
    /// and timestamp offset).
    async fn mine_attested_reward_block_with_reveals(
        quality: f64,
        provenance_class: wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass,
        configure_genesis: Arc<dyn Fn(&mut Blockchain) + Send + Sync>,
        base_height: u64,
        reveal_data: Option<serde_json::Value>,
        reveal_count: usize,
        reveal_age_secs: i64,
    ) -> AttestedRewardScenario {
        mine_attested_reward_block_with_options(
            quality,
            provenance_class,
            configure_genesis,
            base_height,
            reveal_data,
            reveal_count,
            reveal_age_secs,
            false,
            &[],
            &[],
        )
        .await
    }

    /// The full #3565 fixture. `owner_signs_commit` registers the station to
    /// the address of the commit-authorization key (so commits pass the gated
    /// owner binding); `false` keeps the historical fixed owner `0x77…`.
    /// `unattested` lists reveal groups that get NO oracle attestations (they
    /// lack quorum and are held back by the #2777 fence at or above its
    /// activation height). `forecast_typed_groups` (#3565 review B1) turns those
    /// groups into an attacker's: a `ContributionCommit`/`ContributionReveal`
    /// whose payload says `contribution_type: "forecast"`, signed by a key that
    /// is NOT the station owner. `(false, &[], &[])` is the historical fixture,
    /// byte for byte.
    #[allow(clippy::too_many_arguments)]
    async fn mine_attested_reward_block_with_options(
        quality: f64,
        provenance_class: wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass,
        configure_genesis: Arc<dyn Fn(&mut Blockchain) + Send + Sync>,
        base_height: u64,
        reveal_data: Option<serde_json::Value>,
        reveal_count: usize,
        reveal_age_secs: i64,
        owner_signs_commit: bool,
        unattested: &[usize],
        forecast_typed_groups: &[usize],
    ) -> AttestedRewardScenario {
        use wrfcoin_native_chain::blockchain::state::StateTransition;
        use wrfcoin_native_chain::blockchain::transaction::{
            CommitPayload, OracleAttestationPayload, RevealPayload, TransactionInput,
            TransactionOutput,
        };
        use wrfcoin_native_chain::crypto::hash::hash_string;
        use wrfcoin_native_chain::crypto::signature::{PrivateKey, PublicKey};
        use wrfcoin_shared_protocol::common::types::{Address, Balance};
        use wrfcoin_shared_protocol::station_registry::{
            DataQualityTier, StationLocation, WeatherStation,
        };

        fn oracle_identity(seed: u64) -> (PrivateKey, String) {
            use rand::SeedableRng;
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            let sk = PrivateKey::generate(&mut rng);
            let id = PublicKey::from_private_key(&sk).to_string();
            (sk, id)
        }
        let (o1_sk, o1_id) = oracle_identity(901);
        let (o2_sk, o2_id) = oracle_identity(902);

        let recipient = if owner_signs_commit {
            let derived = PublicKey::from_private_key(&oracle_identity(903).0).to_address();
            let bytes: [u8; 20] = hex::decode(derived.trim_start_matches("wrf"))
                .expect("derived address is hex")
                .try_into()
                .expect("derived address is 20 bytes");
            Address(bytes)
        } else {
            Address([0x77; 20])
        };
        let station_id = "station-attest";
        let o1_id_for_seed = o1_id.clone();
        let o2_id_for_seed = o2_id.clone();
        let configure_genesis_for_seed = Arc::clone(&configure_genesis);
        let seed_genesis: Box<dyn Fn(&mut Blockchain) + Send + Sync> =
            Box::new(move |chain: &mut Blockchain| {
                chain
                    .apply_state_transition(StateTransition::SetOracleConfig {
                        oracle_set: vec![o1_id_for_seed.clone(), o2_id_for_seed.clone()],
                        oracle_quorum: 2,
                    })
                    .expect("seed oracle config");
                chain.state.create_account(recipient, Balance::new(0));
                chain
                    .state
                    .register_weather_station(WeatherStation {
                        station_id: station_id.to_string(),
                        owner_address: recipient.to_string(),
                        location: StationLocation {
                            latitude: 10.123456,
                            longitude: 20.654321,
                            elevation_meters: Some(220.0),
                        },
                        hardware_model: "parity-fixture".to_string(),
                        calibration_date: None,
                        is_active: true,
                        reputation_score: 50,
                        data_quality_tier: DataQualityTier::Gold,
                        provenance_class,
                        ring_public_key: None,
                    })
                    .expect("seed canonical station provenance");
                configure_genesis_for_seed(chain);
                chain.state.recalculate_state_root();
            });

        let contribution_type = "contribution";
        // Near-now timestamp so the commit/reveal/attestation fall inside the
        // producer's 3600s submission window (the consensus manager runs on the
        // wall clock; a fixed past timestamp is filtered as stale and the reward
        // path never runs).
        let timestamp: i64 = chrono::Utc::now().timestamp() - reveal_age_secs;
        let metrics = reveal_data.unwrap_or_else(|| {
            serde_json::json!({"quality": quality, "volume": 1.0, "uniqueness": 1.0})
        });
        let tx_ts = chrono::DateTime::from_timestamp(timestamp, 0).expect("ts");
        let mk_tx = |tx_type: TransactionType, tag: &str, data: String| {
            let mut tx = Transaction::new(
                vec![TransactionInput {
                    txid: hash_string(&format!("in-{tag}")),
                    vout: 0,
                    signature: None,
                    public_key: None,
                    coinbase_data: None,
                }],
                vec![TransactionOutput {
                    amount: 1,
                    address: format!("0x{}", hex::encode(recipient.0)),
                    script: "OP_TRUE".to_string(),
                    nft_data: None,
                }],
            );
            tx.tx_type = tx_type;
            tx.timestamp = tx_ts;
            tx.data = Some(data);
            tx.set_id();
            tx
        };

        let mut pop_txs: Vec<Transaction> = Vec::with_capacity(4 * reveal_count);
        for index in 0..reveal_count {
            let sfx = if index == 0 { String::new() } else { format!("-{index}") };
            let nonce = format!("nonce-attest{sfx}");
            let attacker = forecast_typed_groups.contains(&index);
            let contribution_type = if attacker { "forecast" } else { contribution_type };
            let commit_id = Transaction::compute_commit_id(
                &metrics,
                &nonce,
                station_id,
                timestamp,
                contribution_type,
            );

            let mut commit_payload = CommitPayload {
                commit_id: commit_id.clone(),
                station_id: station_id.to_string(),
                timestamp,
                contribution_type: contribution_type.to_string(),
                auth_pubkey: None,
                auth_signature: None,
            };
            // #1848 fail-closes commit admission: an unsigned commit is rejected at
            // the mempool. Sign the authorization exactly as the collector/backend
            // do (the auth key is not bound to the reward recipient — the reward
            // follows the reveal sender — so a dedicated fixture key is fine).
            commit_payload.sign_authorization(&oracle_identity(if attacker { 904 } else { 903 }).0);
            let reveal_payload = RevealPayload {
                commit_id: commit_id.clone(),
                station_id: station_id.to_string(),
                timestamp,
                contribution_type: contribution_type.to_string(),
                data: metrics.clone(),
                nonce,
                data_cid: None,
                data_hash: None,
                storage_provider: None,
                provenance: None,
                manifest: None,
                verification_receipt: None,
                compute_receipt: None,
                quality_tier: None,
            };


            let commit_tx = mk_tx(
                TransactionType::ContributionCommit,
                &format!("commit{sfx}"),
                serde_json::to_string(&commit_payload).unwrap(),
            );
            let mut reveal_tx = mk_tx(
                TransactionType::ContributionReveal,
                &format!("reveal{sfx}"),
                serde_json::to_string(&reveal_payload).unwrap(),
            );
            reveal_tx.sender_address = recipient;
            // Non-zero fee so the reveal is not treated as a feeless weather tx
            // (which would require a valid compute receipt and be rejected by
            // `verify_block`). Mirrors the shared-protocol mining tests.
            reveal_tx.fee = 1;
            reveal_tx.set_id();

            let signed_attestation = |sk: &PrivateKey, oracle_id: &str, tag: &str| {
                let mut payload = OracleAttestationPayload {
                    commit_id: commit_id.clone(),
                    oracle_id: oracle_id.to_string(),
                    approved: true,
                    signature: None,
                    confidence: Some(1.0),
                    timestamp,
                    nonce: Some(format!("nonce-{tag}")),
                };
                let sig = sk.sign(payload.signing_message().as_bytes()).to_string();
                payload.signature = Some(sig);
                mk_tx(
                    TransactionType::OracleAttestation,
                    tag,
                    serde_json::to_string(&payload).unwrap(),
                )
            };
            if unattested.contains(&index) {
                pop_txs.extend([commit_tx, reveal_tx]);
                continue;
            }
            let att1 = signed_attestation(&o1_sk, &o1_id, &format!("att1{sfx}"));
            let att2 = signed_attestation(&o2_sk, &o2_id, &format!("att2{sfx}"));
            pop_txs.extend([commit_tx, reveal_tx, att1, att2]);
        }

        let mut producer_genesis = Blockchain::default();
        seed_genesis(&mut producer_genesis);
        if base_height > 0 {
            producer_genesis.height = base_height;
            producer_genesis.tip =
                wrfcoin_native_chain::crypto::hash::hash_string(&format!("base-tip-{base_height}"));
        }
        let producer_chain = Arc::new(RwLock::new(producer_genesis));
        let producer_mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        {
            // #1722/#1723/#1754: oracle-attestation mempool admission now
            // fail-closes unless the signing-key registry is present AND the
            // oracle identity is registered for the Oracle role. Build a
            // registry seeding both oracles (mirrors production, where every
            // `oracle_set` member is seeded via `ensure_legacy_oracle_keys`).
            use wrfcoin_shared_protocol::security::key_registry::{
                SigningKeyRegistry, SigningRole,
            };
            let mut registry = SigningKeyRegistry::default();
            registry
                .ensure_legacy_identity_key(&o1_id, SigningRole::Oracle, 0)
                .expect("register oracle 1 signing key");
            registry
                .ensure_legacy_identity_key(&o2_id, SigningRole::Oracle, 0)
                .expect("register oracle 2 signing key");

            let mut mp = producer_mempool.write().await;
            for tx in pop_txs {
                mp.add_transaction_with_signing_keys(tx, Some(&registry))
                    .expect("mempool admits pop tx");
            }
        }

        let producer_consensus = test_consensus();
        let block_1 = producer_consensus
            .create_pop_block_with_state(
                Arc::clone(&producer_chain),
                Arc::clone(&producer_mempool),
                &test_address_string(0x62),
            )
            .await
            .expect("producer should mine attested-reward block via create_next_block");

        AttestedRewardScenario {
            producer_chain,
            producer_mempool,
            block_1,
            recipient,
            seed_genesis,
        }
    }

    #[tokio::test]
    async fn producer_applier_block_apply_parity_attested_reward() {
        // High-quality reveal: reaches quorum and mints a reward, so the
        // NON-empty `process_contribution_rewards` path runs. This guards the
        // #1681 (telemetry-exclusion) + #1683 (final root recompute) interaction
        // on the reward path. quality_policy_events/audit_events stay empty here
        // (no challenge), and the root converges.
        let scenario = mine_attested_reward_block(1.0).await;

        assert_eq!(
            scenario
                .block_1
                .metadata
                .get("rewards_minted_count")
                .map(String::as_str),
            Some("1"),
            "attested-reward scenario must mint exactly one reward (quorum path); \
             got metadata {:?}",
            scenario.block_1.metadata.get("rewards_minted_count")
        );

        let applier_chain =
            apply_block_to_fresh_applier(&scenario.block_1, &scenario.seed_genesis).await;

        assert_block_apply_parity(
            "attested-reward",
            &scenario.producer_chain,
            &applier_chain,
        )
        .await;
    }

    /// #2806 GREEN control at the activation fence, through the production
    /// historical-sync transport. The retired chain wedged every fresh joiner at
    /// 11,016 because canonical block 11,017 carried a Real reveal with one
    /// approval against quorum two — produced under a producer that did not yet
    /// wait for quorum, rejected by the validator that does. On the current
    /// binary the producer defers such reveals from the SAME fence with the SAME
    /// quorum source the replay invariant reads (#2777), so the block it builds
    /// at the fence carries the quorum and a fresh follower at the same height
    /// applies it: this is the shape that wedged, crossing the fence. The RED
    /// control is `authoritative_historical_sync_rejects_real_reveal_below_quorum_2786`
    /// (one approval short halts at exactly this height, attributed locally).
    #[tokio::test]
    async fn fresh_follower_crosses_the_reveal_quorum_fence_2806() {
        use wrfcoin_shared_protocol::consensus::proof_of_publishing::{
            ProofOfPublishing, RICH_AMBIENT_REWARD_ACTIVATION_HEIGHT,
        };
        let base = RICH_AMBIENT_REWARD_ACTIVATION_HEIGHT - 1;
        let scenario = mine_attested_reward_block_at_base_height(
            1.0,
            wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass::Real,
            Arc::new(|_| {}),
            base,
        )
        .await;
        let block = scenario.block_1.clone();
        assert_eq!(
            block.height, RICH_AMBIENT_REWARD_ACTIVATION_HEIGHT,
            "the producer must build AT the fence, where the quorum rule is live"
        );
        assert_eq!(
            block
                .transactions
                .iter()
                .filter(|tx| tx.tx_type == TransactionType::OracleAttestation)
                .count(),
            2,
            "the producer must have included the reveal together with its quorum (#2777)"
        );
        assert!(
            block
                .transactions
                .iter()
                .any(|tx| tx.tx_type == TransactionType::ContributionReveal),
            "the block must carry the Real reveal — the 11,017 shape, with quorum"
        );
        assert_eq!(
            block.metadata.get("rewards_minted_count").map(String::as_str),
            Some("1"),
            "the quorum reward path must have run at the fence"
        );

        // The fresh follower: same genesis seed, positioned at the fence.
        let mut authoritative = Blockchain::default();
        (scenario.seed_genesis)(&mut authoritative);
        authoritative.height = base;
        authoritative.tip = block.prev_block_hash;
        assert_eq!(authoritative.state.oracle_quorum, 2);
        ProofOfPublishing::verify_launch_state_invariants(&authoritative, &block)
            .expect("a producer-built quorum block must pass the fence invariant");

        // Through the context-free sync shadow first, exactly as IBD does.
        let mut sync_shadow = Blockchain::default();
        sync_shadow.height = base;
        sync_shadow.tip = block.prev_block_hash;
        let source_peer = libp2p::PeerId::random();
        let event_rx =
            handle_sync_response_for_test(source_peer, vec![block.clone()], &mut sync_shadow)
                .await
                .unwrap_or_else(|error| {
                    panic!("fence block stalled in handle_response: {error:?}")
                });
        assert_eq!(sync_shadow.height, RICH_AMBIENT_REWARD_ACTIVATION_HEIGHT);

        let follower = Arc::new(RwLock::new(authoritative));
        let (p2p, mut command_rx) = P2pClient::for_testing_with_commands(source_peer);
        run_test_p2p_event_transport(event_rx, Arc::clone(&follower), p2p).await;

        let chain = follower.read().await;
        assert_eq!(
            chain.height, RICH_AMBIENT_REWARD_ACTIVATION_HEIGHT,
            "a fresh follower must cross the fence on a producer-built block"
        );
        assert_eq!(chain.tip, block.hash.expect("producer-sealed block has a hash"));
        assert_eq!(
            chain.state.state_root,
            scenario.producer_chain.read().await.state.state_root,
            "follower and producer must agree on the post-fence state root"
        );
        drop(chain);

        let mut saw_accepted_update = false;
        while let Ok(command) = command_rx.try_recv() {
            match command {
                P2pCommand::UpdateLocalChainState { height, .. }
                    if height == RICH_AMBIENT_REWARD_ACTIVATION_HEIGHT =>
                {
                    saw_accepted_update = true;
                },
                P2pCommand::BroadcastChainStatus { .. } => {},
                P2pCommand::AuthoritativeSyncRejected { reason, .. } => panic!(
                    "a producer-built quorum block must not be rejected at the fence: {reason}"
                ),
                other => panic!("unexpected command while crossing the fence: {other:?}"),
            }
        }
        assert!(saw_accepted_update, "the accepted fence height must reach the P2P worker");
    }

    /// Regression for the live IBD stall at an honest attestation block.
    ///
    /// The P2P task owns only a context-free ordering/hash shadow. Its default
    /// state intentionally lacks the canonical oracle registry. The synced
    /// block must therefore cross that shadow and reach `apply_p2p_block`,
    /// where the launch API validates it against the real chain state seeded
    /// with the registered oracle set.
    #[tokio::test]
    async fn sync_shadow_forwards_honest_attestation_to_authoritative_application_2786() {
        let scenario = mine_attested_reward_block(1.0).await;
        assert_eq!(
            scenario
                .block_1
                .transactions
                .iter()
                .filter(|tx| tx.tx_type == TransactionType::OracleAttestation)
                .count(),
            2,
            "fixture must contain the two honest oracle attestations"
        );

        let mut authoritative_genesis = Blockchain::default();
        (scenario.seed_genesis)(&mut authoritative_genesis);
        assert_eq!(
            authoritative_genesis.state.oracle_set.len(),
            2,
            "authoritative state must carry the registered oracle set"
        );
        assert_eq!(authoritative_genesis.state.oracle_quorum, 2);

        let mut sync_shadow = Blockchain::default();
        assert!(
            sync_shadow.state.oracle_set.is_empty(),
            "precondition: context-free sync shadow must not fabricate oracle state"
        );
        let source_peer = libp2p::PeerId::random();
        let event_rx = handle_sync_response_for_test(
            source_peer,
            vec![scenario.block_1.clone()],
            &mut sync_shadow,
        )
        .await
        .unwrap_or_else(|error| {
            panic!(
                "honest attestation block stalled in handle_response before event transport: \
                 {error:?}"
            )
        });
        assert_eq!(
            sync_shadow.height, 1,
            "the context-free handle_response shadow must advance"
        );

        let applier_chain = Arc::new(RwLock::new(authoritative_genesis));
        let (p2p, mut command_rx) = P2pClient::for_testing_with_commands(source_peer);
        run_test_p2p_event_transport(event_rx, Arc::clone(&applier_chain), p2p).await;

        let mut saw_accepted_update = false;
        while let Ok(command) = command_rx.try_recv() {
            match command {
                P2pCommand::UpdateLocalChainState { height: 1, .. } => {
                    saw_accepted_update = true;
                },
                P2pCommand::BroadcastChainStatus { height: 1, .. } => {},
                P2pCommand::AuthoritativeSyncRejected { peer_id, .. } => panic!(
                    "honest block from {peer_id} must not produce authoritative rejection feedback"
                ),
                other => panic!("unexpected honest-sync command: {other:?}"),
            }
        }
        assert!(
            saw_accepted_update,
            "full SyncedBlock event transport must update the P2P worker after authoritative apply"
        );
        {
            let applied = applier_chain.read().await;
            assert_eq!(applied.height, 1, "authoritative P2P apply must advance");
            let applied_approvals = applied
                .state
                .attestations
                .values()
                .map(|set| set.approvals())
                .sum::<usize>();
            assert_eq!(
                applied_approvals, 2,
                "authoritative state must apply both exact honest attestations"
            );
        }
        assert_block_apply_parity(
            "sync-shadow-attested-authoritative",
            &scenario.producer_chain,
            &applier_chain,
        )
        .await;
    }

    #[tokio::test]
    async fn authoritative_historical_sync_rejects_unsigned_seeded_producer_2788() {
        use rand::SeedableRng;
        use wrfcoin_native_chain::crypto::signature::{PrivateKey, PublicKey};
        use wrfcoin_shared_protocol::consensus::producer_set::{
            ProducerMember, ProducerSetChange,
        };

        let member_key = PrivateKey::generate(&mut rand::rngs::StdRng::seed_from_u64(2788));
        let member_public_key = PublicKey::from_private_key(&member_key);
        let member = ProducerMember {
            producer_id: "authoritative-member-2788".to_string(),
            payout_address: test_address_string(0x28),
            public_key: member_public_key.as_bytes().try_into().unwrap(),
        };
        let mut authoritative = Blockchain::default();
        authoritative
            .state
            .producer_set_history
            .push(ProducerSetChange::new(0, 0, vec![member]));

        let block = test_block(1, authoritative.tip, 2788);
        assert!(block.producer_sig.is_empty(), "control block must be unsigned");
        let authority_error =
            wrfcoin_shared_protocol::consensus::producer_authority::verify_block_producer_authorized(
                &block,
                &authoritative.state.producer_set_history,
            )
            .expect_err("seeded authoritative state must reject an unsigned block");
        assert!(
            authority_error.contains("Unsigned"),
            "control must fail specifically as unsigned, got: {authority_error}"
        );

        let chain = Arc::new(RwLock::new(authoritative));
        let before = chain_mark(&chain).await;
        let storage = test_storage();
        let consensus = test_consensus();
        let mut fork_buffer = ForkCandidateBuffer::new();
        let mut orphan_buffer = OrphanBlockBuffer::new();
        let outcome = apply_p2p_block_at(
            SigningProtocolVersion::V2,
            block,
            P2pBlockSource::HistoricalSync,
            &chain,
            &storage,
            &consensus,
            None,
            &mut fork_buffer,
            &mut orphan_buffer,
            std::convert::identity,
        )
        .await;

        // #2807: attribution deliberately changed from `PeerInvalid` to
        // `LocalRuleRejection`. An unsigned block and a canonical block the
        // local rules reject are indistinguishable at this boundary (an
        // honest canonical block can fail the producer-authority check when
        // local producer-set state lags), so the rejection must not feed
        // external reputation; sync-local rotation and the corroborated
        // validation halt bound a genuinely hostile server instead.
        assert!(
            matches!(
                &outcome,
                P2pBlockApplyOutcome::Rejected {
                    attribution: AuthoritativeSyncRejectionAttribution::LocalRuleRejection,
                    reason,
                } if reason.contains("authoritative consensus validation")
            ),
            "authoritative production apply must reject unsigned input on the \
             consensus-validation arm without blaming the serving peer (#2807): {outcome:?}"
        );
        assert_eq!(chain_mark(&chain).await, before);
    }

    #[tokio::test]
    async fn checked_finalization_failure_is_local_not_peer_invalid_2788() {
        let (_, blocks) = committed_fixture_blocks(Blockchain::default(), 1, 0x28).await;
        let block = corrupt_state_root_commitment(&blocks[0]);
        let chain = Arc::new(RwLock::new(Blockchain::default()));
        let before = chain_mark(&chain).await;
        let storage = test_storage();
        let consensus = test_consensus();
        let mut fork_buffer = ForkCandidateBuffer::new();
        let mut orphan_buffer = OrphanBlockBuffer::new();
        let outcome = apply_p2p_block_at(
            SigningProtocolVersion::V2,
            block,
            P2pBlockSource::HistoricalSync,
            &chain,
            &storage,
            &consensus,
            None,
            &mut fork_buffer,
            &mut orphan_buffer,
            std::convert::identity,
        )
        .await;

        assert!(
            matches!(
                &outcome,
                P2pBlockApplyOutcome::Rejected {
                    attribution: AuthoritativeSyncRejectionAttribution::LocalFailure,
                    reason,
                } if reason.contains("checked finalization failed")
            ),
            "local checked-finalization failure must not blame the serving peer: {outcome:?}"
        );
        assert_eq!(chain_mark(&chain).await, before);
    }

    /// #2807 RED regression: `validate_block == Ok(false)` is a verdict of
    /// THIS node's deterministic rules, produced without any information
    /// about what the canonical chain contains. When the rejected block IS
    /// canonical (block 11,017's quorum invariant on the 2026-08-16
    /// follower), the identical rejection reproduces against every honest
    /// peer that serves it, so a penalizing attribution mechanically
    /// graylists the entire peer set and starves sync permanently. The
    /// rejection must therefore never carry a peer-penalizing attribution —
    /// sync-local rotation and the corroborated validation halt handle both
    /// honest and hostile servers instead.
    #[tokio::test]
    async fn consensus_rule_rejection_must_not_be_peer_attributable_2807() {
        use rand::SeedableRng;
        use wrfcoin_native_chain::crypto::signature::{PrivateKey, PublicKey};
        use wrfcoin_shared_protocol::consensus::producer_set::{
            ProducerMember, ProducerSetChange,
        };

        // Same fixture as the unsigned-producer control above: a seeded
        // producer set plus an unsigned block forces the authoritative
        // consensus validator to return Ok(false).
        let member_key = PrivateKey::generate(&mut rand::rngs::StdRng::seed_from_u64(2807));
        let member_public_key = PublicKey::from_private_key(&member_key);
        let member = ProducerMember {
            producer_id: "authoritative-member-2807".to_string(),
            payout_address: test_address_string(0x28),
            public_key: member_public_key.as_bytes().try_into().unwrap(),
        };
        let mut authoritative = Blockchain::default();
        authoritative
            .state
            .producer_set_history
            .push(ProducerSetChange::new(0, 0, vec![member]));

        let block = test_block(1, authoritative.tip, 2807);
        let chain = Arc::new(RwLock::new(authoritative));
        let storage = test_storage();
        let consensus = test_consensus();
        let mut fork_buffer = ForkCandidateBuffer::new();
        let mut orphan_buffer = OrphanBlockBuffer::new();
        let outcome = apply_p2p_block_at(
            SigningProtocolVersion::V2,
            block,
            P2pBlockSource::HistoricalSync,
            &chain,
            &storage,
            &consensus,
            None,
            &mut fork_buffer,
            &mut orphan_buffer,
            std::convert::identity,
        )
        .await;

        let P2pBlockApplyOutcome::Rejected {
            attribution,
            reason,
        } = outcome
        else {
            panic!("consensus-rule failure must reject the block, got {outcome:?}");
        };
        assert!(
            reason.contains("authoritative consensus validation"),
            "control must fail on the consensus-validation arm, got: {reason}"
        );
        assert!(
            !attribution.penalizes_peer(),
            "a deterministic consensus-rule rejection reproduces against every \
             honest peer serving the canonical chain; a penalizing attribution \
             graylists the entire peer set and starves sync forever (#2807), \
             got: {attribution:?}"
        );
    }

    #[test]
    fn authoritative_rejection_sites_keep_attribution_classes_2788() {
        let source = include_str!("p2p_events.rs");
        let production = source
            .split_once("#[cfg(test)]\nmod tests")
            .expect("p2p_events.rs must retain a distinct test module")
            .0;
        for peer_invalid in [
            "P2pBlockApplyOutcome::peer_invalid(\"hash mismatch\")",
            "P2pBlockApplyOutcome::peer_invalid(\"missing hash\")",
            "AuthoritativeSyncRejectionAttribution::PeerInvalid,\n                            format!(\"failed to deserialize synced block",
        ] {
            assert!(
                production.contains(peer_invalid),
                "peer-invalid rejection site lost its attribution: {peer_invalid}"
            );
        }
        // #2807: the consensus-rule verdict deliberately moved OUT of the
        // peer-invalid class — a deterministic local rejection of a canonical
        // block reproduces against every honest peer, so blaming the server
        // graylists the entire peer set. It must stay in the rule-rejection
        // class (rotation + corroborated halt), never regress to peer_invalid.
        assert!(
            production.contains(
                "P2pBlockApplyOutcome::rule_rejection(\n                \"failed authoritative consensus validation\""
            ),
            "the consensus-validation rejection site must keep its #2807 rule-rejection attribution"
        );
        for local_failure in [
            "P2pBlockApplyOutcome::local_failure(format!(\"consensus error:",
            "P2pBlockApplyOutcome::local_failure(format!(\"state transition failed:",
            "P2pBlockApplyOutcome::local_failure(format!(\n            \"checked finalization failed:",
            "P2pBlockApplyOutcome::local_failure(format!(\"add block failed:",
        ] {
            assert!(
                production.contains(local_failure),
                "local rejection site must not become peer-attributable: {local_failure}"
            );
        }
    }

    /// Adversarial control for the other side of the repaired boundary.
    /// Context-free IBD forwarding must not weaken authoritative peer-block
    /// validation: a post-activation Real reveal with one approval against K=2
    /// remains rejected by the PoP adapter used by `apply_p2p_block`.
    #[tokio::test]
    async fn authoritative_historical_sync_rejects_real_reveal_below_quorum_2786() {
        use wrfcoin_shared_protocol::consensus::proof_of_publishing::RICH_AMBIENT_REWARD_ACTIVATION_HEIGHT;

        let scenario = mine_attested_reward_block(1.0).await;
        let mut hostile = scenario.block_1.clone();
        let mut kept_one_attestation = false;
        hostile.transactions.retain(|tx| {
            if tx.tx_type != TransactionType::OracleAttestation {
                return true;
            }
            if kept_one_attestation {
                return false;
            }
            kept_one_attestation = true;
            true
        });
        assert_eq!(
            hostile
                .transactions
                .iter()
                .filter(|tx| tx.tx_type == TransactionType::OracleAttestation)
                .count(),
            1,
            "control must carry exactly one approval against quorum two"
        );
        assert!(
            hostile
                .transactions
                .iter()
                .any(|tx| tx.tx_type == TransactionType::ContributionReveal),
            "control must carry the handcrafted Real reveal"
        );

        hostile.height = RICH_AMBIENT_REWARD_ACTIVATION_HEIGHT;
        hostile.set_hash(hostile.calculate_hash());

        let mut authoritative = Blockchain::default();
        (scenario.seed_genesis)(&mut authoritative);
        authoritative.height = RICH_AMBIENT_REWARD_ACTIVATION_HEIGHT.saturating_sub(1);
        authoritative.tip = hostile.prev_block_hash;
        assert_eq!(authoritative.state.oracle_quorum, 2);
        assert_eq!(authoritative.state.oracle_set.len(), 2);

        let consensus = test_consensus();
        let launch_error = wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing::verify_launch_state_invariants(
            &authoritative,
            &hostile,
        )
        .expect_err("authoritative launch verifier must reject below-quorum Real reveal");
        assert!(
            launch_error
                .to_string()
                .contains("SP_CONS_INV_REAL_REVEAL_NO_QUORUM"),
            "control must reach the exact authoritative quorum fence, got: {launch_error}"
        );
        let accepted = consensus
            .validate_block(&hostile, &authoritative)
            .await
            .expect("authoritative adapter validation must return a verdict");
        assert!(
            !accepted,
            "authoritative adapter must reject post-activation Real reveal below quorum"
        );

        let original_height = authoritative.height;
        let authoritative = Arc::new(RwLock::new(authoritative));
        let source_peer = libp2p::PeerId::random();
        let (event_tx, event_rx) = mpsc::channel(1);
        event_tx
            .send(P2pEvent::SyncedBlock {
                peer_id: source_peer,
                height: hostile.height,
                block_bytes: bincode::serialize(&hostile).expect("hostile block serializes"),
            })
            .await
            .expect("historical sync event must enqueue");
        drop(event_tx);
        let (p2p, mut command_rx) = P2pClient::for_testing_with_commands(source_peer);
        run_test_p2p_event_transport(event_rx, Arc::clone(&authoritative), p2p).await;
        assert_eq!(
            authoritative.read().await.height,
            original_height,
            "rejected authoritative peer block must not mutate the real chain"
        );

        let feedback = command_rx
            .try_recv()
            .expect("authoritative rejection must be returned to the P2P worker");
        match feedback {
            P2pCommand::AuthoritativeSyncRejected {
                peer_id,
                block_height,
                accepted_height,
                attribution,
                reason,
                ..
            } => {
                assert_eq!(peer_id, source_peer, "rejection must retain source peer");
                assert_eq!(block_height, RICH_AMBIENT_REWARD_ACTIVATION_HEIGHT);
                assert_eq!(accepted_height, original_height);
                // #2807: attribution deliberately changed from `PeerInvalid`
                // to `LocalRuleRejection`. A below-quorum Real reveal is the
                // EXACT failure class of canonical block 11,017
                // (SP_CONS_INV_REAL_REVEAL_NO_QUORUM) — the block that, under
                // peer attribution, graylisted every honest peer on the live
                // follower and starved sync permanently. The rejection itself
                // (asserted above: chain unmutated, block refused) is the
                // security boundary; the blame class must not be.
                assert_eq!(
                    attribution,
                    AuthoritativeSyncRejectionAttribution::LocalRuleRejection,
                    "a deterministic rule verdict must not be peer-attributable (#2807)"
                );
                assert!(
                    reason.contains("authoritative consensus validation"),
                    "feedback must identify the authoritative decision: {reason}"
                );
            },
            other => panic!("expected authoritative sync rejection feedback, got {other:?}"),
        }
        assert!(
            command_rx.try_recv().is_err(),
            "a rejected historical block must not emit an accepted-height update"
        );
    }

    #[tokio::test]
    async fn producer_applier_block_apply_parity_synthetic_reward_exclusion() {
        let scenario = mine_attested_reward_block_with_provenance_and_genesis(
            1.0,
            wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass::Synthetic,
            Arc::new(|_| {}),
        )
        .await;
        assert_eq!(
            scenario
                .block_1
                .metadata
                .get("rewards_minted_count")
                .map(String::as_str),
            Some("0"),
            "synthetic contribution must not mint even with oracle quorum"
        );

        let applier_chain =
            apply_block_to_fresh_applier(&scenario.block_1, &scenario.seed_genesis).await;
        assert_block_apply_parity(
            "synthetic-reward-exclusion",
            &scenario.producer_chain,
            &applier_chain,
        )
        .await;

        for (label, chain) in [
            ("producer", &scenario.producer_chain),
            ("applier", &applier_chain),
        ] {
            let chain = chain.read().await;
            let record = chain
                .state
                .contribution_reward_records
                .values()
                .next()
                .expect("synthetic decision must remain durable");
            assert_eq!(
                record.provenance_class,
                wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass::Synthetic,
                "{label} lost synthetic provenance"
            );
            assert_eq!(record.amount.0, 0);
            assert_eq!(
                chain
                    .state
                    .get_account(&scenario.recipient)
                    .expect("recipient seeded")
                    .earned_balance_total
                    .0,
                0,
                "{label} credited a synthetic reward"
            );
        }
    }

    // LOW-QUALITY STREAK PARITY (#1682). A LOW-quality attested reveal is kept
    // (quorum approved) but triggers a quality challenge, so the producer's
    // `process_contribution_rewards` records `quality_policy_events` +
    // `audit_events` (telemetry — correctly EXCLUDED from the root by #1681) AND
    // increments `low_quality_streaks` (CONSENSUS state — hashed INTO the root,
    // see `state.rs::recalculate_state_root`). The P2P apply path now derives
    // the matching low-quality streak transitions locally from validated block
    // state, so the root stays aligned while telemetry-only fields may still
    // differ outside the root.
    //
    // This test permanently guards the former in-process fork. Run it with:
    //   cargo test -p wrfcoin-native-chain --bin launch_api --features p2p \
    //     producer_applier_low_quality_streak_divergence_known_1682 -- --nocapture
    #[tokio::test]
    async fn producer_applier_low_quality_streak_divergence_known_1682() {
        let scenario = mine_attested_reward_block(0.20).await;

        // Sanity: the low-quality reveal was kept and challenged (this is the
        // path that mutates low_quality_streaks on the producer).
        assert_eq!(
            scenario
                .block_1
                .metadata
                .get("quality_challenge_count")
                .map(String::as_str),
            Some("1"),
            "low-quality attested reveal must record a quality challenge; got {:?}",
            scenario.block_1.metadata.get("quality_challenge_count")
        );
        assert!(
            scenario
                .block_1
                .metadata
                .contains_key("low_quality_streak_entries"),
            "low-quality block must serialize streak metadata for observability"
        );

        let applier_chain =
            apply_block_to_fresh_applier(&scenario.block_1, &scenario.seed_genesis).await;

        // This root assertion is the guardrail that catches any future drift in
        // producer/applier replay of low-quality streak consensus state.
        assert_block_apply_parity(
            "attested-reward-low-quality",
            &scenario.producer_chain,
            &applier_chain,
        )
        .await;
    }

    #[tokio::test]
    async fn producer_applier_low_quality_slash_failure_preserves_streak_1682() {
        let scenario = mine_attested_reward_block_with_genesis(
            0.20,
            Arc::new(|chain: &mut Blockchain| {
                let policy = &mut chain.state.governance.params.data_quality_policy;
                policy.low_quality_slash_streak = 1;
                policy.low_quality_slash_amount = 5;
            }),
        )
        .await;

        assert_eq!(
            scenario
                .block_1
                .metadata
                .get("quality_challenge_count")
                .map(String::as_str),
            Some("1"),
            "low-quality attested reveal must record a quality challenge"
        );
        assert_eq!(
            scenario
                .block_1
                .metadata
                .get("quality_slashing_applied_count")
                .map(String::as_str),
            Some("0"),
            "slash should fail against a zero-balance recipient account"
        );

        let applier_chain =
            apply_block_to_fresh_applier(&scenario.block_1, &scenario.seed_genesis).await;

        {
            let producer = scenario.producer_chain.read().await;
            let applier = applier_chain.read().await;
            assert_eq!(
                producer.state.low_quality_streaks.get(&scenario.recipient),
                Some(&1),
                "producer preserves the incremented streak when Slash fails"
            );
            assert_eq!(
                applier.state.low_quality_streaks.get(&scenario.recipient),
                Some(&1),
                "applier must preserve the same streak when derived Slash fails"
            );
        }

        assert_block_apply_parity(
            "attested-reward-low-quality-slash-failure",
            &scenario.producer_chain,
            &applier_chain,
        )
        .await;
    }

    /// Slash-SUCCESS counterpart to
    /// `producer_applier_low_quality_slash_failure_preserves_streak_1682`.
    ///
    /// A low-quality attested reveal hits the slash threshold AND the recipient
    /// is funded, so the producer's reward path commits `Slash` +
    /// `SetLowQualityStreak{None}` (streak reset) through the shared
    /// `apply_state_transitions_batch` sink. This exercises the slash-SUCCESS
    /// arm of `process_contribution_rewards` — an in-root `low_quality_streaks`
    /// mutation plus a balance deduction — and asserts the P2P applier derives
    /// the identical transition list, so `state_root`, account keysets, and the
    /// streak map all converge (#1682). The pre-existing failure-path test only
    /// covered the arm where `Slash` errors and the streak is preserved.
    #[tokio::test]
    async fn producer_applier_reward_and_low_quality_slash_success_parity_1682() {
        let scenario = mine_attested_reward_block_with_genesis(
            0.20,
            Arc::new(|chain: &mut Blockchain| {
                use wrfcoin_shared_protocol::common::types::{Address, Balance};
                {
                    let policy = &mut chain.state.governance.params.data_quality_policy;
                    policy.low_quality_slash_streak = 1;
                    policy.low_quality_slash_amount = 5;
                }
                // Fund the reveal recipient (hardcoded as `Address([0x77; 20])`
                // in `mine_attested_reward_block_with_genesis`, created with a
                // zero balance just before this closure runs) so the `Slash`
                // deducts from `spendable_balance` and SUCCEEDS — unlike the
                // zero-balance failure counterpart.
                let recipient = Address([0x77; 20]);
                if let Some(acct) = chain.state.get_account_mut(&recipient) {
                    acct.balance = Balance::new(100);
                    acct.spendable_balance = Balance::new(100);
                }
            }),
        )
        .await;

        assert_eq!(
            scenario
                .block_1
                .metadata
                .get("quality_challenge_count")
                .map(String::as_str),
            Some("1"),
            "low-quality attested reveal must record a quality challenge"
        );
        assert_eq!(
            scenario
                .block_1
                .metadata
                .get("quality_slashing_applied_count")
                .map(String::as_str),
            Some("1"),
            "slash must succeed against the funded recipient account"
        );

        let applier_chain =
            apply_block_to_fresh_applier(&scenario.block_1, &scenario.seed_genesis).await;

        {
            let producer = scenario.producer_chain.read().await;
            let applier = applier_chain.read().await;
            // A successful slash resets the streak (SetLowQualityStreak{None}),
            // so the recipient must be absent from BOTH streak maps.
            assert_eq!(
                producer.state.low_quality_streaks.get(&scenario.recipient),
                None,
                "producer resets the streak when Slash succeeds"
            );
            assert_eq!(
                applier.state.low_quality_streaks.get(&scenario.recipient),
                None,
                "applier must reset the same streak when derived Slash succeeds"
            );
            assert_eq!(
                producer.state.low_quality_streaks, applier.state.low_quality_streaks,
                "streak maps must be identical across producer/applier"
            );
            // No phantom accounts on either side (the #1680 failure mode).
            let producer_keys: std::collections::HashSet<_> =
                producer.state.accounts.keys().cloned().collect();
            let applier_keys: std::collections::HashSet<_> =
                applier.state.accounts.keys().cloned().collect();
            assert_eq!(
                producer_keys, applier_keys,
                "producer and applier account keysets must be identical"
            );
        }

        // The guardrail: identical bytes in → identical state_root out.
        assert_block_apply_parity(
            "reward+low-quality-slash-success",
            &scenario.producer_chain,
            &applier_chain,
        )
        .await;
    }

    // ----- #1724: runtime invocation of the state_diverged-reset gate -----
    //
    // These two tests drive the *real* `run_p2p_event_loop` (the same function
    // `main.rs` spawns in production) and prove the local-canonical-root gate is
    // exercised at runtime — not merely unit-tested in isolation against the
    // pure `observe_local_canonical_root` helper. Neither test ever sends a
    // `PeerChainStatus` event, so the only thing that can touch the latch on the
    // `NewBlock` path is `try_clear_latch_on_local_root`. The pair forms a
    // positive/negative control on the injected `StateDivergenceRecoveryPolicy`.

    /// #2589 lifecycle proof, driven through the REAL `run_p2p_event_loop`.
    ///
    /// Evidence is the **rendered Prometheus exposition text** — the same bytes a
    /// `/metrics` scrape returns — never a counter object. That distinction is the
    /// point: `IntCounterVec::with_label_values` *creates* a missing series as a
    /// side effect, so any absence assertion written against the counter API
    /// passes unconditionally and proves nothing.
    ///
    /// Three claims, in order:
    ///   1. raw `PeerConnected` churn allocates NO `peer_id` series (the vector),
    ///   2. `PeerChainStatus` — the admission boundary — renders the series at 0,
    ///   3. `PeerDisconnected` REMOVES it from the rendered output.
    #[tokio::test]
    #[serial_test::serial(core_metrics)]
    async fn run_p2p_event_loop_bounds_preadmission_and_releases_series_2589() {
        use libp2p::PeerId;
        use prometheus::{Encoder, TextEncoder};
        use wrfcoin_shared_protocol::monitoring::CORE_METRICS;

        fn render() -> String {
            let mut buf = Vec::new();
            TextEncoder::new()
                .encode(&CORE_METRICS.registry.gather(), &mut buf)
                .expect("metrics must encode");
            String::from_utf8(buf).expect("metrics must be UTF-8")
        }

        // One event-loop run per phase: the loop terminates when its sender is
        // dropped, which is also the synchronisation point that guarantees every
        // event has been processed before we scrape. CORE_METRICS is global, so
        // state carries across the phases.
        async fn drive(events: Vec<P2pEvent>) {
            let mut chain = Blockchain::default();
            chain.state.recalculate_state_root();
            let blockchain = Arc::new(RwLock::new(chain));
            let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
            let state_diverged = Arc::new(AtomicBool::new(false));
            let (event_tx, event_rx) = mpsc::channel(128);
            let event_loop = tokio::spawn(run_p2p_event_loop(
                event_rx,
                Arc::clone(&blockchain),
                mempool,
                test_storage(),
                test_consensus(),
                None,
                DaChallengeRuntime::new(),
                None,
                None,
                #[cfg(feature = "execution")]
                test_forecast_state().0,
                #[cfg(feature = "execution")]
                test_forecast_state().1,
                #[cfg(feature = "execution")]
                test_forecast_state().2,
                Arc::clone(&state_diverged),
                StateDivergenceRecoveryPolicy::LocallyVerifiedRoot,
                std::sync::Arc::new(tokio::sync::RwLock::new(
                    crate::peer_reputation::PeerReputationTracker::new(),
                )),
                test_commit_signing_key(),
            ));
            for event in events {
                event_tx.send(event).await.expect("event loop accepts events");
            }
            drop(event_tx);
            event_loop.await.expect("event loop exits cleanly on channel close");
        }

        // ---- Phase 1: pre-admission churn must not allocate series -----------
        // 25 distinct libp2p identities connect and never report chain status --
        // exactly what an unauthenticated peer cycling identities can do for free.
        let churn: Vec<PeerId> = (0..25).map(|_| PeerId::random()).collect();
        drive(churn.iter().copied().map(P2pEvent::PeerConnected).collect()).await;

        let exposition = render();
        for peer in &churn {
            assert!(
                !exposition.contains(&format!("peer_id=\"{peer}\"")),
                "unadmitted peer {peer} must not allocate a series; \
                 raw connection churn is a cardinality vector (#2589)"
            );
        }
        assert!(
            exposition.contains("wrfcoin_p2p_preadmission_connections_total"),
            "pre-admission connections must still be counted in AGGREGATE"
        );

        // ---- Phase 2: admission renders the series at zero --------------------
        let admitted = PeerId::random();
        let other = PeerId::random();
        let (height, tip_hash, state_root) = {
            let chain = Blockchain::default();
            (chain.height, <[u8; 32]>::from(chain.tip), chain.state.state_root.0)
        };
        let status = |peer_id| P2pEvent::PeerChainStatus { peer_id, height, tip_hash, state_root };
        drive(vec![
            P2pEvent::PeerConnected(admitted),
            status(admitted),
            P2pEvent::PeerConnected(other),
            status(other),
        ])
        .await;

        let exposition = render();
        assert!(
            exposition.contains(&format!(
                "wrfcoin_state_divergence_total{{peer_id=\"{admitted}\"}} 0"
            )),
            "an ADMITTED peer must render at zero before it ever diverges; got:\n{exposition}"
        );

        // ---- Phase 3: teardown removes the series from RENDERED output --------
        drive(vec![P2pEvent::PeerDisconnected(admitted)]).await;

        let exposition = render();
        assert!(
            !exposition.contains(&format!("peer_id=\"{admitted}\"")),
            "disconnect must REMOVE the series from rendered /metrics -- \
             IntCounterVec has no eviction, so without release it outlives the \
             connection for the whole process lifetime; got:\n{exposition}"
        );
        assert!(
            exposition.contains(&format!(
                "wrfcoin_state_divergence_total{{peer_id=\"{other}\"}} 0"
            )),
            "releasing one peer must not disturb another still-connected peer; got:\n{exposition}"
        );
    }

    /// #2589 REGRESSION: the bound survives a **dropped teardown**, through the
    /// real event loop.
    ///
    /// This is the gap an admission-plus-release scheme leaves open, and it is
    /// reachable in production, not hypothetical:
    ///
    ///   - `p2p/src/node.rs:1489` sends `PeerDisconnected` as
    ///     `let _ = event_tx.send(...).await` — a send failure is discarded; and
    ///   - `main.rs:2346` consumes the stream from a detached `tokio::spawn`.
    ///
    /// If that consumer exits, the producer keeps running and release events are
    /// lost. Here every peer is admitted through `PeerChainStatus` and **no
    /// `PeerDisconnected` is ever sent** — the teardown is dropped for all of
    /// them — and the live series count must still be bounded.
    ///
    /// Evidence is the rendered exposition text, both directions: the ceiling
    /// holds, and the eviction counter proves the cap actually engaged rather
    /// than the peers simply never being admitted.
    #[tokio::test]
    #[serial_test::serial(core_metrics)]
    async fn run_p2p_event_loop_bounds_series_when_teardown_is_never_delivered_2589() {
        use libp2p::PeerId;
        use prometheus::{Encoder, TextEncoder};
        use wrfcoin_shared_protocol::monitoring::{
            CORE_METRICS, MAX_STATE_DIVERGENCE_PEER_SERIES, STATE_DIVERGENCE_SENTINEL_PEER,
        };

        fn render() -> String {
            let mut buf = Vec::new();
            TextEncoder::new()
                .encode(&CORE_METRICS.registry.gather(), &mut buf)
                .expect("metrics must encode");
            String::from_utf8(buf).expect("metrics must be UTF-8")
        }

        let evictions_before = CORE_METRICS.state_divergence_peer_evictions_total.get();

        let mut chain = Blockchain::default();
        chain.state.recalculate_state_root();
        let (height, tip_hash, state_root) =
            (chain.height, <[u8; 32]>::from(chain.tip), chain.state.state_root.0);
        let blockchain = Arc::new(RwLock::new(chain));
        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let state_diverged = Arc::new(AtomicBool::new(false));

        let (event_tx, event_rx) = mpsc::channel(256);
        let event_loop = tokio::spawn(run_p2p_event_loop(
            event_rx,
            Arc::clone(&blockchain),
            mempool,
            test_storage(),
            test_consensus(),
            None,
            DaChallengeRuntime::new(),
            None,
            None,
            #[cfg(feature = "execution")]
            test_forecast_state().0,
            #[cfg(feature = "execution")]
            test_forecast_state().1,
            #[cfg(feature = "execution")]
            test_forecast_state().2,
            Arc::clone(&state_diverged),
            StateDivergenceRecoveryPolicy::LocallyVerifiedRoot,
            std::sync::Arc::new(tokio::sync::RwLock::new(
                crate::peer_reputation::PeerReputationTracker::new(),
            )),
            test_commit_signing_key(),
        ));

        // Admit past the ceiling. Every one of these peers "disconnects" without
        // its PeerDisconnected ever reaching the loop.
        let overshoot = 30usize;
        let first = PeerId::random();
        let last = PeerId::random();
        for index in 0..MAX_STATE_DIVERGENCE_PEER_SERIES + overshoot {
            let peer_id = if index == 0 {
                first
            } else if index == MAX_STATE_DIVERGENCE_PEER_SERIES + overshoot - 1 {
                last
            } else {
                PeerId::random()
            };
            event_tx
                .send(P2pEvent::PeerChainStatus { peer_id, height, tip_hash, state_root })
                .await
                .expect("event loop accepts events");
        }
        drop(event_tx);
        event_loop.await.expect("event loop exits cleanly on channel close");

        let exposition = render();
        // #2734 added a permanent startup sentinel series (peer_id="none")
        // that deliberately never enters the admitted set — it is not a live
        // peer series and does not count against the per-peer ceiling.
        let sentinel_label = format!("peer_id=\"{STATE_DIVERGENCE_SENTINEL_PEER}\"");
        let live_series = exposition
            .lines()
            .filter(|line| line.starts_with("wrfcoin_state_divergence_total{peer_id="))
            .filter(|line| !line.contains(&sentinel_label))
            .count();

        assert!(
            live_series <= MAX_STATE_DIVERGENCE_PEER_SERIES,
            "with every teardown dropped, live series must still be bounded by \
             {MAX_STATE_DIVERGENCE_PEER_SERIES}; rendered {live_series}"
        );
        assert!(
            CORE_METRICS.state_divergence_peer_evictions_total.get() > evictions_before,
            "the cap must have ENGAGED -- otherwise this test would pass simply \
             because the peers were never admitted, proving nothing"
        );
        assert!(
            !exposition.contains(&format!("peer_id=\"{first}\"")),
            "the earliest admitted peer must have been evicted once the ceiling was reached"
        );
        assert!(
            exposition.contains(&format!(
                "wrfcoin_state_divergence_total{{peer_id=\"{last}\"}} 0"
            )),
            "the most recently admitted peer must still be rendered"
        );
    }

    /// Under `LocallyVerifiedRoot`, local block progress whose canonical-root
    /// recompute matches the stored root clears the `state_diverged` latch from
    /// the live event loop. Proves the gate runs in the production path.
    #[tokio::test]
    async fn run_p2p_event_loop_local_policy_clears_latch_via_local_canonical_root() {
        let mut chain = Blockchain::default();
        // Force the stored state root to equal hash(state_root_material) so the
        // node's own recompute (byte-identical to `recalculate_state_root`)
        // verifies against the live root and is non-zero.
        chain.state.recalculate_state_root();
        let genesis_tip = chain.tip;
        let next_block = test_block(1, genesis_tip, 4_724);

        let blockchain = Arc::new(RwLock::new(chain));
        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let storage = test_storage();
        let consensus = test_consensus();
        // Latch starts SET, as if a same-height peer mismatch had been observed.
        let state_diverged = Arc::new(AtomicBool::new(true));

        let (event_tx, event_rx) = mpsc::channel(8);
        let event_loop = tokio::spawn(run_p2p_event_loop(
            event_rx,
            Arc::clone(&blockchain),
            mempool,
            storage,
            consensus,
            None,
            DaChallengeRuntime::new(),
            None,
            None,
            #[cfg(feature = "execution")]
            test_forecast_state().0,
            #[cfg(feature = "execution")]
            test_forecast_state().1,
            #[cfg(feature = "execution")]
            test_forecast_state().2,
            Arc::clone(&state_diverged),
            StateDivergenceRecoveryPolicy::LocallyVerifiedRoot,
            std::sync::Arc::new(tokio::sync::RwLock::new(
                crate::peer_reputation::PeerReputationTracker::new(),
            )),
            test_commit_signing_key(),
        ));

        assert!(
            state_diverged.load(Ordering::SeqCst),
            "precondition: latch must be set before local progress"
        );

        event_tx
            .send(P2pEvent::NewBlock(
                bincode::serialize(&next_block).expect("block should serialize"),
            ))
            .await
            .expect("event receiver should accept new block");
        drop(event_tx);
        event_loop
            .await
            .expect("event loop should exit cleanly after channel close");

        assert!(
            !state_diverged.load(Ordering::SeqCst),
            "locally-verified canonical root must clear the latch from the live event loop"
        );
    }

    /// Negative control: under the default `PeerAgreement` policy, local block
    /// progress alone must NOT clear the latch from the live event loop (only
    /// same-height peer agreement does, and none is sent here). Proves the gate
    /// is genuinely policy-gated rather than firing unconditionally.
    #[tokio::test]
    async fn run_p2p_event_loop_peer_agreement_policy_holds_latch_on_local_progress() {
        let mut chain = Blockchain::default();
        chain.state.recalculate_state_root();
        let genesis_tip = chain.tip;
        let next_block = test_block(1, genesis_tip, 1_717);

        let blockchain = Arc::new(RwLock::new(chain));
        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let storage = test_storage();
        let consensus = test_consensus();
        let state_diverged = Arc::new(AtomicBool::new(true));

        let (event_tx, event_rx) = mpsc::channel(8);
        let event_loop = tokio::spawn(run_p2p_event_loop(
            event_rx,
            Arc::clone(&blockchain),
            mempool,
            storage,
            consensus,
            None,
            DaChallengeRuntime::new(),
            None,
            None,
            #[cfg(feature = "execution")]
            test_forecast_state().0,
            #[cfg(feature = "execution")]
            test_forecast_state().1,
            #[cfg(feature = "execution")]
            test_forecast_state().2,
            Arc::clone(&state_diverged),
            StateDivergenceRecoveryPolicy::PeerAgreement,
            std::sync::Arc::new(tokio::sync::RwLock::new(
                crate::peer_reputation::PeerReputationTracker::new(),
            )),
            test_commit_signing_key(),
        ));

        event_tx
            .send(P2pEvent::NewBlock(
                bincode::serialize(&next_block).expect("block should serialize"),
            ))
            .await
            .expect("event receiver should accept new block");
        drop(event_tx);
        event_loop
            .await
            .expect("event loop should exit cleanly after channel close");

        assert!(
            state_diverged.load(Ordering::SeqCst),
            "under PeerAgreement, local progress alone must not clear the latch"
        );
    }

    // ── #1979 rollback tests ──────────────────────────────────────────────────

    /// Verify the apply-to-copy-then-swap pattern (#1979): when
    /// `apply_transitions_batch` fails on the working copy, the live
    /// `chain.state` is completely unchanged (no partial mutation).
    #[test]
    fn working_copy_failure_leaves_chain_state_unchanged() {
        use wrfcoin_native_chain::blockchain::state::StateTransition;
        use wrfcoin_shared_protocol::common::types::{Address, Balance};

        let mut chain = Blockchain::default();
        let addr_a = Address::new([0xaau8; 20]);
        let addr_b = Address::new([0xbbu8; 20]);

        // Give addr_a 100 spendable tokens (stake_required defaults to 0 so
        // the full reward is spendable immediately).
        chain
            .apply_state_transition(StateTransition::ApplyReward {
                address: addr_a,
                amount: Balance::new(100),
                reason: "test setup".to_string(),
            })
            .expect("reward should be applied");

        let root_before = chain.state.state_root;

        // Attempt to transfer 200 on a working copy — must fail with
        // InsufficientSpendableBalance.
        let mut working = chain.state.clone();
        let result = working.apply_transitions_batch(vec![StateTransition::BalanceTransfer {
            from: addr_a,
            to: addr_b,
            amount: Balance::new(200),
            nonce: Some(0), // account nonce starts at 0
            current_block: 1,
        }]);

        assert!(result.is_err(), "over-spend transfer must fail");

        // chain.state must be byte-identical to what it was before — same
        // state_root and same account balance.
        assert_eq!(
            chain.state.state_root, root_before,
            "chain.state.state_root must be unchanged after working-copy failure"
        );
        let account = chain
            .state
            .accounts
            .get(&addr_a)
            .expect("addr_a should still exist in chain.state");
        assert_eq!(
            account.spendable_balance,
            Balance::new(100),
            "spendable_balance must be unchanged after working-copy failure"
        );
    }

    /// Verify that on add_block failure after a successful transition apply,
    /// chain.state is restored to the pre-transition snapshot (#1979).
    #[test]
    fn add_block_failure_restores_pre_transition_state() {
        use wrfcoin_native_chain::blockchain::block::Block;
        use wrfcoin_native_chain::blockchain::state::StateTransition;
        use wrfcoin_shared_protocol::common::types::{Address, Balance};

        let mut chain = Blockchain::default();
        let addr_a = Address::new([0xaau8; 20]);
        let addr_b = Address::new([0xbbu8; 20]);

        // Give addr_a 100 tokens so a valid transfer would succeed.
        chain
            .apply_state_transition(StateTransition::ApplyReward {
                address: addr_a,
                amount: Balance::new(100),
                reason: "test setup".to_string(),
            })
            .expect("reward should be applied");

        let root_before = chain.state.state_root;

        // Apply a valid transition to a working copy and commit it.
        let mut working = chain.state.clone();
        working
            .apply_transitions_batch(vec![StateTransition::BalanceTransfer {
                from: addr_a,
                to: addr_b,
                amount: Balance::new(50),
                nonce: Some(0),
                current_block: 1,
            }])
            .expect("valid transfer on working copy must succeed");
        working.apply_block_finalization(1);

        // Commit the working state, saving the original for rollback.
        let pre_add_state = std::mem::replace(&mut chain.state, working);

        // Simulate add_block failure (block with no hash → add_block returns Err).
        let bad_block = Block::new(1, chain.tip, vec![], 1, chain.height + 1);
        // bad_block.hash is None → add_block will error.
        assert!(chain.add_block(bad_block).is_err(), "block with no hash must be rejected");

        // Restore from pre_add_state, mirroring what the production code does.
        chain.state = pre_add_state;

        // chain.state must be back to the pre-transition snapshot.
        assert_eq!(
            chain.state.state_root, root_before,
            "chain.state.state_root must be restored after add_block failure"
        );
        let account = chain
            .state
            .accounts
            .get(&addr_a)
            .expect("addr_a should still exist after rollback");
        assert_eq!(
            account.spendable_balance,
            Balance::new(100),
            "spendable_balance must be restored after add_block failure"
        );
    }

    // =====================================================================
    // S4 — the genesis staged-state gate for the oracle set (#2055 / #2464)
    // =====================================================================
    /// The oracle set is genesis **content**, not configuration. `oracle_set`,
    /// `oracle_quorum` and `signing_keys` all fold into `state_root`, so
    /// operators cannot be seated after block 1 without another chain cut, and
    /// no oracle signer key exists yet. Block 1 must already carry the
    /// `OracleRegistration` transactions — which makes it unrepeatable, and
    /// makes a false green here a defect discovered at cut time.
    ///
    /// # Why this module lives in the binary and not in `tests/`
    ///
    /// The first cut of this gate lived in
    /// `native-chain/tests/genesis_oracle_registration_s4.rs` and manufactured
    /// `StateTransition::ApplyOracleRegistration` directly, then applied the
    /// batch to two `Blockchain::default()`s. It proved nothing about the
    /// production paths: no block was mined, serialized, validated or applied,
    /// and — the finding that condemned it — deleting a registration's
    /// governance `auth_signature` left the "peer" and "restart" legs GREEN.
    /// That is the #908/#934 manufactured-preconditions class: the test
    /// fabricated the exact registry/quorum/signing-key state it then asserted.
    ///
    /// The real seams are `create_pop_block_with_state` (producer) and
    /// `apply_p2p_block` (peer), both private to the `launch_api` binary and
    /// unreachable from an integration test. Hence this module.
    ///
    /// # The four legs, each on a real seam
    ///
    /// | Leg | Seam actually exercised |
    /// |-----|--------------------------|
    /// | producer | `create_pop_block_with_state` — mempool admission, assembly, the #1730 self-screen, stateful apply |
    /// | peer | `apply_p2p_block` on bincode-round-tripped bytes — consensus `validate_block`, `extract_transitions`, batch apply, `apply_block_finalization_checked` |
    /// | restart | the redb `Storage` **that `apply_p2p_block` itself wrote**, reopened and replayed via `load_state` |
    /// | sync | `build_snapshot_entries` over the **peer-accepted** post-block-1 state, through the same round-trip `ingest_snapshot_sync_payload` performs |
    ///
    /// Nothing downstream of the producer is hand-built: the restart source is
    /// the applier's own storage, and the sync source is the applier's own
    /// accepted state. Every convergence assertion is on **`state_root`**,
    /// never block hash — `state_root` is the only value covering both the
    /// registry (`oracle_set`, `oracle_quorum`) and the signing-key epochs.
    ///
    /// # Rejection is asserted, not assumed
    ///
    /// `tampered_genesis_registration_is_rejected_by_the_producer` and
    /// `..._by_the_peer` mutate one registration's `auth_signature` and prove
    /// each seam refuses it with **no state or root advancement**. They are
    /// permanent tests, not a manual RED control, because the mutation they
    /// carry is precisely the one the previous gate let through.
    ///
    /// # VECTOR-BACKED vs NEEDS THE REAL CEREMONY
    ///
    /// **Vector-backed — proven here, no ceremony required:** the payload and
    /// transaction shape; absolute-set semantics and how three registrations
    /// compose inside one block; the governance authorization binding as
    /// enforced by the producer self-screen and `verify_block`; and
    /// producer/peer/restart/sync agreement on `state_root`. These are
    /// properties of the code — substituting different ed25519 keys changes
    /// none of them.
    ///
    /// **Needs the ceremony — explicitly NOT proven here:** the three operator
    /// identities and their ed25519 public keys (seeded keypairs stand in); the
    /// governance operator pubkey in `WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS`, which
    /// is deployment config resolved at *verification* time and must therefore
    /// be installed on **every** node before block 1 is cut (a node without it
    /// rejects the registrations — see the producer-rejection test, which is
    /// that failure mode); and custody of the oracle signing keys, which do not
    /// exist yet.
    mod genesis_oracle_set_s4 {
        use super::*;
        use crate::routes::snapshot::build_snapshot_entries;
        use crate::transition_extractor::extract_transitions;
        use wrfcoin_native_chain::blockchain::state::{State, StateTransition};
        use wrfcoin_native_chain::crypto::signature::{PrivateKey, PublicKey};
        use wrfcoin_native_chain::security::key_registry::SigningRole;
        use wrfcoin_shared_protocol::blockchain::snapshot_sync::build_state_snapshot;
        use wrfcoin_shared_protocol::crypto::domain_signing::ORACLE_GOVERNANCE_PUBKEYS_ENV;
        use wrfcoin_shared_protocol::crypto::genesis_oracle::{
            GENESIS_ORACLE_QUORUM_KEY, GENESIS_ORACLE_SET_KEY, GenesisOracleDeclaration,
        };

        /// Fixed seeds so the whole module is reproducible. Each maps to
        /// exactly one artifact the real ceremony must supply.
        const SEED_ORACLE_A: u64 = 2055_0001;
        const SEED_ORACLE_B: u64 = 2055_0002;
        const SEED_ORACLE_C: u64 = 2055_0003;
        /// Signing keys are DISTINCT from the identities. The ceremony emits
        /// three identity pubkeys and three *separate* signing pubkeys, and the
        /// distinction is load-bearing here: `ApplyOracleRegistration` derives a
        /// legacy epoch-0 key for every identity named in `oracle_set`, so an
        /// identity that is merely *named* by someone else's registration looks
        /// keyed unless the fixture keeps the two apart.
        const SEED_ORACLE_A_SIGNING: u64 = 2055_0011;
        const SEED_ORACLE_B_SIGNING: u64 = 2055_0012;
        const SEED_ORACLE_C_SIGNING: u64 = 2055_0013;
        const SEED_GOVERNANCE: u64 = 2055_0215;

        const EPOCH: u64 = 1;
        const ACTIVATED_AT: i64 = 1_700_000_000;

        fn s4_keypair(seed: u64) -> (PrivateKey, String) {
            use rand::SeedableRng;
            let sk = PrivateKey::generate(&mut rand::rngs::StdRng::seed_from_u64(seed));
            let hex = PublicKey::from_private_key(&sk).to_hex();
            (sk, hex)
        }

        /// The three registration transactions block 1 must carry, in the
        /// **order-independent** shape: *all three* name the FINAL absolute set
        /// `[A, B, C]` and the final quorum `2`, and each registers its own
        /// operator's signing key.
        ///
        /// This shape is not cosmetic. `oracle_set` is ABSOLUTE, not a delta
        /// (#2055 Option A), so the registration applied LAST decides the set —
        /// and the producer does **not** preserve the order transactions were
        /// staged in (proven by
        /// `block_1_converges_regardless_of_producer_transaction_order`). Under
        /// the intuitive incremental shape — "A alone, then A+B" — a reversed
        /// order applies the complete set first and the single-oracle set
        /// second, and block 1 comes up seating ONE oracle behind a quorum of
        /// two that can never be met. Block 1 is unrepeatable, so the only
        /// remedy is another chain cut.
        ///
        /// Under this shape every ordering yields the same state, so the
        /// ceremony cannot lose to a coin flip inside the block builder.
        ///
        /// The genesis oracle vectors: identities, their distinct signing
        /// keys, and the governance key that authorizes all three registrations.
        struct S4Genesis {
            txs: Vec<Transaction>,
            id_a: String,
            id_b: String,
            id_c: String,
            signing_a: String,
            signing_b: String,
            signing_c: String,
            gov_pk: String,
            declaration: GenesisOracleDeclaration,
        }

        fn s4_registration_txs() -> S4Genesis {
            let (_sk_a, id_a) = s4_keypair(SEED_ORACLE_A);
            let (_sk_b, id_b) = s4_keypair(SEED_ORACLE_B);
            let (_sk_c, id_c) = s4_keypair(SEED_ORACLE_C);
            let (_sk_ak, signing_a) = s4_keypair(SEED_ORACLE_A_SIGNING);
            let (_sk_bk, signing_b) = s4_keypair(SEED_ORACLE_B_SIGNING);
            let (_sk_ck, signing_c) = s4_keypair(SEED_ORACLE_C_SIGNING);
            let (gov_sk, gov_pk) = s4_keypair(SEED_GOVERNANCE);
            let final_set = vec![id_a.clone(), id_b.clone(), id_c.clone()];
            let declaration = GenesisOracleDeclaration::new(final_set.clone(), 2)
                .expect("the owner-decided N=3/K=2 declaration is valid");

            let tx_a = Transaction::new_oracle_registration(
                final_set.clone(),
                2,
                &id_a,
                &signing_a,
                EPOCH,
                ACTIVATED_AT,
                &gov_sk,
            );
            let tx_b = Transaction::new_oracle_registration(
                final_set.clone(),
                2,
                &id_b,
                &signing_b,
                EPOCH,
                ACTIVATED_AT,
                &gov_sk,
            );
            let tx_c = Transaction::new_oracle_registration(
                final_set,
                2,
                &id_c,
                &signing_c,
                EPOCH,
                ACTIVATED_AT,
                &gov_sk,
            );
            S4Genesis {
                txs: vec![tx_a, tx_b, tx_c],
                id_a,
                id_b,
                id_c,
                signing_a,
                signing_b,
                signing_c,
                gov_pk,
                declaration,
            }
        }

        /// The INCREMENTAL shape a drafter reaches for by intuition: A alone at
        /// quorum 1, then the cumulative `[A, B]` at quorum 2. Correct only if
        /// the second transaction is applied second — which nothing guarantees.
        /// Used solely to prove the hazard.
        fn s4_incremental_registration_txs() -> (Vec<Transaction>, String, String) {
            let (_sk_a, id_a) = s4_keypair(SEED_ORACLE_A);
            let (_sk_b, id_b) = s4_keypair(SEED_ORACLE_B);
            let (gov_sk, _gov_pk) = s4_keypair(SEED_GOVERNANCE);

            let tx_a = Transaction::new_oracle_registration(
                vec![id_a.clone()],
                1,
                &id_a,
                &id_a,
                EPOCH,
                ACTIVATED_AT,
                &gov_sk,
            );
            let tx_b = Transaction::new_oracle_registration(
                vec![id_a.clone(), id_b.clone()],
                2,
                &id_b,
                &id_b,
                EPOCH,
                ACTIVATED_AT,
                &gov_sk,
            );
            (vec![tx_a, tx_b], id_a, id_b)
        }

        /// Strip the governance `auth_signature` and recompute the txid.
        ///
        /// This is the exact mutation that exposed the previous gate as
        /// vacuous. It stands in for every way block-1 content can be wrong in
        /// the field: an unauthorized drafter, a governance key that is not on
        /// this node's allowlist, or a payload edited after signing.
        fn s4_strip_authorization(tx: &Transaction) -> Transaction {
            let mut payload = tx
                .parse_oracle_registration()
                // The enclosing `#[cfg(test)]` mask does not reach this module — the gate's
                // `_test_mask` loses the `mod tests` region earlier in this file (scope defect
                // reported with #2478) — so the exemption is annotated explicitly.
                // panic-ok: test-only S4 fixture helper; a panic is a failing test, not a reachable input path.
                .expect("fixture is a well-formed OracleRegistration");
            payload.auth_signature = None;
            let mut tampered = tx.clone();
            // The enclosing `#[cfg(test)]` mask does not reach this module — the gate's
            // `_test_mask` loses the `mod tests` region earlier in this file (scope defect
            // reported with #2478) — so the exemption is annotated explicitly.
            // panic-ok: test-only S4 fixture helper; a panic is a failing test, not a reachable input path.
            tampered.data = Some(serde_json::to_string(&payload).expect("payload re-serializes"));
            tampered.set_id();
            assert!(
                !tampered.verify_oracle_registration_authorization(),
                "the mutation must actually break authorization, or every \
                 rejection assertion below is vacuous"
            );
            tampered
        }

        /// Install the governance allowlist. Held for the whole test: the
        /// allowlist is read at *verification* time, on the producer's
        /// self-screen and on every applier, not at construction time.
        ///
        /// `setenv` mutates the process-wide `environ`, so concurrent writers
        /// race at the libc level regardless of the value written — hence the
        /// shared lock (#2290/#2405 fallout).
        fn s4_install_governance_env(gov_pk: &str) -> std::sync::MutexGuard<'static, ()> {
            let guard = crate::helpers::test_env::lock();
            unsafe { std::env::set_var(ORACLE_GOVERNANCE_PUBKEYS_ENV, gov_pk) };
            guard
        }

        /// Mine block 1 through the real producer seam with `txs` staged in the
        /// mempool. Returns the producer chain (already advanced to height 1)
        /// and the block it built.
        async fn s4_consensus(
            g: &S4Genesis,
        ) -> Arc<TypedConsensusManager<wrfcoin_consensus_engines::PopConsensusAdapter>> {
            let pop_config = ConsensusFactory::default_config(ConsensusType::ProofOfPublishing);
            let pop_adapter = ConsensusFactory::create_pop_adapter(&pop_config);
            pop_adapter
                .set_genesis_oracle_declaration(
                    g.declaration.encode_oracle_set(),
                    g.declaration.oracle_quorum().to_string(),
                )
                .await;
            Arc::new(TypedConsensusManager::new(pop_adapter, pop_config))
        }

        async fn s4_mine_block_1(
            g: &S4Genesis,
            txs: &[Transaction],
        ) -> (Arc<RwLock<Blockchain>>, Block) {
            let producer_chain = Arc::new(RwLock::new(Blockchain::default()));
            let producer_mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
            {
                let mut mp = producer_mempool.write().await;
                for tx in txs {
                    mp.add_transaction(tx.clone())
                        // The enclosing `#[cfg(test)]` mask does not reach this module — the
                        // gate's `_test_mask` loses the `mod tests` region earlier in this file
                        // (scope defect reported with #2478) — so the exemption is annotated
                        // explicitly.
                        // panic-ok: test-only S4 fixture helper; a panic is a failing test, not a reachable input path.
                        .expect("mempool must admit a registration tx");
                }
            }
            let block = s4_consensus(g)
                .await
                .create_pop_block_with_state(
                    Arc::clone(&producer_chain),
                    Arc::clone(&producer_mempool),
                    &test_address_string(0x6a),
                )
                .await
                // The enclosing `#[cfg(test)]` mask does not reach this module — the gate's
                // `_test_mask` loses the `mod tests` region earlier in this file (scope defect
                // reported with #2478) — so the exemption is annotated explicitly.
                // panic-ok: test-only S4 fixture helper; a panic is a failing test, not a reachable input path.
                .expect("producer must mine block 1");

            assert_eq!(
                block.metadata.get(GENESIS_ORACLE_SET_KEY),
                Some(&g.declaration.encode_oracle_set()),
                "block 1 must record the exact final N=3 oracle set as chain content"
            );
            assert_eq!(
                block.metadata.get(GENESIS_ORACLE_QUORUM_KEY),
                Some(&g.declaration.oracle_quorum().to_string()),
                "block 1 must record the final quorum K=2 as chain content"
            );
            for tx in block
                .transactions
                .iter()
                .filter(|tx| tx.tx_type == TransactionType::OracleRegistration)
            {
                let payload = tx
                    .parse_oracle_registration()
                    .expect("every included registration must parse");
                assert!(
                    g.declaration
                        .matches(&payload.oracle_set, payload.oracle_quorum),
                    "every block-1 OracleRegistration must carry the final N=3/K=2 declaration"
                );
            }
            (producer_chain, block)
        }

        /// Apply `block` to a fresh peer over the real P2P seam, writing to
        /// `storage` — the same storage the restart leg later reopens.
        async fn s4_apply_to_fresh_peer(
            block: &Block,
            storage: &Arc<RwLock<Storage>>,
        ) -> Arc<RwLock<Blockchain>> {
            // Round-trip through bincode to mirror exactly what arrives on the wire.
            // The enclosing `#[cfg(test)]` mask does not reach this module — the gate's
            // `_test_mask` loses the `mod tests` region earlier in this file (scope defect
            // reported with #2478) — so the exemption is annotated explicitly.
            // panic-ok: test-only S4 fixture helper; a panic is a failing test, not a reachable input path.
            let block_bytes = bincode::serialize(block).expect("block serializes");
            let block_applied: Block =
                // The enclosing `#[cfg(test)]` mask does not reach this module — the gate's
                // `_test_mask` loses the `mod tests` region earlier in this file (scope defect
                // reported with #2478) — so the exemption is annotated explicitly.
                // panic-ok: test-only S4 fixture helper; a panic is a failing test, not a reachable input path.
                bincode::deserialize(&block_bytes).expect("block deserializes");

            let peer_chain = Arc::new(RwLock::new(Blockchain::default()));
            let peer_consensus = test_consensus();
            let mut fork_buffer = ForkCandidateBuffer::new();
            let mut orphan_buffer = OrphanBlockBuffer::new();

            apply_p2p_block(
                block_applied,
                P2pBlockSource::HistoricalSync,
                &peer_chain,
                storage,
                &peer_consensus,
                None,
                &mut fork_buffer,
                &mut orphan_buffer,
            )
            .await;

            peer_chain
        }

        /// The end state block 1 must produce, asserted structurally rather
        /// than by `state_root` alone — a root comparison alone would pass if
        /// BOTH sides were wrong in the same way.
        fn assert_genesis_oracle_state(state: &State, g: &S4Genesis, label: &str) {
            let (id_a, id_b, id_c) = (g.id_a.as_str(), g.id_b.as_str(), g.id_c.as_str());
            assert_eq!(
                state.oracle_quorum, 2,
                "[{label}] genesis quorum must be 2 — the last absolute registration wins"
            );
            assert!(
                state.oracle_set.contains(id_a),
                "[{label}] oracle A must be seated"
            );
            assert!(
                state.oracle_set.contains(id_b),
                "[{label}] oracle B must be seated"
            );
            assert!(
                state.oracle_set.contains(id_c),
                "[{label}] oracle C must be seated"
            );
            assert_eq!(
                state.oracle_set.len(),
                3,
                "[{label}] exactly the three ceremony oracles, no extras"
            );

            // The signing-key epochs are the half that makes the set usable: an
            // oracle seated without a registered key cannot sign an attestation.
            // `snapshot_for_role` is the same accessor `state_root_material`
            // folds, so asserting through it checks the bytes that get hashed.
            let oracle_keys = state.signing_keys.snapshot_for_role(SigningRole::Oracle);
            for (who, id, signing) in [
                ("A", id_a, g.signing_a.as_str()),
                ("B", id_b, g.signing_b.as_str()),
                ("C", id_c, g.signing_c.as_str()),
            ] {
                let epochs = oracle_keys
                    .iter()
                    .find(|(identity, _)| identity == id)
                    .map(|(_, epochs)| epochs)
                    .unwrap_or_else(|| panic!("[{label}] oracle {who} has no signing-key entry"));
                assert!(
                    // secret-leak-ok: these are the oracle's PUBLIC signing key, its epoch number
                    // and its identity — public registry content hashed into `state_root`. No
                    // private material is in the expression or the message; the scanner matched
                    // the `signing` identifier next to an assertion macro (name-based blind spot,
                    // #2453/#2467). Must sit INSIDE the macro span: the allowlist scans the lines
                    // the invocation spans, not the line above it.
                    epochs
                        .iter()
                        .any(|e| e.epoch == EPOCH && e.public_key == signing),
                    "[{label}] oracle {who} must have its CEREMONY signing key at \
                     epoch {EPOCH} — the epoch-0 key derived from the identity is \
                     not the key it will sign attestations with"
                );
            }

            assert_eq!(
                state.governance.params.oracle_quorum, 2,
                "[{label}] governance params must mirror the consensus quorum"
            );
        }

        // -----------------------------------------------------------------
        // GREEN — all four legs, each rooted in the block the producer built
        // -----------------------------------------------------------------

        /// Both cumulative registrations go into **one** block through the real
        /// producer path; the serialized block is accepted by the real peer
        /// path; the peer's own storage is reopened and replayed; and a
        /// snapshot taken from the peer's accepted state rebuilds the same
        /// `state_root` on a node that never saw the block.
        /// `_env_guard` is deliberately held across the awaits below: the
        /// governance allowlist is read at VERIFICATION time (producer
        /// self-screen, peer `verify_block`), not at construction time, so
        /// dropping it before the mine reopens exactly the race that made
        /// `producer_applier_block_apply_parity_oracle_registration` flaky.
        /// `#[tokio::test]` runs on a current-thread runtime, so the future is
        /// never required to be `Send`.
        #[allow(clippy::await_holding_lock)]
        #[tokio::test]
        async fn genesis_block_1_seats_the_oracle_set_on_producer_peer_restart_and_sync() {
            let g = s4_registration_txs();
            let txs = &g.txs;
            let _env_guard = s4_install_governance_env(&g.gov_pk);

            // ---- 1. PRODUCER ------------------------------------------------
            let (producer_chain, block_1) = s4_mine_block_1(&g, txs).await;

            for (i, tx) in txs.iter().enumerate() {
                assert!(
                    block_1.transactions.iter().any(|t| t.id == tx.id),
                    "registration {i} must survive assembly and the #1730 producer \
                     self-screen and be IN block 1 — if it was dropped, every \
                     assertion below is about a block that does not seat the \
                     oracle set; block tx types = {:?}",
                    block_1
                        .transactions
                        .iter()
                        .map(|t| t.tx_type)
                        .collect::<Vec<_>>()
                );
            }
            assert_eq!(
                block_1.height, 1,
                "the oracle set must be seated in block 1 — there is no later \
                 block that can add it without another chain cut"
            );

            let producer_root = {
                let producer = producer_chain.read().await;
                assert_eq!(producer.height, 1, "producer must have applied block 1");
                assert_genesis_oracle_state(&producer.state, &g, "producer");
                producer.state.state_root
            };
            assert_ne!(
                producer_root,
                Blockchain::default().state.state_root,
                "the root must actually have moved off genesis, or every \
                 convergence comparison below is vacuous"
            );

            // ---- 2. PEER APPLY ----------------------------------------------
            let peer_storage_path = std::env::temp_dir()
                .join(format!("wrfcoin-s4-genesis-oracle-{}.redb", fastrand::u64(..)));
            let peer_storage = Arc::new(RwLock::new(
                Storage::open(&peer_storage_path, DEFAULT_CHECKPOINT_INTERVAL)
                    .expect("peer storage opens"),
            ));
            let peer_chain = s4_apply_to_fresh_peer(&block_1, &peer_storage).await;

            let peer_state = {
                let peer = peer_chain.read().await;
                assert_eq!(
                    peer.height, 1,
                    "peer must ACCEPT block 1 — height 0 means apply_p2p_block \
                     rejected it at hash, consensus validation, transition apply \
                     or checked finalization"
                );
                assert_eq!(peer.tip, block_1.calculate_hash(), "peer must be on the producer's block");
                assert_genesis_oracle_state(&peer.state, &g, "peer");
                assert_eq!(
                    // secret-leak-ok: `signing_keys` here names a state_root SECTION, in prose.
                    // The compared values are two state roots. Same name-based blind spot;
                    // annotation must sit inside the macro span.
                    peer.state.state_root, producer_root,
                    "CONVERGENCE: peer and producer must agree on state_root after \
                     block 1 (block hash is not the property under test — state_root \
                     is what carries oracle_set, oracle_quorum and signing_keys)"
                );
                peer.state.clone()
            };

            // ---- 3. RESTART -------------------------------------------------
            // Reopen the redb `apply_p2p_block` itself wrote. Nothing here is
            // hand-supplied: the transitions and checkpoint under this file are
            // the ones the peer derived from the block it accepted.
            //
            // Close the live handle first — redb is single-writer, so this is
            // also what makes the reopen a genuine restart rather than a second
            // reader on an open database.
            drop(peer_storage);
            {
                let storage = Storage::open(&peer_storage_path, DEFAULT_CHECKPOINT_INTERVAL)
                    .expect("peer storage reopens as a fresh process would");
                let recovered = storage
                    .load_state()
                    .expect("WAL replay must succeed")
                    .expect("a persisted chain must be recovered");

                assert_eq!(recovered.height, 1, "recovered tip must be block 1");
                assert_genesis_oracle_state(&recovered.state, &g, "restart");
                assert_eq!(
                    recovered.state.state_root, producer_root,
                    "RESTART: replay of the peer's own storage must reproduce the \
                     pre-restart state_root — a node that forgets its oracle set on \
                     reboot cannot rejoin consensus"
                );
            }

            // ---- 4. SYNC ----------------------------------------------------
            // Source is the peer's ACCEPTED post-block-1 state, and the checks
            // are the ones `ingest_snapshot_sync_payload` runs: entries must
            // reproduce the signed snapshot root, and the state rebuilt from
            // them must round-trip to the same snapshot root.
            {
                let entries = build_snapshot_entries(&peer_state).expect("snapshot export");
                // #3288 v2: whole-State entries. The oracle set, quorum and
                // signing-key registry must all be present, or a syncing node
                // comes up with an empty or unusable oracle set.
                for key in [&b"state/oracle_set"[..], b"state/oracle_quorum", b"state/signing_keys"] {
                    assert!(
                        entries.iter().any(|(k, _)| k.as_slice() == key),
                        "the snapshot must carry {}",
                        String::from_utf8_lossy(key)
                    );
                }

                let advertised = build_state_snapshot(1, hex::encode(peer_state.state_root.0), &entries);

                let mut synced = State::default();
                synced
                    .apply_snapshot_entries(&entries)
                    .expect("snapshot must apply cleanly");
                synced.recalculate_state_root();

                assert_genesis_oracle_state(&synced, &g, "sync");
                assert_eq!(
                    synced.state_root, producer_root,
                    "SYNC: a node rebuilt from the peer's snapshot must reach the \
                     same state_root — otherwise a fast-synced node disagrees with \
                     the chain about who the oracles are"
                );

                // The ingest handler's lossy-sync guard: rebuild the entries from
                // the applied state and require the same snapshot root. v2's
                // encoding is canonical (#3288), so this holds byte for byte
                // even though the oracle set is a hash set.
                let rebuilt = build_state_snapshot(
                    1,
                    hex::encode(synced.state_root.0),
                    &build_snapshot_entries(&synced).expect("snapshot export"),
                );
                assert_eq!(
                    rebuilt.snapshot_root_hex, advertised.snapshot_root_hex,
                    "SYNC round-trip: applied state must re-emit the advertised \
                     snapshot root (this is the lossy-fast-sync check \
                     ingest_snapshot_sync_payload performs before swapping state in)"
                );
            }

            let _ = std::fs::remove_file(&peer_storage_path);
        }

        // -----------------------------------------------------------------
        // ORDER — the hazard that only a real producer path can surface
        // -----------------------------------------------------------------

        /// The producer does **not** preserve the order transactions were
        /// staged in, and `oracle_set` is absolute — so the registration that
        /// happens to land last decides the genesis oracle set.
        ///
        /// This is the finding that the previous, hand-applied version of this
        /// gate structurally could not make: it fed the transitions to
        /// `apply_transitions_batch` in a fixed order, so the order the block
        /// builder actually chooses never entered the test.
        ///
        /// Two properties, both required before the cut:
        ///
        /// 1. With every registration carrying the FINAL set and quorum, block
        ///    1 converges on one `state_root` for BOTH orderings — asserted
        ///    explicitly, and then confirmed against the real block builder.
        /// 2. Under the incremental shape, order **changes the outcome** — the
        ///    collapsing order seats one oracle behind an unreachable quorum of
        ///    two. Asserted through the applier's own extractor and batch apply
        ///    over two blocks that differ only in transaction order.
        /// `_env_guard` is deliberately held across the awaits below: the
        /// governance allowlist is read at VERIFICATION time (producer
        /// self-screen, peer `verify_block`), not at construction time, so
        /// dropping it before the mine reopens exactly the race that made
        /// `producer_applier_block_apply_parity_oracle_registration` flaky.
        /// `#[tokio::test]` runs on a current-thread runtime, so the future is
        /// never required to be `Send`.
        #[allow(clippy::await_holding_lock)]
        #[tokio::test]
        async fn block_1_converges_regardless_of_producer_transaction_order() {
            let g = s4_registration_txs();
            let txs = &g.txs;
            let _env_guard = s4_install_governance_env(&g.gov_pk);

            // The applier's own pipeline: `extract_transitions` then
            // `apply_transitions_batch`, which is what `apply_p2p_block` runs.
            // Used to drive an EXPLICIT transaction order, which the mempool
            // does not let a caller choose.
            let apply_in_order = |ordered: Vec<Transaction>| {
                let mut block = Block::new(3, CryptoHash::default(), ordered, 1, 1);
                block.set_hash(block.calculate_hash());
                let mut state = State::default();
                let transitions =
                    extract_transitions(&state, &block, block.height, block.timestamp.timestamp());
                state
                    .apply_transitions_batch(transitions)
                    .expect("authorized registrations apply");
                state
            };

            // ---- 1a. final-set shape, BOTH orders, deterministically ---------
            let forward = apply_in_order(txs.clone());
            let backward = apply_in_order(txs.iter().rev().cloned().collect());
            assert_genesis_oracle_state(&forward, &g, "order-forward");
            assert_genesis_oracle_state(&backward, &g, "order-reversed");
            assert_eq!(
                forward.state_root, backward.state_root,
                "ORDER INDEPENDENCE: with every registration carrying the final \
                 set and quorum, reversing block 1's transactions must not change \
                 the genesis state_root"
            );

            // ---- 1b. and the real block builder agrees -----------------------
            // The builder's ordering is stable within a process but varies
            // BETWEEN processes (it falls out of hash iteration order), so this
            // sweep samples one ordering per run and the CI fleet samples both
            // over time. The assertion is on the roots either way, so the test
            // is deterministic regardless of which ordering this process drew.
            const ATTEMPTS: usize = 24;
            let mut roots: std::collections::BTreeSet<[u8; 32]> = std::collections::BTreeSet::new();
            let mut orders = std::collections::BTreeSet::new();
            for attempt in 0..ATTEMPTS {
                let (chain, block) = s4_mine_block_1(&g, txs).await;
                let order: Vec<usize> = block
                    .transactions
                    .iter()
                    .filter_map(|t| txs.iter().position(|s| s.id == t.id))
                    .collect();
                assert_eq!(
                    order.len(),
                    3,
                    "attempt {attempt}: all three registrations must be in block 1"
                );
                orders.insert(order);
                let c = chain.read().await;
                assert_genesis_oracle_state(&c.state, &g, "order-sweep");
                roots.insert(c.state.state_root.0);
            }
            eprintln!(
                "S4: {ATTEMPTS} block-1 builds produced {} distinct tx orderings and {} distinct \
                 state_roots: {orders:?}",
                orders.len(),
                roots.len()
            );
            assert_eq!(
                roots.len(),
                1,
                "ORDER INDEPENDENCE: every ordering the block builder chooses must \
                 land the same genesis state_root. More than one root here means \
                 block 1's contents depend on a coin flip inside the producer, on \
                 a block that cannot be re-cut."
            );

            // ---- 2. the incremental shape is NOT order-independent -----------
            // Same applier pipeline both times — `extract_transitions` then
            // `apply_transitions_batch`, which is what `apply_p2p_block` runs —
            // over two blocks differing ONLY in transaction order.
            let (inc_txs, inc_a, inc_b) = s4_incremental_registration_txs();
            let intended = apply_in_order(inc_txs.clone());
            let reversed = apply_in_order(inc_txs.iter().rev().cloned().collect());

            assert_eq!(
                intended.oracle_set.len(),
                2,
                "control: the INTENDED order does seat both oracles"
            );
            assert_eq!(intended.oracle_quorum, 2);

            assert!(
                !reversed.oracle_set.contains(&inc_b),
                "ORDER HAZARD: reversed, the cumulative registration applies first \
                 and the single-oracle registration overwrites it — oracle B is \
                 evicted from genesis"
            );
            assert!(reversed.oracle_set.contains(&inc_a));
            assert_eq!(
                reversed.oracle_quorum, 1,
                "the surviving quorum comes from whichever registration landed last"
            );
            assert_ne!(
                intended.state_root, reversed.state_root,
                "the two orders must reach different state — this is precisely why \
                 every block-1 registration has to carry the final set"
            );
        }

        // -----------------------------------------------------------------
        // RED — tampered genesis content is REJECTED, with no advancement
        // -----------------------------------------------------------------

        /// The producer refuses to cut block 1 when one declared operator's
        /// registration fails governance authorization.
        ///
        /// This is the failure mode of a node whose
        /// `WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS` is missing or wrong at cut time:
        /// the #1730 self-screen drops the transaction. The block-one outcome
        /// gate must then refuse the whole cut as incompletely seated, rather
        /// than build a dead genesis behind quorum 2.
        /// `_env_guard` is deliberately held across the awaits below: the
        /// governance allowlist is read at VERIFICATION time (producer
        /// self-screen, peer `verify_block`), not at construction time, so
        /// dropping it before the mine reopens exactly the race that made
        /// `producer_applier_block_apply_parity_oracle_registration` flaky.
        /// `#[tokio::test]` runs on a current-thread runtime, so the future is
        /// never required to be `Send`.
        #[allow(clippy::await_holding_lock)]
        #[tokio::test]
        async fn tampered_genesis_registration_is_rejected_by_the_producer() {
            let g = s4_registration_txs();
            let txs = &g.txs;
            let _env_guard = s4_install_governance_env(&g.gov_pk);

            // Positive control: all three signed registrations cut a valid
            // N=3/K=2 block through this exact producer seam.
            let (control_chain, _control_block) = s4_mine_block_1(&g, txs).await;
            assert_eq!(
                control_chain.read().await.height,
                1,
                "control: the unmodified N=3/K=2 ceremony must cut block 1"
            );

            // RED mutation, verbatim: `payload.auth_signature = None`.
            let tampered = s4_strip_authorization(&txs[1]);
            let staged = vec![txs[0].clone(), tampered, txs[2].clone()];
            let producer_chain = Arc::new(RwLock::new(Blockchain::default()));
            let producer_mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
            {
                let mut mp = producer_mempool.write().await;
                for tx in staged {
                    mp.add_transaction(tx)
                        .expect("the mempool admits the RED fixture before self-screening");
                }
            }
            let error = s4_consensus(&g)
                .await
                .create_pop_block_with_state(
                    Arc::clone(&producer_chain),
                    producer_mempool,
                    &test_address_string(0x6a),
                )
                .await
                .expect_err(
                    "the producer must refuse a cut after self-screening drops one \
                     of the three declared registrations",
                );
            let message = format!("{error:?}");
            assert!(
                message.contains("genesis declares 3 oracles but only 2 distinct"),
                "the refusal must be the incomplete-seating gate, not an unrelated \
                 producer error: {message}"
            );
            let producer = producer_chain.read().await;
            assert_eq!(
                producer.height, 0,
                "a refused cut must not advance the producer"
            );
            assert_eq!(
                producer.state.state_root,
                Blockchain::default().state.state_root,
                "a refused cut must leave state_root untouched"
            );
        }

        /// A correctly governance-signed registration still cannot substitute
        /// a partial set for the final N=3/K=2 declaration. This is the direct
        /// RED for #2488 rather than an authorization failure.
        #[allow(clippy::await_holding_lock)]
        #[tokio::test]
        async fn partial_but_authorized_genesis_set_is_rejected_by_the_producer() {
            let g = s4_registration_txs();
            let txs = &g.txs;
            let _env_guard = s4_install_governance_env(&g.gov_pk);

            // Positive control on the exact producer seam.
            let (control_chain, _control_block) = s4_mine_block_1(&g, txs).await;
            assert_eq!(control_chain.read().await.height, 1);

            // RED mutation, verbatim:
            // `oracle_set = [A, B, C]` -> `oracle_set = [A, B]`.
            // Re-sign it with the allowlisted governance key so an
            // authorization failure cannot explain the refusal.
            let (gov_sk, _) = s4_keypair(SEED_GOVERNANCE);
            let partial_b = Transaction::new_oracle_registration(
                vec![g.id_a.clone(), g.id_b.clone()],
                2,
                &g.id_b,
                &g.signing_b,
                EPOCH,
                ACTIVATED_AT,
                &gov_sk,
            );
            assert!(
                partial_b.verify_oracle_registration_authorization(),
                "the RED registration must remain correctly governance-signed"
            );

            let producer_chain = Arc::new(RwLock::new(Blockchain::default()));
            let producer_mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
            {
                let mut mp = producer_mempool.write().await;
                for tx in [txs[0].clone(), partial_b, txs[2].clone()] {
                    mp.add_transaction(tx)
                        .expect("the mempool admits the RED fixture before self-screening");
                }
            }
            let error = s4_consensus(&g)
                .await
                .create_pop_block_with_state(
                    Arc::clone(&producer_chain),
                    producer_mempool,
                    &test_address_string(0x6a),
                )
                .await
                .expect_err(
                    "the producer must refuse a block-1 registration that omits \
                     oracle C from the final set",
                );
            let message = format!("{error:?}");
            assert!(
                message.contains("genesis declares 3 oracles but only 2 distinct")
                    || message.contains("oracle registration disagrees with the set genesis declared"),
                "the partial-set registration must fail closed against the declared \
                 genesis set, not an unrelated producer error: {message}"
            );
            let producer = producer_chain.read().await;
            assert_eq!(producer.height, 0, "a refused cut must not advance");
            assert_eq!(
                producer.state.state_root,
                Blockchain::default().state.state_root,
                "a refused partial-set cut must leave state_root untouched"
            );
        }

        /// A peer refuses a block carrying a registration whose governance
        /// authorization does not verify — and does not advance height or
        /// `state_root` by so much as one field.
        ///
        /// The forged block is the producer's own valid block with one
        /// transaction swapped and the hash recomputed: exactly what a
        /// malicious or misconfigured producer would gossip. The paired control
        /// applies the untampered block through the same code and requires
        /// acceptance, so the rejection is attributable to the tampering rather
        /// than to how the block was assembled.
        /// `_env_guard` is deliberately held across the awaits below: the
        /// governance allowlist is read at VERIFICATION time (producer
        /// self-screen, peer `verify_block`), not at construction time, so
        /// dropping it before the mine reopens exactly the race that made
        /// `producer_applier_block_apply_parity_oracle_registration` flaky.
        /// `#[tokio::test]` runs on a current-thread runtime, so the future is
        /// never required to be `Send`.
        #[allow(clippy::await_holding_lock)]
        #[tokio::test]
        async fn tampered_genesis_registration_is_rejected_by_the_peer() {
            let g = s4_registration_txs();
            let (txs, id_a, id_b, id_c) =
                (&g.txs, g.id_a.clone(), g.id_b.clone(), g.id_c.clone());
            let _env_guard = s4_install_governance_env(&g.gov_pk);

            let (_producer_chain, good_block) = s4_mine_block_1(&g, txs).await;

            let tampered = s4_strip_authorization(&txs[1]);
            let mut forged = good_block.clone();
            let idx = forged
                .transactions
                .iter()
                .position(|t| t.id == txs[1].id)
                .expect("the second registration is in the good block");
            forged.transactions[idx] = tampered.clone();
            let forged_hash = forged.calculate_hash();
            forged.set_hash(forged_hash);

            // Control: the SAME code path accepts the untampered block.
            let good_storage = Arc::new(RwLock::new(
                Storage::open(
                    &std::env::temp_dir()
                        .join(format!("wrfcoin-s4-ctrl-{}.redb", fastrand::u64(..))),
                    DEFAULT_CHECKPOINT_INTERVAL,
                )
                .expect("control storage opens"),
            ));
            let good_peer = s4_apply_to_fresh_peer(&good_block, &good_storage).await;
            let (accepted_height, accepted_root) = {
                let c = good_peer.read().await;
                (c.height, c.state.state_root)
            };
            assert_eq!(
                accepted_height, 1,
                "control: the untampered block must be ACCEPTED, or the rejection \
                 below is not attributable to the tampering"
            );

            // The forged block.
            let genesis_root = Blockchain::default().state.state_root;
            let forged_storage_path = std::env::temp_dir()
                .join(format!("wrfcoin-s4-forged-{}.redb", fastrand::u64(..)));
            let forged_storage = Arc::new(RwLock::new(
                Storage::open(&forged_storage_path, DEFAULT_CHECKPOINT_INTERVAL)
                    .expect("forged-case storage opens"),
            ));
            let forged_peer = s4_apply_to_fresh_peer(&forged, &forged_storage).await;
            {
                let c = forged_peer.read().await;
                assert_eq!(
                    c.height, 0,
                    "PEER GATE: a block carrying an unauthorized oracle \
                     registration must be REJECTED — the peer must not advance"
                );
                assert_eq!(
                    c.state.state_root, genesis_root,
                    "PEER GATE: state_root must be untouched. #1979 applies \
                     transitions to a working copy, so a rejection must leave the \
                     live state byte-identical to genesis"
                );
                assert!(
                    c.state.oracle_set.is_empty(),
                    "PEER GATE: no oracle may be seated from a rejected block"
                );
            }
            assert_ne!(
                accepted_root, genesis_root,
                "the accepted control must have moved the root, or 'unchanged' \
                 above is indistinguishable from 'nothing ever happens'"
            );

            // The restart and sync legs are DOWNSTREAM of peer acceptance, so
            // the rejection must starve them both. Asserting this is what makes
            // the RED control cover all four legs rather than two: a gate that
            // rejected in memory but still persisted, or still handed the bad
            // set to a fast-syncing node, would be no gate at all.
            drop(forged_storage);
            {
                let storage = Storage::open(&forged_storage_path, DEFAULT_CHECKPOINT_INTERVAL)
                    .expect("forged-case storage reopens");
                let recovered = storage.load_state().expect("load_state must not error");
                assert!(
                    recovered.is_none_or(|c| c.height == 0),
                    "RESTART under tampering: a rejected block must leave NOTHING \
                     persisted — a node that replays it on reboot would seat an \
                     unauthorized oracle that no peer agrees with"
                );
            }
            {
                let rejected_state = forged_peer.read().await.state.clone();
                let entries = build_snapshot_entries(&rejected_state).expect("snapshot export");
                let mut synced = State::default();
                synced
                    .apply_snapshot_entries(&entries)
                    .expect("snapshot of the untouched state must still apply");
                synced.recalculate_state_root();
                assert!(
                    synced.oracle_set.is_empty(),
                    "SYNC under tampering: a node fast-syncing from the rejecting \
                     peer must not receive the unauthorized oracle"
                );
                // Compare against a RECALCULATED empty state, not the stored
                // default root: `Blockchain::default()` carries the all-zero
                // placeholder until something recomputes it, and a snapshot
                // round-trip always recomputes.
                let empty_root = {
                    let mut empty = State::default();
                    empty.recalculate_state_root();
                    empty.state_root
                };
                assert_eq!(
                    synced.state_root, empty_root,
                    "SYNC under tampering: the synced root must be the empty-state \
                     root — nothing from the rejected block may reach a fast-syncing peer"
                );
            }
            let _ = std::fs::remove_file(&forged_storage_path);

            // Attribution: same fresh chain, same consensus, one differing
            // transaction. `validate_block` accepts one and refuses the other.
            {
                let fresh = Blockchain::default();
                let consensus = test_consensus();
                assert_eq!(
                    consensus.validate_block(&good_block, &fresh).await.ok(),
                    Some(true),
                    "attribution: the untampered block passes consensus validation"
                );
                assert_eq!(
                    consensus.validate_block(&forged, &fresh).await.ok(),
                    Some(false),
                    "attribution: the ONLY difference is the stripped governance \
                     signature, so the refusal is the #2055 authorization gate in \
                     verify_block and not an assembly artifact"
                );
            }

            // Defense in depth: even with validation bypassed, the extractor is
            // fail-closed — the forged block yields NO registration transition.
            {
                let fresh = Blockchain::default();
                let transitions = crate::transition_extractor::extract_transitions(
                    &fresh.state,
                    &forged,
                    forged.height,
                    forged.timestamp.timestamp(),
                );
                let mut seated: Vec<_> = transitions
                    .iter()
                    .filter_map(|t| match t {
                        StateTransition::ApplyOracleRegistration { identity, .. } => {
                            Some(identity.clone())
                        },
                        _ => None,
                    })
                    .collect();
                seated.sort();
                let mut expected = vec![id_a.clone(), id_c.clone()];
                expected.sort();
                assert_eq!(
                    seated,
                    expected,
                    "the extractor must emit transitions ONLY for the two authorized \
                     registrations — fail-closed, so forged oracle B cannot be seated \
                     even if the block reached the apply path (B={id_b})"
                );
            }
        }
    }

    // ---------------------------------------------------------------------
    // #3288 3b: snapshot sync carries and binds the tip — end-to-end harness.
    // ---------------------------------------------------------------------
    //
    // Node A mines real blocks with transfers. Node B, fresh, ingests A's
    // snapshot at H through the same verifier and installer the ingest route
    // runs, then follows A through the real P2P applier, with a restart right
    // after ingest and another mid-way. Every height compares B's state_root
    // and tip with A's.
    //
    // This test binary runs at signing protocol V1 (see
    // `desktop_peer_completes_ibd_past_genesis_against_a_seed_1614`), where
    // blocks carry no state-root commitment. The V2 commitment binding is
    // covered directly in `routes::snapshot::snapshot_tip_3288_3b_tests` and
    // `chain_snapshot`'s tests.
    mod snapshot_tip_carry_3288 {
        use super::*;
        use crate::routes::snapshot::{
            SnapshotApplyRefusal, TestGenesisPin, current_snapshot_for_tests,
            install_verified_snapshot, verify_and_apply_snapshot,
        };
        use rand::SeedableRng;
        use wrfcoin_native_chain::blockchain::transaction::{
            TransactionInput, TransactionOutput, transparent_transfer_authorization_message,
        };
        use wrfcoin_native_chain::crypto::hash::hash_string;
        use wrfcoin_native_chain::crypto::signature::{PrivateKey, PublicKey};

        const SNAPSHOT_HEIGHT: u64 = 70;
        const RESTART_HEIGHT: u64 = 80;
        const FINAL_HEIGHT: u64 = 90;

        struct MinedChain {
            /// Blocks `1 ..= FINAL_HEIGHT`, index `h - 1`.
            blocks: Vec<Block>,
            /// A's state_root after each block, index `h - 1`.
            roots: Vec<[u8; 32]>,
            /// A's snapshot at `SNAPSHOT_HEIGHT`: payload and entries.
            payload: wrfcoin_native_chain::blockchain::snapshot_sync::SnapshotSyncPayload,
            entries: Vec<(Vec<u8>, Vec<u8>)>,
        }

        fn transfer(sender_sk: &PrivateKey, nonce: u64) -> Transaction {
            let sender_pk = PublicKey::from_private_key(sender_sk);
            let sender = Address::from_public_key(&sender_pk);
            let sender_hex = format!("0x{}", hex::encode(sender.0));
            let recipient_hex = format!("0x{}", hex::encode([0x40 + (nonce % 32) as u8; 20]));
            let amount = 1_000 + nonce;
            let auth_message =
                transparent_transfer_authorization_message(&sender_hex, &recipient_hex, amount, nonce);
            let payload = serde_json::json!({
                "from": sender_hex,
                "to": recipient_hex,
                "amount": amount,
                "transparent_opt_out": true,
                "auth_pubkey": sender_pk.to_string(),
                "auth_signature": sender_sk.sign(&auth_message).to_string(),
            });
            let mut tx = Transaction::new(
                vec![TransactionInput {
                    txid: hash_string(&format!("snapshot-3288-transfer-{nonce}")),
                    vout: 0,
                    signature: None,
                    public_key: Some(sender_hex.clone()),
                    coinbase_data: None,
                }],
                vec![TransactionOutput {
                    amount: 0,
                    address: sender_hex,
                    script: "OP_TRUE".to_string(),
                    nft_data: None,
                }],
            );
            tx.tx_type = TransactionType::Transfer;
            tx.sender_address = sender;
            tx.nonce = nonce;
            tx.data = Some(serde_json::to_string(&payload).expect("payload json"));
            tx.set_id();
            assert!(tx.verify_transparent_transfer_authorization());
            tx
        }

        fn sender_key() -> PrivateKey {
            PrivateKey::generate(&mut rand::rngs::StdRng::seed_from_u64(0x3288_3b))
        }

        /// The genesis every node of this chain starts from: the sender funded.
        fn genesis() -> Blockchain {
            let sender = Address::from_public_key(&PublicKey::from_private_key(&sender_key()));
            let mut genesis = Blockchain::default();
            genesis.state.create_account(sender, Balance::new(10_000_000));
            genesis.state.recalculate_state_root();
            genesis
        }

        /// Node A: mine `FINAL_HEIGHT` blocks, one transfer each, and take the
        /// snapshot at `SNAPSHOT_HEIGHT` through the export the routes serve.
        async fn mine_source_chain() -> MinedChain {
            mine_source_chain_from(genesis()).await
        }

        async fn mine_source_chain_from(genesis: Blockchain) -> MinedChain {
            let sender_sk = sender_key();
            let chain = Arc::new(RwLock::new(genesis));
            let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
            let consensus = test_consensus();
            let archive_dir = tempfile::tempdir().expect("tempdir");
            let archive = Storage::open(&archive_dir.path().join("a.redb"), DEFAULT_CHECKPOINT_INTERVAL)
                .expect("source archive opens");

            let (mut blocks, mut roots, mut snapshot) = (Vec::new(), Vec::new(), None);
            for height in 1..=FINAL_HEIGHT {
                mempool
                    .write()
                    .await
                    .add_transaction(transfer(&sender_sk, height - 1))
                    .expect("mempool admits the transfer");
                let block = consensus
                    .create_pop_block_with_state(
                        Arc::clone(&chain),
                        Arc::clone(&mempool),
                        &test_address_string(0x3b),
                    )
                    .await
                    .unwrap_or_else(|e| panic!("A mines block {height}: {e}"));
                assert_eq!(block.height, height);
                assert!(
                    block.transactions.iter().any(|t| t.tx_type == TransactionType::Transfer),
                    "block {height} must carry its transfer, or the state never moves"
                );
                let a = chain.read().await;
                assert_eq!(a.height, height);
                roots.push(a.state.state_root.0);
                if height == SNAPSHOT_HEIGHT {
                    snapshot = Some(current_snapshot_for_tests(&a, &archive).expect("A exports"));
                }
                drop(a);
                blocks.push(block);
            }
            let (payload, entries) = snapshot.expect("snapshot taken");
            MinedChain { blocks, roots, payload, entries }
        }

        fn open_storage(path: &std::path::Path) -> Arc<RwLock<Storage>> {
            Arc::new(RwLock::new(
                Storage::open(path, DEFAULT_CHECKPOINT_INTERVAL).expect("B storage opens"),
            ))
        }

        /// A restart: close B's only storage handle, reopen the file, and load
        /// the chain exactly as boot does.
        fn restart(storage: Arc<RwLock<Storage>>, path: &std::path::Path) -> (Arc<RwLock<Blockchain>>, Arc<RwLock<Storage>>) {
            match Arc::try_unwrap(storage) {
                Ok(lock) => drop(lock.into_inner()),
                Err(_) => panic!("B's storage must have one handle, or this is not a restart"),
            }
            let storage = open_storage(path);
            let recovered = storage
                .try_read()
                .expect("uncontended")
                .load_state()
                .expect("B's database loads")
                .expect("B's database holds a chain");
            (Arc::new(RwLock::new(recovered)), storage)
        }

        async fn assert_b_matches_a(label: &str, b: &Arc<RwLock<Blockchain>>, a: &MinedChain, height: u64) {
            let b = b.read().await;
            // The median-time-past gate is live: the window below the tip is
            // there, so `median_time_past` does not fall back to `None`.
            let recent: Vec<i64> = b
                .blocks()
                .range(..=height)
                .rev()
                .take(wrfcoin_shared_protocol::consensus::proof_of_publishing::MEDIAN_TIME_PAST_WINDOW)
                .map(|(_, block)| block.timestamp.timestamp())
                .collect();
            assert!(
                wrfcoin_shared_protocol::consensus::proof_of_publishing::median_time_past(&recent).is_some(),
                "[{label}] B must hold a full median-time-past window at {height}"
            );
            assert_eq!(b.height, height, "[{label}] B height");
            assert_eq!(
                b.tip,
                a.blocks[height as usize - 1].hash.expect("hashed"),
                "[{label}] B tip must be A's block {height}"
            );
            assert_eq!(
                b.state.state_root.0,
                a.roots[height as usize - 1],
                "[{label}] B state_root must equal A's at {height}"
            );
        }

        /// Finalized = H must come back from DISK: B holds no state below the
        /// snapshot, so after a restart a finalized height below H would let
        /// `plan_reorg` revert into history B cannot rebuild (#3330 review R5a).
        async fn assert_finalized_survives(label: &str, b: &Arc<RwLock<Blockchain>>) {
            let finalized = b.read().await.finalized_height();
            assert!(
                finalized >= SNAPSHOT_HEIGHT,
                "[{label}] finalized height {finalized} must stay >= the snapshot height {SNAPSHOT_HEIGHT} across a restart"
            );
        }

        /// A competing branch that shares A's blocks up to `fork` and differs
        /// from `fork + 1` through `tip` (a different nonce, rehashed and
        /// relinked). `plan_reorg` checks structure only, so this is enough to
        /// reach its finality guard.
        fn competing_branch(a: &MinedChain, fork: u64, tip: u64) -> Vec<Block> {
            let mut prev = a.blocks[fork as usize - 1].hash.expect("hashed");
            (fork + 1..=tip)
                .map(|height| {
                    let mut block = a.blocks[height as usize - 1].clone();
                    block.prev_block_hash = prev;
                    block.nonce ^= 0x3288_3b;
                    block.hash = None;
                    let hash = block.calculate_hash();
                    block.set_hash(hash);
                    prev = hash;
                    block
                })
                .collect()
        }

        /// Plan a reorg the way `try_reorg_inner` does: in-memory window plus
        /// the MAX_REORG_DEPTH-bounded range from disk.
        async fn plan(
            b: &Arc<RwLock<Blockchain>>,
            storage: &Arc<RwLock<Storage>>,
            candidate: &[Block],
        ) -> Option<wrfcoin_native_chain::blockchain::chain::ReorgPlan> {
            let chain = b.read().await;
            let disk: std::collections::HashMap<u64, Block> = storage
                .read()
                .await
                .load_blocks_range(
                    chain
                        .height
                        .saturating_sub(wrfcoin_native_chain::blockchain::chain::MAX_REORG_DEPTH),
                    chain.height,
                )
                .expect("B's blocks load")
                .into_iter()
                .map(|block| (block.height, block))
                .collect();
            chain
                .plan_reorg_with_loader(candidate, |h| disk.get(&h).cloned())
                .expect("plan_reorg does not error")
        }

        async fn apply(b: &Arc<RwLock<Blockchain>>, storage: &Arc<RwLock<Storage>>, block: &Block) -> P2pBlockApplyOutcome {
            let wire: Block = bincode::deserialize(&bincode::serialize(block).expect("ser")).expect("de");
            let consensus = test_consensus();
            apply_p2p_block(
                wire,
                P2pBlockSource::HistoricalSync,
                b,
                storage,
                &consensus,
                None,
                &mut ForkCandidateBuffer::new(),
                &mut OrphanBlockBuffer::new(),
            )
            .await
        }

        #[tokio::test]
        async fn a_snapshot_synced_node_follows_the_chain_across_restarts_3288() {
            let a = mine_source_chain().await;
            // B's operator pins A's genesis, as WRFCOIN_GENESIS_HASH would.
            let _pin = TestGenesisPin::set(a.blocks[0].hash.expect("hashed").to_hex());
            assert_ne!(a.roots[SNAPSHOT_HEIGHT as usize - 1], a.roots[0], "the state moved");

            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join("b.redb");
            let b = Arc::new(RwLock::new(Blockchain::default()));
            let storage = open_storage(&path);

            // Ingest at H, through the route's verifier and installer.
            let local = b.read().await.state.clone();
            let verified = verify_and_apply_snapshot(local, &a.payload, &a.entries)
                .expect("A's snapshot verifies on fresh B");
            install_verified_snapshot(&b, &storage, verified, SNAPSHOT_HEIGHT)
                .await
                .expect("fresh B installs A's snapshot");
            assert_b_matches_a("ingest", &b, &a, SNAPSHOT_HEIGHT).await;
            assert_eq!(b.read().await.finalized_height(), SNAPSHOT_HEIGHT, "no reorg below the snapshot");

            // Restart 1: straight after ingest.
            let (b, storage) = restart(storage, &path);
            assert_b_matches_a("restart after ingest", &b, &a, SNAPSHOT_HEIGHT).await;
            assert_finalized_survives("restart after ingest", &b).await;
            let lowest = storage.read().await.load_lowest_block().expect("read").expect("a block");
            assert_eq!(lowest.height, 1, "boot needs the height-1 genesis block as the lowest block");
            assert_eq!(lowest.hash, a.blocks[0].hash);

            // Follow A through the real applier to the mid-way restart.
            for height in SNAPSHOT_HEIGHT + 1..=RESTART_HEIGHT {
                let outcome = apply(&b, &storage, &a.blocks[height as usize - 1]).await;
                assert_eq!(outcome, P2pBlockApplyOutcome::Applied, "B applies block {height}");
                assert_b_matches_a("follow", &b, &a, height).await;
            }

            // Restart 2: mid-way.
            let (b, storage) = restart(storage, &path);
            assert_b_matches_a("restart mid-way", &b, &a, RESTART_HEIGHT).await;
            assert_finalized_survives("restart mid-way", &b).await;

            for height in RESTART_HEIGHT + 1..=FINAL_HEIGHT {
                let outcome = apply(&b, &storage, &a.blocks[height as usize - 1]).await;
                assert_eq!(outcome, P2pBlockApplyOutcome::Applied, "B applies block {height}");
                assert_b_matches_a("follow after restart", &b, &a, height).await;
            }
        }

        /// After a restart, B still refuses a reorg that forks below the
        /// snapshot height: B holds no state below H to revert into. The
        /// positive control, a fork AT H through the same planner, is accepted,
        /// so the refusal is the finality guard and not a failure to find the
        /// fork point (#3330 review R5a).
        #[tokio::test]
        async fn a_restarted_snapshot_node_refuses_a_fork_below_the_snapshot_3288() {
            let a = mine_source_chain().await;
            // B's operator pins A's genesis, as WRFCOIN_GENESIS_HASH would.
            let _pin = TestGenesisPin::set(a.blocks[0].hash.expect("hashed").to_hex());
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join("b.redb");
            let b = Arc::new(RwLock::new(Blockchain::default()));
            let storage = open_storage(&path);
            let verified = verify_and_apply_snapshot(b.read().await.state.clone(), &a.payload, &a.entries)
                .expect("verifies");
            install_verified_snapshot(&b, &storage, verified, SNAPSHOT_HEIGHT)
                .await
                .expect("fresh B installs A's snapshot");

            let (b, storage) = restart(storage, &path);
            assert_b_matches_a("restart after ingest", &b, &a, SNAPSHOT_HEIGHT).await;

            // A longer branch forking at H - 5, inside B's carried window, so
            // the fork point is found and only finality can refuse it.
            let below = competing_branch(&a, SNAPSHOT_HEIGHT - 5, SNAPSHOT_HEIGHT + 1);
            let refused = plan(&b, &storage, &below).await;
            assert!(
                refused.is_none(),
                "a restarted snapshot node must refuse a fork below the snapshot height, got {refused:?}"
            );

            // Positive control: the same planner, a branch forking AT H.
            let at_h = competing_branch(&a, SNAPSHOT_HEIGHT, SNAPSHOT_HEIGHT + 2);
            let planned = plan(&b, &storage, &at_h).await.expect("a fork at the snapshot height is plannable");
            assert_eq!(planned.fork_height, SNAPSHOT_HEIGHT);
        }

        /// #3277 S5, core4#3364 review P3-3: a FRESH node that joins by
        /// snapshot never applies block 1, so without the carried-block-1 hook
        /// it keeps its provisional (cut-time) base schedule — no bootstrap, no
        /// schedule-extension authority — and would reject every post-H_ENABLE
        /// extension its peers accept. After `install_verified_snapshot` the
        /// base is re-resolved from the snapshot's pin-verified block 1.
        ///
        /// (This chain's block 1 is not the live one, so the bootstrap and
        /// authority rows do not match here; that the same hook installs them
        /// for the live block 1 is
        /// `schedule_extension_node_paths_3277::the_block1_hook_installs_bootstrap_and_authority_on_a_fresh_node`.)
        /// Own process (re-exec child): the provisional base is process-wide.
        #[tokio::test]
        async fn a_snapshot_joined_fresh_node_resolves_block1_inputs_3277() {
            const CHILD: &str = "WRFCOIN_SNAPSHOT_JOIN_BLOCK1_3277_CHILD";
            if std::env::var_os(CHILD).is_none() {
                let status = std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "p2p_events::tests::snapshot_tip_carry_3288::a_snapshot_joined_fresh_node_resolves_block1_inputs_3277",
                        "--nocapture",
                    ])
                    .env(CHILD, "1")
                    .status()
                    .expect("re-exec child");
                assert!(status.success(), "isolated snapshot-join child failed");
                return;
            }
            use wrfcoin_shared_protocol::crypto::chain_binding::{
                base_feature_schedule_is_provisional, install_uncut_chain_protocol,
                resolve_uncut_chain_protocol_with_schedule, try_feature_schedule, ChainIdentity,
                FeatureSchedule,
            };
            // B boots on an empty volume: its base is provisional.
            let uncut = resolve_uncut_chain_protocol_with_schedule(
                None,
                ChainIdentity::active(),
                None,
            )
            .expect("uncut");
            install_uncut_chain_protocol(uncut);
            assert!(base_feature_schedule_is_provisional(), "fixture: a fresh node");

            let a = mine_source_chain().await;
            let _pin = TestGenesisPin::set(a.blocks[0].hash.expect("hashed").to_hex());
            let dir = tempfile::tempdir().expect("tempdir");
            let storage = open_storage(&dir.path().join("b.redb"));
            let b = Arc::new(RwLock::new(Blockchain::default()));
            let verified = verify_and_apply_snapshot(b.read().await.state.clone(), &a.payload, &a.entries)
                .expect("verifies");
            install_verified_snapshot(&b, &storage, verified, SNAPSHOT_HEIGHT)
                .await
                .expect("fresh B installs A's snapshot");

            assert!(
                !base_feature_schedule_is_provisional(),
                "the snapshot join must re-resolve the base from the carried block 1"
            );
            assert_eq!(
                try_feature_schedule().cloned(),
                Some(FeatureSchedule::from_genesis(&a.blocks[0]).expect("block 1 schedule")),
                "B's base is block 1's own schedule (plus its chain's bootstrap rows, none here)"
            );
        }

        /// #3277 S3 F2 on the 3b path: a tip-carry install makes the imported
        /// `feature_schedule_extensions` the effective-schedule overlay, not the
        /// overlay this node held before the sync.
        ///
        /// `paymaster` is a feature this binary implements, so no clean-halt
        /// deadline is installed, and `u64::MAX - 3288` is an activation no
        /// other test uses. The overlay is process-global, so each attempt
        /// first sets a sentinel and then installs on a FRESH node; the
        /// observation is retried, as in
        /// `snapshot_import_replaces_feature_schedule_extensions_3277`.
        #[tokio::test]
        async fn a_tip_carry_install_makes_the_imported_extensions_the_overlay_3288() {
            use wrfcoin_shared_protocol::crypto::chain_binding::{
                feature_schedule_extension_entries, refresh_feature_schedule_extensions,
            };
            use wrfcoin_shared_protocol::governance::schedule_extension::{
                AppliedExtension, ScheduleEntry,
            };
            const ACTIVATION: u64 = u64::MAX - 3288;
            let mut source_genesis = genesis();
            source_genesis.state.feature_schedule_extensions.push(AppliedExtension {
                sequence: 0,
                carried_at_height: 1,
                entries: vec![ScheduleEntry::new("paymaster", ACTIVATION)],
                version_steps: Vec::new(),
                min_protocol_version: 3,
                release_commit: "tip-carry-3288".into(),
                signers: vec!["ab".repeat(32)],
            });
            source_genesis.state.recalculate_state_root();
            let a = mine_source_chain_from(source_genesis).await;
            let _pin = TestGenesisPin::set(a.blocks[0].hash.expect("hashed").to_hex());
            let imported = vec![("paymaster".to_string(), ACTIVATION)];
            let sentinel = vec![("tip_carry_sentinel_3288".to_string(), ACTIVATION)];

            let before = feature_schedule_extension_entries();
            let mut observed = false;
            for _ in 0..20 {
                let dir = tempfile::tempdir().expect("tempdir");
                let storage = open_storage(&dir.path().join("b.redb"));
                let b = Arc::new(RwLock::new(Blockchain::default()));
                let verified = verify_and_apply_snapshot(b.read().await.state.clone(), &a.payload, &a.entries)
                    .expect("verifies");
                refresh_feature_schedule_extensions(sentinel.clone());
                install_verified_snapshot(&b, &storage, verified, SNAPSHOT_HEIGHT)
                    .await
                    .expect("fresh B installs A's snapshot");
                let held: Vec<(String, u64)> = b
                    .read()
                    .await
                    .state
                    .feature_schedule_extensions
                    .iter()
                    .flat_map(|ext| ext.entries.iter().map(|e| (e.feature.clone(), e.activation_height)))
                    .collect();
                assert_eq!(held, imported, "B holds A's applied extension list");
                if feature_schedule_extension_entries() == imported {
                    observed = true;
                    break;
                }
            }
            refresh_feature_schedule_extensions(before);
            assert!(observed, "the imported extension list must become the effective-schedule overlay");
        }

        /// RED control: the pre-3b ingest installed only state and height.
        /// Such a node cannot take block H + 1: PoP rejects a block whose
        /// prev_block_hash is not the local tip.
        #[tokio::test]
        async fn control_a_state_and_height_only_install_cannot_take_the_next_block_3288() {
            let a = mine_source_chain().await;
            // B's operator pins A's genesis, as WRFCOIN_GENESIS_HASH would.
            let _pin = TestGenesisPin::set(a.blocks[0].hash.expect("hashed").to_hex());
            let dir = tempfile::tempdir().expect("tempdir");
            let storage = open_storage(&dir.path().join("b.redb"));
            let b = Arc::new(RwLock::new(Blockchain::default()));
            let verified = verify_and_apply_snapshot(b.read().await.state.clone(), &a.payload, &a.entries)
                .expect("verifies");
            {
                let mut chain = b.write().await;
                chain.state = verified.state;
                chain.height = SNAPSHOT_HEIGHT;
            }
            let outcome = apply(&b, &storage, &a.blocks[SNAPSHOT_HEIGHT as usize]).await;
            assert_ne!(outcome, P2pBlockApplyOutcome::Applied, "the old install must not follow the chain");
            assert_eq!(b.read().await.height, SNAPSHOT_HEIGHT);
        }

        /// Empty-chain-only, both sides: a node that has taken one block is
        /// refused with the fresh-node message, and nothing changes.
        #[tokio::test]
        async fn a_node_with_history_refuses_the_install_3288() {
            let a = mine_source_chain().await;
            // B's operator pins A's genesis, as WRFCOIN_GENESIS_HASH would.
            let _pin = TestGenesisPin::set(a.blocks[0].hash.expect("hashed").to_hex());
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join("b.redb");
            let storage = open_storage(&path);
            let b = Arc::new(RwLock::new(genesis()));
            // B has already joined the chain the ordinary way: block 1.
            assert_eq!(apply(&b, &storage, &a.blocks[0]).await, P2pBlockApplyOutcome::Applied);
            let verified = verify_and_apply_snapshot(b.read().await.state.clone(), &a.payload, &a.entries)
                .expect("verifies");
            let refused = install_verified_snapshot(&b, &storage, verified, SNAPSHOT_HEIGHT).await;
            assert!(
                matches!(refused, Err(crate::routes::snapshot::InstallRefusal::NotAFreshNode)),
                "got {refused:?}"
            );
            assert_eq!(b.read().await.height, 1, "nothing installed");

            // Disk-only history (memory looks fresh, as after a crash before
            // load): refused by the check inside the install transaction.
            let fresh_memory = Arc::new(RwLock::new(Blockchain::default()));
            let verified = verify_and_apply_snapshot(
                wrfcoin_native_chain::blockchain::state::State::default(),
                &a.payload,
                &a.entries,
            )
            .expect("verifies");
            let refused = install_verified_snapshot(&fresh_memory, &storage, verified, SNAPSHOT_HEIGHT).await;
            assert!(
                matches!(refused, Err(crate::routes::snapshot::InstallRefusal::NotAFreshNode)),
                "got {refused:?}"
            );
            assert_eq!(fresh_memory.read().await.height, 0);
            let _ = SnapshotApplyRefusal::EntriesMismatch; // the refusal type is part of the route API
        }
    }
}

#[cfg(test)]
mod station_registration_admission_tests {
    //! Station registrations are admitted only through the operator route:
    //! the P2P ingress does not take them, whatever the node's role, while the
    //! other transaction types keep arriving by gossip as before.
    use super::{GossipIngest, ingest_gossiped_transaction};
    use wrfcoin_native_chain::blockchain::transaction::{
        StationRegistrationPayload, Transaction, TransactionInput, TransactionOutput,
        TransactionType,
    };
    use wrfcoin_native_chain::config::settings::Config;
    use wrfcoin_native_chain::mempool::pool::Mempool;
    use wrfcoin_native_chain::security::key_registry::SigningKeyRegistry;
    use wrfcoin_shared_protocol::crypto::hash::hash_string;
    use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
    use wrfcoin_shared_protocol::station_registry::{ContributionProvenanceClass, DataQualityTier};

    fn key(seed: u8) -> PrivateKey {
        PrivateKey::from_bytes(&[seed; 32]).expect("32-byte ed25519 seed")
    }

    fn envelope(tag: &str, tx_type: TransactionType, data: String) -> Transaction {
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_string(&format!("gossip-input-{tag}")),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 1,
                address: format!("wrf-{tag}"),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.tx_type = tx_type;
        tx.data = Some(data);
        tx.set_id();
        tx
    }

    /// A fully signed registration (valid on the operator route).
    fn registration_tx(tag: &str) -> Transaction {
        let (owner, gov) = (key(0xA1), key(0x60));
        let mut payload = StationRegistrationPayload::new(
            "ws-gossip-policy",
            PublicKey::from_private_key(&owner).to_address(),
            46.7867,
            -92.1005,
            "test-real-station",
            DataQualityTier::Gold,
            ContributionProvenanceClass::Real,
        );
        payload.sign_ownership(&owner);
        payload.sign(&gov);
        envelope(
            tag,
            TransactionType::StationRegistration,
            serde_json::to_string(&payload).unwrap(),
        )
    }

    /// A transaction type unrelated to stations that the mempool admits as is.
    fn plain_tx(tag: &str) -> Transaction {
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_string(&format!("plain-input-{tag}")),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 1,
                address: format!("wrf-plain-{tag}"),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.set_id();
        tx
    }

    #[test]
    fn station_registrations_are_admitted_only_through_the_operator_route() {
        let keys = SigningKeyRegistry::default();
        let mut mempool = Mempool::new(&Config::default());
        let tx = registration_tx("a");

        // The operator route's mempool call admits it...
        assert!(
            Mempool::new(&Config::default())
                .add_transaction(tx.clone())
                .is_ok(),
            "control: the same registration is valid for the mempool"
        );
        // ...but the gossip ingress does not take it.
        assert_eq!(
            ingest_gossiped_transaction(&mut mempool, tx.clone(), &keys),
            GossipIngest::RefusedByPolicy("station_registration_gossip"),
        );
        assert_eq!(mempool.size(), 0, "a gossiped registration never reaches the mempool");
        assert!(!mempool.contains(&tx.id.unwrap()));
    }

    #[test]
    fn declined_registration_logging_is_rate_limited() {
        use super::should_log_declined_registration as logged;
        // The first ten are all logged.
        assert!((1..=10).all(logged));
        // After that only powers of two: a flood of N declines logs ~log2(N) lines.
        let lines = (1..=1_000_000u64).filter(|n| logged(*n)).count();
        assert!(lines <= 10 + 20, "{lines} log lines for a million declines");
        assert!(!logged(11) && !logged(1000) && !logged(1_000_001));
        assert!(logged(16) && logged(1024) && logged(1 << 20));
    }

    #[test]
    fn other_transaction_types_still_arrive_by_gossip() {
        let keys = SigningKeyRegistry::default();
        let mut mempool = Mempool::new(&Config::default());
        let tx = plain_tx("b");
        assert_eq!(
            ingest_gossiped_transaction(&mut mempool, tx.clone(), &keys),
            GossipIngest::Admitted
        );
        assert!(mempool.contains(&tx.id.unwrap()));
        // A duplicate is still the mempool's own rejection, not a policy refusal.
        assert_eq!(
            ingest_gossiped_transaction(&mut mempool, tx, &keys),
            GossipIngest::Rejected
        );
    }

    /// A `StationRegistration`-typed tx that is neither a registration nor an
    /// owner-ledger kind is not caught by either rule.
    #[test]
    fn only_registrations_and_owner_ledger_txs_are_declined_not_the_whole_transaction_type() {
        let keys = SigningKeyRegistry::default();
        let mut mempool = Mempool::new(&Config::default());
        let other = envelope(
            "c",
            TransactionType::StationRegistration,
            "{\"tx_kind\":\"some_other_kind_v1\"}".to_string(),
        );
        assert_eq!(
            ingest_gossiped_transaction(&mut mempool, other, &keys),
            GossipIngest::Admitted,
            "a StationRegistration-typed tx of an unknown kind is unaffected"
        );
    }

    /// Every owner-ledger kind is declined at gossip, while the mempool call the
    /// HTTP submit route makes still admits the same transaction: owner txs
    /// enter a producer only through the state-checked, deduplicating route.
    #[test]
    fn owner_ledger_transactions_enter_only_through_the_submit_route() {
        use wrfcoin_shared_protocol::blockchain::station_owner::{
            StationOwnerActionPayload, StationOwnerRecoveryCancelPayload,
            StationOwnerRecoveryExecutePayload, StationOwnerRecoveryFilingPayload,
            StationOwnerTransferPayload,
        };
        use wrfcoin_native_chain::blockchain::transaction::StationOwnerActionKind;
        let keys = SigningKeyRegistry::default();
        fn json<T: serde::Serialize>(v: &T) -> String {
            serde_json::to_string(v).expect("serialisable payload")
        }
        let kinds: Vec<(&str, String)> = vec![
            (
                "transfer",
                json(&StationOwnerTransferPayload::new("ws-g", "wrf-a", "aa", "wrf-b", "bb", 0, 10)),
            ),
            (
                "action",
                json(&StationOwnerActionPayload::new(
                    "ws-g",
                    "wrf-a",
                    "aa",
                    StationOwnerActionKind::ReportStolen,
                    0,
                    10,
                )),
            ),
            (
                "file",
                json(&StationOwnerRecoveryFilingPayload::new("ws-g", "wrf-a", "wrf-b", "bb", 0)),
            ),
            (
                "cancel",
                json(&StationOwnerRecoveryCancelPayload::new("ws-g", "wrf-a", "aa", 0)),
            ),
            ("execute", json(&StationOwnerRecoveryExecutePayload::new("ws-g", 0))),
        ];
        for (name, data) in kinds {
            let tx = envelope(name, TransactionType::StationRegistration, data);
            assert!(tx.is_station_owner_ledger_tx(), "{name}");
            // Declined at gossip: never reaches the mempool.
            let mut gossip_pool = Mempool::new(&Config::default());
            assert_eq!(
                ingest_gossiped_transaction(&mut gossip_pool, tx.clone(), &keys),
                GossipIngest::RefusedByPolicy("station_owner_tx_gossip"),
                "{name}"
            );
            assert_eq!(gossip_pool.size(), 0, "{name}: nothing queued from gossip");
            // The route's own mempool call still admits it.
            let mut route_pool = Mempool::new(&Config::default());
            assert!(route_pool.add_transaction(tx).is_ok(), "{name}: the route still admits it");
        }
        // A registration is declined by the OTHER rule, with its own reason, and the
        // two rules do not shadow each other.
        let mut pool = Mempool::new(&Config::default());
        assert_eq!(
            ingest_gossiped_transaction(&mut pool, registration_tx("r"), &keys),
            GossipIngest::RefusedByPolicy("station_registration_gossip")
        );
    }

    /// The decision does not depend on the node's role: the ingress has no
    /// role input, so a producer and a relay behave identically.
    #[test]
    fn the_policy_has_no_role_input_so_producers_and_relays_agree() {
        let keys = SigningKeyRegistry::default();
        let tx = registration_tx("d");
        let verdicts: Vec<_> = (0..2)
            .map(|_| ingest_gossiped_transaction(&mut Mempool::new(&Config::default()), tx.clone(), &keys))
            .collect();
        assert_eq!(verdicts[0], verdicts[1]);
        assert_eq!(verdicts[0], GossipIngest::RefusedByPolicy("station_registration_gossip"));
    }
}
