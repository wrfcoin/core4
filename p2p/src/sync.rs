//! Initial block download (IBD) protocol and block-range request/response.

use std::{
    collections::{BTreeMap, VecDeque},
    io,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use chrono::Utc;
use libp2p::{
    PeerId, StreamProtocol,
    futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    request_response::{self, OutboundRequestId},
};
use log::{debug, warn};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use wrfcoin_shared_protocol::blockchain::{block::Block, chain::Blockchain};
use wrfcoin_shared_protocol::consensus::proof_of_publishing::max_future_drift;
use wrfcoin_shared_protocol::crypto::hash::Hash;

use crate::scoring::{SyncPeerScore, TransportClass};

/// Protocol name used for block-range sync requests.
///
/// The trailing version is the wire-format version and is test-bound to
/// [`crate::message::WIRE_FORMAT_VERSION`] — see its doc for the bump policy
/// (#2884). Bumping the constant without renaming this string is a RED test;
/// renaming it makes a version-skewed peer's requests fail at libp2p
/// negotiation instead of connecting and then failing every
/// `BlockRequest`/`BlockResponse` decode. Note that negotiation failure
/// surfaces as a generic `OutboundFailure::UnsupportedProtocols` that names
/// neither protocol and leaves the connection open — the version attribution
/// comes from [`crate::message::detect_block_sync_version_skew`], run on
/// every identify exchange.
pub const BLOCK_SYNC_PROTOCOL: &str = "/wrfcoin/block-sync/1.0.0";

/// Operator-facing attribution for a block-sync
/// `OutboundFailure::UnsupportedProtocols`: libp2p's rendering of that error
/// ("The remote supports none of the requested protocols") names neither
/// side's protocol, so this names the local one and points at the
/// identify-time skew warning that names the remote's (#2884). A pure
/// function so the surfaced text is unit-testable.
pub fn block_sync_negotiation_refused_report(peer: &impl std::fmt::Display) -> String {
    format!(
        "block-sync request to peer {peer} refused at protocol negotiation: the peer supports \
         none of this node's request protocols (this node speaks {BLOCK_SYNC_PROTOCOL}). The \
         connection stays OPEN. If the peer is a wrfcoin node this is wire-format version skew \
         (#2884); the identify-time skew warning for this peer names the generation it advertises"
    )
}

/// Maximum number of blocks allowed in a single request.
pub const MAX_BLOCKS_PER_REQUEST: u32 = 100;
/// Maximum number of block-sync ranges queued at once.
const MAX_PENDING_SYNC_RANGES: usize = 128;
/// Maximum height window planned from untrusted peer advertisements at once.
pub const MAX_SYNC_TARGET_ADVANCE_BLOCKS: u64 =
    MAX_BLOCKS_PER_REQUEST as u64 * MAX_PENDING_SYNC_RANGES as u64;

const MAX_PARALLEL_PEERS: usize = 4;
pub const BLOCK_SYNC_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const SYNC_RETRY_BASE_DELAY: Duration = Duration::from_secs(10);
const SYNC_RETRY_MAX_DELAY: Duration = Duration::from_secs(60);
const SYNC_RETRY_MAX_JITTER: Duration = Duration::from_millis(250);
/// #2807: cadence floor for retrying a banned peer when it is the only
/// candidate left for a needed range. Starving with pending work and
/// connected peers is never better than a bounded retry.
const LAST_RESORT_RETRY_INTERVAL: Duration = Duration::from_secs(60);
const RETAINED_SYNC_PEER_HEALTH_TTL: Duration = Duration::from_secs(60 * 60);
const MAX_RETAINED_SYNC_PEER_HEALTH: usize = 1_024;
/// While latched on a missing genesis parent (#2036), how long to wait between
/// probe requests for the missing height. Bounds probe traffic while still
/// letting a cold-start node autonomously recover instead of stalling forever.
const MISSING_PARENT_RETRY_INTERVAL: Duration = Duration::from_secs(5);
const ORPHAN_TTL: Duration = Duration::from_secs(300);
const MAX_ORPHANS: usize = 1000;
/// Maximum size of a single block-sync message in bytes (16 MiB).
///
/// Prevents a malicious peer from triggering multi-GiB heap allocations via a
/// forged length prefix on the raw TCP stream (which bypasses gossipsub's
/// `max_transmit_size`).
const MAX_SYNC_MESSAGE_BYTES: usize = 16_777_216;

/// Amplification ceiling for served block-sync responses (#2119).
///
/// # The defect this bounds
///
/// [`BlockRequest`] is `{ start_height: u64, count: u32 }` — **12 bytes**. The only
/// response ceiling is [`MAX_SYNC_MESSAGE_BYTES`] (16 MiB), and that ceiling is
/// **independent of `count`**: a request for ONE block is bounded exactly the same as a
/// request for a hundred. Worst-case amplification is therefore
/// `16_777_216 / 12 ≈ 1_398_101×`, and nothing relates the work served to the work asked
/// for.
///
/// # Why not "a fixed multiple of the request size"
///
/// #2119 proposes bounding the response as a multiple of the request size. That does not
/// work here: the request is a fixed 12-byte struct, so the multiple is dominated by a
/// constant. Any value large enough to serve a legitimate 100-block range is also large
/// enough to serve 16 MiB for a 1-block request — the exact case that needs bounding.
///
/// The meaningful unit is **bytes per requested block**: is the response proportionate to
/// what the peer actually asked for? That is what this enforces.
///
/// # Default headroom (the #2112 red-team constraint)
///
/// `max_bytes_per_requested_block` defaults to 1 MiB. On the ~12-node testnet a block is
/// orders of magnitude smaller than that, so:
///
/// * a normal 100-block IBD request allows `100 MiB`, which the absolute
///   [`MAX_SYNC_MESSAGE_BYTES`] cap already reduces to 16 MiB — **behaviour is unchanged
///   for every honest sync path**;
/// * a 1-block request is capped at 1 MiB instead of 16 MiB — a 16× tightening that binds
///   *only* when a response is wildly disproportionate to the ask.
///
/// The ceiling therefore cannot cause the self-inflicted liveness incident #2112's
/// red-team pass warns about: it does not bind on any request shape an honest node makes.
///
/// # Observe-only by default
///
/// `enforce` defaults to **false**: a violation is reported, not refused. A ceiling on a
/// live network should be measured before it is enforced — the observation tells us
/// whether the default is right, and turning it on is then a config change with evidence
/// behind it rather than a guess. Enforcement is a follow-up, deliberately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncServeLimits {
    /// Response bytes permitted per block the peer actually requested.
    pub max_bytes_per_requested_block: usize,
    /// Absolute ceiling regardless of `count`.
    pub max_bytes_per_response: usize,
    /// When false, a violation is reported but the response is still served.
    pub enforce: bool,
}

impl Default for SyncServeLimits {
    fn default() -> Self {
        Self {
            max_bytes_per_requested_block: 1_048_576,
            max_bytes_per_response: MAX_SYNC_MESSAGE_BYTES,
            enforce: false,
        }
    }
}

/// Outcome of the amplification check for one served response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServeVerdict {
    /// Within both ceilings.
    Within,
    /// Over a ceiling. `budget` is the byte allowance the request earned.
    Exceeds { estimated: usize, budget: usize },
}

impl ServeVerdict {
    /// Whether the response must be refused, given the configured limits.
    ///
    /// Deliberately separate from the verdict itself: the verdict records what was
    /// observed, this decides what to do about it. Observe-only mode reports the same
    /// verdict and refuses nothing.
    pub fn should_refuse(self, limits: &SyncServeLimits) -> bool {
        limits.enforce && matches!(self, ServeVerdict::Exceeds { .. })
    }
}

/// Is `estimated_bytes` proportionate to a request for `count` blocks?
///
/// Pure and total, so the policy is testable without a network, a peer, or a chain.
/// `count` is the count the peer asked for, before any server-side clamp — clamping first
/// would measure our own limit rather than their ask.
pub fn serve_verdict(count: u32, estimated_bytes: usize, limits: &SyncServeLimits) -> ServeVerdict {
    let per_block_budget = limits
        .max_bytes_per_requested_block
        .saturating_mul(count.max(1) as usize);
    let budget = per_block_budget.min(limits.max_bytes_per_response);
    if estimated_bytes > budget {
        ServeVerdict::Exceeds {
            estimated: estimated_bytes,
            budget,
        }
    } else {
        ServeVerdict::Within
    }
}

/// Request a contiguous range of blocks, starting at `start_height`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockRequest {
    pub start_height: u64,
    pub count: u32,
}

/// Response containing zero or more blocks for a requested range.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockResponse {
    pub blocks: Vec<Block>,
    /// Server-side refusal backoff, distinguishing "I am refusing you, retry
    /// after this long" from "the requested range contains no blocks".
    ///
    /// # Wire compatibility: this is a BREAKING change, not an additive field
    ///
    /// [`BlockSyncCodec`] serializes with **bincode**, which is not
    /// self-describing. A `#[serde(default, skip_serializing_if = ...)]`
    /// declaration — the shape that would make this additive on a JSON-like
    /// format — is actively wrong here: `skip_serializing_if` omits the
    /// `Option` tag byte while the derived deserializer unconditionally reads
    /// two fields, so an ordinary `None` response becomes **undecodable**
    /// (measured: 8 bytes out, `UnexpectedEof` back). `serde(default)` cannot
    /// rescue it, because bincode has no way to report a field as absent.
    ///
    /// So the tag is always written. New↔new round-trips for both variants and
    /// is pinned by `block_response_roundtrips_through_the_codec_2539`.
    /// **Old→new remains undecodable in every variant** — an old sender's
    /// two-field-less payload is short by one byte and there is no encoding
    /// that fixes it. This therefore requires lockstep deployment, which is
    /// what the #2034 hard cut already does. It is NOT safe to roll out to one
    /// node of a running pair.
    pub retry_after_ms: Option<u64>,
}

/// Codec for block-range sync traffic.
#[derive(Debug, Clone, Default)]
pub struct BlockSyncCodec;

#[async_trait]
impl request_response::Codec for BlockSyncCodec {
    type Protocol = StreamProtocol;
    type Request = BlockRequest;
    type Response = BlockResponse;

    async fn read_request<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        read_message(io).await
    }

    async fn read_response<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        read_message(io).await
    }

    async fn write_request<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
        request: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        write_message(io, &request).await
    }

    async fn write_response<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
        response: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        write_message(io, &response).await
    }
}

pub(crate) async fn read_message<T, V>(io: &mut T) -> io::Result<V>
where
    T: AsyncRead + Unpin + Send,
    V: for<'de> Deserialize<'de>,
{
    let mut len_bytes = [0_u8; 4];
    io.read_exact(&mut len_bytes).await?;
    let len = u32::from_be_bytes(len_bytes) as usize;
    if len > MAX_SYNC_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("sync message length {len} exceeds maximum {MAX_SYNC_MESSAGE_BYTES}"),
        ));
    }
    let mut buf = vec![0u8; len];
    io.read_exact(&mut buf).await?;
    // #2884: the frame arrived intact (length header honored, ceiling passed)
    // but the body does not parse — with an honest peer this is the version-
    // skew signature: a peer built from a binary whose BlockRequest/
    // BlockResponse/Block layout differs (bincode is positional, so a
    // mid-struct field insertion shifts every later byte; the #2727 hazard
    // class on the wire). Name it in the error so the request_response
    // failure an operator eventually sees is attributable.
    bincode::deserialize(&buf).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "block-sync payload failed bincode decode ({err}); if this recurs \
                 against one peer, the likely cause is VERSION SKEW — the peer's \
                 sync message layout differs from this binary's (#2884)"
            ),
        )
    })
}

pub(crate) async fn write_message<T, V>(io: &mut T, value: &V) -> io::Result<()>
where
    T: AsyncWrite + Unpin + Send,
    V: Serialize,
{
    let bytes =
        bincode::serialize(value).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    if bytes.len() > MAX_SYNC_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "outgoing sync message length {} exceeds maximum {MAX_SYNC_MESSAGE_BYTES}",
                bytes.len()
            ),
        ));
    }
    io.write_all(&(bytes.len() as u32).to_be_bytes()).await?;
    io.write_all(&bytes).await?;
    io.close().await
}

/// The high-level state of the sync protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncState {
    Idle,
    Discovering,
    Downloading {
        current_height: u64,
        target_height: u64,
    },
    /// IBD halted because every visible height-1 candidate is the stale
    /// orphan seed (`prev=zeros`) and the only descendants peers will
    /// serve do not chain onto an empty local chain. The required
    /// height-1 parent hash is surfaced so operators can identify which
    /// genesis block is missing from the seed and recover storage
    /// out-of-band. Set when `skip_orphan_seed_block` fires against an
    /// empty local chain; cleared by `recover_missing_genesis_parent`
    /// when a later batch carries the exact missing parent block (#1482).
    Stalled {
        required_parent: Hash,
    },
    /// Sync halted because blocks served at `height` by multiple DISTINCT
    /// peers each failed this node's deterministic authoritative validation
    /// (#2807). That corroboration means the data is what the network
    /// actually holds — canonical divergence from local rules, not peer
    /// misbehaviour — so re-downloading cannot cure it and penalizing the
    /// servers would only graylist the entire honest peer set. Cleared when
    /// local height reaches `height` through another path (reorg, snapshot
    /// restore, or a fixed validator).
    ValidationHalted {
        height: u64,
    },
    Complete,
}

impl SyncState {
    /// Stable, bounded label used by JSON and Prometheus observability.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Discovering => "discovering",
            Self::Downloading { .. } => "downloading",
            Self::Stalled { .. } => "stalled",
            Self::ValidationHalted { .. } => "validation_halted",
            Self::Complete => "complete",
        }
    }
}

/// Unit of work for a block-range request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedRequest {
    pub peer_id: PeerId,
    pub request: BlockRequest,
}

/// Snapshot of per-peer sync metrics for tests and observability.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SyncPeerMetrics {
    pub advertised_height: u64,
    pub invalid_blocks: u32,
    pub stall_count: u32,
    pub response_latency_ms: f64,
    pub priority: i64,
    pub banned: bool,
    /// Transport class last reported for the peer by telemetry (#1800).
    pub transport: TransportClass,
    /// Boolean equivalent of `transport.is_metered()` — exposed so metered
    /// (LTE failover) peers are visible to operators without matching the enum.
    pub metered: bool,
    pub consecutive_transport_failures: u32,
    pub cooling_down: bool,
    pub retry_after_ms: u64,
}

/// Per-peer sync rotation status for the operator API surface (#2820).
///
/// Everything an operator needs to answer "why is rotation not using this
/// peer": the advertised height that gates eligibility, how stale that
/// advertisement is, and each disqualifier of the selection filter as an
/// individual flag plus a single first-match reason.
#[derive(Debug, Clone, PartialEq)]
pub struct SyncPeerStatus {
    pub peer_id: PeerId,
    pub advertised_height: u64,
    /// The height a peer must advertise to serve the next needed range.
    pub required_height: u64,
    /// Milliseconds since the advertised height was last refreshed; `None`
    /// for entries recorded before timestamping existed (pre-restart state).
    pub advertisement_age_ms: Option<u64>,
    pub in_flight_requests: u32,
    pub consecutive_transport_failures: u32,
    pub cooling_down: bool,
    pub retry_after_ms: u64,
    pub banned: bool,
    pub metered: bool,
    pub priority: i64,
    pub eligible: bool,
    /// How the peer qualifies when `eligible`: `"strict"` for the normal
    /// height gate, `"fallback_probe"` when it qualifies only through the
    /// bounded stale-alternate probe (#2818) armed by a failure of the next
    /// needed range. `None` when ineligible.
    pub eligible_via: Option<&'static str>,
    pub ineligible_reason: Option<&'static str>,
}

/// Aggregate requester lifecycle counters for sync-liveness diagnostics.
///
/// These deliberately avoid peer/request labels: identifiers and ranges belong
/// in structured logs, while this bounded snapshot proves which state-machine
/// paths executed without creating unbounded metric cardinality (#2816).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncLivenessMetrics {
    pub requests_dispatched: u64,
    pub responses_received: u64,
    pub transport_failures: u64,
    pub ranges_requeued: u64,
    pub peer_rotations: u64,
    pub cooldown_deferrals: u64,
    /// Dispatches to a banned peer because no un-banned candidate existed
    /// for a needed range — the #2807 starvation floor firing.
    pub last_resort_dispatches: u64,
}

/// Identifies whether an authoritative launch-layer rejection is evidence of
/// invalid peer input or a failure in this node's own application path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthoritativeSyncRejectionAttribution {
    PeerInvalid,
    LocalFailure,
    /// The block failed this node's deterministic consensus rules
    /// (`validate_block == Ok(false)`). At this boundary a forged block and
    /// a canonical block the local rules reject are indistinguishable, and
    /// blaming the server graylists every honest peer when the block is
    /// canonical (#2807) — so this class never penalizes externally.
    /// Instead the node loop feeds it to
    /// [`SyncManager::note_rule_rejection`], which rotates away from the
    /// serving peer and latches a loud validation halt once a second
    /// distinct peer corroborates the same data.
    LocalRuleRejection,
}

impl AuthoritativeSyncRejectionAttribution {
    pub fn penalizes_peer(self) -> bool {
        matches!(self, Self::PeerInvalid)
    }
}

#[derive(Debug, Clone)]
struct PeerChainInfo {
    height: u64,
    health: PeerSyncHealth,
    /// When `height` was last refreshed (ChainStatus advert or observed-block
    /// bump). Drives the advertisement-age surface (#2820).
    last_status_at: Option<Instant>,
}

#[derive(Debug, Clone, Default)]
struct PeerSyncHealth {
    score: SyncPeerScore,
    consecutive_transport_failures: u32,
    retry_not_before: Option<Instant>,
    last_failure_at: Option<Instant>,
    last_failed_range: Option<HeightRange>,
}

#[derive(Debug, Clone)]
struct RetainedPeerSyncHealth {
    health: PeerSyncHealth,
    disconnected_at: Instant,
}

/// Evidence behind a `SyncState::ValidationHalted` latch (#2807): blocks
/// served at `height` by `distinct_peers` different peers each failed
/// authoritative validation, corroborating canonical divergence from this
/// node's rules rather than peer misbehaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidationHalt {
    pub height: u64,
    pub distinct_peers: u32,
}

/// Pre-halt corroboration state: which distinct peers have served a
/// rule-failing block at `height` so far (#2807).
#[derive(Debug, Clone)]
struct RuleRejectionWatch {
    height: u64,
    peers: Vec<PeerId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HeightRange {
    start: u64,
    end: u64,
}

impl HeightRange {
    fn len(self) -> u32 {
        if self.start > self.end {
            return 0;
        }
        (self.end - self.start + 1) as u32
    }
}

#[derive(Debug, Clone)]
struct InFlightRange {
    peer_id: PeerId,
    range: HeightRange,
    sent_at: Instant,
}

#[derive(Debug, Clone)]
struct OrphanBlock {
    block: Block,
    peer_id: PeerId,
    received_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncError {
    InvalidBatchSize,
    MissingHash,
    HashMismatch,
    WrongHeight { expected: u64, actual: u64 },
    WrongParentHash,
    MissingGenesisParent { required_parent: Hash },
    TimestampTooFarInFuture,
    TimestampWentBackwards,
    /// Block-producer authentication/authority failed (#2290). The block's
    /// producer is unsigned or not in the authorised set at this height, or does
    /// not pay the member's payout address.
    ProducerUnauthorized(String),
}

/// Drives initial block download across multiple peers.
pub struct SyncManager {
    state: SyncState,
    local_height: u64,
    target_height: u64,
    peer_chains: FxHashMap<PeerId, PeerChainInfo>,
    retained_peer_health: FxHashMap<PeerId, RetainedPeerSyncHealth>,
    pending_ranges: VecDeque<HeightRange>,
    /// Ranges refused by a peer, held until its wire-visible retry deadline.
    deferred_ranges: Vec<(HeightRange, Instant)>,
    in_flight: FxHashMap<OutboundRequestId, InFlightRange>,
    orphans: BTreeMap<u64, Vec<OrphanBlock>>,
    missing_genesis_parent: Option<Hash>,
    /// Timestamp of the last missing-genesis-parent probe request (#2036).
    /// Used to back off latched re-probes so a stalled cold-start node keeps
    /// asking for the missing parent (rotating peers) rather than going silent.
    missing_parent_last_request: Option<Instant>,
    liveness_metrics: SyncLivenessMetrics,
    /// Latched #2807 validation halt; terminal until local height reaches
    /// the halted height through a non-sync path.
    validation_halt: Option<ValidationHalt>,
    /// Corroboration ledger feeding `validation_halt` (#2807).
    rule_rejection_watch: Option<RuleRejectionWatch>,
}

impl Default for SyncManager {
    fn default() -> Self {
        Self::new(0)
    }
}

impl SyncManager {
    pub fn new(local_height: u64) -> Self {
        Self {
            state: SyncState::Idle,
            local_height,
            target_height: local_height,
            peer_chains: FxHashMap::default(),
            retained_peer_health: FxHashMap::default(),
            pending_ranges: VecDeque::new(),
            deferred_ranges: Vec::new(),
            in_flight: FxHashMap::default(),
            orphans: BTreeMap::default(),
            missing_genesis_parent: None,
            missing_parent_last_request: None,
            liveness_metrics: SyncLivenessMetrics::default(),
            validation_halt: None,
            rule_rejection_watch: None,
        }
    }

    pub fn state(&self) -> &SyncState {
        &self.state
    }

    /// Returns the missing height-1 parent hash if IBD is currently halted
    /// pending the trusted genesis block (#1482). `None` means either
    /// sync has never tripped the halt, or a later batch carried the
    /// exact missing parent and `recover_missing_genesis_parent` cleared
    /// it.
    ///
    /// Surfacing this on the public API lets a launch-API consumer
    /// (e.g. `/network/info`, `/readyz`) report the halt to operators
    /// without trusting peer-supplied genesis material.
    pub fn missing_genesis_parent(&self) -> Option<Hash> {
        self.missing_genesis_parent
    }

    pub fn local_height(&self) -> u64 {
        self.local_height
    }

    pub fn pending_range_count(&self) -> usize {
        self.pending_ranges.len().saturating_add(self.deferred_ranges.len())
    }

    pub fn in_flight_range_count(&self) -> usize {
        self.in_flight.len()
    }

    pub fn known_peer_count(&self) -> usize {
        self.peer_chains.len()
    }

    pub fn retained_peer_health_count(&self) -> usize {
        self.retained_peer_health.len()
    }

    pub fn liveness_metrics(&self) -> SyncLivenessMetrics {
        self.liveness_metrics
    }

    pub fn set_local_height(&mut self, height: u64) {
        // Successful authoritative acknowledgements are delivered
        // asynchronously after the downloader may already have validated a
        // whole response batch.  Regressing here reopens already-downloaded
        // ranges and can make honest peers look invalid.  The explicit
        // `note_authoritative_rejection` path remains the sole rewind path.
        if height < self.local_height {
            return;
        }
        self.local_height = height;
        if let Some(halt) = self.validation_halt {
            if self.local_height >= halt.height {
                // The halted block (or its whole segment) was applied through
                // another path — reorg, snapshot restore, or a fixed
                // validator. Resume normal operation (#2807).
                self.validation_halt = None;
                self.rule_rejection_watch = None;
                self.state = SyncState::Idle;
            } else {
                // Hold the loud halt state.
                return;
            }
        }
        self.refresh_target_from_peers();
        if self.local_height >= self.target_height && self.target_height > 0 {
            self.state = SyncState::Complete;
        }
    }

    /// Reconcile the context-free downloader after authoritative application
    /// rejects a block that the shadow could not decide (#2786).
    ///
    /// The launch API supplies its accepted height, while the node loop resets
    /// the shadow tip itself. Drop all work planned from the speculative shadow
    /// height and rebuild the immediate gap from still-eligible peer
    /// advertisements. Penalize the serving peer only when the launch layer
    /// classified the rejection as peer-attributable. This prevents both a
    /// premature `SyncCompleted` and honest-peer bans caused by local failures.
    pub fn note_authoritative_rejection(
        &mut self,
        peer_id: &PeerId,
        accepted_height: u64,
        attribution: AuthoritativeSyncRejectionAttribution,
    ) {
        if self.validation_halt.is_some() {
            // #2807: the halt is terminal until local height passes it; do
            // not let a straggling rejection flap state back to
            // Discovering/Idle or reopen planning.
            return;
        }
        if attribution.penalizes_peer() {
            self.penalize_peer(peer_id);
        }
        self.local_height = accepted_height;
        self.target_height = accepted_height;
        self.pending_ranges.clear();
        self.in_flight.clear();
        self.deferred_ranges.clear();
        self.orphans.clear();
        self.refresh_target_from_peers();
        if self.has_peers_ahead() {
            self.state = SyncState::Discovering;
            self.refill_pending_ranges();
        } else {
            self.state = SyncState::Idle;
        }
    }

    /// The latched #2807 validation halt, if any, for the operator surface.
    pub fn validation_halt(&self) -> Option<ValidationHalt> {
        self.validation_halt
    }

    pub fn note_rule_rejection(&mut self, peer_id: &PeerId, block_height: u64) {
        self.note_rule_rejection_at(peer_id, block_height, Instant::now());
    }

    /// Record that authoritative validation deterministically rejected the
    /// block `peer_id` served at `block_height` (#2807,
    /// `AuthoritativeSyncRejectionAttribution::LocalRuleRejection`).
    ///
    /// Deliberately NOT a peer penalty: the verdict comes from this node's
    /// own rules with no knowledge of what the canonical chain contains, so
    /// when the block is canonical the identical rejection reproduces
    /// against every honest peer. The serving peer instead gets a
    /// sync-local cooldown and priority knock so planning rotates to
    /// someone else; a SECOND distinct peer serving rule-failing data at
    /// the same height corroborates that this is what the network actually
    /// holds, which latches the loud `ValidationHalted` state instead of
    /// graylisting the peer set.
    #[doc(hidden)]
    pub fn note_rule_rejection_at(
        &mut self,
        peer_id: &PeerId,
        block_height: u64,
        now: Instant,
    ) {
        if self.validation_halt.is_some() {
            return;
        }
        if let Some(info) = self.peer_chains.get_mut(peer_id) {
            info.health.score.record_stall();
            info.health.last_failure_at = Some(now);
            info.health.retry_not_before = Some(now + SYNC_RETRY_BASE_DELAY);
        }
        let watch = match &mut self.rule_rejection_watch {
            Some(watch) if watch.height == block_height => watch,
            _ => {
                // #2981: the old `.expect("watch was just latched above")` was a true
                // claim — this is `&mut self`, so the assignment and the read are
                // straight-line under exclusive access and no other thread can clear
                // the field between them. But a comment asserting an invariant is
                // weaker than not needing one: `Option::insert` stores the value and
                // returns `&mut T` directly, so there is no Option to unwrap and the
                // panic cannot exist regardless of how this function is later edited.
                self.rule_rejection_watch.insert(RuleRejectionWatch {
                    height: block_height,
                    peers: Vec::new(),
                })
            },
        };
        if !watch.peers.contains(peer_id) {
            watch.peers.push(*peer_id);
        }
        if watch.peers.len() < 2 {
            return;
        }
        let halt = ValidationHalt {
            height: block_height,
            distinct_peers: watch.peers.len() as u32,
        };
        warn!(
            "P2P sync VALIDATION HALT at height {}: blocks served by {} distinct peers each \
             failed local authoritative validation. This is canonical-chain divergence from \
             this node's rules, not peer misbehaviour — no peer was penalized. Sync holds \
             here until local height passes the rejected block (snapshot restore, reorg, or \
             a fixed validator) (#2807)",
            halt.height, halt.distinct_peers
        );
        self.validation_halt = Some(halt);
        self.state = SyncState::ValidationHalted {
            height: block_height,
        };
        self.pending_ranges.clear();
        self.deferred_ranges.clear();
        self.in_flight.clear();
    }

    /// Hint to the sync manager that a block has been observed at `height`
    /// from some peer (e.g. via `NewBlock` gossipsub broadcast that landed
    /// in the orphan buffer because it was ahead of the local tip). If
    /// `height` is greater than the current `target_height`, bumps the
    /// target and refills `pending_ranges` so the next `plan_requests`
    /// call can dispatch a `BlockRequest` covering the new gap (#1664).
    ///
    /// Necessary because IBD's initial `target_height` is set from the
    /// first `ChainStatus` gossip received from each peer, but ChainStatus
    /// is broadcast on a slow cadence (10-300s depending on chain
    /// height). NewBlock gossip is faster — so a node finishing IBD may
    /// see a far-ahead NewBlock before the next ChainStatus refresh. Without
    /// this hook the orphan-skip + KickSync path is a no-op because
    /// `refill_pending_ranges` reads the stale `target_height` and finds
    /// no new range to plan, even though a peer clearly has a higher
    /// block.
    ///
    /// Also bumps the per-peer `height` for any `peer_chains` entry that
    /// is currently advertised at-or-above `local_height` (i.e. peers we
    /// already know are ahead of us). At least one such peer must have
    /// the observed block to have gossiped it; without bumping the per-
    /// peer height, `select_best_peer_for_range` filters every known
    /// peer out because the stale `info.height` is below the new
    /// `range.end`, and `plan_requests` returns empty even though the
    /// pending queue is freshly refilled (#1664 follow-up). If the peer
    /// did not actually have that height, the next `BlockRequest`
    /// response will be empty/partial and we'll requeue + retry until
    /// the next `ChainStatus` corrects the per-peer entry.
    pub fn note_observed_height(&mut self, height: u64) {
        if self.validation_halt.is_some() {
            // #2807: gossip cannot cure a validation halt; the gap starts at
            // a block this node deterministically rejects.
            return;
        }
        let bounded_height = self.bounded_target_height(height);
        if bounded_height <= self.target_height {
            return;
        }
        self.target_height = bounded_height;

        // Bump per-peer heights for peers already known to be ahead. The
        // NewBlock envelope doesn't carry the propagation source on the
        // launch_api side, so we conservatively bump every "ahead" peer
        // (at least one must have served this block). Without this, the
        // refilled pending range cannot match any peer in
        // `select_best_peer_for_range`'s `info.height >= range.end`
        // filter.
        let observed_at = Instant::now();
        for info in self.peer_chains.values_mut() {
            if info.height >= self.local_height && info.height < height {
                info.height = height;
                info.last_status_at = Some(observed_at);
            }
        }

        if self.missing_genesis_parent.is_some() {
            return;
        }
        if height > self.local_height && !matches!(self.state, SyncState::Downloading { .. }) {
            self.state = SyncState::Discovering;
        }
        self.refill_pending_ranges();
    }

    pub fn peer_metrics(&self, peer_id: &PeerId) -> Option<SyncPeerMetrics> {
        let now = Instant::now();
        self.peer_chains.get(peer_id).map(|info| SyncPeerMetrics {
            advertised_height: info.height,
            invalid_blocks: info.health.score.invalid_blocks(),
            stall_count: info.health.score.stall_count(),
            response_latency_ms: info.health.score.response_latency_ms(),
            priority: info.health.score.priority(),
            banned: info.health.score.is_banned(),
            transport: info.health.score.transport(),
            metered: info.health.score.is_metered(),
            consecutive_transport_failures: info.health.consecutive_transport_failures,
            cooling_down: info
                .health
                .retry_not_before
                .is_some_and(|deadline| deadline > now),
            retry_after_ms: info
                .health
                .retry_not_before
                .map(|deadline| deadline.saturating_duration_since(now).as_millis() as u64)
                .unwrap_or(0),
        })
    }

    /// Per-peer sync status for the operator surface (#2820).
    ///
    /// `busy_peers` is the node loop's in-flight view (one entry per
    /// outstanding request), the same input `plan_requests` feeds into
    /// `select_best_peer_for_range` — so the eligibility verdict here mirrors
    /// the strict selection filter rather than re-deriving a parallel rule.
    /// `required_height` is the end of the next pending range when one exists,
    /// else `local_height + 1` (the height a peer must advertise to be a
    /// useful sync source right now).
    pub fn peer_sync_statuses(
        &self,
        busy_peers: &[PeerId],
        now: Instant,
    ) -> Vec<SyncPeerStatus> {
        let front_range = self.pending_ranges.front().copied();
        let required_height = front_range
            .map(|range| range.end)
            .unwrap_or_else(|| self.local_height.saturating_add(1));
        // The bounded stale-alternate probe (#2818) arms only after the next
        // needed range has failed on a connected peer or on a retained
        // (disconnected) peer's health record — mirror that arming condition
        // exactly, so the surface reports eligible the same peers the live
        // planner can actually select during recovery.
        let fallback_armed_for_front = front_range.is_some_and(|range| {
            self.peer_chains
                .values()
                .any(|info| info.health.last_failed_range == Some(range))
                || self
                    .retained_peer_health
                    .values()
                    .any(|retained| retained.health.last_failed_range == Some(range))
        });
        let required_parent = front_range.map(|range| range.start.saturating_sub(1));
        self.peer_chains
            .iter()
            .map(|(peer_id, info)| {
                let in_flight_requests =
                    busy_peers.iter().filter(|busy| *busy == peer_id).count() as u32;
                let cooling_down = info
                    .health
                    .retry_not_before
                    .is_some_and(|deadline| deadline > now);
                let banned = info.health.score.is_banned();
                // Mirror select_best_peer_for_range's strict filter, reported
                // as the FIRST disqualifying reason in filter order — then the
                // relaxed fallback gate for peers the strict rule calls stale.
                let mut eligible_via = None;
                let ineligible_reason = if banned {
                    Some("banned")
                } else if in_flight_requests > 0 {
                    Some("busy_in_flight")
                } else if cooling_down {
                    Some("cooldown")
                } else if info.height >= required_height {
                    eligible_via = Some("strict");
                    None
                } else if fallback_armed_for_front
                    && front_range.is_some()
                    && info.health.last_failed_range == front_range
                {
                    // The probe never re-selects the peer that failed the
                    // exact range, whatever it advertises.
                    Some("failed_this_range")
                } else if fallback_armed_for_front
                    && required_parent.is_some_and(|parent| info.height >= parent)
                {
                    eligible_via = Some("fallback_probe");
                    None
                } else {
                    Some("stale_advertisement")
                };
                SyncPeerStatus {
                    peer_id: *peer_id,
                    advertised_height: info.height,
                    required_height,
                    advertisement_age_ms: info
                        .last_status_at
                        .map(|at| now.saturating_duration_since(at).as_millis() as u64),
                    in_flight_requests,
                    consecutive_transport_failures: info
                        .health
                        .consecutive_transport_failures,
                    cooling_down,
                    retry_after_ms: info
                        .health
                        .retry_not_before
                        .map(|deadline| {
                            deadline.saturating_duration_since(now).as_millis() as u64
                        })
                        .unwrap_or(0),
                    banned,
                    metered: info.health.score.is_metered(),
                    priority: info.health.score.priority(),
                    eligible: ineligible_reason.is_none(),
                    eligible_via,
                    ineligible_reason,
                }
            })
            .collect()
    }

    /// Record a peer's transport class from node/peer telemetry (#1800).
    ///
    /// Called by the node event loop when it observes (or re-classifies) the
    /// transport a peer is reachable over — e.g. an LTE-failover bearer flips a
    /// peer to [`TransportClass::Lte`] (metered), de-prioritising it for bulk
    /// block sync. The hint persists across `record_peer_status` height updates
    /// (the score is preserved). A no-op if the peer is unknown.
    pub fn set_peer_transport(&mut self, peer_id: PeerId, transport: TransportClass) {
        if let Some(info) = self.peer_chains.get_mut(&peer_id) {
            info.health.score.set_transport(transport);
        }
    }

    pub fn peers_ahead(&self) -> Vec<PeerId> {
        self.peer_chains
            .iter()
            .filter(|(_, info)| {
                info.height > self.local_height && !info.health.score.is_banned()
            })
            .map(|(peer_id, _)| *peer_id)
            .collect()
    }

    pub fn record_peer_status(&mut self, peer_id: PeerId, height: u64) {
        self.record_peer_status_at(peer_id, height, Instant::now());
    }

    #[doc(hidden)]
    pub fn record_peer_status_at(&mut self, peer_id: PeerId, height: u64, now: Instant) {
        self.prune_retained_peer_health(now);
        let connected_health = self.peer_chains.remove(&peer_id).map(|info| info.health);
        let retained_health = self.retained_peer_health.remove(&peer_id);
        let restored_after_reconnect = connected_health.is_none() && retained_health.is_some();
        let health = connected_health
            .or_else(|| retained_health.map(|retained| retained.health))
            .unwrap_or_default();
        if restored_after_reconnect {
            debug!(
                "Restored sync health for reconnected peer {peer_id}: failures={} \
                 retry_in_ms={}",
                health.consecutive_transport_failures,
                health
                    .retry_not_before
                    .map(|deadline| deadline.saturating_duration_since(now).as_millis())
                    .unwrap_or(0)
            );
        }

        self.peer_chains
            .insert(
                peer_id,
                PeerChainInfo {
                    height,
                    health,
                    last_status_at: Some(now),
                },
            );

        self.refresh_target_from_peers();

        if self.missing_genesis_parent.is_some() || self.validation_halt.is_some() {
            return;
        }

        if height > self.local_height {
            self.state = SyncState::Discovering;
            self.refill_pending_ranges();
        } else if self
            .peer_chains
            .values()
            .all(|info| info.height <= self.local_height)
        {
            self.state = SyncState::Idle;
        }
    }

    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        self.remove_peer_at(peer_id, Instant::now());
    }

    #[doc(hidden)]
    pub fn remove_peer_at(&mut self, peer_id: &PeerId, now: Instant) {
        self.prune_retained_peer_health(now);

        let request_ids = self
            .in_flight
            .iter()
            .filter_map(|(request_id, entry)| {
                if &entry.peer_id == peer_id {
                    Some(*request_id)
                } else {
                    None
                }
            })
            // #2981. `in_flight` is populated ONLY by this node issuing its own sync
            // requests, gated by `self.in_flight.len() + planned.len() <
            // MAX_PARALLEL_PEERS` where MAX_PARALLEL_PEERS = 4. A peer cannot insert
            // into it. The collect is required rather than incidental: the loop below
            // calls `note_request_failure_at`, which needs `&mut self`, so the
            // immutable borrow of `self.in_flight` must end first.
            // panic-ok: bounded by MAX_PARALLEL_PEERS = 4 request ids issued by this node; not peer-supplied length
            .collect::<Vec<_>>();

        for request_id in request_ids {
            self.note_request_failure_at(request_id, now);
        }

        if let Some(info) = self.peer_chains.remove(peer_id) {
            debug!(
                "Retaining sync health for disconnected peer {peer_id}: failures={} \
                 retry_in_ms={}",
                info.health.consecutive_transport_failures,
                info.health
                    .retry_not_before
                    .map(|deadline| deadline.saturating_duration_since(now).as_millis())
                    .unwrap_or(0)
            );
            self.retain_peer_health(*peer_id, info.health, now);
        }

        // Preserve `Stalled` across peer churn — losing peers does not
        // mean the missing-genesis-parent halt has been resolved, and
        // observers must keep seeing the halt until a peer actually
        // serves the required height-1 parent (#1482). The #2807
        // validation halt is preserved for the same reason.
        if self.missing_genesis_parent.is_none()
            && self.validation_halt.is_none()
            && self
                .peer_chains
                .values()
                .all(|info| info.height <= self.local_height)
        {
            self.state = SyncState::Idle;
        }
    }

    /// Plan a set of block-range requests to dispatch.
    pub fn plan_requests(&mut self, now: Instant) -> Vec<PlannedRequest> {
        self.prune_retained_peer_health(now);
        if self.missing_genesis_parent.is_some() {
            // #2036: do NOT go silent while latched on a missing genesis parent.
            // Previously this returned empty, so no request was ever dispatched
            // while latched and recovery could only happen via a coincidental
            // unsolicited `NewBlock` gossip — a fresh node that hit the
            // orphan-seed cold-start path sat at height 0 forever with no
            // self-healing (recovery is delegated to `/readyz` monitoring that
            // self-serve joiners don't run). Instead keep issuing a bounded,
            // backed-off probe for the specific missing parent height to peers
            // that are ahead, so `recover_missing_genesis_parent` can fire.
            return self.plan_missing_parent_probe(now);
        }
        if self.validation_halt.is_some() {
            // #2807: deterministically invalid canonical data cannot be
            // cured by re-downloading it; hold position loudly (the state
            // gauge and `validation_halt()` expose the halt) instead of
            // burning the peer set on retries.
            return Vec::new();
        }

        self.handle_stalls(now);
        let mut ready = Vec::new();
        self.deferred_ranges.retain(|(range, not_before)| {
            if *not_before <= now {
                ready.push(*range);
                false
            } else {
                true
            }
        });
        // `requeue_range` uses `push_front`, so feeding ready ranges in
        // ascending order reverses them and retries the HIGHEST gap first.
        // Those blocks are stored as orphans; local height does not advance,
        // so `handle_response` requeues the identical high range at the front
        // and the requester selects it again instead of closing the low gap.
        // Multiple refusals from one rolling window naturally share an
        // absolute deadline, so order the ready set explicitly and push it
        // back-to-front. This bounds that livelock: if a lower range is still
        // deferred, a high range can temporarily win until the lower deadline
        // releases it. Response arrival order is not a contract.
        ready.sort_unstable_by_key(|range| (range.start, range.end));
        for range in ready.into_iter().rev() {
            self.requeue_range(range);
        }
        self.refill_pending_ranges();

        let mut busy_peers = self
            .in_flight
            .values()
            .map(|entry| entry.peer_id)
            .collect::<Vec<_>>();
        let mut planned = Vec::new();

        while self.in_flight.len() + planned.len() < MAX_PARALLEL_PEERS {
            let Some(range) = self.next_assignable_range() else {
                break;
            };

            let Some(peer_id) = self.select_best_peer_for_range(range, &busy_peers, now) else {
                self.pending_ranges.push_front(range);
                break;
            };

            busy_peers.push(peer_id);
            planned.push(PlannedRequest {
                peer_id,
                request: BlockRequest {
                    start_height: range.start,
                    count: range.len(),
                },
            });
            self.state = SyncState::Downloading {
                current_height: self.local_height,
                target_height: self.target_height,
            };
        }

        planned
    }

    /// While latched on a missing genesis parent (#2036), plan a single
    /// backed-off probe for the missing height so the node can autonomously
    /// recover instead of stalling forever.
    ///
    /// The probe requests exactly `local_height + 1` (the required parent
    /// height) from the best available peer that is ahead. When a peer serves a
    /// block at that height whose hash matches the required parent,
    /// [`recover_missing_genesis_parent`](Self::recover_missing_genesis_parent)
    /// clears the latch and normal range planning resumes. Peers that serve the
    /// wrong parent are naturally rotated out via scoring on the resulting
    /// failure, so a single bad seed peer cannot pin the node.
    fn plan_missing_parent_probe(&mut self, now: Instant) -> Vec<PlannedRequest> {
        // Clear timed-out in-flight requests first — otherwise a probe that a
        // peer never answers would block all future probes (handle_stalls does
        // not otherwise run on the latched path since we return above it).
        self.handle_stalls(now);

        // Back off between probes to bound latched network traffic.
        if let Some(last) = self.missing_parent_last_request {
            if now.saturating_duration_since(last) < MISSING_PARENT_RETRY_INTERVAL {
                return Vec::new();
            }
        }

        let probe_height = self.local_height.saturating_add(1);

        // Don't double-probe a height already in flight.
        if self
            .in_flight
            .values()
            .any(|entry| entry.range.start <= probe_height && probe_height <= entry.range.end)
        {
            return Vec::new();
        }

        let range = HeightRange {
            start: probe_height,
            end: probe_height,
        };
        let Some(peer_id) = self.select_best_peer_for_range(range, &[], now) else {
            // No peer is ahead of us right now; nothing to probe. We'll retry on
            // the next tick once a suitable peer appears.
            return Vec::new();
        };

        self.missing_parent_last_request = Some(now);
        vec![PlannedRequest {
            peer_id,
            request: BlockRequest {
                start_height: probe_height,
                count: 1,
            },
        }]
    }

    pub fn register_request(
        &mut self,
        request_id: OutboundRequestId,
        peer_id: PeerId,
        request: &BlockRequest,
        now: Instant,
    ) {
        self.liveness_metrics.requests_dispatched = self
            .liveness_metrics
            .requests_dispatched
            .saturating_add(1);
        let end = request.start_height + u64::from(request.count.saturating_sub(1));
        self.in_flight.insert(
            request_id,
            InFlightRange {
                peer_id,
                range: HeightRange {
                    start: request.start_height,
                    end,
                },
                sent_at: now,
            },
        );
    }

    pub fn note_request_failure(&mut self, request_id: OutboundRequestId) {
        self.note_request_failure_at(request_id, Instant::now());
    }

    #[doc(hidden)]
    pub fn note_request_failure_at(&mut self, request_id: OutboundRequestId, now: Instant) {
        if let Some(entry) = self.in_flight.remove(&request_id) {
            self.record_transport_failure(entry.peer_id, entry.range, now);
            self.requeue_range(entry.range);
        }
    }

    pub fn handle_response(
        &mut self,
        request_id: OutboundRequestId,
        response: BlockResponse,
        now: Instant,
        chain: &mut Blockchain,
    ) -> Result<Vec<Block>, SyncError> {
        let Some(in_flight) = self.in_flight.remove(&request_id) else {
            return Ok(Vec::new());
        };
        self.liveness_metrics.responses_received = self
            .liveness_metrics
            .responses_received
            .saturating_add(1);

        if let Some(info) = self.peer_chains.get_mut(&in_flight.peer_id) {
            info.health
                .score
                .record_latency(now.saturating_duration_since(in_flight.sent_at));
            info.health.consecutive_transport_failures = 0;
            info.health.retry_not_before = None;
            info.health.last_failed_range = None;
        }

        if response.blocks.len() > in_flight.range.len() as usize {
            self.penalize_peer(&in_flight.peer_id);
            return Err(SyncError::InvalidBatchSize);
        }

        if let Some(retry_after_ms) = response.retry_after_ms {
            if !response.blocks.is_empty() {
                self.penalize_peer(&in_flight.peer_id);
                return Err(SyncError::InvalidBatchSize);
            }
            let not_before = now + Duration::from_millis(retry_after_ms);
            self.deferred_ranges.push((in_flight.range, not_before));
            self.refresh_target_from_peers();
            return Ok(Vec::new());
        }

        let mut blocks = response.blocks;
        blocks.sort_by_key(|block| block.height);
        self.recover_missing_genesis_parent(chain, &blocks)?;
        if let Some(required_parent) = skip_orphan_seed_block(chain, &mut blocks) {
            if chain.height == 0
                && chain.blocks().is_empty()
                && blocks
                    .first()
                    .map(|next| next.height > chain.height + 1)
                    .unwrap_or(false)
            {
                self.missing_genesis_parent = Some(required_parent);
                self.pending_ranges.clear();
                self.state = SyncState::Stalled { required_parent };
                log::warn!(
                    "IBD: halted after orphan seed drop because required height-1 parent {} is missing; refusing to re-request the same non-converging range (#1482)",
                    required_parent,
                );
                return Err(SyncError::MissingGenesisParent { required_parent });
            }
        }

        let mut applied = Vec::new();
        for block in blocks {
            if let Err(error) =
                self.ingest_block(in_flight.peer_id, block, now, chain, &mut applied)
            {
                self.penalize_peer(&in_flight.peer_id);
                let next_missing = self.local_height.saturating_add(1);
                if next_missing <= in_flight.range.end {
                    self.requeue_range(HeightRange {
                        start: next_missing.max(in_flight.range.start),
                        end: in_flight.range.end,
                    });
                }
                self.refresh_target_from_peers();
                return Err(error);
            }
        }

        let next_missing = self.local_height.saturating_add(1);
        if next_missing <= in_flight.range.end {
            self.requeue_range(HeightRange {
                start: next_missing.max(in_flight.range.start),
                end: in_flight.range.end,
            });
        }

        self.refresh_target_from_peers();
        if self.local_height >= self.target_height && self.target_height > 0 {
            self.state = SyncState::Complete;
        } else if !self.pending_ranges.is_empty() || !self.in_flight.is_empty() {
            self.state = SyncState::Downloading {
                current_height: self.local_height,
                target_height: self.target_height,
            };
        } else if self.has_peers_ahead() {
            self.state = SyncState::Discovering;
        } else {
            self.state = SyncState::Idle;
        }

        Ok(applied)
    }

    fn recover_missing_genesis_parent(
        &mut self,
        chain: &Blockchain,
        blocks: &[Block],
    ) -> Result<(), SyncError> {
        let Some(required_parent) = self.missing_genesis_parent else {
            return Ok(());
        };

        let next_height = chain.height.saturating_add(1);
        let Some(candidate) = blocks.iter().find(|block| block.height == next_height) else {
            return Err(SyncError::MissingGenesisParent { required_parent });
        };

        let candidate_hash = candidate.hash.unwrap_or_else(|| candidate.calculate_hash());
        if candidate_hash != required_parent {
            return Err(SyncError::MissingGenesisParent { required_parent });
        }

        log::info!(
            "IBD: recovered missing height-{} parent {}; resuming sync (#1482)",
            next_height,
            required_parent,
        );
        self.missing_genesis_parent = None;
        self.missing_parent_last_request = None;
        Ok(())
    }

    pub fn inject_blocks_for_test(
        &mut self,
        peer_id: PeerId,
        blocks: Vec<Block>,
        now: Instant,
        chain: &mut Blockchain,
    ) -> Result<Vec<Block>, SyncError> {
        let mut blocks = blocks;
        blocks.sort_by_key(|block| block.height);
        if let Some(required_parent) = skip_orphan_seed_block(chain, &mut blocks) {
            if chain.height == 0
                && chain.blocks().is_empty()
                && blocks
                    .first()
                    .map(|next| next.height > chain.height + 1)
                    .unwrap_or(false)
            {
                self.missing_genesis_parent = Some(required_parent);
                self.pending_ranges.clear();
                self.state = SyncState::Stalled { required_parent };
                log::warn!(
                    "IBD: halted after orphan seed drop because required height-1 parent {} is missing; refusing to apply peer-provided descendants without the trusted parent (#1482)",
                    required_parent,
                );
                return Err(SyncError::MissingGenesisParent { required_parent });
            }
        }
        let mut applied = Vec::new();

        for block in blocks {
            self.ingest_block(peer_id, block, now, chain, &mut applied)?;
        }

        Ok(applied)
    }

    pub fn orphan_count(&self) -> usize {
        self.orphans.values().map(Vec::len).sum()
    }

    /// Buffer an out-of-order block so it can be re-applied once its ancestors
    /// arrive via `handle_response` or `inject_blocks_for_test`.
    ///
    /// This is the public entry point used by consumers that observe a block
    /// via an independent channel (e.g. gossipsub's `NewBlock` topic) that is
    /// ahead of the local tip. Internally the orphan eventually drains when
    /// `ingest_block` or the flush loop catches up to it.
    ///
    /// Silently ignores blocks that are at or below the manager's local
    /// height, and blocks whose height equals `local_height + 1` (those are
    /// not orphans — the caller should apply them directly).
    pub fn buffer_orphan(&mut self, peer_id: PeerId, block: Block, now: Instant) {
        if block.height <= self.local_height {
            return;
        }
        if block.height == self.local_height + 1 {
            return;
        }
        self.insert_orphan(peer_id, block, now);
    }

    /// Returns the highest advertised height across all known peers.
    ///
    /// Used by the consumer event loop to identify a peer to request missing
    /// ranges from when a forward-jumped block is observed via gossipsub.
    pub fn target_height(&self) -> u64 {
        self.target_height
    }

    fn ingest_block(
        &mut self,
        peer_id: PeerId,
        block: Block,
        now: Instant,
        chain: &mut Blockchain,
        applied: &mut Vec<Block>,
    ) -> Result<(), SyncError> {
        self.prune_orphans(now);

        if block.height <= chain.height {
            return Ok(());
        }

        let expected_height = chain.height + 1;
        if block.height > expected_height {
            self.insert_orphan(peer_id, block, now);
            return Ok(());
        }

        self.validate_block(chain, &block)?;
        chain
            .add_block(block.clone())
            .map_err(|_| SyncError::WrongParentHash)?;
        self.local_height = chain.height;
        applied.push(block);
        self.flush_orphans(now, chain, applied)?;
        Ok(())
    }

    fn flush_orphans(
        &mut self,
        now: Instant,
        chain: &mut Blockchain,
        applied: &mut Vec<Block>,
    ) -> Result<(), SyncError> {
        loop {
            let next_height = chain.height + 1;
            let Some(mut candidates) = self.orphans.remove(&next_height) else {
                break;
            };
            candidates.sort_by_key(|entry| entry.block.timestamp);

            let mut consumed = false;
            for orphan in candidates {
                if self.validate_block(chain, &orphan.block).is_ok() {
                    chain
                        .add_block(orphan.block.clone())
                        .map_err(|_| SyncError::WrongParentHash)?;
                    self.local_height = chain.height;
                    applied.push(orphan.block);
                    consumed = true;
                    break;
                }

                self.penalize_peer(&orphan.peer_id);
            }

            if !consumed {
                break;
            }
            self.prune_orphans(now);
        }

        Ok(())
    }

    fn validate_block(&mut self, chain: &Blockchain, block: &Block) -> Result<(), SyncError> {
        let Some(stored_hash) = block.hash else {
            return Err(SyncError::MissingHash);
        };

        if stored_hash != block.calculate_hash() {
            return Err(SyncError::HashMismatch);
        }

        let expected_height = chain.height + 1;
        if block.height != expected_height {
            return Err(SyncError::WrongHeight {
                expected: expected_height,
                actual: block.height,
            });
        }

        if (chain.height > 0 || !chain.blocks().is_empty()) && block.prev_block_hash != chain.tip {
            return Err(SyncError::WrongParentHash);
        }

        let drift_secs = max_future_drift(block.height);
        let max_future = Utc::now() + chrono::Duration::seconds(drift_secs);
        if block.timestamp > max_future {
            return Err(SyncError::TimestampTooFarInFuture);
        }

        if let Some(previous_block) = chain.last_block() {
            if block.timestamp < previous_block.timestamp {
                return Err(SyncError::TimestampWentBackwards);
            }
        }

        // This worker's `Blockchain` is deliberately a context-free ordering
        // shadow: `UpdateLocalChainState` gives it the accepted height/tip but
        // not canonical consensus state, including producer-set history.
        // Stateful verification here is therefore vacuous at best and rejects
        // honest blocks at worst. Forward blocks that pass context-free
        // hash/order/time checks to the launch API; `apply_p2p_block` runs the
        // PoP adapter against authoritative chain state before either gossip
        // or IBD blocks can be applied.

        Ok(())
    }

    fn insert_orphan(&mut self, peer_id: PeerId, block: Block, now: Instant) {
        self.orphans
            .entry(block.height)
            .or_default()
            .push(OrphanBlock {
                block,
                peer_id,
                received_at: now,
            });
        self.prune_orphans(now);
    }

    fn prune_orphans(&mut self, now: Instant) {
        self.orphans.retain(|_, entries| {
            entries.retain(|entry| now.duration_since(entry.received_at) <= ORPHAN_TTL);
            !entries.is_empty()
        });

        let mut count = self.orphan_count();
        if count <= MAX_ORPHANS {
            return;
        }

        let heights = self.orphans.keys().copied().collect::<Vec<_>>();
        for height in heights {
            if count <= MAX_ORPHANS {
                break;
            }

            if let Some(entries) = self.orphans.get_mut(&height) {
                while count > MAX_ORPHANS && !entries.is_empty() {
                    entries.remove(0);
                    count -= 1;
                }
            }

            if self
                .orphans
                .get(&height)
                .map(|entries| entries.is_empty())
                .unwrap_or(false)
            {
                self.orphans.remove(&height);
            }
        }
    }

    fn penalize_peer(&mut self, peer_id: &PeerId) {
        if let Some(info) = self.peer_chains.get_mut(peer_id) {
            info.health.score.record_invalid_block();
        }
    }

    fn handle_stalls(&mut self, now: Instant) {
        let stalled = self
            .in_flight
            .iter()
            .filter_map(|(request_id, entry)| {
                if now.duration_since(entry.sent_at) >= BLOCK_SYNC_REQUEST_TIMEOUT {
                    Some(*request_id)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        for request_id in stalled {
            self.note_request_failure_at(request_id, now);
        }
    }

    fn bounded_target_height(&self, height: u64) -> u64 {
        height.min(
            self.local_height
                .saturating_add(MAX_SYNC_TARGET_ADVANCE_BLOCKS),
        )
    }

    fn refresh_target_from_peers(&mut self) {
        let Some(advertised_height) = self
            .peer_chains
            .values()
            .filter(|info| !info.health.score.is_banned())
            .map(|info| info.height)
            .max()
        else {
            return;
        };

        if advertised_height <= self.local_height {
            return;
        }

        let bounded_height = self.bounded_target_height(advertised_height);
        if bounded_height > self.target_height || self.target_height <= self.local_height {
            self.target_height = bounded_height;
        }
    }

    fn refill_pending_ranges(&mut self) {
        if self.target_height <= self.local_height {
            return;
        }

        let highest_known = self
            .pending_ranges
            .iter()
            .map(|range| range.end)
            .chain(self.in_flight.values().map(|entry| entry.range.end))
            // #2539: a range held under a peer's advertised retry deadline is
            // work already accounted for. Leaving it out of the high-water mark
            // let refill regenerate the same heights before the deadline — the
            // exact resend the deferral exists to prevent. Ported from #2599,
            // which is the only place this was handled.
            .chain(self.deferred_ranges.iter().map(|(range, _)| range.end))
            .max()
            .unwrap_or(self.local_height);

        // Cap the lookahead to avoid OOM when target_height is far ahead of
        // local_height.  plan_requests() calls this on every iteration, so a
        // small buffer is sufficient; ranges beyond the buffer are generated on
        // subsequent calls as in-flight ranges complete and pending_ranges drains.
        let lookahead = MAX_PARALLEL_PEERS * 4;
        let want = lookahead.saturating_sub(self.pending_ranges.len());

        let mut start = highest_known.saturating_add(1);
        let mut added = 0;
        while start <= self.target_height
            && self.pending_ranges.len() + self.in_flight.len() < MAX_PENDING_SYNC_RANGES
            && added < want
        {
            let end = start
                .saturating_add(u64::from(MAX_BLOCKS_PER_REQUEST) - 1)
                .min(self.target_height);
            let range = HeightRange { start, end };
            // Overlap, not equality. Stated honestly: while the high-water mark
            // above folds in deferred ends, `start` is always greater than
            // every deferred `end`, so this branch is **unreachable today** —
            // reverting it to the original equality check leaves all three
            // #2539 tests green (measured). It is kept because it is the local
            // statement of the invariant: if the high-water computation is ever
            // changed and stops covering deferred ranges, this is what keeps a
            // straddling range from being queued before its deadline, and
            // equality alone would not have.
            if !self
                .deferred_ranges
                .iter()
                .any(|(deferred, _)| deferred.start <= range.end && range.start <= deferred.end)
            {
                self.pending_ranges.push_back(range);
            }
            added += 1;
            let next = end.saturating_add(1);
            if next <= end {
                // Saturated at u64::MAX; this range covers the rest of the chain.
                break;
            }
            start = next;
        }
    }

    fn requeue_range(&mut self, range: HeightRange) {
        if range.start > range.end {
            return;
        }

        if !self.pending_ranges.contains(&range) {
            self.pending_ranges.push_front(range);
            self.liveness_metrics.ranges_requeued = self
                .liveness_metrics
                .ranges_requeued
                .saturating_add(1);
        }
    }

    fn next_assignable_range(&mut self) -> Option<HeightRange> {
        let mut skipped = Vec::new();

        while let Some(range) = self.pending_ranges.pop_front() {
            if range.end <= self.local_height {
                continue;
            }

            if self
                .in_flight
                .values()
                .any(|entry| entry.range.start == range.start && entry.range.end == range.end)
            {
                skipped.push(range);
                continue;
            }

            for skipped_range in skipped.into_iter().rev() {
                self.pending_ranges.push_front(skipped_range);
            }
            return Some(range);
        }

        for skipped_range in skipped.into_iter().rev() {
            self.pending_ranges.push_front(skipped_range);
        }

        None
    }

    fn select_best_peer_for_range(
        &mut self,
        range: HeightRange,
        busy_peers: &[PeerId],
        now: Instant,
    ) -> Option<PeerId> {
        let strict_selected = self
            .peer_chains
            .iter()
            .filter(|(peer_id, info)| {
                info.height >= range.end
                    && !info.health.score.is_banned()
                    && !busy_peers.contains(peer_id)
                    && !info
                        .health
                        .retry_not_before
                        .is_some_and(|deadline| deadline > now)
            })
            .max_by_key(|(_, info)| info.health.score.priority())
            .map(|(peer_id, _)| *peer_id);

        // A peer's ChainStatus can lag its actual storage across a producer
        // cutover. If the only advertised-ahead peer failed this exact range,
        // make one bounded probe of a different connected peer that is known
        // to hold the parent immediately before the range. Honest stale peers
        // can then serve the gap; honest behind peers return partial/empty and
        // remain bounded by the existing response and retry controls. Never
        // relax the height gate for the peer that already failed the range.
        let selected = strict_selected.or_else(|| {
            let connected_failure = self
                .peer_chains
                .iter()
                .any(|(_, info)| info.health.last_failed_range == Some(range));
            let retained_failure = self
                .retained_peer_health
                .values()
                .any(|retained| retained.health.last_failed_range == Some(range));
            if !connected_failure && !retained_failure {
                return None;
            }

            let required_parent = range.start.saturating_sub(1);
            self.peer_chains
                .iter()
                .filter(|(peer_id, info)| {
                    info.health.last_failed_range != Some(range)
                        && info.height >= required_parent
                        && !info.health.score.is_banned()
                        && !busy_peers.contains(peer_id)
                        && !info
                            .health
                            .retry_not_before
                            .is_some_and(|deadline| deadline > now)
                })
                .max_by_key(|(_, info)| (info.height, info.health.score.priority()))
                .map(|(peer_id, _)| *peer_id)
        });

        // #2807 starvation floor: when EVERY peer that could serve this
        // range is banned, a bounded last-resort retry beats starving
        // forever with pending work — the live 2026-08-16 wedge sat at zero
        // in-flight requests for hours while gossip kept proving peers had
        // the data. Fires only when no un-banned candidate exists at all (a
        // merely cooling-down healthy peer defers instead, above), and each
        // dispatch re-arms a long cooldown so a genuinely hostile peer is
        // retried at a bounded cadence, never hammered.
        let last_resort = selected.is_none()
            && !self
                .peer_chains
                .values()
                .any(|info| info.height >= range.end && !info.health.score.is_banned());
        let selected = if last_resort {
            self.peer_chains
                .iter()
                .filter(|(peer_id, info)| {
                    info.height >= range.end
                        && info.health.score.is_banned()
                        && !busy_peers.contains(peer_id)
                        && !info
                            .health
                            .retry_not_before
                            .is_some_and(|deadline| deadline > now)
                })
                .max_by_key(|(_, info)| info.health.score.priority())
                .map(|(peer_id, _)| *peer_id)
        } else {
            selected
        };
        if last_resort {
            if let Some(selected_peer) = selected {
                self.liveness_metrics.last_resort_dispatches = self
                    .liveness_metrics
                    .last_resort_dispatches
                    .saturating_add(1);
                if let Some(info) = self.peer_chains.get_mut(&selected_peer) {
                    info.health.retry_not_before = Some(now + LAST_RESORT_RETRY_INTERVAL);
                }
                warn!(
                    "P2P sync last-resort dispatch: every candidate for range {}..={} is \
                     banned; retrying banned peer {selected_peer} at a bounded cadence \
                     instead of starving (#2807)",
                    range.start, range.end
                );
            }
        }

        if let Some(selected_peer) = selected {
            let rotated_from_connected = self.peer_chains.iter().any(|(peer_id, info)| {
                *peer_id != selected_peer && info.health.last_failed_range == Some(range)
            });
            let rotated_from_disconnected = self.retained_peer_health.iter().any(|(peer_id, info)| {
                *peer_id != selected_peer && info.health.last_failed_range == Some(range)
            });
            if rotated_from_connected || rotated_from_disconnected {
                self.liveness_metrics.peer_rotations =
                    self.liveness_metrics.peer_rotations.saturating_add(1);
            }
        } else if self.peer_chains.iter().any(|(peer_id, info)| {
            info.height >= range.end
                && !info.health.score.is_banned()
                && !busy_peers.contains(peer_id)
                && info
                    .health
                    .retry_not_before
                    .is_some_and(|deadline| deadline > now)
        }) {
            self.liveness_metrics.cooldown_deferrals = self
                .liveness_metrics
                .cooldown_deferrals
                .saturating_add(1);
        }

        selected
    }

    fn has_peers_ahead(&self) -> bool {
        self.peer_chains
            .values()
            .any(|info| info.height > self.local_height && !info.health.score.is_banned())
    }

    fn record_transport_failure(&mut self, peer_id: PeerId, range: HeightRange, now: Instant) {
        self.liveness_metrics.transport_failures = self
            .liveness_metrics
            .transport_failures
            .saturating_add(1);
        let Some(info) = self.peer_chains.get_mut(&peer_id) else {
            return;
        };
        info.health.score.record_stall();
        info.health.consecutive_transport_failures = info
            .health
            .consecutive_transport_failures
            .saturating_add(1);
        info.health.last_failure_at = Some(now);
        info.health.last_failed_range = Some(range);
        let retry_delay = sync_retry_delay(peer_id, info.health.consecutive_transport_failures);
        info.health.retry_not_before = Some(now + retry_delay);
        debug!(
            "Recorded sync transport failure for {peer_id}: range={}..={} failures={} \
             retry_in_ms={}",
            range.start,
            range.end,
            info.health.consecutive_transport_failures,
            retry_delay.as_millis()
        );
    }

    fn retain_peer_health(&mut self, peer_id: PeerId, health: PeerSyncHealth, now: Instant) {
        self.retained_peer_health.insert(
            peer_id,
            RetainedPeerSyncHealth {
                health,
                disconnected_at: now,
            },
        );
        while self.retained_peer_health.len() > MAX_RETAINED_SYNC_PEER_HEALTH {
            let Some(oldest_peer) = self
                .retained_peer_health
                .iter()
                .min_by_key(|(_, retained)| retained.disconnected_at)
                .map(|(peer_id, _)| *peer_id)
            else {
                break;
            };
            self.retained_peer_health.remove(&oldest_peer);
        }
    }

    fn prune_retained_peer_health(&mut self, now: Instant) {
        self.retained_peer_health.retain(|_, retained| {
            now.saturating_duration_since(retained.disconnected_at)
                <= RETAINED_SYNC_PEER_HEALTH_TTL
        });
    }
}

fn sync_retry_delay(peer_id: PeerId, consecutive_failures: u32) -> Duration {
    let exponent = consecutive_failures.saturating_sub(1).min(6);
    let exponential = SYNC_RETRY_BASE_DELAY
        .saturating_mul(1_u32.checked_shl(exponent).unwrap_or(u32::MAX))
        .min(SYNC_RETRY_MAX_DELAY);
    let peer_mix = peer_id
        .to_bytes()
        .into_iter()
        .fold(u64::from(consecutive_failures), |acc, byte| {
            acc.rotate_left(5) ^ u64::from(byte)
        });
    let jitter_ceiling_ms = SYNC_RETRY_MAX_JITTER.as_millis() as u64;
    let jitter_ms = peer_mix % jitter_ceiling_ms.saturating_add(1);
    exponential
        .saturating_add(Duration::from_millis(jitter_ms))
        .min(SYNC_RETRY_MAX_DELAY)
}

/// Detect and remove an orphaned seed block at the head of an IBD batch (#1466).
///
/// When a seed peer's persistent storage contains a stale orphan at height 1
/// (e.g. left over from a prior chain reset that did not clear redb), it
/// serves the orphan plus the live chain in a single batch. Without this
/// filter the IBD client would happily apply the orphan to its empty local
/// chain (the parent-hash check in `validate_block` is gated on a non-empty
/// chain), then every subsequent block in the batch would fail
/// `WrongParentHash`, stalling IBD permanently.
///
/// Detection is intentionally conservative — `blocks[0]` is dropped only
/// when **every** condition holds:
///
/// 1. `chain.blocks().is_empty()` — local chain has not yet ingested any
///    block from the current sync session
/// 2. `blocks.len() >= 2` — we have a successor in the same batch to verify
///    against (without one we cannot prove the seed is orphaned)
/// 3. `blocks[0].height <= 1` — orphan-genesis tier
/// 4. `blocks[0].prev_block_hash == Hash::default()` — claims to descend
///    from the genesis-zeros sentinel
/// 5. `blocks[1].height == blocks[0].height + 1` — successor is the
///    immediate next height (no gap)
/// 6. `blocks[1].prev_block_hash != blocks[0].hash` — successor's parent
///    does not chain onto the seed, proving the seed is an orphan
///
/// All other `WrongParentHash` cases preserve the existing peer-penalty
/// behaviour — this helper does **not** generalise to "skip any bad block".
///
/// Returns the missing parent hash needed by the next block if a seed block
/// was skipped.
fn skip_orphan_seed_block(chain: &Blockchain, blocks: &mut Vec<Block>) -> Option<Hash> {
    if !chain.blocks().is_empty() || blocks.len() < 2 {
        return None;
    }

    let candidate_height = blocks[0].height;
    let candidate_prev = blocks[0].prev_block_hash;
    if candidate_height > 1 || candidate_prev != Hash::default() {
        return None;
    }

    if blocks[1].height != candidate_height + 1 {
        return None;
    }

    let candidate_hash = blocks[0].hash.unwrap_or_else(|| blocks[0].calculate_hash());
    if blocks[1].prev_block_hash == candidate_hash {
        return None;
    }
    let required_parent = blocks[1].prev_block_hash;

    log::warn!(
        "IBD: dropping likely orphan seed block at height {} (hash={}, prev=zeros); next block at height {} has prev={} which does not chain onto it (#1466)",
        candidate_height,
        candidate_hash,
        blocks[1].height,
        blocks[1].prev_block_hash,
    );
    blocks.remove(0);
    Some(required_parent)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- #2119 amplification ceiling -------------------------------------------------

    /// The case that motivates the whole design: a ONE-block request currently earns the
    /// same 16 MiB allowance as a hundred-block one, because the only ceiling is
    /// independent of `count`.
    #[test]
    fn single_block_request_cannot_earn_the_whole_message_budget() {
        let limits = SyncServeLimits::default();
        let whole_cap = limits.max_bytes_per_response;

        // 100 blocks: the absolute cap governs, so nothing changes for honest IBD.
        assert_eq!(serve_verdict(100, whole_cap, &limits), ServeVerdict::Within);

        // 1 block: the same byte count is now disproportionate.
        match serve_verdict(1, whole_cap, &limits) {
            ServeVerdict::Exceeds { estimated, budget } => {
                assert_eq!(estimated, whole_cap);
                assert_eq!(budget, limits.max_bytes_per_requested_block);
            },
            ServeVerdict::Within => panic!(
                "a 1-block request served {whole_cap} bytes must be reported as \
                 disproportionate; that is the #2119 defect"
            ),
        }
    }

    /// The ceiling must not bind on any request an honest node makes.
    #[test]
    fn honest_ibd_request_shapes_are_unaffected() {
        let limits = SyncServeLimits::default();
        // A generous 64 KiB per block across every legal count.
        for count in [1u32, 2, 10, 50, MAX_BLOCKS_PER_REQUEST] {
            let plausible = 65_536usize.saturating_mul(count as usize);
            assert_eq!(
                serve_verdict(count, plausible, &limits),
                ServeVerdict::Within,
                "count={count} at 64 KiB/block must stay within the default ceiling"
            );
        }
    }

    /// The absolute cap still governs when the per-block budget would exceed it.
    #[test]
    fn absolute_cap_governs_large_counts() {
        let limits = SyncServeLimits::default();
        let over = limits.max_bytes_per_response + 1;
        assert!(matches!(
            serve_verdict(MAX_BLOCKS_PER_REQUEST, over, &limits),
            ServeVerdict::Exceeds { .. }
        ));
    }

    /// `count = 0` must not divide the budget to nothing and reject everything.
    #[test]
    fn zero_count_is_treated_as_one_block() {
        let limits = SyncServeLimits::default();
        assert_eq!(
            serve_verdict(0, limits.max_bytes_per_requested_block, &limits),
            ServeVerdict::Within
        );
    }

    /// Observe-only is the default, and it refuses nothing.
    #[test]
    fn observe_only_reports_but_does_not_refuse() {
        let observing = SyncServeLimits::default();
        assert!(!observing.enforce, "the shipped default must be observe-only");

        let verdict = serve_verdict(1, observing.max_bytes_per_response, &observing);
        assert!(matches!(verdict, ServeVerdict::Exceeds { .. }), "still reported");
        assert!(!verdict.should_refuse(&observing), "but not refused");

        let enforcing = SyncServeLimits { enforce: true, ..SyncServeLimits::default() };
        assert!(verdict.should_refuse(&enforcing), "enforcement flips only the action");
    }

    /// A within-budget response is never refused, in either mode.
    #[test]
    fn within_budget_is_never_refused() {
        for enforce in [false, true] {
            let limits = SyncServeLimits { enforce, ..SyncServeLimits::default() };
            let v = serve_verdict(10, 1024, &limits);
            assert_eq!(v, ServeVerdict::Within);
            assert!(!v.should_refuse(&limits));
        }
    }
    use chrono::DateTime;
    use libp2p::request_response::{
        Behaviour as RequestResponseBehaviour, Config, ProtocolSupport,
    };
    use std::time::Duration;
    use wrfcoin_shared_protocol::{
        blockchain::{
            state::StateTransition,
            transaction::{CommitPayload, RevealPayload, Transaction, TransactionType},
        },
        crypto::hash::{Hash, hash_bytes},
        station_registry::{
            ContributionProvenanceClass, DataQualityTier, StationLocation, WeatherStation,
        },
    };

    fn fake_peer() -> PeerId {
        PeerId::random()
    }

    // ----- #2820: per-peer sync status surface -----

    /// The eligibility verdicts must mirror `select_best_peer_for_range`'s
    /// strict filter: a peer plan_requests would select reports eligible; a
    /// stale peer reports `stale_advertisement` against the same required
    /// height the filter uses.
    #[test]
    fn peer_sync_statuses_reports_eligibility_and_stale_reason() {
        let mut mgr = SyncManager::new(100);
        let ahead = fake_peer();
        let stale = fake_peer();
        mgr.record_peer_status(ahead, 108);
        mgr.record_peer_status(stale, 100);

        let now = Instant::now();
        let statuses = mgr.peer_sync_statuses(&[], now);
        assert_eq!(statuses.len(), 2);
        let ahead_status = statuses.iter().find(|s| s.peer_id == ahead).unwrap();
        let stale_status = statuses.iter().find(|s| s.peer_id == stale).unwrap();

        assert!(ahead_status.eligible, "the selectable peer must read eligible");
        assert_eq!(ahead_status.eligible_via, Some("strict"));
        assert_eq!(ahead_status.ineligible_reason, None);
        assert_eq!(ahead_status.advertised_height, 108);

        assert!(!stale_status.eligible);
        assert_eq!(stale_status.ineligible_reason, Some("stale_advertisement"));
        assert_eq!(stale_status.required_height, ahead_status.required_height);
        assert!(
            stale_status.advertised_height < stale_status.required_height,
            "stale reason must be consistent with the reported heights"
        );

        // Consistency control: the eligible peer is exactly what the live
        // selection filter picks for the first planned request.
        let first = mgr.plan_requests(now).remove(0);
        assert_eq!(first.peer_id, ahead);
    }

    #[test]
    fn peer_sync_statuses_reports_busy_and_cooldown_reasons() {
        let mut mgr = SyncManager::new(100);
        let ahead = fake_peer();
        mgr.record_peer_status(ahead, 108);
        let now = Instant::now();

        // Busy: same in-flight view plan_requests uses.
        let busy = mgr.peer_sync_statuses(&[ahead, ahead], now);
        assert_eq!(busy[0].ineligible_reason, Some("busy_in_flight"));
        assert_eq!(busy[0].in_flight_requests, 2);
        assert!(!busy[0].eligible);

        // Cooldown: a real transport failure arms retry_not_before.
        let first = mgr.plan_requests(now).remove(0);
        let mut behaviour = RequestResponseBehaviour::with_codec(
            BlockSyncCodec,
            std::iter::once((
                StreamProtocol::new(BLOCK_SYNC_PROTOCOL),
                ProtocolSupport::Full,
            )),
            Config::default(),
        );
        let request_id = behaviour.send_request(&first.peer_id, first.request.clone());
        mgr.register_request(request_id, first.peer_id, &first.request, now);
        mgr.note_request_failure_at(request_id, now);

        let cooled = mgr.peer_sync_statuses(&[], now + Duration::from_millis(1));
        let status = cooled.iter().find(|s| s.peer_id == ahead).unwrap();
        assert!(status.cooling_down);
        assert!(status.retry_after_ms > 0);
        assert_eq!(status.consecutive_transport_failures, 1);
        assert_eq!(status.ineligible_reason, Some("cooldown"));
    }

    #[test]
    fn peer_sync_statuses_reports_banned_first_and_advertisement_age() {
        let mut mgr = SyncManager::new(100);
        let peer = fake_peer();
        let t0 = Instant::now();
        mgr.record_peer_status_at(peer, 108, t0);

        // Advertisement age tracks the recording instant.
        let aged = mgr.peer_sync_statuses(&[], t0 + Duration::from_secs(7));
        assert_eq!(aged[0].advertisement_age_ms, Some(7_000));

        // Banned outranks every other reason, including busy.
        for _ in 0..3 {
            mgr.peer_chains
                .get_mut(&peer)
                .unwrap()
                .health
                .score
                .record_invalid_block();
        }
        let banned = mgr.peer_sync_statuses(&[peer], t0 + Duration::from_secs(8));
        assert!(banned[0].banned);
        assert_eq!(banned[0].ineligible_reason, Some("banned"));
        assert!(!banned[0].eligible);
    }

    /// The 2834-review blocker: after the next needed range fails on the
    /// advertised-ahead peer, the live planner selects the stale alternate
    /// through the bounded fallback (#2818) — the surface must report that
    /// peer ELIGIBLE (via fallback_probe), not stale, during exactly that
    /// recovery window. Consistency control: the retry plan_requests actually
    /// selects the peer the surface calls eligible.
    #[test]
    fn peer_sync_statuses_mirrors_fallback_probe_eligibility() {
        let mut mgr = SyncManager::new(100);
        let advertised_ahead = fake_peer();
        let stale_alternate = fake_peer();
        mgr.record_peer_status(advertised_ahead, 108);
        mgr.record_peer_status(stale_alternate, 100);

        let now = Instant::now();
        // Before any failure the stale alternate reads stale_advertisement.
        let before = mgr.peer_sync_statuses(&[], now);
        let stale_before = before
            .iter()
            .find(|s| s.peer_id == stale_alternate)
            .unwrap();
        assert_eq!(stale_before.ineligible_reason, Some("stale_advertisement"));

        // Fail the exact range on the ahead peer, arming the fallback.
        let first = mgr.plan_requests(now).remove(0);
        assert_eq!(first.peer_id, advertised_ahead);
        let mut behaviour = RequestResponseBehaviour::with_codec(
            BlockSyncCodec,
            std::iter::once((
                StreamProtocol::new(BLOCK_SYNC_PROTOCOL),
                ProtocolSupport::Full,
            )),
            Config::default(),
        );
        let request_id = behaviour.send_request(&first.peer_id, first.request.clone());
        mgr.register_request(request_id, first.peer_id, &first.request, now);
        mgr.note_request_failure_at(request_id, now);

        let during = mgr.peer_sync_statuses(&[], now + Duration::from_millis(1));
        let stale_status = during
            .iter()
            .find(|s| s.peer_id == stale_alternate)
            .unwrap();
        assert!(
            stale_status.eligible,
            "the fallback-selectable alternate must read eligible: {stale_status:?}"
        );
        assert_eq!(stale_status.eligible_via, Some("fallback_probe"));
        let ahead_status = during
            .iter()
            .find(|s| s.peer_id == advertised_ahead)
            .unwrap();
        assert!(!ahead_status.eligible, "the failed peer stays ineligible");

        // Consistency control: the planner's retry selects exactly the peer
        // the surface reports eligible.
        let retry = mgr.plan_requests(now + Duration::from_millis(2));
        assert_eq!(retry.len(), 1);
        assert_eq!(retry[0].peer_id, stale_alternate);

        // Once cooldown lapses the STRICT gate re-admits the failed peer
        // (only the fallback excludes range-failers) — planner parity, not a
        // bug: strict_selected would select it again too.
        let after_cooldown = mgr.peer_sync_statuses(&[], now + Duration::from_secs(120));
        let ahead_after = after_cooldown
            .iter()
            .find(|s| s.peer_id == advertised_ahead)
            .unwrap();
        assert_eq!(ahead_after.eligible_via, Some("strict"));

    }

    /// A range-failer that is ALSO below the strict gate is excluded from the
    /// fallback for that range, whatever the relaxed gate says — the surface
    /// must name the exclusion, not report generic staleness. (Separate from
    /// the test above because the failed range must still be the front
    /// pending range: planning the retry there moves it in flight.)
    #[test]
    fn peer_sync_statuses_names_failed_range_exclusion() {
        let mut mgr = SyncManager::new(100);
        let failer = fake_peer();
        let alternate = fake_peer();
        mgr.record_peer_status(failer, 108);
        mgr.record_peer_status(alternate, 100);

        let now = Instant::now();
        let first = mgr.plan_requests(now).remove(0);
        assert_eq!(first.peer_id, failer);
        let mut behaviour = RequestResponseBehaviour::with_codec(
            BlockSyncCodec,
            std::iter::once((
                StreamProtocol::new(BLOCK_SYNC_PROTOCOL),
                ProtocolSupport::Full,
            )),
            Config::default(),
        );
        let request_id = behaviour.send_request(&first.peer_id, first.request.clone());
        mgr.register_request(request_id, first.peer_id, &first.request, now);
        mgr.note_request_failure_at(request_id, now);

        // Demote the failer below the strict gate; health (and the failed
        // range) survives the status update. Read after its cooldown lapsed
        // so the exclusion — not the cooldown — is the operative reason.
        mgr.record_peer_status_at(failer, 100, now + Duration::from_secs(1));
        let statuses = mgr.peer_sync_statuses(&[], now + Duration::from_secs(120));
        let failer_status = statuses.iter().find(|s| s.peer_id == failer).unwrap();
        assert_eq!(failer_status.ineligible_reason, Some("failed_this_range"));
        assert!(!failer_status.eligible);
        let alternate_status =
            statuses.iter().find(|s| s.peer_id == alternate).unwrap();
        assert_eq!(alternate_status.eligible_via, Some("fallback_probe"));
    }

    // ----- #1800: transport-aware sync peer selection -----

    /// Detector (#1800): two peers with identical latency/invalid/stall counters
    /// but different transports — the non-metered (WAN) peer must win selection
    /// over the metered (LTE failover) peer.
    #[test]
    fn peer_selection_prefers_non_metered_transport() {
        let mut mgr = SyncManager::new(0);
        let metered = fake_peer();
        let fast = fake_peer();
        // Equal height; fresh scores ⇒ identical latency/invalid/stall.
        mgr.record_peer_status(metered, 150);
        mgr.record_peer_status(fast, 150);
        // Only the transport differs.
        mgr.set_peer_transport(metered, TransportClass::Lte); // metered failover
        mgr.set_peer_transport(fast, TransportClass::Wan); // wired backbone

        let chosen = mgr
            .select_best_peer_for_range(
                HeightRange { start: 1, end: 150 },
                &[],
                Instant::now(),
            )
            .expect("a non-banned same-height peer must be selectable");
        assert_eq!(
            chosen, fast,
            "non-metered (WAN) peer must win when latency/invalid are equal"
        );
        // And the metered peer must score strictly lower.
        assert!(
            mgr.peer_metrics(&fast).unwrap().priority
                > mgr.peer_metrics(&metered).unwrap().priority,
            "WAN priority must exceed LTE priority for otherwise-equal peers"
        );
    }

    /// Detector (#1800): `peer_metrics()` must expose the transport hint and its
    /// boolean `metered` equivalent once telemetry marks the peer.
    #[test]
    fn peer_selection_peer_metrics_exposes_transport_hint() {
        let mut mgr = SyncManager::new(0);
        let peer = fake_peer();
        mgr.record_peer_status(peer, 10);

        let before = mgr.peer_metrics(&peer).unwrap();
        assert_eq!(before.transport, TransportClass::Unknown);
        assert!(!before.metered, "unknown transport must not report metered");

        mgr.set_peer_transport(peer, TransportClass::Lte);
        let after = mgr.peer_metrics(&peer).unwrap();
        assert_eq!(after.transport, TransportClass::Lte);
        assert!(
            after.metered,
            "peer_metrics must expose the metered boolean once telemetry marks LTE"
        );
    }

    /// Detector (#1800): banned/invalid logic still dominates — a banned peer is
    /// excluded from selection even when it has the best possible transport.
    #[test]
    fn peer_selection_excludes_banned_peer_despite_best_transport() {
        let mut mgr = SyncManager::new(0);
        let banned = fake_peer();
        let clean = fake_peer();
        mgr.record_peer_status(banned, 150);
        mgr.record_peer_status(clean, 150);

        // Give the to-be-banned peer the BEST transport, the clean peer the worst.
        mgr.set_peer_transport(banned, TransportClass::Wan);
        mgr.set_peer_transport(clean, TransportClass::Lte);
        // Ban it via repeated invalid blocks (is_banned() at >= 3).
        for _ in 0..3 {
            mgr.peer_chains
                .get_mut(&banned)
                .unwrap()
                .health
                .score
                .record_invalid_block();
        }
        assert!(mgr.peer_metrics(&banned).unwrap().banned, "peer must be banned");

        let chosen = mgr
            .select_best_peer_for_range(
                HeightRange { start: 1, end: 150 },
                &[],
                Instant::now(),
            )
            .expect("the clean peer must still be selectable");
        assert_eq!(
            chosen, clean,
            "banned peer must be excluded even with superior (WAN) transport"
        );
    }

    fn make_block(height: u64, prev_hash: Hash, timestamp: DateTime<Utc>) -> Block {
        let mut block = Block::new(1, prev_hash, Vec::<Transaction>::new(), 1, height);
        block.timestamp = timestamp;
        let hash = block.calculate_hash();
        block.set_hash(hash);
        block
    }

    fn build_chain(count: usize) -> Result<(Blockchain, Vec<Block>), SyncError> {
        let mut chain = Blockchain::default();
        let mut blocks = Vec::new();
        let mut prev_hash = hash_bytes(b"genesis");
        let base = Utc::now() - chrono::Duration::seconds(60);

        for height in 1..=count as u64 {
            let block = make_block(
                height,
                prev_hash,
                base + chrono::Duration::seconds(height as i64),
            );
            prev_hash = block.hash.ok_or(SyncError::MissingHash)?;
            chain.add_block(block.clone()).map_err(|_| SyncError::WrongParentHash)?;
            blocks.push(block);
        }

        Ok((chain, blocks))
    }

    #[test]
    fn transitions_idle_to_discovering() {
        let mut mgr = SyncManager::new(0);
        assert_eq!(*mgr.state(), SyncState::Idle);

        mgr.record_peer_status(fake_peer(), 100);
        assert_eq!(*mgr.state(), SyncState::Discovering);
    }

    #[test]
    fn stays_idle_when_peer_not_ahead() {
        let mut mgr = SyncManager::new(100);
        mgr.record_peer_status(fake_peer(), 50);
        assert_eq!(*mgr.state(), SyncState::Idle);
    }

    #[test]
    fn sync_parallel_download() {
        let mut mgr = SyncManager::new(0);
        let peer_a = fake_peer();
        let peer_b = fake_peer();
        let peer_c = fake_peer();

        mgr.record_peer_status(peer_a, 250);
        mgr.record_peer_status(peer_b, 250);
        mgr.record_peer_status(peer_c, 250);

        let planned = mgr.plan_requests(Instant::now());
        assert_eq!(planned.len(), 3);
        assert_eq!(planned[0].request.start_height, 1);
        assert_eq!(planned[1].request.start_height, 101);
        assert_eq!(planned[2].request.start_height, 201);
    }

    #[test]
    fn peer_advertised_absurd_height_is_windowed_before_planning_ranges() {
        let mut mgr = SyncManager::new(0);
        let peer = fake_peer();

        mgr.record_peer_status(peer, u64::MAX);

        assert_eq!(
            mgr.target_height(),
            MAX_SYNC_TARGET_ADVANCE_BLOCKS,
            "untrusted peer height must be capped to the first sync window"
        );
        assert!(
            mgr.pending_ranges.len() <= MAX_PENDING_SYNC_RANGES,
            "pending ranges must stay bounded, got {}",
            mgr.pending_ranges.len()
        );
        assert_eq!(
            mgr.pending_ranges.front().map(|range| range.start),
            Some(1),
            "planning must start at the first block above local height"
        );
        // refill_pending_ranges() only plans a small lookahead buffer per call
        // (an OOM guard); later ranges are generated as in-flight ranges
        // complete. The security property is that nothing is ever planned past
        // the bounded target, not that the whole window is planned at once.
        let last_end = mgr.pending_ranges.back().map(|range| range.end);
        assert!(
            last_end.is_some_and(|end| end > 0 && end <= MAX_SYNC_TARGET_ADVANCE_BLOCKS),
            "planned ranges must stay inside the bounded target, got {last_end:?}"
        );
    }

    #[test]
    fn sync_target_advances_in_bounded_windows_after_local_progress() {
        let mut mgr = SyncManager::new(0);
        let peer = fake_peer();
        let second_window = MAX_SYNC_TARGET_ADVANCE_BLOCKS * 2;

        mgr.record_peer_status(peer, second_window);
        assert_eq!(mgr.target_height(), MAX_SYNC_TARGET_ADVANCE_BLOCKS);

        mgr.pending_ranges.clear();
        mgr.set_local_height(MAX_SYNC_TARGET_ADVANCE_BLOCKS);

        assert_eq!(
            mgr.target_height(),
            second_window,
            "known peer height should open the next bounded sync window"
        );
        assert_ne!(
            *mgr.state(),
            SyncState::Complete,
            "manager must keep discovering while known peers remain ahead"
        );
    }

    #[test]
    fn observed_absurd_height_is_windowed_before_planning_ranges() {
        let mut mgr = SyncManager::new(500);
        let peer = fake_peer();
        mgr.record_peer_status(peer, 500);

        mgr.note_observed_height(u64::MAX);

        assert_eq!(
            mgr.target_height(),
            500 + MAX_SYNC_TARGET_ADVANCE_BLOCKS,
            "observed orphan height must be capped relative to local height"
        );
        assert!(
            mgr.pending_ranges.len() <= MAX_PENDING_SYNC_RANGES,
            "observed-height refill must stay bounded"
        );
    }

    #[test]
    fn sync_rejects_invalid_block() {
        let (chain, blocks) = build_chain(1).unwrap();
        let peer = fake_peer();
        let mut mgr = SyncManager::new(chain.height);
        mgr.record_peer_status(peer, 2);
        // Build a second identical chain (Blockchain no longer implements Clone).
        let (mut local, _) = build_chain(1).unwrap();

        let mut bad_block = make_block(
            2,
            hash_bytes(b"wrong-parent"),
            blocks[0].timestamp + chrono::Duration::seconds(1),
        );
        bad_block.set_hash(bad_block.calculate_hash());

        let err = mgr
            .inject_blocks_for_test(peer, vec![bad_block], Instant::now(), &mut local)
            .unwrap_err();

        assert_eq!(err, SyncError::WrongParentHash);
        let metrics = mgr.peer_metrics(&peer).unwrap();
        assert_eq!(metrics.invalid_blocks, 0);
    }

    #[test]
    fn sync_orphan_reorder() {
        let (_, blocks) = build_chain(4).unwrap();
        let peer = fake_peer();
        let mut mgr = SyncManager::new(0);
        mgr.record_peer_status(peer, 4);
        let mut local = Blockchain::default();

        let applied = mgr
            .inject_blocks_for_test(
                peer,
                vec![
                    blocks[2].clone(),
                    blocks[3].clone(),
                    blocks[0].clone(),
                    blocks[1].clone(),
                ],
                Instant::now(),
                &mut local,
            )
            .unwrap();

        assert_eq!(applied.len(), 4);
        assert_eq!(mgr.local_height(), 4);
        assert_eq!(mgr.orphan_count(), 0);
    }

    /// Helper used by the orphan-seed unit tests. Builds a height-1 block with
    /// `prev_block_hash = [0; 32]` whose hash is *guaranteed distinct* from
    /// `real_genesis_block.hash` by varying the timestamp.
    fn make_orphan_seed_distinct_from(real: &Block, offset_secs: i64) -> Block {
        let orphan = make_block(
            1,
            Hash::default(),
            real.timestamp + chrono::Duration::seconds(offset_secs),
        );
        assert_ne!(
            orphan.hash, real.hash,
            "orphan must have a distinct hash from the real height-1 block"
        );
        orphan
    }

    #[test]
    fn skip_orphan_seed_block_drops_seed_when_chain_empty_and_next_block_diverges() {
        let (_, real_blocks) = build_chain(3).unwrap();
        let orphan = make_orphan_seed_distinct_from(&real_blocks[0], -30);
        let mut batch = vec![
            orphan.clone(),
            real_blocks[1].clone(),
            real_blocks[2].clone(),
        ];
        let chain = Blockchain::default();

        let dropped = skip_orphan_seed_block(&chain, &mut batch);

        assert_eq!(
            dropped,
            Some(real_blocks[1].prev_block_hash),
            "orphan must be detected and the missing parent surfaced"
        );
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].height, 2, "real height 2 is now at index 0");
        assert_eq!(batch[1].height, 3);
    }

    #[test]
    fn skip_orphan_seed_block_preserves_seed_when_next_block_chains_correctly() {
        // Standard, valid IBD batch heights 1..=3 — must NOT be filtered.
        let (_, real_blocks) = build_chain(3).unwrap();
        let mut batch = real_blocks.clone();
        let chain = Blockchain::default();

        let dropped = skip_orphan_seed_block(&chain, &mut batch);

        assert_eq!(dropped, None, "valid genesis batch must be preserved");
        assert_eq!(batch.len(), 3);
    }

    #[test]
    fn skip_orphan_seed_block_noop_when_chain_already_has_blocks() {
        // If the local chain has already ingested any block, the orphan-seed
        // pattern cannot apply. This guards against accidentally filtering
        // a legitimate fork-point block during reorg ingestion.
        let (chain, blocks) = build_chain(2).unwrap();
        let orphan = make_orphan_seed_distinct_from(&blocks[0], -30);
        let mut batch = vec![orphan, blocks[1].clone()];

        let dropped = skip_orphan_seed_block(&chain, &mut batch);

        assert_eq!(
            dropped, None,
            "must not run when chain.blocks() is non-empty"
        );
        assert_eq!(batch.len(), 2);
    }

    #[test]
    fn skip_orphan_seed_block_noop_when_only_one_block_in_batch() {
        // Cannot prove a single block is an orphan without a successor.
        let (_, blocks) = build_chain(1).unwrap();
        let orphan = make_orphan_seed_distinct_from(&blocks[0], -30);
        let mut batch = vec![orphan];
        let chain = Blockchain::default();

        let dropped = skip_orphan_seed_block(&chain, &mut batch);

        assert_eq!(dropped, None, "single-block batch cannot trigger detection");
        assert_eq!(batch.len(), 1);
    }

    #[test]
    fn skip_orphan_seed_block_noop_when_height_above_one() {
        // Detection is gated on height ≤ 1. A height-5 block with a bad
        // parent hash is a different bug (genuine WrongParentHash) and
        // must keep the existing peer-penalty path.
        let (_, real_blocks) = build_chain(6).unwrap();
        let mut bad = make_block(
            5,
            Hash::default(),
            real_blocks[4].timestamp + chrono::Duration::seconds(1),
        );
        bad.set_hash(bad.calculate_hash());
        let mut batch = vec![bad, real_blocks[5].clone()];
        let chain = Blockchain::default();

        let dropped = skip_orphan_seed_block(&chain, &mut batch);

        assert_eq!(
            dropped, None,
            "height > 1 must not trigger orphan-seed detection"
        );
    }

    #[test]
    fn skip_orphan_seed_block_noop_when_prev_hash_not_genesis_zeros() {
        // A height-1 block with non-zero prev is malformed but is not the
        // orphan-seed pattern from #1466. Leave it for normal validation.
        let (_, real_blocks) = build_chain(2).unwrap();
        let mut bad = make_block(
            1,
            hash_bytes(b"not-genesis-zeros"),
            real_blocks[0].timestamp + chrono::Duration::seconds(1),
        );
        bad.set_hash(bad.calculate_hash());
        let mut batch = vec![bad, real_blocks[1].clone()];
        let chain = Blockchain::default();

        let dropped = skip_orphan_seed_block(&chain, &mut batch);

        assert_eq!(
            dropped, None,
            "non-zero prev_block_hash must not trigger orphan-seed detection"
        );
    }

    /// The response must survive the REAL codec, not just the state machine.
    ///
    /// Every other test in this file builds a `BlockResponse` in memory and
    /// hands it straight to `handle_response`; the only pre-existing codec
    /// round-trip covers the *request*. So the whole suite sat above the wire
    /// and stayed green while an ordinary `retry_after_ms: None` response was
    /// undecodable — bincode is not self-describing, and the original
    /// `skip_serializing_if` omitted the `Option` tag byte that the derived
    /// deserializer unconditionally reads. This drives `write_message` and
    /// `read_message` themselves so that boundary can never go untested again.
    #[tokio::test]
    async fn block_response_roundtrips_through_the_codec_2539() {
        for (label, retry_after_ms) in [("no-blocks", None), ("refusal", Some(1_500u64))] {
            let original = BlockResponse {
                blocks: Vec::new(),
                retry_after_ms,
            };

            let mut wire = Vec::new();
            write_message(&mut wire, &original)
                .await
                .unwrap_or_else(|e| panic!("{label}: write_message failed: {e}"));

            let mut cursor = libp2p::futures::io::Cursor::new(wire);
            let decoded: BlockResponse = read_message(&mut cursor)
                .await
                .unwrap_or_else(|e| panic!("{label}: read_message failed: {e}"));

            assert_eq!(
                decoded.retry_after_ms, retry_after_ms,
                "{label}: retry_after_ms must survive the codec unchanged"
            );
            assert!(decoded.blocks.is_empty(), "{label}: blocks must survive");
        }
    }

    #[test]
    fn refused_response_defers_range_until_wire_retry_deadline_2539() {
        let peer = fake_peer();
        let mut mgr = SyncManager::new(0);
        mgr.record_peer_status(peer, 4);
        let mut behaviour = RequestResponseBehaviour::with_codec(
            BlockSyncCodec,
            std::iter::once((StreamProtocol::new(BLOCK_SYNC_PROTOCOL), ProtocolSupport::Full)),
            Config::default(),
        );
        let request = mgr.plan_requests(Instant::now()).remove(0).request;
        let request_id = behaviour.send_request(&peer, request.clone());
        let now = Instant::now();
        mgr.register_request(request_id, peer, &request, now);
        let mut chain = Blockchain::default();

        assert!(mgr.handle_response(request_id, BlockResponse { blocks: Vec::new(), retry_after_ms: Some(10) }, now, &mut chain).unwrap().is_empty());
        assert!(mgr.plan_requests(now + Duration::from_millis(9)).is_empty(), "refusal must not be treated as an empty range and requeued immediately");
        assert!(!mgr.plan_requests(now + Duration::from_millis(11)).is_empty(), "range must become eligible after the advertised retry deadline");
    }

    /// The deferral must survive a change in refill ALIGNMENT, not just an
    /// exact re-generation of the held range.
    ///
    /// The first version of this fix suppressed a refilled range only when it
    /// equalled a deferred one. Refill alignment is derived from the high-water
    /// mark and `MAX_BLOCKS_PER_REQUEST`, so the moment the target moves — a
    /// second peer advertising a longer chain, which is the ordinary case
    /// during IBD — the regenerated range straddles the deferred one instead of
    /// matching it, and equality-only suppression lets it straight through.
    /// That resends the heights the peer just refused, before its deadline.
    #[test]
    fn a_deferred_range_is_not_resent_under_a_different_refill_alignment_2539() {
        let peer = fake_peer();
        let mut mgr = SyncManager::new(0);
        // Target 4 => the first plan plans exactly [1, 4].
        mgr.record_peer_status(peer, 4);
        let mut behaviour = RequestResponseBehaviour::with_codec(
            BlockSyncCodec,
            std::iter::once((StreamProtocol::new(BLOCK_SYNC_PROTOCOL), ProtocolSupport::Full)),
            Config::default(),
        );
        let request = mgr.plan_requests(Instant::now()).remove(0).request;
        assert_eq!((request.start_height, request.count), (1, 4), "precondition");
        let request_id = behaviour.send_request(&peer, request.clone());
        let now = Instant::now();
        mgr.register_request(request_id, peer, &request, now);
        let mut chain = Blockchain::default();

        // The peer refuses [1, 4] for 10s.
        assert!(
            mgr.handle_response(
                request_id,
                BlockResponse { blocks: Vec::new(), retry_after_ms: Some(10_000) },
                now,
                &mut chain,
            )
            .unwrap()
            .is_empty()
        );

        // A second peer now advertises a much longer chain. The target jumps,
        // so refill no longer wants a 4-block range — it wants a full
        // MAX_BLOCKS_PER_REQUEST one, which OVERLAPS [1, 4] without equalling it.
        mgr.record_peer_status(fake_peer(), 500);

        let planned = mgr.plan_requests(now + Duration::from_millis(1));
        for entry in &planned {
            let start = entry.request.start_height;
            let end = start + u64::from(entry.request.count) - 1;
            assert!(
                end < 1 || start > 4,
                "planned range {start}..={end} overlaps the range this peer \
                 refused until +10s; a deferral that only survives an exact \
                 alignment match is not a deferral"
            );
        }
    }

    /// Deferring a range must hold back THAT range, not the rest of the chain.
    ///
    /// This is why the deferred ends are folded into the high-water mark rather
    /// than the overlap check carrying the fix alone. Suppression alone would
    /// generate `[1, 100]`, drop the whole thing because it overlaps the held
    /// `[1, 4]`, and resume at 101 — silently skipping heights 5..=100 until the
    /// deadline expires. Including the deferred end instead moves the refill
    /// origin to 5, so exactly the four refused heights wait and everything
    /// above them is fetched immediately.
    #[test]
    fn a_deferral_holds_back_only_its_own_range_not_the_chain_behind_it_2539() {
        let peer = fake_peer();
        let mut mgr = SyncManager::new(0);
        mgr.record_peer_status(peer, 4);
        let mut behaviour = RequestResponseBehaviour::with_codec(
            BlockSyncCodec,
            std::iter::once((StreamProtocol::new(BLOCK_SYNC_PROTOCOL), ProtocolSupport::Full)),
            Config::default(),
        );
        let request = mgr.plan_requests(Instant::now()).remove(0).request;
        let request_id = behaviour.send_request(&peer, request.clone());
        let now = Instant::now();
        mgr.register_request(request_id, peer, &request, now);
        let mut chain = Blockchain::default();

        mgr.handle_response(
            request_id,
            BlockResponse { blocks: Vec::new(), retry_after_ms: Some(10_000) },
            now,
            &mut chain,
        )
        .unwrap();
        mgr.record_peer_status(fake_peer(), 500);

        let planned = mgr.plan_requests(now + Duration::from_millis(1));
        assert!(
            planned.iter().any(|entry| entry.request.start_height == 5),
            "the block immediately after the deferred range must be fetched \
             now, not after the refusal deadline: planned {:?}",
            planned
                .iter()
                .map(|e| (e.request.start_height, e.request.count))
                .collect::<Vec<_>>()
        );
    }

    /// Runtime #2764: a one-peer IBD consumed the server's rolling bandwidth
    /// allowance, then every lookahead range was refused against the same
    /// absolute window deadline. When that deadline elapsed, `plan_requests`
    /// moved the ready ranges back with `push_front` in their original order,
    /// reversing them. The first retry therefore started at the HIGHEST range.
    /// `ingest_block` stored that response as orphans without advancing local
    /// height, after which `handle_response` requeued the identical high range
    /// at the front. The requester selected it again indefinitely instead of
    /// closing the low gap. Restarting the requester cleared that in-memory
    /// queue state, which is why Mango resumed only after each restart.
    ///
    /// Drive the same shape through real request ids and sequential responses:
    /// one peer, four adjacent ranges, staggered retry durations sharing one
    /// deadline. The first post-deadline request must close the lowest gap.
    #[test]
    fn simultaneous_deferred_ranges_resume_from_lowest_gap_2764() {
        let peer = fake_peer();
        let mut mgr = SyncManager::new(0);
        mgr.record_peer_status(peer, 400);
        let mut behaviour = RequestResponseBehaviour::with_codec(
            BlockSyncCodec,
            std::iter::once((
                StreamProtocol::new(BLOCK_SYNC_PROTOCOL),
                ProtocolSupport::Full,
            )),
            Config::default(),
        );
        let base = Instant::now();
        let mut chain = Blockchain::default();

        for (index, elapsed_ms) in [0u64, 10, 20, 30].into_iter().enumerate() {
            let now = base + Duration::from_millis(elapsed_ms);
            let planned = mgr.plan_requests(now);
            assert_eq!(planned.len(), 1, "one peer can carry one range at a time");
            let request = planned[0].request.clone();
            assert_eq!(
                request.start_height,
                1 + index as u64 * u64::from(MAX_BLOCKS_PER_REQUEST),
                "pre-deadline planning must walk adjacent ranges in height order"
            );
            let request_id = behaviour.send_request(&peer, request.clone());
            mgr.register_request(request_id, peer, &request, now);
            mgr.handle_response(
                request_id,
                BlockResponse {
                    blocks: Vec::new(),
                    retry_after_ms: Some(100 - elapsed_ms),
                },
                now,
                &mut chain,
            )
            .expect("a refusal is a valid response");
        }

        // Different peers and network timing can deliver refusal responses in
        // any order. Deliberately disturb insertion order so the fix must sort,
        // not merely reverse today's one-peer arrival sequence.
        mgr.deferred_ranges.swap(0, 1);

        assert!(
            mgr.plan_requests(base + Duration::from_millis(99))
                .is_empty(),
            "none of the four ranges may wake before the shared deadline"
        );
        let resumed = mgr.plan_requests(base + Duration::from_millis(101));
        assert_eq!(resumed.len(), 1, "the one peer should resume autonomously");
        assert_eq!(
            resumed[0].request.start_height, 1,
            "after the bandwidth window, retry the LOWEST missing range first; \
             a fully orphaned higher response requeues the same range at the front \
             and starves the low gap"
        );
    }

    #[test]
    fn test_handle_response_halts_when_orphan_seed_masks_required_height_1_parent() {
        // Integration test reproducing #1466 end-to-end through
        // `handle_response`. A misconfigured seed serves an orphaned
        // height-1 block followed by heights 2..=4 of the live chain;
        // the live chain's height-2 prev_block_hash does NOT chain
        // onto the orphan. Without the fix, ingest_block applies the
        // orphan, then height 2 fails WrongParentHash, returning Err
        // for the entire response and stalling IBD permanently.
        // With #1482, the orphan is filtered before any ingestion
        // occurs and `handle_response` fails closed instead of
        // re-requesting the same non-converging range forever.
        let (_, real_blocks) = build_chain(4).unwrap(); // heights 1..=4
        let orphan = make_orphan_seed_distinct_from(&real_blocks[0], -30);
        assert_ne!(
            real_blocks[1].prev_block_hash,
            orphan.hash.unwrap(),
            "real height 2 must not chain onto the orphan — that is what proves the orphan is stale"
        );

        // Build the batch as it would arrive from a misconfigured seed.
        // Note: the real height-1 block (real_blocks[0]) is intentionally
        // omitted — the seed only has the stale orphan at height 1, just
        // as in the production bug on node host.
        let response = BlockResponse {
            blocks: vec![
                orphan.clone(),
                real_blocks[1].clone(),
                real_blocks[2].clone(),
                real_blocks[3].clone(),
            ],
            retry_after_ms: None,
        };

        let peer = fake_peer();
        let mut mgr = SyncManager::new(0);
        mgr.record_peer_status(peer, 4);

        // Register a real OutboundRequestId via the libp2p
        // request_response behaviour (matching the existing
        // `sync_peer_stall_reassignment` test pattern).
        let mut behaviour = RequestResponseBehaviour::with_codec(
            BlockSyncCodec,
            std::iter::once((
                StreamProtocol::new(BLOCK_SYNC_PROTOCOL),
                ProtocolSupport::Full,
            )),
            Config::default(),
        );
        let request = BlockRequest {
            start_height: 1,
            count: 4,
        };
        let request_id = behaviour.send_request(&peer, request.clone());
        mgr.register_request(request_id, peer, &request, Instant::now());

        let mut local = Blockchain::default();
        let pre_orphan_count = mgr.orphan_count();

        let err = mgr
            .handle_response(request_id, response, Instant::now(), &mut local)
            .expect_err("missing trusted height-1 parent must halt sync");

        assert_eq!(
            err,
            SyncError::MissingGenesisParent {
                required_parent: real_blocks[1].prev_block_hash
            }
        );

        // Heights 2..=4 cannot chain onto an empty local chain (the real
        // height-1 ancestor is missing from this batch). The KEY invariants
        // for #1482 are:
        //   * the main chain was NOT poisoned with the stale orphan
        //   * descendants are not accepted as peer-provided genesis material
        //   * the identical range is not immediately planned again
        assert_eq!(
            local.height, 0,
            "main chain must stay at height 0 — no orphan poisoning"
        );
        assert_eq!(
            local.tip,
            Hash::default(),
            "tip must remain the genesis-zeros sentinel"
        );
        assert_ne!(
            local.tip,
            orphan.hash.unwrap(),
            "main chain tip must NOT be the dropped orphan hash"
        );
        assert_eq!(
            mgr.orphan_count(),
            pre_orphan_count,
            "descendants without the trusted height-1 parent must not be buffered"
        );
        // #2036: the planner must NOT re-request the non-converging descendant
        // range [2..=4] (the #1482 hot-loop guard), but it also must NOT go
        // fully silent — it issues a bounded, single-block probe for the MISSING
        // PARENT height (height 1) so a cold-start node can autonomously recover
        // instead of stalling at 0 forever. Verify it probes exactly height 1,
        // count 1 — not the descendant range — and then backs off.
        let probe = mgr.plan_requests(Instant::now());
        assert_eq!(
            probe.len(),
            1,
            "latched planner must issue exactly one missing-parent probe"
        );
        assert_eq!(
            probe[0].request.start_height, 1,
            "probe must target the missing height-1 parent, not the descendant range"
        );
        assert_eq!(
            probe[0].request.count, 1,
            "probe must be a single-block parent probe, never the non-converging range"
        );
        assert!(
            mgr.plan_requests(Instant::now()).is_empty(),
            "the missing-parent probe must back off — no hot-loop within the retry interval"
        );

        let req2 = BlockRequest {
            start_height: 2,
            count: 3,
        };
        let request_id2 = behaviour.send_request(&peer, req2.clone());
        mgr.register_request(request_id2, peer, &req2, Instant::now());
        let later_err = mgr
            .handle_response(
                request_id2,
                BlockResponse {
                    blocks: vec![
                        real_blocks[1].clone(),
                        real_blocks[2].clone(),
                        real_blocks[3].clone(),
                    ],
                    retry_after_ms: None,
                },
                Instant::now(),
                &mut local,
            )
            .expect_err("parallel descendants must also fail closed after the halt");
        assert_eq!(
            later_err,
            SyncError::MissingGenesisParent {
                required_parent: real_blocks[1].prev_block_hash
            }
        );
        assert_eq!(
            mgr.orphan_count(),
            pre_orphan_count,
            "parallel descendants must not fill the orphan buffer after halt"
        );

        let req3 = BlockRequest {
            start_height: 1,
            count: 4,
        };
        let request_id3 = behaviour.send_request(&peer, req3.clone());
        mgr.register_request(request_id3, peer, &req3, Instant::now());
        let recovered = mgr
            .handle_response(
                request_id3,
                BlockResponse {
                    blocks: real_blocks.clone(),
                    retry_after_ms: None,
                },
                Instant::now(),
                &mut local,
            )
            .expect("exact missing height-1 parent should recover without restart");
        assert_eq!(
            recovered.len(),
            4,
            "the real height-1 parent and its descendants should apply"
        );
        assert_eq!(local.height, 4, "chain should recover to the peer tip");
        assert_eq!(mgr.local_height(), 4);
        assert_eq!(mgr.orphan_count(), 0);

        // The peer that served the orphan must NOT be penalised — this
        // is a stale-storage condition on the seed, not malicious behaviour,
        // and penalising would shrink the available peer set unnecessarily.
        let metrics = mgr.peer_metrics(&peer).expect("peer metrics present");
        assert_eq!(metrics.invalid_blocks, 0, "no peer penalty for stale seed");
    }

    #[test]
    fn latched_missing_parent_probe_retries_after_backoff_and_rotates_peers_2036() {
        // #2036 liveness: a node latched on a missing genesis parent must keep
        // probing — the backoff must RELEASE after the retry interval (so the
        // node is not silent forever), and the probe must reach whichever peer
        // is currently best (peer rotation), so one bad seed cannot pin it.
        let (_, real_blocks) = build_chain(4).unwrap();
        let orphan = make_orphan_seed_distinct_from(&real_blocks[0], -30);

        let peer_a = fake_peer();
        let mut mgr = SyncManager::new(0);
        mgr.record_peer_status(peer_a, 4);

        let mut behaviour = RequestResponseBehaviour::with_codec(
            BlockSyncCodec,
            std::iter::once((
                StreamProtocol::new(BLOCK_SYNC_PROTOCOL),
                ProtocolSupport::Full,
            )),
            Config::default(),
        );
        let request = BlockRequest {
            start_height: 1,
            count: 4,
        };
        let request_id = behaviour.send_request(&peer_a, request.clone());
        let t0 = Instant::now();
        mgr.register_request(request_id, peer_a, &request, t0);

        let mut local = Blockchain::default();
        let _ = mgr.handle_response(
            request_id,
            BlockResponse {
                blocks: vec![
                    orphan.clone(),
                    real_blocks[1].clone(),
                    real_blocks[2].clone(),
                    real_blocks[3].clone(),
                ],
                retry_after_ms: None,
            },
            t0,
            &mut local,
        );
        assert!(mgr.missing_genesis_parent().is_some(), "must be latched");

        // First probe fires immediately (no prior probe timestamp).
        let p1 = mgr.plan_requests(t0);
        assert_eq!(p1.len(), 1, "first latched tick must probe");
        assert_eq!(p1[0].request.start_height, 1);

        // Within the retry interval: silent (backoff holds).
        assert!(
            mgr.plan_requests(t0).is_empty(),
            "probe must back off within the retry interval"
        );

        // After the retry interval elapses: probe fires AGAIN — this is the
        // autonomous-retry guarantee that a silent-forever stall lacked.
        let t1 = t0 + MISSING_PARENT_RETRY_INTERVAL + Duration::from_millis(1);
        let p2 = mgr.plan_requests(t1);
        assert_eq!(p2.len(), 1, "probe must resume after the backoff interval");
        assert_eq!(p2[0].request.start_height, 1, "still probing the missing parent");

        // With no peer ahead, the probe is silent (nothing to ask) rather than
        // erroring or busy-looping.
        mgr.remove_peer(&peer_a);
        let t2 = t1 + MISSING_PARENT_RETRY_INTERVAL + Duration::from_millis(1);
        assert!(
            mgr.plan_requests(t2).is_empty(),
            "no peer ahead → no probe (still latched, awaiting a usable peer)"
        );
        assert!(
            mgr.missing_genesis_parent().is_some(),
            "still latched — losing the only peer does not clear the halt"
        );
    }

    #[test]
    fn sync_state_reports_stalled_after_missing_genesis_halt_and_clears_on_recovery() {
        // Regression for #1482: when `handle_response` halts because the
        // only height-1 candidate is the stale orphan seed and the
        // descendants cannot chain onto an empty local chain, the public
        // `SyncState` and `missing_genesis_parent()` getter must both
        // surface the halt with the required-parent hash. Once a later
        // batch carries the exact missing parent block, both signals
        // must clear without trusting any peer-supplied height-0
        // material.
        let (_, real_blocks) = build_chain(4).unwrap(); // heights 1..=4
        let orphan = make_orphan_seed_distinct_from(&real_blocks[0], -30);
        let required_parent = real_blocks[1].prev_block_hash;

        let peer = fake_peer();
        let mut mgr = SyncManager::new(0);
        mgr.record_peer_status(peer, 4);

        let mut behaviour = RequestResponseBehaviour::with_codec(
            BlockSyncCodec,
            std::iter::once((
                StreamProtocol::new(BLOCK_SYNC_PROTOCOL),
                ProtocolSupport::Full,
            )),
            Config::default(),
        );
        let request = BlockRequest {
            start_height: 1,
            count: 4,
        };
        let request_id = behaviour.send_request(&peer, request.clone());
        mgr.register_request(request_id, peer, &request, Instant::now());

        let mut local = Blockchain::default();
        let _ = mgr
            .handle_response(
                request_id,
                BlockResponse {
                    blocks: vec![
                        orphan.clone(),
                        real_blocks[1].clone(),
                        real_blocks[2].clone(),
                        real_blocks[3].clone(),
                    ],
                    retry_after_ms: None,
                },
                Instant::now(),
                &mut local,
            )
            .expect_err("missing trusted height-1 parent must halt sync");

        assert_eq!(
            *mgr.state(),
            SyncState::Stalled { required_parent },
            "public SyncState must surface the halt"
        );
        assert_eq!(
            mgr.missing_genesis_parent(),
            Some(required_parent),
            "missing-parent getter must surface the required hash"
        );

        // Peer churn must not clear `Stalled` — the halt is an unresolved
        // storage condition on the seed, not a peer-availability issue.
        mgr.remove_peer(&peer);
        assert_eq!(
            *mgr.state(),
            SyncState::Stalled { required_parent },
            "Stalled must survive remove_peer"
        );

        // Re-introduce a peer and serve the real chain (which carries the
        // exact missing height-1 parent). Halt must clear and state must
        // advance — without ever accepting peer-provided height-0 material.
        mgr.record_peer_status(peer, 4);
        let req2 = BlockRequest {
            start_height: 1,
            count: 4,
        };
        let request_id2 = behaviour.send_request(&peer, req2.clone());
        mgr.register_request(request_id2, peer, &req2, Instant::now());
        let _ = mgr
            .handle_response(
                request_id2,
                BlockResponse {
                    blocks: real_blocks.clone(),
                    retry_after_ms: None,
                },
                Instant::now(),
                &mut local,
            )
            .expect("exact missing parent should recover without restart");

        assert_eq!(
            mgr.missing_genesis_parent(),
            None,
            "missing-parent getter must clear on recovery"
        );
        assert!(
            !matches!(*mgr.state(), SyncState::Stalled { .. }),
            "state must leave Stalled after recovery, got {:?}",
            mgr.state(),
        );
    }

    #[test]
    fn sync_peer_stall_reassignment() {
        let mut mgr = SyncManager::new(0);
        let peer_a = fake_peer();
        let peer_b = fake_peer();
        mgr.record_peer_status(peer_a, 150);
        mgr.record_peer_status(peer_b, 150);

        let planned = mgr.plan_requests(Instant::now());
        assert!(!planned.is_empty());
        let mut behaviour = RequestResponseBehaviour::with_codec(
            BlockSyncCodec,
            std::iter::once((
                StreamProtocol::new(BLOCK_SYNC_PROTOCOL),
                ProtocolSupport::Full,
            )),
            Config::default(),
        );
        let request_id = behaviour.send_request(&planned[0].peer_id, planned[0].request.clone());
        mgr.register_request(
            request_id,
            planned[0].peer_id,
            &planned[0].request,
            Instant::now(),
        );

        let planned_after_timeout = mgr.plan_requests(Instant::now() + Duration::from_secs(11));
        assert!(!planned_after_timeout.is_empty());
        let stalled_peer = mgr.peer_metrics(&planned[0].peer_id).unwrap();
        assert_eq!(stalled_peer.stall_count, 1);
    }

    /// A transport failure must not immediately redispatch the failed range to
    /// the same peer while every alternate peer is busy (#2816).
    #[test]
    fn sync_transport_failure_waits_for_peer_rotation_2816() {
        let mut mgr = SyncManager::new(0);
        let peer_a = fake_peer();
        let peer_b = fake_peer();
        mgr.record_peer_status(peer_a, 150);
        mgr.record_peer_status(peer_b, 150);

        let now = Instant::now();
        let planned = mgr.plan_requests(now);
        assert_eq!(planned.len(), 2, "two ranges must occupy both eligible peers");

        let mut behaviour = RequestResponseBehaviour::with_codec(
            BlockSyncCodec,
            std::iter::once((
                StreamProtocol::new(BLOCK_SYNC_PROTOCOL),
                ProtocolSupport::Full,
            )),
            Config::default(),
        );
        let failed = &planned[0];
        let request_id = behaviour.send_request(&failed.peer_id, failed.request.clone());
        mgr.register_request(request_id, failed.peer_id, &failed.request, now);
        let still_busy = &planned[1];
        let busy_request_id =
            behaviour.send_request(&still_busy.peer_id, still_busy.request.clone());
        mgr.register_request(
            busy_request_id,
            still_busy.peer_id,
            &still_busy.request,
            now,
        );

        mgr.note_request_failure(request_id);

        assert!(
            mgr.plan_requests(now + Duration::from_millis(1)).is_empty(),
            "the failed peer must cool down instead of being selected again immediately"
        );
    }

    #[test]
    fn sync_transport_failure_rotates_to_available_peer_2816() {
        let mut mgr = SyncManager::new(0);
        let peer_a = fake_peer();
        let peer_b = fake_peer();
        mgr.record_peer_status(peer_a, 100);
        mgr.record_peer_status(peer_b, 100);

        let now = Instant::now();
        let first = mgr.plan_requests(now).remove(0);
        let mut behaviour = RequestResponseBehaviour::with_codec(
            BlockSyncCodec,
            std::iter::once((
                StreamProtocol::new(BLOCK_SYNC_PROTOCOL),
                ProtocolSupport::Full,
            )),
            Config::default(),
        );
        let request_id = behaviour.send_request(&first.peer_id, first.request.clone());
        mgr.register_request(request_id, first.peer_id, &first.request, now);
        mgr.note_request_failure_at(request_id, now);

        let retry = mgr.plan_requests(now + Duration::from_millis(1));
        assert_eq!(retry.len(), 1);
        assert_ne!(
            retry[0].peer_id, first.peer_id,
            "an available alternate peer must receive the failed range"
        );
        assert_eq!(mgr.liveness_metrics().peer_rotations, 1);
    }

    /// Live follow-up to #2816/#2817: during the a953130a producer cutover,
    /// the seed advertised the new target while the connected producer's
    /// ChainStatus remained at the follower's local height. After the seed
    /// timed out, the strict `advertised_height >= range.end` eligibility
    /// gate rejected the producer, so the same seed was retried until it
    /// disconnected. A connected peer caught up through the range start is a
    /// safe bounded probe target: a stale advertisement may understate what it
    /// can serve, while an honest behind peer will return a partial/empty
    /// response and remain subject to the existing retry controls.
    #[test]
    fn sync_transport_failure_probes_stale_alternate_at_local_tip() {
        let mut mgr = SyncManager::new(100);
        let advertised_ahead = fake_peer();
        let stale_alternate = fake_peer();
        mgr.record_peer_status(advertised_ahead, 108);
        mgr.record_peer_status(stale_alternate, 100);

        let now = Instant::now();
        let first = mgr.plan_requests(now).remove(0);
        assert_eq!(first.peer_id, advertised_ahead);
        assert_eq!(first.request.start_height, 101);
        assert_eq!(first.request.count, 8);

        let mut behaviour = RequestResponseBehaviour::with_codec(
            BlockSyncCodec,
            std::iter::once((
                StreamProtocol::new(BLOCK_SYNC_PROTOCOL),
                ProtocolSupport::Full,
            )),
            Config::default(),
        );
        let request_id = behaviour.send_request(&first.peer_id, first.request.clone());
        mgr.register_request(request_id, first.peer_id, &first.request, now);
        mgr.note_request_failure_at(request_id, now);

        let retry = mgr.plan_requests(now + Duration::from_millis(1));
        assert_eq!(retry.len(), 1, "a stale connected alternate must be probed");
        assert_eq!(retry[0].peer_id, stale_alternate);
        assert_eq!(retry[0].request, first.request);
        assert_eq!(mgr.liveness_metrics().peer_rotations, 1);
    }

    #[test]
    fn sync_transport_failure_does_not_probe_alternate_behind_range_parent() {
        let mut mgr = SyncManager::new(100);
        let advertised_ahead = fake_peer();
        let genuinely_behind = fake_peer();
        mgr.record_peer_status(advertised_ahead, 108);
        mgr.record_peer_status(genuinely_behind, 99);

        let now = Instant::now();
        let first = mgr.plan_requests(now).remove(0);
        assert_eq!(first.peer_id, advertised_ahead);
        assert_eq!(first.request.start_height, 101);

        let mut behaviour = RequestResponseBehaviour::with_codec(
            BlockSyncCodec,
            std::iter::once((
                StreamProtocol::new(BLOCK_SYNC_PROTOCOL),
                ProtocolSupport::Full,
            )),
            Config::default(),
        );
        let request_id = behaviour.send_request(&first.peer_id, first.request.clone());
        mgr.register_request(request_id, first.peer_id, &first.request, now);
        mgr.note_request_failure_at(request_id, now);

        assert!(
            mgr.plan_requests(now + Duration::from_millis(1)).is_empty(),
            "a peer below the range parent must not receive a speculative probe"
        );
        assert_eq!(mgr.liveness_metrics().peer_rotations, 0);
    }

    #[test]
    fn disconnected_ahead_peer_retains_failed_range_for_stale_alternate_rotation() {
        let mut mgr = SyncManager::new(100);
        let advertised_ahead = fake_peer();
        let stale_alternate = fake_peer();
        mgr.record_peer_status(advertised_ahead, 108);
        mgr.record_peer_status(stale_alternate, 100);

        let now = Instant::now();
        let first = mgr.plan_requests(now).remove(0);
        assert_eq!(first.peer_id, advertised_ahead);

        let mut behaviour = RequestResponseBehaviour::with_codec(
            BlockSyncCodec,
            std::iter::once((
                StreamProtocol::new(BLOCK_SYNC_PROTOCOL),
                ProtocolSupport::Full,
            )),
            Config::default(),
        );
        let request_id = behaviour.send_request(&first.peer_id, first.request.clone());
        mgr.register_request(request_id, first.peer_id, &first.request, now);

        // Exercise the event ordering that failed in the serialized suite:
        // ConnectionClosed removes the ahead peer before retry planning.
        mgr.remove_peer_at(&advertised_ahead, now + Duration::from_millis(1));
        assert!(mgr.peer_metrics(&advertised_ahead).is_none());
        assert_eq!(mgr.retained_peer_health_count(), 1);

        let retry = mgr.plan_requests(now + Duration::from_millis(2));
        assert_eq!(retry.len(), 1);
        assert_eq!(retry[0].peer_id, stale_alternate);
        assert_eq!(retry[0].request, first.request);
        assert_eq!(mgr.liveness_metrics().peer_rotations, 1);
    }

    #[test]
    fn sync_retry_delay_is_deterministic_exponential_and_capped_2816() {
        let peer = fake_peer();
        let delays = (1..=8)
            .map(|failures| sync_retry_delay(peer, failures))
            .collect::<Vec<_>>();

        assert_eq!(delays[0], sync_retry_delay(peer, 1));
        assert!(delays[0] >= SYNC_RETRY_BASE_DELAY);
        assert!(delays.windows(2).all(|pair| pair[1] >= pair[0]));
        assert!(delays.iter().all(|delay| *delay <= SYNC_RETRY_MAX_DELAY));
        assert_eq!(
            sync_retry_delay(peer, u32::MAX),
            SYNC_RETRY_MAX_DELAY,
            "failure storms must remain bounded by the configured cap"
        );
    }

    /// Disconnect/reconnect must not erase transport-failure state and make a
    /// timed-out peer immediately eligible again (#2816).
    #[test]
    fn sync_transport_health_survives_reconnect_2816() {
        let mut mgr = SyncManager::new(0);
        let peer = fake_peer();
        mgr.record_peer_status(peer, 100);

        let now = Instant::now();
        let planned = mgr.plan_requests(now);
        assert_eq!(planned.len(), 1);
        let mut behaviour = RequestResponseBehaviour::with_codec(
            BlockSyncCodec,
            std::iter::once((
                StreamProtocol::new(BLOCK_SYNC_PROTOCOL),
                ProtocolSupport::Full,
            )),
            Config::default(),
        );
        let request_id = behaviour.send_request(&peer, planned[0].request.clone());
        mgr.register_request(request_id, peer, &planned[0].request, now);
        mgr.note_request_failure(request_id);
        assert_eq!(mgr.peer_metrics(&peer).expect("known peer").stall_count, 1);

        mgr.remove_peer(&peer);
        mgr.record_peer_status(peer, 100);

        assert_eq!(
            mgr.peer_metrics(&peer)
                .expect("reconnected peer must be observable")
                .stall_count,
            1,
            "sync-local transport history must survive reconnect"
        );
        assert!(
            mgr.plan_requests(now + Duration::from_millis(1)).is_empty(),
            "reconnect must not bypass the peer's retry cooldown"
        );
    }

    #[test]
    fn disconnect_records_in_flight_failure_before_reconnect_2816() {
        let mut mgr = SyncManager::new(0);
        let peer = fake_peer();
        let now = Instant::now();
        mgr.record_peer_status_at(peer, 100, now);
        let planned = mgr.plan_requests(now).remove(0);
        let mut behaviour = RequestResponseBehaviour::with_codec(
            BlockSyncCodec,
            std::iter::once((
                StreamProtocol::new(BLOCK_SYNC_PROTOCOL),
                ProtocolSupport::Full,
            )),
            Config::default(),
        );
        let request_id = behaviour.send_request(&peer, planned.request.clone());
        mgr.register_request(request_id, peer, &planned.request, now);

        mgr.remove_peer_at(&peer, now + Duration::from_millis(10));
        mgr.record_peer_status_at(peer, 100, now + Duration::from_millis(20));

        let metrics = mgr.peer_metrics(&peer).expect("reconnected peer");
        assert_eq!(metrics.stall_count, 1);
        assert_eq!(metrics.consecutive_transport_failures, 1);
        assert!(metrics.cooling_down);
    }

    #[test]
    fn disconnected_sync_health_is_ttl_pruned_and_capped_2816() {
        let now = Instant::now();
        let mut capped = SyncManager::new(0);
        for offset in 0..=MAX_RETAINED_SYNC_PEER_HEALTH {
            let peer = fake_peer();
            let at = now + Duration::from_millis(offset as u64);
            capped.record_peer_status_at(peer, 1, at);
            capped.remove_peer_at(&peer, at);
        }
        assert_eq!(
            capped.retained_peer_health_count(),
            MAX_RETAINED_SYNC_PEER_HEALTH,
            "disconnected peer churn must not create an unbounded health map"
        );

        let mut expiring = SyncManager::new(0);
        let peer = fake_peer();
        expiring.record_peer_status_at(peer, 100, now);
        let planned = expiring.plan_requests(now).remove(0);
        let mut behaviour = RequestResponseBehaviour::with_codec(
            BlockSyncCodec,
            std::iter::once((
                StreamProtocol::new(BLOCK_SYNC_PROTOCOL),
                ProtocolSupport::Full,
            )),
            Config::default(),
        );
        let request_id = behaviour.send_request(&peer, planned.request.clone());
        expiring.register_request(request_id, peer, &planned.request, now);
        expiring.note_request_failure_at(request_id, now);
        expiring.remove_peer_at(&peer, now);
        assert_eq!(expiring.retained_peer_health_count(), 1);

        expiring.record_peer_status_at(
            peer,
            100,
            now + RETAINED_SYNC_PEER_HEALTH_TTL + Duration::from_millis(1),
        );
        assert_eq!(
            expiring.peer_metrics(&peer).expect("reconnected peer").stall_count,
            0,
            "expired transport history must not penalize a peer forever"
        );
    }

    /// Request lifecycle counters are the anti-vacuity seam for the eventual
    /// three-node regression: a catch-up PASS is insufficient unless the test
    /// can prove dispatch, failure, and requeue paths actually executed.
    #[test]
    fn sync_liveness_metrics_prove_failed_request_lifecycle_2816() {
        let mut mgr = SyncManager::new(0);
        let peer = fake_peer();
        mgr.record_peer_status(peer, 100);
        assert_eq!(mgr.liveness_metrics(), SyncLivenessMetrics::default());

        let now = Instant::now();
        let planned = mgr.plan_requests(now);
        assert_eq!(planned.len(), 1);
        let mut behaviour = RequestResponseBehaviour::with_codec(
            BlockSyncCodec,
            std::iter::once((
                StreamProtocol::new(BLOCK_SYNC_PROTOCOL),
                ProtocolSupport::Full,
            )),
            Config::default(),
        );
        let request_id = behaviour.send_request(&peer, planned[0].request.clone());
        mgr.register_request(request_id, peer, &planned[0].request, now);
        mgr.note_request_failure(request_id);

        let metrics = mgr.liveness_metrics();
        assert_eq!(metrics.requests_dispatched, 1);
        assert_eq!(metrics.transport_failures, 1);
        assert_eq!(metrics.ranges_requeued, 1);
        assert_eq!(metrics.responses_received, 0);
    }

    #[tokio::test]
    async fn read_message_rejects_oversized_length_prefix() {
        let oversized_len = (MAX_SYNC_MESSAGE_BYTES as u32 + 1).to_be_bytes();
        let mut buf = Vec::new();
        buf.extend_from_slice(&oversized_len);
        buf.extend_from_slice(&[0u8; 64]);

        let mut cursor = libp2p::futures::io::Cursor::new(buf);
        let result: io::Result<BlockRequest> = read_message(&mut cursor).await;
        let err = result.expect_err("oversized length prefix must be rejected");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(
            err.to_string().contains("exceeds maximum"),
            "error should mention the limit, got: {err}"
        );
    }

    /// #2884: an intact frame (length header honored, ceiling passed) whose
    /// body fails bincode decode must name VERSION SKEW as the candidate
    /// cause. The body used here is the authentic skew shape from the
    /// #2727-class audit: a pre-`retry_after_ms` `BlockResponse` — just an
    /// empty `Vec<Block>` — which the current two-field layout reads to EOF.
    /// Before this error text existed, the operator-visible failure was an
    /// anonymous InvalidData on the request_response channel, attributable to
    /// nothing.
    #[tokio::test]
    async fn read_message_decode_failure_names_version_skew() {
        // Old-layout BlockResponse: one field, `blocks: Vec<Block>`, empty.
        let payload = bincode::serialize(&Vec::<Block>::new()).unwrap();
        let mut buf = Vec::new();
        buf.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        buf.extend_from_slice(&payload);

        let mut cursor = libp2p::futures::io::Cursor::new(buf);
        let result: io::Result<BlockResponse> = read_message(&mut cursor).await;
        let err = result.expect_err("old-layout body must fail to decode");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        let text = err.to_string();
        assert!(
            text.contains("VERSION SKEW") && text.contains("#2884"),
            "decode failure must name version skew as the candidate cause, got: {text}"
        );
    }

    #[tokio::test]
    async fn read_message_accepts_valid_message() {
        let request = BlockRequest {
            start_height: 42,
            count: 10,
        };
        let payload = bincode::serialize(&request).unwrap();
        let mut buf = Vec::new();
        buf.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        buf.extend_from_slice(&payload);

        let mut cursor = libp2p::futures::io::Cursor::new(buf);
        let decoded: BlockRequest = read_message(&mut cursor)
            .await
            .expect("valid message should decode");
        assert_eq!(decoded.start_height, 42);
        assert_eq!(decoded.count, 10);
    }

    #[test]
    fn height_range_len_normal() {
        let range = HeightRange { start: 5, end: 10 };
        assert_eq!(range.len(), 6);
    }

    #[test]
    fn height_range_len_inverted_returns_zero() {
        let range = HeightRange { start: 10, end: 5 };
        assert_eq!(range.len(), 0);
    }

    #[test]
    fn height_range_len_equal_returns_one() {
        let range = HeightRange { start: 7, end: 7 };
        assert_eq!(range.len(), 1);
    }

    /// The IBD shadow is context-free and must forward a stateful decision to
    /// authoritative application. The launch API owns the real station,
    /// commit, oracle registry and quorum state and rejects this block there.
    #[test]
    fn sync_ibd_forwards_real_reveal_for_authoritative_quorum_validation_2786() {
        use wrfcoin_shared_protocol::consensus::proof_of_publishing::RICH_AMBIENT_REWARD_ACTIVATION_HEIGHT;

        let activation = RICH_AMBIENT_REWARD_ACTIVATION_HEIGHT;
        let mut chain = Blockchain::default();
        chain.height = activation.saturating_sub(1);
        chain.state.oracle_quorum = 2;

        let station_id = "station-real-2777-ibd";
        let timestamp = 1_700_000_000;
        let nonce = "nonce-2777-ibd";
        let metrics = serde_json::json!({
            "temperature_c": 21.5,
            "humidity_pct": 50.0,
        });
        let commit_id = Transaction::compute_commit_id(
            &metrics,
            nonce,
            station_id,
            timestamp,
            "contribution",
        );
        chain
            .state
            .station_registry
            .register(WeatherStation {
                station_id: station_id.to_string(),
                owner_address: "0x1111111111111111111111111111111111111111".to_string(),
                location: StationLocation {
                    latitude: 40.0,
                    longitude: -75.0,
                    elevation_meters: Some(100.0),
                },
                hardware_model: "ibd-boundary-fixture".to_string(),
                calibration_date: None,
                is_active: true,
                reputation_score: 100,
                data_quality_tier: DataQualityTier::Gold,
                provenance_class: ContributionProvenanceClass::Real,
                ring_public_key: None,
            })
            .unwrap();
        chain
            .state
            .apply_transition(StateTransition::RegisterCommit {
                payload: CommitPayload {
                    commit_id: commit_id.clone(),
                    station_id: station_id.to_string(),
                    timestamp,
                    contribution_type: "contribution".to_string(),
                    auth_pubkey: None,
                    auth_signature: None,
                },
                block_height: activation.saturating_sub(1),
            })
            .unwrap();

        let reveal = RevealPayload {
            commit_id,
            station_id: station_id.to_string(),
            timestamp,
            contribution_type: "contribution".to_string(),
            data: metrics,
            nonce: nonce.to_string(),
            data_cid: None,
            data_hash: None,
            storage_provider: None,
            provenance: None,
            manifest: None,
            verification_receipt: None,
            compute_receipt: None,
            quality_tier: None,
        };
        let mut reveal_tx = Transaction::new(Vec::new(), Vec::new());
        reveal_tx.tx_type = TransactionType::ContributionReveal;
        reveal_tx.timestamp = chrono::DateTime::from_timestamp(timestamp, 0).unwrap();
        reveal_tx.data = Some(serde_json::to_string(&reveal).unwrap());
        reveal_tx.set_id();

        let mut block = Block::new(1, chain.tip, vec![reveal_tx], 1, activation);
        block.timestamp = Utc::now();
        block.set_hash(block.calculate_hash());

        let original_height = chain.height;
        let peer = fake_peer();
        let mut mgr = SyncManager::new(original_height);
        mgr.record_peer_status(peer, activation);
        let mut applied = Vec::new();
        let result = mgr.ingest_block(
            peer,
            block,
            Instant::now(),
            &mut chain,
            &mut applied,
        );

        result.expect("context-free IBD must forward stateful quorum decisions");
        assert_eq!(
            chain.height,
            original_height + 1,
            "context-free shadow must advance after forwarding the block"
        );
        assert_eq!(
            applied.len(),
            1,
            "block must be reported once for authoritative application"
        );

        mgr.note_authoritative_rejection(
            &peer,
            original_height,
            AuthoritativeSyncRejectionAttribution::PeerInvalid,
        );
        assert_eq!(mgr.local_height(), original_height);
        assert_eq!(
            mgr.peer_metrics(&peer).unwrap().invalid_blocks,
            1,
            "authoritative rejection must remain attributed to the response peer"
        );
        let retry = mgr.plan_requests(Instant::now());
        assert_eq!(retry.len(), 1, "the rejected height must be replanned");
        assert_eq!(retry[0].request.start_height, original_height + 1);
    }

    #[test]
    fn local_authoritative_failures_replan_without_penalizing_seed_2788() {
        let peer = fake_peer();
        let accepted_height = 41;
        let mut mgr = SyncManager::new(accepted_height);
        mgr.record_peer_status(peer, accepted_height + 1);

        for _ in 0..3 {
            mgr.note_authoritative_rejection(
                &peer,
                accepted_height,
                AuthoritativeSyncRejectionAttribution::LocalFailure,
            );
        }

        let metrics = mgr
            .peer_metrics(&peer)
            .expect("seed metrics must remain available");
        assert_eq!(
            metrics.invalid_blocks, 0,
            "local failures must not blame the seed"
        );
        assert!(
            !metrics.banned,
            "repeated local failures must not ban an honest seed"
        );
        let retry = mgr.plan_requests(Instant::now());
        assert_eq!(
            retry.len(), 1,
            "local failure must still replan the rejected height"
        );
        assert_eq!(retry[0].peer_id, peer);
        assert_eq!(retry[0].request.start_height, accepted_height + 1);
    }

    #[test]
    fn delayed_success_ack_cannot_regress_download_height() {
        let peer = fake_peer();
        let mut mgr = SyncManager::new(28_866);
        mgr.record_peer_status(peer, 28_867);

        mgr.set_local_height(28_839);

        assert_eq!(mgr.local_height(), 28_866);
        let planned = mgr.plan_requests(Instant::now());
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].request.start_height, 28_867);
    }

    #[test]
    fn peer_invalid_authoritative_failures_increment_and_ban_2788() {
        let peer = fake_peer();
        let accepted_height = 41;
        let mut mgr = SyncManager::new(accepted_height);
        mgr.record_peer_status(peer, accepted_height + 1);

        for expected in 1..=3 {
            mgr.note_authoritative_rejection(
                &peer,
                accepted_height,
                AuthoritativeSyncRejectionAttribution::PeerInvalid,
            );
            assert_eq!(
                mgr.peer_metrics(&peer).unwrap().invalid_blocks,
                expected,
                "each peer-attributable rejection must count exactly once"
            );
        }
        assert!(mgr.peer_metrics(&peer).unwrap().banned);
        assert!(
            mgr.plan_requests(Instant::now()).is_empty(),
            "a banned peer must not receive the replanned request"
        );
    }

    /// #2807 RED regression: once every tracked peer is banned, the manager
    /// sits in `Discovering` with pending ranges and dispatches nothing, at
    /// any time horizon — the exact starvation observed live on 2026-08-16
    /// (`sync_ranges{kind="in_flight"} 0`, `{kind="pending"} 17`, forever).
    /// Starving with pending work and connected peers is never better than a
    /// bounded last-resort retry, so at least one request must eventually be
    /// dispatched even when the whole candidate set is banned.
    #[test]
    fn banned_peer_set_with_pending_work_must_not_starve_forever_2807() {
        let peer_a = fake_peer();
        let peer_b = fake_peer();
        let t0 = Instant::now();
        let mut mgr = SyncManager::new(0);
        mgr.record_peer_status_at(peer_a, 60, t0);
        mgr.record_peer_status_at(peer_b, 60, t0);

        // Ban both peers through genuine peer-attributable strikes.
        for _ in 0..3 {
            mgr.note_authoritative_rejection(
                &peer_a,
                0,
                AuthoritativeSyncRejectionAttribution::PeerInvalid,
            );
            mgr.note_authoritative_rejection(
                &peer_b,
                0,
                AuthoritativeSyncRejectionAttribution::PeerInvalid,
            );
        }
        assert!(mgr.peer_metrics(&peer_a).unwrap().banned);
        assert!(mgr.peer_metrics(&peer_b).unwrap().banned);

        // Gossip keeps reporting the network tip, exactly as on the wedged
        // follower (blocks kept arriving and were buffered as orphans).
        mgr.note_observed_height(60);
        assert!(
            mgr.pending_range_count() > 0,
            "the gossip hint must refill pending ranges (control: the wedge \
             is a dispatch gap, not a planning gap)"
        );

        // Two simulated hours of planning ticks.
        let mut dispatched = 0;
        for tick in 0..240u64 {
            let now = t0 + Duration::from_secs(30 * tick);
            dispatched += mgr.plan_requests(now).len();
        }
        assert!(
            dispatched > 0,
            "an all-banned peer set with pending work must not starve sync \
             forever; a bounded last-resort dispatch must eventually fire (#2807)"
        );
    }

    /// #2807 feature coverage: a rule rejection from ONE peer only rotates
    /// (cooldown, no ban, no halt); a second DISTINCT peer serving
    /// rule-failing data at the same height corroborates canonical
    /// divergence and latches the loud `ValidationHalted` state — with no
    /// peer banned and planning quiesced. Local height passing the halted
    /// block clears the latch.
    #[test]
    fn corroborated_rule_rejection_halts_loudly_without_banning_2807() {
        let peer_a = fake_peer();
        let peer_b = fake_peer();
        let t0 = Instant::now();
        let mut mgr = SyncManager::new(10);
        mgr.record_peer_status_at(peer_a, 60, t0);
        mgr.record_peer_status_at(peer_b, 60, t0);

        mgr.note_rule_rejection_at(&peer_a, 11, t0);
        assert!(
            mgr.validation_halt().is_none(),
            "a single serving peer must not corroborate a halt — it may be lying"
        );
        assert!(
            mgr.peer_metrics(&peer_a).unwrap().cooling_down,
            "the first server must be rotated away from via cooldown"
        );
        assert!(!mgr.peer_metrics(&peer_a).unwrap().banned);

        mgr.note_rule_rejection_at(&peer_b, 11, t0);
        let halt = mgr
            .validation_halt()
            .expect("two distinct servers must corroborate the halt");
        assert_eq!(halt.height, 11);
        assert_eq!(halt.distinct_peers, 2);
        assert_eq!(mgr.state().label(), "validation_halted");
        assert!(!mgr.peer_metrics(&peer_a).unwrap().banned);
        assert!(!mgr.peer_metrics(&peer_b).unwrap().banned);
        assert!(
            mgr.plan_requests(t0 + Duration::from_secs(600)).is_empty(),
            "re-downloading cannot cure a deterministic rejection; planning \
             must quiesce while halted"
        );

        // Gossip must not flap the halt state.
        mgr.note_observed_height(90);
        assert_eq!(mgr.state().label(), "validation_halted");

        // The halted block landing through another path clears the latch.
        mgr.set_local_height(11);
        assert!(mgr.validation_halt().is_none());
        assert_ne!(mgr.state().label(), "validation_halted");
    }

    #[test]
    fn first_retry_after_authoritative_rewind_checks_parent_and_replans_2788() {
        let peer = fake_peer();
        let accepted_height = 41;
        let accepted_tip = hash_bytes(b"accepted-authoritative-tip");
        let mut chain = Blockchain::default();
        chain.height = accepted_height;
        chain.tip = accepted_tip;
        assert!(
            chain.blocks().is_empty(),
            "rewound shadow intentionally has no block window"
        );

        let mut mgr = SyncManager::new(accepted_height);
        mgr.record_peer_status(peer, accepted_height + 1);
        mgr.note_authoritative_rejection(
            &peer,
            accepted_height,
            AuthoritativeSyncRejectionAttribution::LocalFailure,
        );

        let planned = mgr.plan_requests(Instant::now());
        assert_eq!(planned.len(), 1, "rewind must plan the first retry");
        let request = planned[0].request.clone();
        let mut behaviour = RequestResponseBehaviour::with_codec(
            BlockSyncCodec,
            std::iter::once((
                StreamProtocol::new(BLOCK_SYNC_PROTOCOL),
                ProtocolSupport::Full,
            )),
            Config::default(),
        );
        let request_id = behaviour.send_request(&peer, request.clone());
        let now = Instant::now();
        mgr.register_request(request_id, peer, &request, now);

        let wrong_parent = make_block(
            accepted_height + 1,
            hash_bytes(b"wrong-retry-parent"),
            Utc::now(),
        );
        let result = mgr.handle_response(
            request_id,
            BlockResponse {
                blocks: vec![wrong_parent],
                retry_after_ms: None,
            },
            now,
            &mut chain,
        );
        assert!(
            matches!(&result, Err(SyncError::WrongParentHash)),
            "first retry with the wrong authoritative parent must fail closed: {result:?}"
        );
        assert_eq!(mgr.peer_metrics(&peer).unwrap().invalid_blocks, 1);
        let retry = mgr.plan_requests(Instant::now());
        assert_eq!(retry.len(), 1, "wrong-parent retry must be replanned");
        assert_eq!(retry[0].request.start_height, accepted_height + 1);
    }

    #[test]
    fn sync_accepts_genesis_era_future_block() {
        // At genesis (height 1), max_future_drift = 14400/4 = 3600s.
        // A block 1800s in the future should be accepted.
        let (chain, _) = build_chain(0).unwrap();
        let mut mgr = SyncManager::new(0);
        let prev_hash = hash_bytes(b"genesis");

        let block = make_block(1, prev_hash, Utc::now() + chrono::Duration::seconds(1800));
        let result = mgr.validate_block(&chain, &block);
        assert!(result.is_ok(), "genesis-era block 1800s in future should be accepted, got: {:?}", result);
    }

    #[test]
    fn sync_rejects_far_future_at_terminal_height() {
        // At height 10_000_000 (terminal), max_future_drift = max(15, 1/4) = 15s.
        // A block 60s in the future should be rejected.
        let (mut chain, _) = build_chain(0).unwrap();
        let prev_hash = hash_bytes(b"genesis");
        // The sync worker's `Blockchain` is a context-free shadow: `UpdateLocalChainState`
        // gives it height AND tip, never block bodies (#2788). Setting height alone left
        // this fixture in a state the shadow never occupies, and #2788 widened the
        // parent-hash guard to `chain.height > 0 || !blocks().is_empty()` — so the block
        // was rejected with `WrongParentHash` before the drift rule under test was reached.
        // Setting `tip` restores the shape #2788's own rewind test uses.
        chain.height = 9_999_999;
        chain.tip = prev_hash;
        let mut mgr = SyncManager::new(9_999_999);

        let block = make_block(10_000_000, prev_hash, Utc::now() + chrono::Duration::seconds(60));
        let result = mgr.validate_block(&chain, &block);
        assert_eq!(result, Err(SyncError::TimestampTooFarInFuture));
    }

    #[test]
    fn sync_accepts_near_future_at_terminal_height() {
        // At height 10_000_000 (terminal), max_future_drift = 15s.
        // A block 10s in the future should be accepted.
        let (mut chain, _) = build_chain(0).unwrap();
        let prev_hash = hash_bytes(b"genesis");
        // Same shadow-shape fixture as the far-future case above (#2788). Together the two
        // tests are a control pair on one fixture: this one proves the fixture reaches the
        // drift rule at all, which is what stops the rejecting test from being satisfied by
        // an unrelated early error.
        chain.height = 9_999_999;
        chain.tip = prev_hash;
        let mut mgr = SyncManager::new(9_999_999);

        let block = make_block(10_000_000, prev_hash, Utc::now() + chrono::Duration::seconds(10));
        let result = mgr.validate_block(&chain, &block);
        assert!(result.is_ok(), "terminal block 10s in future should be accepted, got: {:?}", result);
    }

    /// Regression test for #1664: after IBD completes against the initial
    /// target_height, a subsequent `record_peer_status` call advancing
    /// target_height (e.g. from a `ChainStatus` gossip from a peer that has
    /// mined more blocks while IBD was running) MUST cause the next
    /// `plan_requests` call to emit a `PlannedRequest` covering the new
    /// gap.
    ///
    /// On node host / desktop two-node testnet (issue #1664), the desktop peer
    /// would IBD cleanly to h=6000 (60 × MAX_BLOCKS_PER_REQUEST = the
    /// original `target_height` at the moment IBD started), then stall
    /// permanently even though N5 advanced to h=6283+ and broadcast both
    /// `ChainStatus` and `NewBlock` gossip. Every subsequent `KickSync` /
    /// `plan_requests` returned an empty plan because either the
    /// pending-range refill never re-ran, or it ran but found no
    /// assignable range.
    #[test]
    fn plans_new_range_after_ibd_target_grows_post_completion() {
        let peer = fake_peer();
        let mut mgr = SyncManager::new(0);

        // Phase 1: IBD start — peer advertises h=6000, mgr plans 60 ranges.
        mgr.record_peer_status(peer, 6000);
        assert_eq!(mgr.target_height(), 6000);
        let planned = mgr.plan_requests(Instant::now());
        // Only one peer available — at most one request can be dispatched
        // in parallel because `select_best_peer_for_range` rejects busy
        // peers. The shape that matters is "≥1 in-flight against this
        // peer", not exactly MAX_PARALLEL_PEERS.
        assert!(
            !planned.is_empty(),
            "IBD should immediately dispatch at least one request"
        );

        // Phase 2: simulate IBD completion. The real code path is
        // `handle_response` → `ingest_block` writing `chain.add_block`
        // then `self.local_height = chain.height`. The in-flight entries
        // are removed as each response lands; for this regression we just
        // model the post-IBD steady state by clearing in_flight + pending
        // and setting local_height directly. The external launch_api side
        // then calls `set_local_height(6000)` via `UpdateLocalChainState`.
        mgr.in_flight.clear();
        mgr.pending_ranges.clear();
        mgr.local_height = 6000;
        mgr.set_local_height(6000);
        // With target_height == local_height == 6000, set_local_height
        // legitimately marks the manager as Complete — this is the state
        // the desktop ended in after IBD.
        assert_eq!(*mgr.state(), SyncState::Complete);

        // Phase 3: peer advances past the original IBD target. This models
        // a `ChainStatus` gossip from N5 (which kept mining during/after
        // the desktop's IBD). After this call, the next `plan_requests`
        // MUST emit a real PlannedRequest for the 6001..=6283 gap.
        mgr.record_peer_status(peer, 6283);
        assert_eq!(
            mgr.target_height(),
            6283,
            "target_height must grow when peer advertises a higher tip"
        );
        assert_ne!(
            *mgr.state(),
            SyncState::Complete,
            "manager must exit Complete once a peer goes ahead of local_height"
        );

        // Critical assertion for #1664: the next plan_requests must NOT
        // be empty. The original bug was that this call returned [].
        let planned_after_growth = mgr.plan_requests(Instant::now());
        assert!(
            !planned_after_growth.is_empty(),
            "plan_requests must dispatch after target_height grows post-IBD; \
             this is the #1664 stall: desktop stuck at 6000, N5 at 6283+, \
             plan_requests returned empty forever"
        );
        let first = &planned_after_growth[0];
        assert_eq!(
            first.request.start_height, 6001,
            "first request must cover the immediate gap above local_height"
        );
        assert!(
            first.request.count > 0,
            "first request must request at least one block"
        );
        assert_eq!(first.peer_id, peer, "must dispatch against the only known peer");
    }

    /// Regression test for the exact #1664 production stall. The desktop
    /// peer finishes IBD against the initial `target_height` learned from
    /// a single early `ChainStatus` broadcast. By the time IBD completes,
    /// the next `ChainStatus` from the producer is still seconds away
    /// (the broadcast interval at h=6000 is ~20s and at lower heights up
    /// to 300s — see `chain_status_broadcast_interval`). During that gap,
    /// the producer mines new blocks and gossips them via `NewBlock`,
    /// which lands in the desktop's orphan buffer and triggers a
    /// `KickSync` — but `target_height` is still stuck at the original
    /// IBD ceiling so `refill_pending_ranges` produces zero work.
    ///
    /// Without `note_observed_height`, this test reproduces the stall:
    /// `plan_requests` returns empty even though a peer is at the
    /// observed height. With the fix, `note_observed_height(observed)`
    /// before `plan_requests` causes the next dispatch to cover the gap.
    #[test]
    fn note_observed_height_recovers_from_stalled_ibd_ceiling() {
        let peer = fake_peer();
        let mut mgr = SyncManager::new(0);

        // Phase 1: model IBD start with target=6000 (initial ChainStatus).
        mgr.record_peer_status(peer, 6000);
        let _ = mgr.plan_requests(Instant::now());

        // Phase 2: model IBD completion. Local height = original target.
        // The producer's next ChainStatus has NOT yet arrived — so the
        // peer_chains entry is still height=6000 and target_height is
        // still 6000.
        mgr.in_flight.clear();
        mgr.pending_ranges.clear();
        mgr.local_height = 6000;
        mgr.set_local_height(6000);
        assert_eq!(*mgr.state(), SyncState::Complete);
        assert_eq!(mgr.target_height(), 6000);

        // Phase 3: the producer has been mining. A NewBlock at h=6283
        // arrives via gossipsub. p2p_events.rs sees block.height
        // (6283) > local_height + 1 (6001) and buffers it as an orphan,
        // then sends KickSync. WITHOUT the fix, KickSync calls
        // `plan_requests` which calls `refill_pending_ranges` — but
        // target_height (6000) <= local_height (6000), early return.
        // Result: empty plan. Permanent stall.
        let planned_without_hint = mgr.plan_requests(Instant::now());
        assert!(
            planned_without_hint.is_empty(),
            "without the hint, the stall reproduces (target_height still 6000)"
        );

        // With the fix: the KickSync path passes the observed height as
        // a hint. `note_observed_height(6283)` bumps target_height AND
        // also updates the peer_chains entry for `peer` (the only known
        // peer) so `select_best_peer_for_range` can match.
        mgr.note_observed_height(6283);
        // peer is still at advertised 6000 in peer_chains, but the
        // observed height is 6283. For the planner to select a peer,
        // `select_best_peer_for_range` filters on `info.height >=
        // range.end`. The first planned range will be 6001..6100; we
        // need at least one peer at height >= 6100. The desktop's
        // peer_chains entry for the producer is stale at 6000, so we
        // ALSO need to refresh it. In production this happens via the
        // next ChainStatus or via inferring that the peer who broadcast
        // a NewBlock(6283) must have height >= 6283. The minimal fix
        // takes the conservative path: assume the observed height
        // applies to the BEST known peer.
        //
        // For the regression test, we simulate that follow-up by
        // calling record_peer_status with the observed height. In
        // production this is layered as: note_observed_height bumps
        // target, then the next ChainStatus (which comes shortly)
        // updates the per-peer height. The two work together.
        mgr.record_peer_status(peer, 6283);

        let planned_after_hint = mgr.plan_requests(Instant::now());
        assert!(
            !planned_after_hint.is_empty(),
            "with note_observed_height + peer status, plan_requests must dispatch"
        );
        assert_eq!(
            planned_after_hint[0].request.start_height, 6001,
            "the new range must start one above local_height"
        );
    }

    /// Companion to `plans_new_range_after_ibd_target_grows_post_completion`
    /// — exercises the path where IBD completed naturally through
    /// `handle_response` (state ends `Downloading` because target_height
    /// outgrew local_height mid-IBD via concurrent ChainStatus), but the
    /// in-flight + pending queues drained. The next KickSync /
    /// plan_requests must still refill and dispatch.
    #[test]
    fn refills_and_dispatches_when_pending_drained_but_peer_still_ahead() {
        let peer = fake_peer();
        let mut mgr = SyncManager::new(0);

        mgr.record_peer_status(peer, 100);
        let planned = mgr.plan_requests(Instant::now());
        assert!(!planned.is_empty());

        // Simulate all in-flight completed, local_height advanced to the
        // original target, then peer advertises a higher height — but
        // through the more realistic ordering where `record_peer_status`
        // arrives after local_height has been bumped (not before).
        mgr.in_flight.clear();
        mgr.pending_ranges.clear();
        mgr.local_height = 100;

        // Peer status arrives ahead. record_peer_status should refill.
        mgr.record_peer_status(peer, 250);
        assert!(
            !mgr.pending_ranges.is_empty(),
            "record_peer_status with a higher peer height must refill pending_ranges"
        );

        // KickSync path: plan_requests must dispatch.
        let planned2 = mgr.plan_requests(Instant::now());
        assert!(
            !planned2.is_empty(),
            "plan_requests must dispatch when peer height grows past local_height"
        );
        assert_eq!(planned2[0].request.start_height, 101);
    }

    /// Follow-up regression for #1664 / #1665. After PR #1665, the desktop
    /// peer advances exactly ONE chunk past the original IBD ceiling
    /// (e.g. 6000 → 6100 with MAX_BLOCKS_PER_REQUEST=100), then stalls
    /// again. The cause: `note_observed_height(H)` correctly bumps the
    /// global `target_height` to H and refills `pending_ranges`, but the
    /// per-peer `peer_chains[peer].height` is still stuck at the last
    /// value learned from `record_peer_status` (which is rate-limited by
    /// `chain_status_broadcast_interval`, 20s+ at h=6100). The pending
    /// range 6101..6200 then fails `select_best_peer_for_range`'s
    /// `info.height >= range.end` filter because the only peer is still
    /// advertised at h=6100 — so `plan_requests` returns empty and the
    /// node stalls until the next ChainStatus arrives.
    ///
    /// Fix: `note_observed_height` ALSO bumps the per-peer height of any
    /// peer currently advertised at-or-above `local_height` (i.e. peers
    /// known to be ahead — at least one of them must have served the
    /// block we just observed).
    #[test]
    fn note_observed_height_bumps_per_peer_height_for_known_ahead_peers() {
        let peer = fake_peer();
        let mut mgr = SyncManager::new(0);

        // Initial state: peer ahead at h=6100 (the original IBD target).
        mgr.record_peer_status(peer, 6100);
        // Simulate IBD finishing: local_height advanced to 6100.
        mgr.in_flight.clear();
        mgr.pending_ranges.clear();
        mgr.local_height = 6100;
        mgr.set_local_height(6100);
        assert_eq!(*mgr.state(), SyncState::Complete);

        // peer_chains entry is still at 6100 (no fresh ChainStatus yet).
        assert_eq!(mgr.peer_metrics(&peer).unwrap().advertised_height, 6100);

        // KickSync arrives with observed_height=6358 (orphan NewBlock).
        // Without the per-peer bump, the next plan_requests will be
        // empty: pending refills to 6101..=6358, but
        // select_best_peer_for_range filters out the only peer because
        // 6100 < 6200 (the first range's end).
        mgr.note_observed_height(6358);
        assert_eq!(mgr.target_height(), 6358);

        // The fix MUST have bumped the peer's height to 6358.
        assert_eq!(
            mgr.peer_metrics(&peer).unwrap().advertised_height,
            6358,
            "note_observed_height must bump per-peer height for known-ahead peers"
        );

        // Now plan_requests must dispatch a real BlockRequest.
        let planned = mgr.plan_requests(Instant::now());
        assert!(
            !planned.is_empty(),
            "plan_requests must dispatch after note_observed_height bumps both \
             target_height AND per-peer heights (this was the #1664 follow-up stall)"
        );
        assert_eq!(planned[0].request.start_height, 6101);
        assert_eq!(planned[0].peer_id, peer);
    }

    /// Companion to the above — `note_observed_height` must NOT promote
    /// peers that the manager has never seen as "ahead" (e.g. a peer
    /// that previously reported `height=0` and was treated as behind).
    /// Those peers might be a fresh light-client or a misbehaving node;
    /// silently promoting them risks dispatching requests they cannot
    /// serve.
    #[test]
    fn note_observed_height_does_not_promote_behind_peers() {
        let ahead_peer = fake_peer();
        let behind_peer = fake_peer();
        let mut mgr = SyncManager::new(6100);

        mgr.record_peer_status(ahead_peer, 6100);
        mgr.record_peer_status(behind_peer, 50);

        mgr.note_observed_height(6358);

        assert_eq!(
            mgr.peer_metrics(&ahead_peer).unwrap().advertised_height,
            6358,
            "peer already at-or-above local_height must be promoted"
        );
        assert_eq!(
            mgr.peer_metrics(&behind_peer).unwrap().advertised_height,
            50,
            "peer behind local_height must NOT be promoted"
        );
    }
}
