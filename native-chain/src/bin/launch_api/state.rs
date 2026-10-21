#[path = "snapshot_service.rs"]
pub mod snapshot_service;

use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
#[cfg(feature = "zk")]
use tokio::sync::Semaphore;
use tokio::sync::{Mutex, RwLock, broadcast};
use wrfcoin_consensus_engines::equivocation::EvidenceStore;
use wrfcoin_consensus_engines::{PopConsensusAdapter, TypedConsensusManager};
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::blockchain::state::TrustWeightingPolicy;
use wrfcoin_native_chain::bridge::safety_manager::BridgeSafetyManager;
use wrfcoin_native_chain::mempool::pool::Mempool;
use wrfcoin_native_chain::network_failover::FailoverMonitor;
use wrfcoin_native_chain::security::api_auth::AuthPrincipal;
use wrfcoin_shared_protocol::crypto::signer::BlockSigner;
use wrfcoin_native_chain::storage::Storage;
use wrfcoin_shared_protocol::consensus::finality::FinalitySignalSender;
use wrfcoin_shared_protocol::consensus::partition_policy::PartitionMode;

#[cfg(feature = "p2p")]
use wrfcoin_p2p::P2pClient;
#[cfg(feature = "p2p")]
use crate::da_challenge_producer::DaChallengeRuntime;
use wrfcoin_native_chain::network_failover::NodeCapabilitySnapshot;

#[cfg(feature = "p2p")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P2pConsumerStatus {
    Disabled = 0,
    Starting = 1,
    Running = 2,
    Failed = 3,
    GracefulStop = 4,
}

#[cfg(feature = "p2p")]
impl P2pConsumerStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Failed => "failed",
            Self::GracefulStop => "graceful_stop",
        }
    }
}

/// Liveness latch for the protocol-critical inbound P2P consumer (#3224).
///
/// Tracks the one consumer failure the process can observe: the consumer
/// returning (event channel closed, or the loop ended without a panic). Outside
/// graceful shutdown that makes readiness red until the node is restarted; the
/// receiver is single-owner and cannot be reconstructed without restarting P2P,
/// so there is deliberately no in-process restart.
///
/// A consumer **panic** is not represented here. It is a whole-process fail-stop
/// (`panic = "abort"` in release; an abort guard in unwind builds), recovered by
/// the container restart policy and healthcheck — owner decision 2026-09-18.
#[cfg(feature = "p2p")]
#[derive(Clone, Debug)]
pub struct P2pConsumerHealth {
    status: Arc<AtomicU8>,
    shutting_down: Arc<AtomicBool>,
    detail: Arc<std::sync::Mutex<Option<String>>>,
}

#[cfg(feature = "p2p")]
impl P2pConsumerHealth {
    pub fn new(enabled: bool) -> Self {
        Self {
            status: Arc::new(AtomicU8::new(if enabled {
                P2pConsumerStatus::Starting as u8
            } else {
                P2pConsumerStatus::Disabled as u8
            })),
            shutting_down: Arc::new(AtomicBool::new(false)),
            detail: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub fn status(&self) -> P2pConsumerStatus {
        match self.status.load(Ordering::SeqCst) {
            1 => P2pConsumerStatus::Starting,
            2 => P2pConsumerStatus::Running,
            3 => P2pConsumerStatus::Failed,
            4 => P2pConsumerStatus::GracefulStop,
            _ => P2pConsumerStatus::Disabled,
        }
    }

    /// Internal failure detail. Never served on `/readyz`, which is public.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn detail(&self) -> Option<String> {
        self.detail.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn mark_running(&self) {
        self.status.store(P2pConsumerStatus::Running as u8, Ordering::SeqCst);
    }

    pub fn begin_shutdown(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
    }

    pub fn finish(&self, failure: Option<String>) {
        if self.shutting_down.load(Ordering::SeqCst) {
            self.status.store(P2pConsumerStatus::GracefulStop as u8, Ordering::SeqCst);
            return;
        }
        *self.detail.lock().unwrap_or_else(|e| e.into_inner()) = failure;
        self.status.store(P2pConsumerStatus::Failed as u8, Ordering::SeqCst);
    }

    pub fn blocks_readiness(&self) -> bool {
        matches!(self.status(), P2pConsumerStatus::Starting | P2pConsumerStatus::Failed)
    }
}

#[cfg(feature = "privacy")]
use wrfcoin_privacy::KeyImageRegistry;

#[cfg(feature = "execution")]
use wrfcoin_execution_engine::docker::ContainerRuntime;
#[cfg(feature = "execution")]
use wrfcoin_execution_engine::registry::ModelRegistry;
#[cfg(feature = "execution")]
use wrfcoin_execution_engine::types::ModelRunResult;
#[cfg(feature = "execution")]
use wrfcoin_shared_protocol::blockchain::transaction::{
    ForecastArtifactManifest, ForecastVerificationReceipt,
};

/// How many recent blocks the DA challenge producer samples for content to
/// challenge. Lives here, not in the p2p-gated `da_challenge_producer`,
/// because the snapshot-sync block window (#3288 3b) is derived from it and
/// must be the same on every build.
pub const RECENT_DA_BLOCK_WINDOW: usize = 64;

pub const MAX_JSON_PAYLOAD_BYTES: usize = 64 * 1024;
pub const MAX_PARTICIPANT_ID_LEN: usize = 128;
pub const MAX_STATION_ID_LEN: usize = 128;
pub const MAX_NONCE_LEN: usize = 256;
pub const MAX_NFT_ID_LEN: usize = 128;
pub const MAX_ORACLE_SET_SIZE: usize = 256;
pub const MAX_SNAPSHOT_PAGE_SIZE: usize = 1_000;
pub const MAX_QUALITY_EVENTS_PAGE_SIZE: usize = 500;
pub const MAX_REWARD_IMPACT_PAGE_SIZE: usize = 500;
pub const MAX_LAW_BOOK_PAGE_SIZE: usize = 200;
pub const DEFAULT_LAW_BOOK_PAGE_SIZE: usize = 50;
pub const MAX_CONTRIBUTION_STATUS_PAGE_SIZE: usize = 200;
pub const DEFAULT_CONTRIBUTION_STATUS_PAGE_SIZE: usize = 50;
pub const LEGACY_SUBMIT_RATE_LIMIT_WINDOW_SECS: i64 = 60;
pub const LEGACY_SUBMIT_RATE_LIMIT_MAX_REQUESTS: usize = 2;
pub const LEGACY_SUBMIT_RETRY_AFTER_SECS: u64 = 60;
pub const LEGACY_SUBMIT_RATE_LIMIT_MAX_KEYS: usize = 10_000;
/// #3337: ceiling on AUTHENTICATED legacy transfers per bearer token per
/// window, across all signers. A bearer may be a relay (the backend sends
/// every user's wallet transfer with one contributor token), so each signed
/// `from` gets its own `LEGACY_SUBMIT_RATE_LIMIT_MAX_REQUESTS`; this bounds
/// what one token can admit in total with freshly minted signers. Override
/// with `LEGACY_SUBMIT_TOKEN_RATE_LIMIT_PER_MIN`.
pub const LEGACY_SUBMIT_TOKEN_RATE_LIMIT_MAX_REQUESTS: usize = 60;
pub const PERMISSIONLESS_SUBMIT_WINDOW_SECS: i64 = 60;
pub const PERMISSIONLESS_SUBMIT_MAX_REQUESTS: usize = 6;
pub const PERMISSIONLESS_REPLAY_TTL_SECS: i64 = 10 * 60;
/// Hard cap on tracked signer-fingerprint keys before stale entries are swept,
/// to bound the permissionless-submit throttle map's memory. This is the only
/// fully-anonymous write endpoint (no auth token, no session) — an attacker
/// can rotate fresh keypairs per request to mint a fresh map key each time, so
/// this cap (mirroring `LEGACY_SUBMIT_RATE_LIMIT_MAX_KEYS`) is what actually
/// bounds memory growth under that attack (#2077).
pub const PERMISSIONLESS_SUBMIT_RATE_LIMIT_MAX_KEYS: usize = 10_000;

/// Boot-time P2P posture exposed by `/readyz` even when no client exists.
/// A required failure normally aborts startup; retaining the `failed` state in
/// this value keeps readiness fail-closed in tests and future optional profiles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct P2pStartupStatus {
    pub state: &'static str,
    pub profile: &'static str,
    pub required: bool,
    pub identity: &'static str,
    pub simulated: bool,
    pub detail: Option<String>,
}

impl Default for P2pStartupStatus {
    fn default() -> Self {
        Self {
            state: "disabled",
            profile: "disabled",
            required: false,
            identity: "none",
            simulated: false,
            detail: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Contribution-commit endpoint throttle (#1609)
// ---------------------------------------------------------------------------
//
// The IoT sensor pipeline submits `ContributionCommit` transactions via
// `POST /transactions/contribution/commit`. A misconfigured collector or a
// retry storm can flood this endpoint, growing the mempool unboundedly and
// starving the block-production scheduler while the node still looks healthy
// from the outside (NODE_SYNC_STALE_AFTER_SECS = 600). The global per-IP
// limiter is too coarse here — authenticated collectors get a 10x boost and
// several stations may sit behind one egress IP — so the commit endpoint
// carries its own per-contributor backpressure on top of it.
pub const CONTRIBUTION_COMMIT_RATE_LIMIT_WINDOW_SECS: i64 = 60;
/// Default per-contributor commit budget per window. Tuned well above the
/// legitimate per-station cadence (a station commits on the order of once per
/// few minutes) so honest collectors are never throttled, while a runaway
/// retry loop is capped. Override with `CONTRIBUTION_COMMIT_RATE_LIMIT_PER_MIN`.
pub const CONTRIBUTION_COMMIT_RATE_LIMIT_MAX_REQUESTS: usize = 30;
/// Hard cap on tracked contributor keys before stale entries are swept, to
/// bound the throttle map's memory under a high-cardinality spoof attempt.
pub const CONTRIBUTION_COMMIT_RATE_LIMIT_MAX_KEYS: usize = 10_000;
/// Privacy key registry entries expire after 30 minutes.
#[cfg(feature = "privacy")]
pub const PRIVACY_KEY_TTL_SECS: i64 = 30 * 60;

// ---------------------------------------------------------------------------
// Per-token execution quotas (#1244)
// ---------------------------------------------------------------------------

/// Default sliding-window length (ms) for execution run quotas.
#[cfg(feature = "execution")]
pub const EXECUTION_QUOTA_WINDOW_MS: i64 = 60_000;

/// Default maximum execution runs a single API token may submit per window.
#[cfg(feature = "execution")]
pub const DEFAULT_EXECUTION_RUNS_PER_MIN: usize = 10;

/// Default maximum concurrent in-flight execution runs per API token.
#[cfg(feature = "execution")]
pub const DEFAULT_EXECUTION_MAX_INFLIGHT: usize = 4;

/// Retry-After value advertised when an execution quota is exceeded.
#[cfg(feature = "execution")]
pub const EXECUTION_QUOTA_RETRY_AFTER_SECS: u64 = 60;

/// Hard cap on tracked tokens before stale entries get swept.
#[cfg(feature = "execution")]
pub const EXECUTION_QUOTA_MAX_TRACKED_TOKENS: usize = 10_000;

#[derive(Clone, Debug)]
pub struct SlidingWindowCounter {
    window_ms: i64,
    max_requests: usize,
    timestamps: VecDeque<i64>,
}

impl SlidingWindowCounter {
    pub fn new(window_ms: i64, max_requests: usize) -> Self {
        Self {
            window_ms,
            max_requests,
            timestamps: VecDeque::with_capacity(max_requests),
        }
    }

    pub fn check_and_record(&mut self, now: i64) -> bool {
        self.prune_expired(now);
        if self.timestamps.len() >= self.max_requests {
            return false;
        }
        self.timestamps.push_back(now);
        true
    }

    /// Whether one more request would be admitted at `now`, WITHOUT recording
    /// it. Lets a caller check several buckets and record into all of them
    /// only when every one admits (#3337).
    pub fn has_room(&mut self, now: i64) -> bool {
        self.prune_expired(now);
        self.timestamps.len() < self.max_requests
    }

    pub fn prune_expired(&mut self, now: i64) {
        let cutoff = now - self.window_ms;
        while self
            .timestamps
            .front()
            .map(|timestamp| *timestamp <= cutoff)
            .unwrap_or(false)
        {
            self.timestamps.pop_front();
        }
    }

    pub fn is_empty_after_prune(&mut self, now: i64) -> bool {
        self.prune_expired(now);
        self.timestamps.is_empty()
    }
}

/// Per-token quota bucket for execution run submissions (#1244).
///
/// Tracks two caps that a single API token must stay under:
/// 1. A sliding-window **submission rate** (runs per minute) to prevent a
///    hot-looping caller from burning the whole global execution budget.
/// 2. A **concurrent in-flight run count** so a single token cannot queue
///    up dozens of expensive Docker jobs and starve other callers.
///
/// The bucket intentionally lives in a tokio `RwLock`-keyed map rather than
/// sharing the `IpBucket` from `rate_limit.rs` because:
///   - we need the concurrency count (not just a timestamp queue)
///   - the lifecycle is tied to async background task completion
///   - the key is a bearer token id, not an IP
#[cfg(feature = "execution")]
#[derive(Debug)]
pub struct ExecutionQuotaBucket {
    /// Recent submission timestamps (ms epoch) within the rolling window.
    submit_timestamps: VecDeque<i64>,
    /// Number of background runs currently in flight for this token.
    in_flight: usize,
}

#[cfg(feature = "execution")]
impl ExecutionQuotaBucket {
    pub fn new() -> Self {
        Self {
            submit_timestamps: VecDeque::new(),
            in_flight: 0,
        }
    }

    fn prune(&mut self, now: i64, window_ms: i64) {
        let cutoff = now - window_ms;
        while self
            .submit_timestamps
            .front()
            .map(|ts| *ts <= cutoff)
            .unwrap_or(false)
        {
            self.submit_timestamps.pop_front();
        }
    }

    /// Attempt to admit a new execution run for this token.
    ///
    /// Returns `Ok(())` if admission is allowed (and records the submission
    /// + increments in-flight), or `Err(reason)` describing which cap was
    /// exceeded. The caller is expected to decrement in-flight via
    /// [`Self::release`] when the background job finishes.
    pub fn try_admit(
        &mut self,
        now: i64,
        window_ms: i64,
        max_runs_per_window: usize,
        max_inflight: usize,
    ) -> Result<(), ExecutionQuotaDenied> {
        self.prune(now, window_ms);
        if self.submit_timestamps.len() >= max_runs_per_window {
            return Err(ExecutionQuotaDenied::RateExceeded);
        }
        if self.in_flight >= max_inflight {
            return Err(ExecutionQuotaDenied::ConcurrencyExceeded);
        }
        self.submit_timestamps.push_back(now);
        self.in_flight += 1;
        Ok(())
    }

    /// Decrement the in-flight counter when a background job completes.
    pub fn release(&mut self) {
        self.in_flight = self.in_flight.saturating_sub(1);
    }

    /// Returns `true` if the bucket carries no live state after pruning and
    /// has no in-flight runs — used to evict idle tokens from the map.
    pub fn is_idle_after_prune(&mut self, now: i64, window_ms: i64) -> bool {
        self.prune(now, window_ms);
        self.submit_timestamps.is_empty() && self.in_flight == 0
    }
}

/// Reason an execution run was denied by per-token quota enforcement.
#[cfg(feature = "execution")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionQuotaDenied {
    /// Token exceeded its sliding-window submission rate.
    RateExceeded,
    /// Token has too many concurrent in-flight runs.
    ConcurrencyExceeded,
}

#[cfg(feature = "execution")]
impl ExecutionQuotaDenied {
    pub fn code(self) -> &'static str {
        match self {
            Self::RateExceeded => "EXECUTION_QUOTA_RATE",
            Self::ConcurrencyExceeded => "EXECUTION_QUOTA_CONCURRENCY",
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::RateExceeded => "Per-token execution run rate exceeded. Please slow down.",
            Self::ConcurrencyExceeded => {
                "Per-token concurrent execution run cap reached. Wait for in-flight runs to finish."
            },
        }
    }
}

/// Configured per-token execution quota values, sourced once at startup.
#[cfg(feature = "execution")]
#[derive(Debug, Clone, Copy)]
pub struct ExecutionQuotaConfig {
    pub window_ms: i64,
    pub max_runs_per_window: usize,
    pub max_inflight: usize,
}

#[cfg(feature = "execution")]
impl ExecutionQuotaConfig {
    pub fn from_env() -> Self {
        let max_runs_per_window = std::env::var("EXECUTION_MAX_RUNS_PER_MIN")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .filter(|&v| v > 0)
            .unwrap_or(DEFAULT_EXECUTION_RUNS_PER_MIN);
        let max_inflight = std::env::var("EXECUTION_MAX_INFLIGHT_PER_TOKEN")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .filter(|&v| v > 0)
            .unwrap_or(DEFAULT_EXECUTION_MAX_INFLIGHT);
        Self {
            window_ms: EXECUTION_QUOTA_WINDOW_MS,
            max_runs_per_window,
            max_inflight,
        }
    }
}

#[cfg(feature = "execution")]
impl Default for ExecutionQuotaConfig {
    fn default() -> Self {
        Self {
            window_ms: EXECUTION_QUOTA_WINDOW_MS,
            max_runs_per_window: DEFAULT_EXECUTION_RUNS_PER_MIN,
            max_inflight: DEFAULT_EXECUTION_MAX_INFLIGHT,
        }
    }
}

/// Snapshot of stake aggregate metrics, cached at a specific chain height (#1385).
/// Avoids re-iterating all accounts on every `/stake/metrics` scrape.
/// When the current blockchain height matches `computed_at_height`, the cached
/// values are returned directly; otherwise the aggregation reruns.
#[derive(Debug, Clone)]
pub struct StakeMetricsSnapshot {
    pub computed_at_height: u64,
    pub account_count: usize,
    pub total_stake_required: u128,
    pub total_stake_locked: u128,
    pub total_spendable_balance: u128,
    pub total_earned_balance: u128,
    pub network_stake_maturity_percent: f64,
}

/// Per-block trust/quality data cached for `/chain/metrics` (#1392, #1476).
/// JSON metadata is parsed once per block and stored here; subsequent scrapes
/// read the struct fields directly without re-parsing. Keyed by block height
/// in `AppState::trust_reward_metrics_cache`.
#[derive(Debug, Clone)]
pub struct TrustRewardCacheEntry {
    pub trust_unverified: u64,
    pub trust_verified_station: u64,
    pub trust_trusted_provider: u64,
    pub quality_high: u64,
    pub quality_borderline: u64,
    pub quality_warning_only: u64,
    pub quality_low: u64,
    /// Per-block trust weight override; `None` when metadata is absent or fails validation.
    pub trust_weights: Option<TrustWeightingPolicy>,
    /// `[high, borderline, warning_only, low]`; `None` when metadata is absent.
    pub quality_multipliers: Option<[f64; 4]>,
}

/// One-way, at-most-once-written cell for the node's genesis identity
/// (#2762 residual; see the `genesis_hash` field doc on [`AppState`]).
///
/// Concurrency contract, stated for the request path: `get` is
/// `OnceLock::get` — an atomic acquire-load, lock-free, uncontended however
/// many Actix workers poll `/network/info` concurrently. `promote` is
/// `OnceLock::set`: the first writer wins and every later attempt is a no-op
/// returning `false`, which makes the cache one-way — once a process has
/// stated its chain identity it can never restate it. Writers are exactly
/// two, both anchor-validated upstream (`height == 1` + zero previous hash):
/// boot's `resolve_genesis_hash` via [`GenesisHashCell::new`], and the first
/// successful fallback resolution in `get_network_info`. Nothing else may
/// write this cell — a promotable cache that accepts unvalidated writers is a
/// poisonable one.
#[derive(Debug)]
pub struct GenesisHashCell(OnceLock<String>);

impl GenesisHashCell {
    /// Cell seeded from the boot-time resolution (`None` on an
    /// empty/never-initialized chain — the promotable case).
    pub fn new(initial: Option<String>) -> Self {
        let cell = OnceLock::new();
        if let Some(hash) = initial {
            let _ = cell.set(hash);
        }
        Self(cell)
    }

    /// Lock-free read (atomic acquire-load; no lock on the request path).
    pub fn get(&self) -> Option<String> {
        self.0.get().cloned()
    }

    /// One-way promotion: returns `true` only for the write that won.
    /// Callers must pass a value that already passed the height-1 anchor
    /// check (both current callers derive it from `resolve_genesis_hash` /
    /// `resolve_genesis_from_archive`, which enforce it).
    pub fn promote(&self, hash: String) -> bool {
        self.0.set(hash).is_ok()
    }
}

/// Immutable result of the boot-time check for the producer of the next block.
///
/// A follower is allowed to start without a producer signer, but its inability
/// to produce is still surfaced here so `/readyz` cannot present an unexplained
/// clean bill of health. A producing node with a non-member signer is refused
/// before this status is installed in `AppState` when producer auth is required.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProducerAuthorityBootStatus {
    pub status: String,
    pub next_height: u64,
    pub node_key: Option<String>,
    pub authorized_set: Vec<String>,
    pub message: String,
    pub readiness_gate: bool,
}

impl Default for ProducerAuthorityBootStatus {
    fn default() -> Self {
        Self {
            status: "not_checked".to_string(),
            next_height: 0,
            node_key: None,
            authorized_set: Vec::new(),
            message: "next-block producer authority was not checked".to_string(),
            readiness_gate: false,
        }
    }
}

// Test-only, like AppState's Clone below: struct-update in the test harness
// duplicates the cell's current value into an independent cell.
#[cfg(test)]
impl Clone for GenesisHashCell {
    fn clone(&self) -> Self {
        Self::new(self.get())
    }
}

// Production never clones AppState: Actix workers share it through the Arc
// inside `web::Data<AppState>`, whose Clone does not require `AppState: Clone`
// (verified by removing the derive — both feature sets compile). Only the
// launch_api test harness clones it, to build variant states by struct-update.
// Keeping the derive test-only retires the derived-exposure surface outright
// instead of justifying it.
#[cfg_attr(test, derive(Clone))]
pub struct AppState {
    pub consensus: Arc<TypedConsensusManager<PopConsensusAdapter>>,
    pub blockchain: Arc<RwLock<Blockchain>>,
    /// Node-scoped immutable export cut and bounded service evidence ledger (#3129).
    pub snapshot_service: Arc<snapshot_service::SnapshotServiceState>,
    /// Gates the node-local NFT *mutation* API (`/nft/mint`, `/nft/{id}/verify`,
    /// `/nft/{id}/transfer`). Defaults to `false` (#1764).
    ///
    /// NFT state is hashed into the consensus `state_root`
    /// (`state.rs::state_root_material`), but NFT mutations are applied **only**
    /// via these local, non-gossiped API calls and are explicitly dropped from
    /// block extraction (`transition_extractor.rs`). They also stamp
    /// wall-clock `updated_at` timestamps. The result is that any NFT mutation
    /// makes this node's `state_root` diverge from every other honest node that
    /// never saw the call, tripping the P2P divergence latch and wedging sync.
    ///
    /// On an untrusted multi-validator testnet this is a guaranteed silent fork
    /// on the first NFT op, so the mutation endpoints are **disabled by default**
    /// and return `503 Service Unavailable`. Read endpoints (`GET /nft/{id}`,
    /// `GET /nft/list`) are unaffected. Set
    /// `WRFCOIN_ENABLE_NFT_MUTATION_API=1` to re-enable them (single-node /
    /// trusted-operator deployments only). When enabled the underlying state-root
    /// divergence is NOT fixed — it is merely operator-acknowledged.
    pub nft_mutation_api_enabled: bool,
    /// Whether the node-local oracle/signing-key admin endpoints
    /// (`POST /oracle/register`, `POST /oracle/config`, signing-key
    /// register/revoke) are enabled. Disabled by default (#1775): `oracle_set` /
    /// `oracle_quorum` and Oracle-role `signing_keys` are folded into the
    /// consensus `state_root`, but these routes mutate them OUT OF BAND (no
    /// gossiped tx, never reconstructed by `extract_transitions`), so any call
    /// forks this node's `state_root` from its peers — same divergence class as
    /// the NFT mutation API (#1764). Disabled routes return `503 Service
    /// Unavailable`; read endpoints are unaffected. Multi-node oracle/signing-key
    /// config must be seeded deterministically at genesis. Set
    /// `WRFCOIN_ENABLE_ORACLE_ADMIN_API=1` to re-enable on single-node /
    /// trusted-operator deployments only.
    pub oracle_admin_api_enabled: bool,
    /// Whether the node-local debug seeding endpoint (`POST /debug/seed-account`)
    /// is enabled. Disabled by default (#1811): `seed_account` mutates `accounts`
    /// (via `ApplyReward`) and `stake_locked` / `spendable_balance` directly —
    /// all hashed into the consensus `state_root` — OUT OF BAND (no gossiped tx,
    /// never reconstructed by `extract_transitions`), so any call forks this
    /// node's `state_root` from its peers. Same divergence class as the NFT
    /// mutation API (#1764) and oracle/signing-key admin API (#1775). Disabled
    /// returns `503 Service Unavailable`. Set `WRFCOIN_ENABLE_DEBUG_SEED_API=1`
    /// to re-enable on single-node / trusted-operator deployments only.
    // Read only by the dev-endpoints seed route (#2139); the field itself stays so
    // the boot path is untouched.
    #[cfg_attr(not(any(test, feature = "dev-endpoints")), allow(dead_code))]
    pub debug_seed_api_enabled: bool,
    /// Genesis block hash, resolved once at boot (#1714) and promotable
    /// exactly once afterwards (#2762 residual).
    ///
    /// This chain's genesis is the **height-1** block (anchored on
    /// `height == 1 && prev_block_hash == Hash::default()`); there is no
    /// height-0 block. Format: `0x` + lowercase hex. Unset only when no block
    /// exists at startup (empty/never-initialized chain). `/network/info`
    /// reads this for the stable `genesis_hash` chain-identity field so it
    /// stays reportable even after the genesis block is evicted from the
    /// in-memory block window on a long-running node. When boot could not
    /// resolve it (fresh-P2P-synced joiner, #2797), the first successful
    /// anchor-checked fallback resolution in `get_network_info` promotes the
    /// value here so later polls stop reading the durable archive.
    pub genesis_hash: GenesisHashCell,
    /// Boot-time next-block producer-authority result. Immutable after startup;
    /// `/readyz` reads it to expose follower posture or a non-member producer
    /// signer instead of reporting a misleading healthy node.
    pub producer_authority_boot: Arc<ProducerAuthorityBootStatus>,
    /// Exact startup result for the networking layer. Unlike `p2p: Option<_>`,
    /// this distinguishes intentional disablement from failed initialization.
    pub p2p_startup: Arc<P2pStartupStatus>,
    pub mempool: Arc<RwLock<Mempool>>,
    pub snapshot_sync_seen: Arc<RwLock<FxHashSet<String>>>,
    /// Block signer used to sign snapshot-sync payloads. In debug builds
    /// this may wrap an env-key loaded from `WRFCOIN_SNAPSHOT_SIGNER_PRIVATE_KEY`;
    /// in release builds it MUST be a `Pkcs11Signer`. See core4#1394 and
    /// `docs/operator/hsm-migration.md`.
    pub snapshot_signing_key: Option<Arc<dyn BlockSigner>>,
    pub snapshot_trusted_signers: Arc<FxHashSet<String>>,
    pub bridge_safety: Arc<RwLock<BridgeSafetyManager>>,
    pub token_store: Arc<FxHashMap<String, AuthPrincipal>>,
    pub legacy_submit_rate_limit: Arc<RwLock<FxHashMap<String, SlidingWindowCounter>>>,
    pub permissionless_submit_rate_limit: Arc<RwLock<FxHashMap<String, SlidingWindowCounter>>>,
    /// Per-contributor sliding-window throttle for the contribution-commit
    /// endpoint (#1609). Keyed by contributor id so a single misconfigured
    /// collector cannot flood `POST /transactions/contribution/commit` and
    /// stall block production, while independent contributors keep their own
    /// budgets.
    pub contribution_commit_rate_limit: Arc<RwLock<FxHashMap<String, SlidingWindowCounter>>>,
    pub permissionless_replay_seen: Arc<RwLock<FxHashMap<String, i64>>>,
    pub storage: Arc<RwLock<Storage>>,
    /// Durable equivocation evidence store (#1218).
    ///
    /// Opened at startup from `<data_dir>/equivocation_evidence.redb`. Held
    /// as an `Arc<dyn EvidenceStore>` so that any future consensus engine
    /// constructed by this process (notably Proof-of-Stake via
    /// `ConsensusFactory::create_pos_with_evidence_store`) can attach to
    /// the same backing store. The current active engine is
    /// Proof-of-Publishing, which does not produce equivocation evidence
    /// itself, but the store is online and ready so that when PoS is
    /// enabled the durable slashing-evidence path is already wired.
    ///
    /// `#[allow(dead_code)]` because the read path (PoS engine attachment
    /// + operator audit export) lands in a follow-up PR; the infra must
    /// be online first so double-sign proofs survive restart the moment
    /// PoS is enabled.
    #[allow(dead_code)]
    pub evidence_store: Arc<dyn EvidenceStore>,
    /// Counts transactions that failed validation or were rejected at submission.
    pub failed_transactions_total: Arc<AtomicU64>,
    /// Per-block ZK-proof count memoization for the Prometheus
    /// `zk_proofs_observed_total` metric (#1377). Maps block height to the
    /// proof count for that block. Entries are inserted on first scrape that
    /// observes a height, and pruned to the current in-memory window on each
    /// subsequent scrape so the cache cannot grow beyond
    /// `Blockchain::blocks()`. Held under `std::sync::Mutex` because the
    /// hold is brief (memoize + sum) with no `.await` between lock and drop.
    pub zk_proofs_per_block_cache: Arc<std::sync::Mutex<FxHashMap<u64, u64>>>,
    /// Per-block trust-reward aggregation cache for `/chain/metrics` (#1392, #1476).
    /// Maps block height → parsed trust/quality metadata. Populated lazily on first
    /// scrape, pruned to the in-memory window on each call. Same pattern as
    /// `zk_proofs_per_block_cache` from #1469.
    pub trust_reward_metrics_cache: Arc<std::sync::Mutex<FxHashMap<u64, TrustRewardCacheEntry>>>,
    /// Per-block tx-count memoization for `/mining/stats` (#1385).
    /// Maps block height → transaction count. Same lazy-populate + prune pattern
    /// as `zk_proofs_per_block_cache`.
    pub mining_tx_count_cache: Arc<std::sync::Mutex<FxHashMap<u64, usize>>>,
    /// Stake aggregate snapshot for `/stake/metrics` (#1385).
    /// Invalidated when `blockchain.height` advances; avoids re-iterating all
    /// accounts on every scrape.
    pub stake_metrics_cache: Arc<std::sync::Mutex<Option<StakeMetricsSnapshot>>>,
    /// Serializes concurrent block production requests so only one mine
    /// operation builds a block at a time, preventing wasted work (#974).
    pub block_production_lock: Arc<Mutex<()>>,
    /// Current network-partition operating mode (#1217).
    ///
    /// Reflects the partition state as last reported by the partition
    /// orchestrator (detection lives in
    /// `wrfcoin_consensus_engines::coordination::adaptive_fault_tolerance`).
    /// HTTP handlers consult this to refuse block production and other
    /// writes when the local node sits in a minority partition and
    /// would produce state that must later be rolled back.
    ///
    /// Mutations MUST go through [`AppState::set_partition_mode`] so the
    /// change propagates to the mempool in lockstep.
    pub partition_mode: Arc<RwLock<PartitionMode>>,
    /// Runtime snapshot of contributor hardware/network capability telemetry.
    /// Written by the WAN failover monitor; read by `GET /node/capability`.
    pub node_capability: Arc<RwLock<NodeCapabilitySnapshot>>,
    /// Set to `true` when a peer reports the same height but a different state
    /// root.  While this flag is set the node refuses to accept new P2P blocks
    /// and the `/readyz` endpoint reports `not_ready` (#1030).
    /// Currently set during construction; read path wired by P2P block handler (future).
    #[allow(dead_code)]
    pub state_diverged: Arc<AtomicBool>,
    /// Consecutive local-acceptance transactions whose P2P gossip command
    /// could not be queued. The mutex orders concurrent enqueue outcomes and
    /// readiness-driven channel recovery as one state transition (#3225).
    #[cfg(feature = "p2p")]
    pub tx_gossip_backpressure_streak: Arc<std::sync::Mutex<u64>>,
    /// Broadcast sender for WebSocket events (issue #1081).
    /// Route handlers send events here; connected WS clients receive them
    /// filtered by their active subscriptions.
    pub event_tx: broadcast::Sender<crate::websocket::WsEvent>,
    /// Sender half of the BFT finality signal channel (#1216).
    ///
    /// Cloned and handed to any BFT consensus source (HotStuff commit
    /// path, `BftFinalityGadget`, or a future gadget) so that the
    /// `run_finality_event_loop` task can forward finalized heights
    /// into `Blockchain::set_finalized_height`. That call is what arms
    /// the reorg rejection guard inside `plan_reorg`. Without at least
    /// one BFT source emitting on this channel the guard stays inert
    /// and deep reorgs are only bounded by `MAX_REORG_DEPTH`.
    ///
    /// The consumer task is spawned during startup in `main.rs`; a
    /// single consumer multiplexes signals from every attached source.
    ///
    /// Currently unread because the production consensus engine is PoP
    /// and no runtime path is wired to emit yet — the channel exists
    /// specifically so a BFT source can pick it up from `AppState`
    /// without further plumbing when HotStuff or a standalone BFT
    /// gadget is activated.
    #[allow(dead_code)]
    pub finality_signal_tx: FinalitySignalSender,
    #[cfg(feature = "p2p")]
    pub p2p: Option<P2pClient>,
    /// Supervised lifecycle of the inbound P2P protocol event consumer.
    #[cfg(feature = "p2p")]
    pub p2p_consumer_health: P2pConsumerHealth,
    /// Node-local ed25519 key used to sign `WeatherDataV2` gossip before
    /// re-broadcast (#1969). Peers verify this signature and reject any V2
    /// payload that is unsigned or signed by an unknown/garbage key, closing
    /// the prior hole where any peer could inject readings under any
    /// `station_id`. Generated once at startup (see `main.rs`).
    #[cfg(feature = "p2p")]
    pub weather_gossip_signing_key:
        Arc<wrfcoin_shared_protocol::crypto::signature::PrivateKey>,
    #[cfg(feature = "p2p")]
    pub da_challenge_runtime: DaChallengeRuntime,
    #[cfg(feature = "privacy")]
    pub key_image_registry: Arc<KeyImageRegistry>,
    /// Server-side ephemeral key registry for privacy operations (issue #453).
    /// Maps key_id → (expiry_unix_timestamp, private_key_bytes).
    #[cfg(feature = "privacy")]
    pub privacy_key_registry: Arc<RwLock<FxHashMap<String, (i64, [u8; 32])>>>,
    /// Shared ZK verifier for use when constructing PoP instances (issue #527/#530).
    #[cfg(feature = "zk")]
    pub zk_verifier: Arc<dyn wrfcoin_shared_protocol::consensus::zk_verification::ZkVerifier>,
    /// Shared concurrency budget for expensive Groth16 proof generation.
    #[cfg(feature = "zk")]
    pub zk_prover_gate: Arc<Semaphore>,
    /// Pluggable model registry for weather model execution (issues #517, #528).
    #[cfg(feature = "execution")]
    pub execution_registry: Arc<ModelRegistry>,
    /// Docker container runtime for model execution.
    #[cfg(feature = "execution")]
    pub execution_runtime: Arc<dyn ContainerRuntime>,
    /// Background execution results keyed by run_id.
    #[cfg(feature = "execution")]
    pub execution_results: Arc<RwLock<FxHashMap<String, ModelRunResult>>>,
    /// Per-token quota buckets for the execution run endpoint (#1244).
    ///
    /// Keyed by bearer token id (the same string that appears in
    /// `token_store`). Enforces a sliding-window submission rate and a
    /// concurrent in-flight cap so a single authenticated caller cannot
    /// burn all Docker execution capacity.
    #[cfg(feature = "execution")]
    pub execution_token_quotas: Arc<RwLock<FxHashMap<String, ExecutionQuotaBucket>>>,
    /// Configured quota limits (read once at startup from env).
    #[cfg(feature = "execution")]
    pub execution_quota_config: ExecutionQuotaConfig,
    /// In-memory forecast task assignment ledger (`task_id -> manifest`) for
    /// scheduler consumption by node.
    #[cfg(feature = "execution")]
    pub forecast_task_assignments: Arc<RwLock<FxHashMap<String, ForecastArtifactManifest>>>,
    /// Latest heartbeat timestamps by node id.
    #[cfg(feature = "execution")]
    pub forecast_node_heartbeats: Arc<RwLock<FxHashMap<String, i64>>>,
    /// Most recent forecast verification receipts by task id.
    #[cfg(feature = "execution")]
    pub forecast_task_results: Arc<RwLock<FxHashMap<String, ForecastVerificationReceipt>>>,
    /// WAN failover monitor (#1798): operational state machine tracking the
    /// primary↔LTE/5G fallback. A background probe task feeds it connectivity
    /// results; `/node/network/failover` and `readyz` read its state. This is
    /// node-local operational state — it is NEVER hashed into the consensus
    /// `state_root` (it lives on the HTTP `AppState`, not `blockchain.state`).
    pub failover_monitor: Arc<RwLock<FailoverMonitor>>,
}

#[cfg(test)]
mod tests {
    use super::SlidingWindowCounter;

    #[test]
    fn rate_limit_allows_within_window() {
        let mut counter = SlidingWindowCounter::new(60_000, 3);
        assert!(counter.check_and_record(1_000));
        assert!(counter.check_and_record(2_000));
        assert!(counter.check_and_record(3_000));
    }

    #[test]
    fn rate_limit_blocks_over_limit() {
        let mut counter = SlidingWindowCounter::new(60_000, 2);
        assert!(counter.check_and_record(1_000));
        assert!(counter.check_and_record(2_000));
        assert!(!counter.check_and_record(3_000));
    }

    #[test]
    fn rate_limit_resets_after_window() {
        let mut counter = SlidingWindowCounter::new(1_000, 1);
        assert!(counter.check_and_record(10_000));
        assert!(!counter.check_and_record(10_100));
        assert!(counter.check_and_record(11_001));
    }
}
