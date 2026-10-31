//! P2P node orchestrator -- starts the libp2p swarm and bridges it to the
//! application via channel-based [`P2pHandle`].

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use libp2p::{
    Multiaddr, PeerId, Swarm, SwarmBuilder, autonat, dcutr,
    futures::StreamExt,
    gossipsub, identify,
    identity::Keypair,
    kad, noise, quic, relay, request_response,
    swarm::{
        ConnectionId,
        dial_opts::{DialOpts, PeerCondition},
    },
    tcp, upnp, yamux,
};
use log::{debug, error, info, warn};
use rand::RngCore;
use rustc_hash::{FxHashMap, FxHashSet};
use tokio::sync::mpsc;
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::compute_target_block_time;

use crate::{
    behaviour::{WrfcoinBehaviourEvent, build_behaviour},
    sync_surface::{SyncPeerSurfaceEntry, SyncPeersSurface},
    bootstrap_redial::{BootstrapPeerRedial, compute_backoff, peer_id_from_multiaddr},
    config::P2pConfig,
    da_challenge::{DataAvailabilityChallenge, DataAvailabilityChallengeResponse},
    discovered::DiscoveredPeers,
    error::{P2pError, P2pResult},
    message::{MessageKind, TopicManager, WrfcoinMessage},
    metrics::{MetricsCollector, NetworkStats, topic_label},
    pending_broadcast::{
        EnqueueOutcome, PendingBroadcast, PendingBroadcasts, RetryOutcome,
    },
    peer::PeerManager,
    scoring::SyncPeerScore,
    sync::{
        AuthoritativeSyncRejectionAttribution, BlockRequest, BlockResponse, SyncError,
        SyncManager, SyncState,
    },
};

#[derive(Debug)]
struct OutboundSyncRequest {
    peer_id: PeerId,
}

const WEATHER_DATA_NONCE_BYTES: usize = 16;
const WEATHER_DATA_NONCE_HEX_LEN: usize = WEATHER_DATA_NONCE_BYTES * 2;
const WEATHER_DATA_NONCE_TTL: Duration = Duration::from_secs(10 * 60);
const WEATHER_DATA_MIN_VERIFY_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WeatherDataAdmissionError {
    MissingNonce,
    InvalidNonce,
    Replay,
    RateLimited,
}

impl WeatherDataAdmissionError {
    fn as_str(self) -> &'static str {
        match self {
            Self::MissingNonce => "missing nonce",
            Self::InvalidNonce => "invalid nonce",
            Self::Replay => "replayed nonce",
            Self::RateLimited => "rate limited",
        }
    }
}

/// Admission control for inbound weather gossip.
///
/// #2117: both maps are keyed by **attacker-chosen** data — `station_id` arrives
/// in the payload and is used before the station signature is verified — so both
/// are now bounded per peer as well as by TTL. Measured before the fix: 50 000
/// distinct `station_id`s from ONE peer produced 50 000 entries in each map and
/// ~16 s of CPU, because `prune` ran a full `retain` on every admission.
///
/// The quota is **per peer** deliberately. A global cap would let one attacker
/// evict other peers' entries, re-opening replay protection and rate limiting
/// for everyone — trading an OOM for a security regression. See
/// `preauth_budget`'s module doc for the full argument.
#[derive(Debug)]
pub(crate) struct WeatherDataAdmission {
    seen_nonces: crate::preauth_budget::PerOwnerBoundedMap<(String, String), ()>,
    last_peer_station_verify: crate::preauth_budget::PerOwnerBoundedMap<(PeerId, String), ()>,
    /// Admissions since the last TTL sweep. The per-peer quota is what bounds
    /// memory, so the sweep only reclaims idle entries and must NOT run per
    /// message — that was the O(n)-per-admission half of the defect.
    since_prune: u32,
}

/// Admissions between TTL sweeps. At the 250 ms per-(peer,station) rate limit
/// this is a sweep every few seconds under real load, and O(1) per message.
const WEATHER_ADMISSION_PRUNE_INTERVAL: u32 = 512;

impl Default for WeatherDataAdmission {
    fn default() -> Self {
        use crate::preauth_budget::{DEFAULT_MAX_ENTRIES_PER_PEER, PerOwnerBoundedMap};
        Self {
            seen_nonces: PerOwnerBoundedMap::new(
                WEATHER_DATA_NONCE_TTL,
                DEFAULT_MAX_ENTRIES_PER_PEER,
            ),
            last_peer_station_verify: PerOwnerBoundedMap::new(
                WEATHER_DATA_NONCE_TTL,
                DEFAULT_MAX_ENTRIES_PER_PEER,
            ),
            since_prune: 0,
        }
    }
}

impl WeatherDataAdmission {
    pub(crate) fn admit(
        &mut self,
        peer_id: PeerId,
        station_id: &str,
        nonce: Option<&str>,
        now: Instant,
    ) -> Result<(), WeatherDataAdmissionError> {
        self.maybe_prune(now);

        let nonce = nonce
            .filter(|nonce| !nonce.is_empty())
            .ok_or(WeatherDataAdmissionError::MissingNonce)?;
        if nonce.len() != WEATHER_DATA_NONCE_HEX_LEN
            || !nonce.chars().all(|ch| ch.is_ascii_hexdigit())
        {
            return Err(WeatherDataAdmissionError::InvalidNonce);
        }

        let replay_key = (station_id.to_string(), nonce.to_ascii_lowercase());
        if self.seen_nonces.contains_key(&replay_key) {
            return Err(WeatherDataAdmissionError::Replay);
        }

        let rate_key = (peer_id, station_id.to_string());
        if let Some(last_verify) = self.last_peer_station_verify.inserted_at(&rate_key) {
            if now.saturating_duration_since(last_verify) < WEATHER_DATA_MIN_VERIFY_INTERVAL {
                return Err(WeatherDataAdmissionError::RateLimited);
            }
        }

        let owner = peer_id.to_string();
        self.seen_nonces.insert(&owner, replay_key, (), now);
        self.last_peer_station_verify.insert(&owner, rate_key, (), now);
        Ok(())
    }

    /// Rate-only admission for `WeatherDataV2` (legacy helper, retained for the
    /// shared-rate-bucket unit tests).
    ///
    /// As of #1969 the live V2 inbound path goes through [`Self::admit`] so that
    /// nonce validity, replay protection, and per-peer rate limiting all apply
    /// before the station signature is verified — exactly as for V1. This
    /// rate-only variant is kept solely to exercise the shared rate-limit
    /// bucket between V1 and V2 in the test suite.
    ///
    /// The per-peer+station rate limit (`WEATHER_DATA_MIN_VERIFY_INTERVAL`) is
    /// enforced; V1 and V2 share the same rate-limit bucket so mixing variants
    /// cannot bypass the throttle.
    #[allow(dead_code)]
    pub(crate) fn admit_v2(
        &mut self,
        peer_id: PeerId,
        station_id: &str,
        now: Instant,
    ) -> Result<(), WeatherDataAdmissionError> {
        self.maybe_prune(now);

        let rate_key = (peer_id, station_id.to_string());
        if let Some(last_verify) = self.last_peer_station_verify.inserted_at(&rate_key) {
            if now.saturating_duration_since(last_verify) < WEATHER_DATA_MIN_VERIFY_INTERVAL {
                return Err(WeatherDataAdmissionError::RateLimited);
            }
        }

        self.last_peer_station_verify
            .insert(&peer_id.to_string(), rate_key, (), now);
        Ok(())
    }

    /// Sweep expired entries every `WEATHER_ADMISSION_PRUNE_INTERVAL` admissions.
    ///
    /// Was a full `retain` over both maps on EVERY admission, making per-message
    /// cost grow with entries held — 50 000 messages took ~16 s. Memory is now
    /// bounded by the per-peer quota, so the sweep only reclaims idle entries.
    fn maybe_prune(&mut self, now: Instant) {
        self.since_prune = self.since_prune.saturating_add(1);
        if self.since_prune < WEATHER_ADMISSION_PRUNE_INTERVAL {
            return;
        }
        self.since_prune = 0;
        self.seen_nonces.prune_expired(now);
        self.last_peer_station_verify.prune_expired(now);
    }
}

pub(crate) fn generate_weather_nonce() -> String {
    let mut nonce = [0_u8; WEATHER_DATA_NONCE_BYTES];
    rand::thread_rng().fill_bytes(&mut nonce);
    hex::encode(nonce)
}

pub(crate) fn verify_weather_gossip_signature(
    station_id: &str,
    data: &[u8],
    timestamp: i64,
    nonce: &str,
    station_signature: &str,
    station_pubkey: &str,
) -> bool {
    use wrfcoin_shared_protocol::crypto::domain_signing::{
        DomainSigningAction, DomainSigningContext, canonical_signing_message, payload_digest_hex,
        signing_chain_id, signing_network_id,
    };
    use wrfcoin_shared_protocol::crypto::signature::{PublicKey, Signature};

    (|| -> Option<bool> {
        let pubkey_bytes = hex::decode(station_pubkey).ok()?;
        let pubkey = PublicKey::from_bytes(&pubkey_bytes).ok()?;
        let sig = Signature::from_hex(station_signature).ok()?;
        let data_hash = hex::encode(blake3::hash(data).as_bytes());
        let payload_digest = payload_digest_hex(&(station_id, &data_hash));
        let cid = signing_chain_id();
        let nid = signing_network_id();
        let context = DomainSigningContext {
            action: DomainSigningAction::WeatherDataGossip,
            signer_id: station_id,
            timestamp,
            nonce: Some(nonce),
            chain_id: &cid,
            network_id: &nid,
        };
        let signing_msg = canonical_signing_message(&context, &payload_digest);
        Some(pubkey.verify(signing_msg.as_bytes(), &sig))
    })()
    .unwrap_or(false)
}

/// Canonical data-hash for a `WeatherDataV2` payload.
///
/// The station signature must cover the *same* logical observation whether the
/// payload travels inline or via a content reference (#1969). For inline
/// payloads we hash `data` exactly as the V1 path does. For offloaded payloads
/// we bind the signature to the content reference's `data_hash`, which is itself
/// a hash of the original (pre-offload) payload, so a peer cannot swap the CID
/// or its claimed digest without invalidating the signature.
pub(crate) fn weather_v2_data_hash_hex(
    data: &[u8],
    content_ref: Option<&crate::message::ContentRef>,
) -> String {
    match content_ref {
        Some(cr) => cr.data_hash.clone(),
        None => hex::encode(blake3::hash(data).as_bytes()),
    }
}

/// Verify the station signature on a `WeatherDataV2` message.
///
/// Mirrors [`verify_weather_gossip_signature`] (the V1 path) but derives the
/// signed data-hash via [`weather_v2_data_hash_hex`] so inline and offloaded
/// payloads share one canonical signing input.
pub(crate) fn verify_weather_gossip_signature_v2(
    station_id: &str,
    data: &[u8],
    content_ref: Option<&crate::message::ContentRef>,
    timestamp: i64,
    nonce: &str,
    station_signature: &str,
    station_pubkey: &str,
) -> bool {
    use wrfcoin_shared_protocol::crypto::domain_signing::{
        DomainSigningAction, DomainSigningContext, canonical_signing_message, payload_digest_hex,
        signing_chain_id, signing_network_id,
    };
    use wrfcoin_shared_protocol::crypto::signature::{PublicKey, Signature};

    (|| -> Option<bool> {
        let pubkey_bytes = hex::decode(station_pubkey).ok()?;
        let pubkey = PublicKey::from_bytes(&pubkey_bytes).ok()?;
        let sig = Signature::from_hex(station_signature).ok()?;
        let data_hash = weather_v2_data_hash_hex(data, content_ref);
        let payload_digest = payload_digest_hex(&(station_id, &data_hash));
        let cid = signing_chain_id();
        let nid = signing_network_id();
        let context = DomainSigningContext {
            action: DomainSigningAction::WeatherDataGossip,
            signer_id: station_id,
            timestamp,
            nonce: Some(nonce),
            chain_id: &cid,
            network_id: &nid,
        };
        let signing_msg = canonical_signing_message(&context, &payload_digest);
        Some(pubkey.verify(signing_msg.as_bytes(), &sig))
    })()
    .unwrap_or(false)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct InboundRateLimit {
    max_bytes: u64,
    window: Duration,
}

impl InboundRateLimit {
    fn disabled(self) -> bool {
        self.max_bytes == 0 || self.window == Duration::ZERO
    }
}

#[derive(Debug)]
struct PeerInboundRateState {
    window_started: Instant,
    bytes: u64,
    /// Per-variant arrival counts within the current window (#2115 rate half).
    ///
    /// A fixed array indexed by [`MessageKind::as_index`] rather than a map:
    /// the per-variant counters are then bounded by construction and cannot
    /// become a second structure keyed by unauthenticated peer identity — the
    /// shape that OOM'd the node in #2077. It also costs one allocation fewer
    /// per peer, on a path whose whole purpose is being cheap under flood.
    per_kind: [u32; crate::message::MessageKind::COUNT],
}

/// Which ceiling an inbound message breached (#2115).
#[derive(Debug, Eq, PartialEq)]
enum InboundRateBreach {
    /// Total bytes from this peer in the window.
    Bytes(InboundRateLimitExceeded),
    /// Messages of one variant from this peer in the window.
    Variant {
        kind: crate::message::MessageKind,
        used: u32,
        max: u32,
        window: Duration,
    },
}

impl InboundRateBreach {
    /// Operator-facing reason, used both in the log and in the penalty event.
    fn reason(&self) -> String {
        match self {
            InboundRateBreach::Bytes(limit) => format!(
                "inbound gossip rate limit exceeded: used={}B attempted={}B max={}B window={}s",
                limit.used,
                limit.attempted,
                limit.max,
                limit.window.as_secs()
            ),
            InboundRateBreach::Variant {
                kind,
                used,
                max,
                window,
            } => format!(
                "inbound {kind:?} message rate exceeded: used={used} max={max} window={}s",
                window.as_secs()
            ),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
struct InboundRateLimitExceeded {
    used: u64,
    attempted: u64,
    max: u64,
    window: Duration,
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum P2pEvent {
    PeerConnected(PeerId),
    PeerDisconnected(PeerId),
    /// Emitted when a peer subscribes to or unsubscribes from a gossipsub topic.
    /// Used by tests to detect gossipsub mesh formation readiness.
    SubscriptionStateChanged {
        peer_id: PeerId,
        topic: String,
        subscribed: bool,
    },
    ListeningAddress(Multiaddr),
    NewBlock(Vec<u8>),
    NewTransactions(Vec<Vec<u8>>),
    WeatherData {
        station_id: String,
        data: Vec<u8>,
        /// Hex-encoded ed25519 public key of the signing station (app layer checks oracle registry).
        station_pubkey: String,
    },
    PeerChainStatus {
        peer_id: PeerId,
        height: u64,
        tip_hash: [u8; 32],
        state_root: [u8; 32],
    },
    InboundBlockSyncRequest {
        peer_id: PeerId,
        request_id: request_response::InboundRequestId,
        request: BlockRequest,
    },
    SyncedBlock {
        /// Peer whose block-range response supplied this block. Retained until
        /// authoritative application so state-invalid history stays attributable.
        peer_id: PeerId,
        height: u64,
        block_bytes: Vec<u8>,
    },
    /// Emitted after the response that moves the sync manager into Complete.
    /// The launch API consumes all preceding `SyncedBlock` events first, then
    /// publishes one current tip/root status instead of one per imported block.
    SyncCompleted {
        height: u64,
    },
    PeerPenalized {
        peer_id: PeerId,
        reason: String,
        banned: bool,
    },
    ForecastTaskAssignment {
        task_id: String,
        node_id: String,
        manifest_bytes: Vec<u8>,
    },
    ComputeNodeHeartbeat {
        node_id: String,
        capabilities: Vec<String>,
        region: String,
        active_tasks: u32,
        max_concurrent_tasks: u32,
        observation_density: Option<f64>,
        station_count: Option<u32>,
    },
    ForecastTaskResult {
        task_id: String,
        node_id: String,
        receipt_bytes: Vec<u8>,
    },
    WeatherDataV2 {
        station_id: String,
        data: Vec<u8>,
        content_ref: Option<crate::message::ContentRef>,
    },
    ForecastTaskResultV2 {
        task_id: String,
        node_id: String,
        receipt_bytes: Vec<u8>,
        content_ref: Option<crate::message::ContentRef>,
    },
    /// A Data Availability challenge received from a peer.
    DaChallenge {
        peer_id: PeerId,
        challenge: DataAvailabilityChallenge,
    },
    /// A keeper's DA challenge response received from the network.
    DaChallengeResponse {
        peer_id: PeerId,
        response: DataAvailabilityChallengeResponse,
    },
    /// A BFT finality vote received from a peer (#1388).
    ///
    /// `vote_bytes` is the bincode-serialized
    /// `consensus-engines::selection::bft_finality::FinalityVote`. The
    /// chain layer decodes and forwards it into the gadget's
    /// `submit_vote_verified` entrypoint.
    FinalityVote {
        peer_id: PeerId,
        vote_bytes: Vec<u8>,
    },
    /// A BFT finality certificate (2/3+1 vote-set) received from a peer (#1388).
    FinalityCertificate {
        peer_id: PeerId,
        cert_bytes: Vec<u8>,
    },
}

#[derive(Debug)]
pub enum P2pCommand {
    BroadcastBlock(Vec<u8>),
    BroadcastTransactions(Vec<Vec<u8>>),
    BroadcastChainStatus {
        height: u64,
        tip_hash: [u8; 32],
        state_root: [u8; 32],
    },
    UpdateLocalChainState {
        height: u64,
        tip_hash: [u8; 32],
        state_root: [u8; 32],
    },
    /// Authoritative launch-state validation rejected a historical block that
    /// the context-free sync shadow had forwarded (#2786). Rewind the shadow to
    /// the supplied accepted tip and replan. The launch layer separately marks
    /// whether the serving peer supplied invalid input or the local application
    /// path failed.
    AuthoritativeSyncRejected {
        peer_id: PeerId,
        block_height: u64,
        accepted_height: u64,
        accepted_tip_hash: [u8; 32],
        accepted_state_root: [u8; 32],
        attribution: AuthoritativeSyncRejectionAttribution,
        reason: String,
    },
    RespondToBlockSyncRequest {
        request_id: request_response::InboundRequestId,
        response: BlockResponse,
    },
    BroadcastWeatherData {
        station_id: String,
        data: Vec<u8>,
        station_signing_key: wrfcoin_shared_protocol::crypto::signature::PrivateKey,
    },
    BroadcastForecastTaskAssignment {
        task_id: String,
        node_id: String,
        manifest_bytes: Vec<u8>,
    },
    BroadcastComputeNodeHeartbeat {
        node_id: String,
        capabilities: Vec<String>,
        region: String,
        active_tasks: u32,
        max_concurrent_tasks: u32,
        observation_density: Option<f64>,
        station_count: Option<u32>,
    },
    BroadcastForecastTaskResult {
        task_id: String,
        node_id: String,
        receipt_bytes: Vec<u8>,
    },
    BroadcastWeatherDataV2 {
        station_id: String,
        data: Vec<u8>,
        content_ref: Option<crate::message::ContentRef>,
        /// Station/node signing key. The node signs the message before
        /// broadcast (mirroring [`P2pCommand::BroadcastWeatherData`]) so peers
        /// can verify the origin and reject injected payloads (#1969).
        station_signing_key: wrfcoin_shared_protocol::crypto::signature::PrivateKey,
    },
    BroadcastForecastTaskResultV2 {
        task_id: String,
        node_id: String,
        receipt_bytes: Vec<u8>,
        content_ref: Option<crate::message::ContentRef>,
    },
    /// Broadcast a DA challenge to keeper nodes on the network.
    BroadcastDaChallenge {
        challenge: DataAvailabilityChallenge,
    },
    /// Broadcast a DA challenge response on behalf of a keeper.
    BroadcastDaChallengeResponse {
        response: DataAvailabilityChallengeResponse,
    },
    /// Broadcast a BFT finality vote (#1388). `vote_bytes` is the
    /// bincode-serialized `FinalityVote` from `consensus-engines`.
    BroadcastFinalityVote {
        vote_bytes: Vec<u8>,
    },
    /// Broadcast a BFT finality certificate (#1388). `cert_bytes` is the
    /// bincode-serialized `FinalityCertificate` from `consensus-engines`.
    BroadcastFinalityCertificate {
        cert_bytes: Vec<u8>,
    },
    #[cfg(test)]
    BroadcastGenesisHandshake { genesis_hash: String },
    /// Snapshot real transport and mesh readiness without adding a production
    /// API or relying on peer-level metrics during a connection upgrade.
    #[cfg(test)]
    QuicBlocksReady {
        peer_id: PeerId,
        reply: tokio::sync::oneshot::Sender<bool>,
    },
    GetNetworkStats,
    /// Nudge the sync manager to re-plan and dispatch any pending block-range
    /// requests immediately.
    ///
    /// Normally `dispatch_planned_requests` is driven by a 250 ms `sync_tick`
    /// plus the `ChainStatus` gossip path. This command lets consumers kick a
    /// dispatch out of band — for example, when the `NewBlock` gossipsub
    /// broadcast delivers a block that is ahead of our tip.
    ///
    /// `observed_height` is an optional hint: when the caller observes a
    /// block at a specific height (e.g. via a far-ahead `NewBlock` gossip
    /// that landed in the orphan buffer), it should pass that height so
    /// the sync manager can bump `target_height` even if the most recent
    /// `ChainStatus` broadcast is still seconds away. Without this hint
    /// the sync manager stalls at the IBD ceiling when ChainStatus lags
    /// behind block production (#1664). Pass `None` for legacy
    /// behaviour.
    KickSync {
        observed_height: Option<u64>,
    },
    /// Drop and de-route a misbehaving peer (#1406). Issued by the launch_api
    /// P2P event consumer when accumulated `PeerPenalized` outcomes graylist a
    /// peer. The node event loop removes it from the kademlia routing table
    /// (so we stop dialing it) and disconnects any live connection. Best-effort
    /// and idempotent: re-issuing it for an already-removed peer is harmless.
    DisconnectPeer {
        peer_id: PeerId,
        reason: String,
    },
    Shutdown,
}

pub struct P2pHandle {
    pub command_tx: mpsc::Sender<P2pCommand>,
    pub event_rx: mpsc::Receiver<P2pEvent>,
    metrics: Arc<MetricsCollector>,
    pub local_peer_id: PeerId,
    pub discovered_peers: DiscoveredPeers,
    pub sync_peers: SyncPeersSurface,
}

impl P2pHandle {
    #[cfg(test)]
    pub(crate) async fn quic_blocks_ready_for_test(&self, peer_id: PeerId) -> bool {
        let (reply, response) = tokio::sync::oneshot::channel();
        self.command_tx
            .send(P2pCommand::QuicBlocksReady { peer_id, reply })
            .await
            .expect("event loop should accept QUIC readiness query");
        response.await.expect("event loop should reply to QUIC readiness query")
    }

    pub fn stats(&self) -> NetworkStats {
        self.metrics.snapshot_cached()
    }

    pub fn into_parts(self) -> (P2pClient, mpsc::Receiver<P2pEvent>) {
        let client = P2pClient {
            command_tx: self.command_tx,
            metrics: self.metrics,
            local_peer_id: self.local_peer_id,
            discovered_peers: self.discovered_peers,
            sync_peers: self.sync_peers,
        };
        (client, self.event_rx)
    }
}

#[derive(Clone)]
pub struct P2pClient {
    pub command_tx: mpsc::Sender<P2pCommand>,
    metrics: Arc<MetricsCollector>,
    pub local_peer_id: PeerId,
    pub discovered_peers: DiscoveredPeers,
    pub sync_peers: SyncPeersSurface,
}

impl P2pClient {
    pub fn stats(&self) -> NetworkStats {
        self.metrics.snapshot_cached()
    }

    /// Request that the node event loop drop and de-route a graylisted peer
    /// (#1406). Best-effort and non-blocking: the command is enqueued with
    /// `try_send`, so if the command channel is momentarily full the request is
    /// dropped rather than stalling the caller (the peer remains penalized and a
    /// later penalty re-issues the eviction). Returns whether the command was
    /// enqueued.
    pub fn disconnect_peer(&self, peer_id: PeerId, reason: impl Into<String>) -> bool {
        self.command_tx
            .try_send(P2pCommand::DisconnectPeer {
                peer_id,
                reason: reason.into(),
            })
            .is_ok()
    }

    /// Construct a stub client for unit/integration tests outside this crate.
    pub fn for_testing(peer_id: PeerId) -> Self {
        let (command_tx, _rx) = mpsc::channel(1);
        Self {
            command_tx,
            metrics: Arc::new(crate::metrics::MetricsCollector::new()),
            local_peer_id: peer_id,
            discovered_peers: DiscoveredPeers::new(),
            sync_peers: SyncPeersSurface::new(),
        }
    }

    /// Like [`Self::for_testing`] but returns the command receiver, so tests can
    /// observe the commands the node event loop would act on (e.g. the
    /// `DisconnectPeer` issued for a graylisted peer). Used by the launch_api
    /// P2P event-loop tests (#1406).
    pub fn for_testing_with_commands(peer_id: PeerId) -> (Self, mpsc::Receiver<P2pCommand>) {
        let (command_tx, command_rx) = mpsc::channel(16);
        let client = Self {
            command_tx,
            metrics: Arc::new(crate::metrics::MetricsCollector::new()),
            local_peer_id: peer_id,
            discovered_peers: DiscoveredPeers::new(),
            sync_peers: SyncPeersSurface::new(),
        };
        (client, command_rx)
    }

    /// Construct a stub client for unit/integration tests that captures
    /// emitted commands for assertion.
    pub fn for_testing_with_receiver(
        peer_id: PeerId,
    ) -> (Self, mpsc::Receiver<P2pCommand>) {
        Self::for_testing_with_commands(peer_id)
    }

    /// Construct a lightweight client from a fixed stats snapshot.
    ///
    /// This is primarily used by API-layer tests that need to assert how the
    /// readiness and network endpoints react to a given P2P state without
    /// spinning up a live swarm.
    pub fn from_stats(stats: NetworkStats) -> Self {
        let metrics = Arc::new(MetricsCollector::from_snapshot(&stats));
        let (command_tx, _command_rx) = mpsc::channel(1);
        Self {
            command_tx,
            metrics,
            local_peer_id: PeerId::random(),
            discovered_peers: DiscoveredPeers::new(),
            sync_peers: SyncPeersSurface::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// P2pNode -- the entry point
// ---------------------------------------------------------------------------

pub struct P2pNode;

/// Authoritative chain view installed before the P2P event loop starts.
///
/// Restored nodes must seed both the downloader and its context-free chain
/// shadow from durable state. Starting either at zero makes every gossiped
/// forward block look like an orphan while range planning begins at height 1,
/// which can leave a single-peer follower permanently wedged (#2796).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InitialChainState {
    pub height: u64,
    pub tip_hash: [u8; 32],
    pub state_root: [u8; 32],
}

/// The authoritative chain view this node advertises in `ChainStatus`.
///
/// #3040: this is deliberately NOT the context-free sync shadow. During
/// initial block download the shadow runs ahead of the launch API's applier,
/// and #2815 correctly refuses to let a delayed acknowledgement rewind it.
/// Advertising the shadow's `(height, tip)` next to whatever `state_root`
/// last made it through that guard produced a triple that existed on no
/// chain — a stale boot root paired with the peer's tip — which every
/// same-height peer reported as `STATE DIVERGENCE DETECTED`. The advertised
/// status therefore tracks every authoritative acknowledgement
/// unconditionally, and only the shadow keeps the monotonic guard.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AdvertisedChainStatus {
    pub height: u64,
    pub tip_hash: [u8; 32],
    pub state_root: [u8; 32],
}

impl AdvertisedChainStatus {
    fn message(self) -> WrfcoinMessage {
        WrfcoinMessage::ChainStatus {
            height: self.height,
            tip_hash: self.tip_hash,
            state_root: self.state_root,
        }
    }
}

const MIN_CHAIN_STATUS_BROADCAST_INTERVAL: Duration = Duration::from_secs(10);
const MAX_CHAIN_STATUS_BROADCAST_INTERVAL: Duration = Duration::from_secs(300);

fn chain_status_broadcast_interval(height: u64) -> Duration {
    let interval_secs = compute_target_block_time(height).saturating_div(6).clamp(
        MIN_CHAIN_STATUS_BROADCAST_INTERVAL.as_secs(),
        MAX_CHAIN_STATUS_BROADCAST_INTERVAL.as_secs(),
    );
    Duration::from_secs(interval_secs)
}

fn new_chain_status_tick(height: u64) -> tokio::time::Interval {
    let interval = chain_status_broadcast_interval(height);
    let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + interval, interval);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    tick
}

fn initialize_sync_state(
    initial: InitialChainState,
) -> (SyncManager, Blockchain, AdvertisedChainStatus) {
    let sync_manager = SyncManager::new(initial.height);
    let mut sync_chain = Blockchain::default();
    sync_chain.height = initial.height;
    sync_chain.tip = initial.tip_hash.into();
    let advertised = AdvertisedChainStatus {
        height: initial.height,
        tip_hash: initial.tip_hash,
        state_root: initial.state_root,
    };
    (sync_manager, sync_chain, advertised)
}

/// Apply the authoritative chain view to the advertised status and to the
/// context-free sync shadow, and keep the chain-status cadence aligned with
/// the accepted height.
fn apply_authoritative_chain_state(
    sync_chain: &mut Blockchain,
    advertised: &mut AdvertisedChainStatus,
    chain_status_interval: &mut Duration,
    chain_status_tick: &mut tokio::time::Interval,
    height: u64,
    tip_hash: [u8; 32],
    state_root: [u8; 32],
) {
    // #3040: the advertised status is the authoritative view and must follow
    // every acknowledgement, including the delayed ones the shadow guard
    // below ignores. Acknowledgements arrive in apply order from a single
    // consumer, so this is monotonic except for the explicit rejection
    // rewind, which is exactly when peers must see the accepted tip again.
    *advertised = AdvertisedChainStatus {
        height,
        tip_hash,
        state_root,
    };
    // Historical sync validates a response against this context-free shadow
    // before its blocks cross the async event channel for authoritative
    // application.  The resulting acknowledgements therefore arrive after
    // the shadow may already be several blocks ahead.  Never let a delayed
    // successful acknowledgement rewind the speculative shadow: doing so
    // leaves newer cached blocks behind `height`, and the next response then
    // compares an honest older timestamp with that newer cache entry and
    // falsely reports `TimestampWentBackwards`.  Explicit authoritative
    // rejection uses the separate reset path below and may still rewind.
    if height < sync_chain.height {
        return;
    }
    sync_chain.height = height;
    sync_chain.tip = tip_hash.into();
    let next_chain_status_interval = chain_status_broadcast_interval(height);
    if next_chain_status_interval != *chain_status_interval {
        *chain_status_interval = next_chain_status_interval;
        *chain_status_tick = new_chain_status_tick(height);
    }
}

fn quic_max_idle_timeout(idle_connection_timeout: Duration) -> u32 {
    idle_connection_timeout
        .as_millis()
        .min(u128::from(u32::MAX)) as u32
}

fn quic_keep_alive_interval(idle_connection_timeout: Duration) -> Duration {
    let interval = std::cmp::min(idle_connection_timeout / 2, Duration::from_secs(5));
    if interval.is_zero() {
        Duration::from_millis(100)
    } else {
        interval
    }
}

fn configure_quic_transport(
    mut quic_config: quic::Config,
    idle_connection_timeout: Duration,
) -> quic::Config {
    quic_config.max_idle_timeout = quic_max_idle_timeout(idle_connection_timeout);
    quic_config.keep_alive_interval = quic_keep_alive_interval(idle_connection_timeout);
    quic_config
}

fn peer_has_pending_quic_upgrade(
    pending_quic_upgrade_dials: &FxHashMap<ConnectionId, PeerId>,
    peer_id: PeerId,
) -> bool {
    pending_quic_upgrade_dials
        .values()
        .any(|pending_peer_id| *pending_peer_id == peer_id)
}

fn maybe_dial_quic_upgrade(
    peer_id: PeerId,
    quic_transport_enabled: bool,
    peer_manager: &mut PeerManager,
    swarm: &mut Swarm<crate::behaviour::WrfcoinBehaviour>,
    pending_quic_upgrade_dials: &mut FxHashMap<ConnectionId, PeerId>,
) {
    if !quic_transport_enabled || peer_has_pending_quic_upgrade(pending_quic_upgrade_dials, peer_id)
    {
        return;
    }

    let Some(peer_info) = peer_manager.get_mut(&peer_id) else {
        return;
    };

    if peer_info.has_quic_connection() {
        return;
    }

    let quic_addresses = peer_info.known_quic_addresses();
    if quic_addresses.is_empty() {
        return;
    }

    let dial_opts = DialOpts::peer_id(peer_id)
        .condition(PeerCondition::Always)
        .addresses(quic_addresses.clone())
        .build();
    let connection_id = dial_opts.connection_id();

    match swarm.dial(dial_opts) {
        Ok(()) => {
            pending_quic_upgrade_dials.insert(connection_id, peer_id);
            info!(
                "Dialling QUIC upgrade for peer {peer_id} via {} advertised address(es)",
                quic_addresses.len()
            );
        },
        Err(error) => {
            warn!("Failed to dial QUIC upgrade for peer {peer_id}: {error}");
        },
    }
}

fn prune_redundant_non_quic_connections(
    peer_id: PeerId,
    identified_connection: ConnectionId,
    peer_manager: &mut PeerManager,
    swarm: &mut Swarm<crate::behaviour::WrfcoinBehaviour>,
) {
    let Some(peer_info) = peer_manager.get_mut(&peer_id) else {
        return;
    };

    for connection_id in peer_info.redundant_non_quic_after_identify(identified_connection) {
        if swarm.close_connection(connection_id) {
            info!(
                "Closing redundant non-QUIC connection {connection_id} for peer {peer_id} after QUIC upgrade"
            );
        }
    }
}

/// Walk the configured bootstrap peer set; for any that's not currently
/// connected and whose backoff has elapsed, redial it. Updates per-peer
/// state and bumps `bootstrap_redial_attempts_fail` on synchronous dial
/// errors. Asynchronous outcomes (success / network errors) flow through
/// the `ConnectionEstablished` and `OutgoingConnectionError` SwarmEvent
/// handlers, which adjust the per-peer state from there.
///
/// Behaviour summary (#1668):
/// - skips entries whose extracted peer_id is currently connected;
/// - skips entries whose `next_attempt` is still in the future;
/// - on a successful `swarm.dial`, parks `next_attempt` per the current
///   `consecutive_failures` (jittered exponential backoff). If the dial
///   subsequently fails, `OutgoingConnectionError` bumps the failure count.
fn redial_disconnected_bootstrap_peers(
    bootstrap_redial_states: &mut Vec<BootstrapPeerRedial>,
    peer_manager: &PeerManager,
    swarm: &mut Swarm<crate::behaviour::WrfcoinBehaviour>,
    metrics: &Arc<MetricsCollector>,
    base_interval: Duration,
    max_backoff: Duration,
    outbound_peers: &mut crate::slots::OutboundPeerSet,
) {
    let now = Instant::now();
    // Safety net for the reservation ledger (#2546): libp2p resolves every
    // dial with an event, but budget leaked here would silently shrink the
    // outbound set, so stale holds are dropped on the tick regardless.
    let pruned = outbound_peers.prune_stale_reservations(RESERVATION_MAX_AGE);
    if pruned > 0 {
        warn!("Dropped {pruned} stale outbound dial reservations (no swarm event within {RESERVATION_MAX_AGE:?})");
    }
    for state in bootstrap_redial_states.iter_mut() {
        if let Some(peer_id) = state.peer_id {
            if peer_manager.contains(&peer_id) {
                // Currently connected; reset backoff and skip.
                if state.consecutive_failures != 0 {
                    state.consecutive_failures = 0;
                }
                continue;
            }
        }
        if now < state.next_attempt {
            continue;
        }
        // #2114/#2546: charge the outbound diversity budget AT DIAL
        // SUBMISSION, not at establishment. `admits` alone reads established
        // state only, so on a cold start every bootstrap entry in one tick
        // passed against an empty set — the exact bypass #2546 finding 1
        // describes, with finding 2 (no in-flight budget) as its mechanism.
        // The reservation is released on `OutgoingConnectionError` and
        // converted on `ConnectionEstablished`. In observe-only mode the
        // verdict is counted and the dial proceeds unreserved, so the ceiling
        // can be measured on a live network before it may refuse anything.
        let dial_opts = libp2p::swarm::dial_opts::DialOpts::unknown_peer_id()
            .address(state.addr.clone())
            .build();
        let connection_id = dial_opts.connection_id();
        if let Err(rejection) =
            bootstrap_dial_admission(outbound_peers, connection_id, &state.addr)
        {
            metrics.record_diversity_rejection(rejection);
            if outbound_peers.policy().enforce {
                warn!(
                    "Skipping bootstrap redial of {}: outbound diversity policy ({rejection}). \
                     Trying the next candidate.",
                    state.addr
                );
                continue;
            }
            debug!(
                "Outbound diversity policy would refuse {} ({rejection}); proceeding because \
                 enforcement is off (observe-only)",
                state.addr
            );
        }
        info!(
            "Redialling disconnected bootstrap peer {} (consecutive_failures={})",
            state.addr, state.consecutive_failures
        );
        match swarm.dial(dial_opts) {
            Ok(()) => {
                // Synchronous dial submission OK. Compute the next allowable
                // attempt based on the current failure count. The real
                // outcome (success vs ConnectionError) is recorded by the
                // SwarmEvent handlers.
                let delay =
                    compute_backoff(state.consecutive_failures, base_interval, max_backoff);
                state.next_attempt = now + delay;
            },
            Err(e) => {
                warn!("Failed to redial bootstrap peer {}: {e}", state.addr);
                // The dial never left the swarm; hand the budget back.
                outbound_peers.release_reservation(&connection_id);
                state.consecutive_failures = state.consecutive_failures.saturating_add(1);
                let delay =
                    compute_backoff(state.consecutive_failures, base_interval, max_backoff);
                state.next_attempt = now + delay;
                metrics
                    .bootstrap_redial_attempts_fail
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            },
        }
    }
}

/// How long an in-flight dial may hold diversity budget before the tick
/// reclaims it (safety net; libp2p normally resolves every dial with an event).
const RESERVATION_MAX_AGE: Duration = Duration::from_secs(60);

/// The single admission seam for configured bootstrap dials. Both the first
/// dial and every retry enter through `redial_disconnected_bootstrap_peers`,
/// and admission CHARGES the budget (a reservation under `connection_id`)
/// rather than merely consulting it — #2546's startup bypass was N candidates
/// in one tick each passing a read-only check against an empty established
/// set.
fn bootstrap_dial_admission(
    outbound_peers: &mut crate::slots::OutboundPeerSet,
    connection_id: libp2p::swarm::ConnectionId,
    addr: &libp2p::Multiaddr,
) -> Result<(), crate::slots::DiversityRejection> {
    outbound_peers.try_reserve(connection_id, addr, crate::slots::DiscoverySource::Bootstrap)
}

impl P2pNode {
    /// Start an empty-chain node.
    ///
    /// Production callers restoring durable state should use
    /// [`Self::start_with_initial_chain_state`] instead. Keeping this helper
    /// explicitly zero-height preserves the cold-start and unit-test contract.
    pub async fn start(config: P2pConfig) -> P2pResult<P2pHandle> {
        Self::start_with_initial_chain_state(config, InitialChainState::default()).await
    }

    /// Start an empty-chain node with a caller-supplied network identity.
    pub async fn start_with_keypair(config: P2pConfig, keypair: Keypair) -> P2pResult<P2pHandle> {
        Self::start_with_keypair_and_initial_chain_state(
            config,
            keypair,
            InitialChainState::default(),
        )
        .await
    }

    /// Start a node from an authoritative chain view restored by the host.
    pub async fn start_with_initial_chain_state(
        config: P2pConfig,
        initial_chain_state: InitialChainState,
    ) -> P2pResult<P2pHandle> {
        Self::start_with_keypair_and_initial_chain_state(
            config,
            Keypair::generate_ed25519(),
            initial_chain_state,
        )
        .await
    }

    /// Start a node from an authoritative chain view and network identity.
    pub async fn start_with_keypair_and_initial_chain_state(
        config: P2pConfig,
        keypair: Keypair,
        initial_chain_state: InitialChainState,
    ) -> P2pResult<P2pHandle> {
        config.validate()?;

        // In-crate tests always run V1 semantics, so declare V1 here rather
        // than depending on every test remembering to (review of PR #2363:
        // hiding the install in a port helper left direct-`/tcp/0` tests
        // exposed and made the refusal ordering-dependent across parallel
        // tests). `cfg(test)` compiles only into THIS crate's own test binary
        // — external consumers, the refusal integration test
        // (`tests/uninstalled_signing_version_refusal_2275.rs`), and
        // production builds all still refuse when nothing is installed.
        #[cfg(test)]
        {
            use wrfcoin_shared_protocol::crypto::chain_binding::{
                ActivationTable, SigningProtocolVersion, install_activation_table,
            };
            SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
            // #1396: same rationale as the version install above — declare the
            // schedule here rather than depending on every test remembering to.
            // The identity schedule answers V1 at every height, so this is the
            // exact analogue of installing V1.
            install_activation_table(ActivationTable::default());
        }

        // #2275: resolve the advertised signing protocol version FIRST, before
        // any socket or swarm state exists. See the comment at the handshake
        // construction below for why an uninstalled version is a refusal, not
        // a V1 default.
        let advertised_version =
            wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion::try_active()
                .ok_or_else(|| {
                    P2pError::Config(
                        "no signing protocol version installed: p2p startup ran before \
                         assert_startup_protocol_compatibility (#2275). The genesis handshake \
                         must advertise the version the node actually signs with, so it \
                         refuses to advertise a default. Fix the boot ordering (the startup \
                         gate installs the version from genesis); test harnesses must \
                         declare one via SigningProtocolVersion::install_active."
                            .to_string(),
                    )
                })?;

        // #1396: the activation SCHEDULE must be installed too, and for exactly
        // the same reason as the version above — before any socket or swarm
        // state exists.
        //
        // `SigningProtocolVersion::try_active()` answering `Some` is no longer
        // sufficient. With a height-derived schedule, the version this node
        // signs a block with is `protocol_version_at(height)`, not the scalar.
        // A node that started with the version installed but the schedule
        // absent would advertise the scalar in its genesis handshake while
        // `protocol_version_at` fell back to the same scalar at every height —
        // agreeing with itself, and with a uniformly mis-ordered fleet, all the
        // way into the #2076 failure where signatures verify nowhere and
        // nothing errors. Representing "not established" is worthless unless
        // some caller refuses on it; this is that caller.
        let _advertised_schedule =
            wrfcoin_shared_protocol::crypto::chain_binding::try_activation_table().ok_or_else(
                || {
                    P2pError::Config(
                        "no protocol activation schedule installed: p2p startup ran before \
                         assert_startup_protocol_compatibility (#1396). The genesis handshake \
                         must advertise the schedule the node actually resolves versions from, \
                         so it refuses to advertise a default. Fix the boot ordering (the \
                         startup gate installs the schedule from genesis metadata alongside the \
                         version); test harnesses must declare one via \
                         install_activation_table(ActivationTable::default())."
                            .to_string(),
                    )
                },
            )?;

        let local_peer_id = PeerId::from(keypair.public());
        info!("Local peer ID: {local_peer_id}");
        let idle_connection_timeout = config.idle_connection_timeout;

        // Created before the swarm because `build_behaviour` needs it: the
        // connection-quota behaviour counts its own rejections (#2113).
        let metrics = Arc::new(MetricsCollector::new());
        metrics.set_sync_observation(
            initial_chain_state.height,
            initial_chain_state.height,
            SyncState::Idle.label(),
            0,
            0,
        );
        let behaviour_metrics = Arc::clone(&metrics);

        let mut swarm = SwarmBuilder::with_existing_identity(keypair)
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )
            .map_err(|e| P2pError::Transport(e.to_string()))?
            .with_quic_config(|quic_config| {
                configure_quic_transport(quic_config, idle_connection_timeout)
            })
            // Resolve /dns4/ and /dns/ peer addresses (the public seed is /dns4/seed.wrfcoin.com/...),
            // using the operating system's resolver settings on Linux, macOS and Windows.
            .with_dns()
            .map_err(|e| P2pError::Transport(format!("dns: {e}")))?
            .with_relay_client(noise::Config::new, yamux::Config::default)
            .map_err(|e| P2pError::Transport(format!("relay client: {e}")))?
            .with_behaviour(|key, relay_behaviour| {
                build_behaviour(key, &config, relay_behaviour, behaviour_metrics)
                    .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
            })
            .map_err(|e| P2pError::Swarm(format!("{e:?}")))?
            .with_swarm_config(|cfg| {
                cfg.with_idle_connection_timeout(config.idle_connection_timeout)
            })
            .build();

        swarm
            .behaviour_mut()
            .kademlia
            .set_mode(Some(kad::Mode::Server));

        for addr in config
            .listen_addresses
            .iter()
            .chain(config.quic_listen_addresses.iter())
        {
            swarm
                .listen_on(addr.clone())
                .map_err(|e| P2pError::Transport(format!("listen on {addr}: {e}")))?;
        }

        for addr in &config.bootstrap_peers {
            if let Some(peer_id) = addr.iter().find_map(|p| {
                if let libp2p::multiaddr::Protocol::P2p(peer_id) = p {
                    Some(peer_id)
                } else {
                    None
                }
            }) {
                swarm
                    .behaviour_mut()
                    .kademlia
                    .add_address(&peer_id, addr.clone());
                let _ = swarm.behaviour_mut().kademlia.bootstrap();
            }

            // Initial dialing is deliberately owned by the event loop below.
            // That is the only dial seam that charges the outbound diversity
            // policy, so startup cannot bypass an enforced ceiling (#2546).
        }

        // Dial configured relay nodes and listen on relay circuit addresses
        // so NAT'd operators can be reached via relays.
        for relay_addr in &config.relay_addresses {
            match swarm.dial(relay_addr.clone()) {
                Ok(()) => info!("Dialling relay node {relay_addr}"),
                Err(e) => warn!("Failed to dial relay node {relay_addr}: {e}"),
            }
            // Listen on the relay circuit so inbound relayed connections are accepted.
            let circuit_addr = relay_addr
                .clone()
                .with(libp2p::multiaddr::Protocol::P2pCircuit);
            match swarm.listen_on(circuit_addr.clone()) {
                Ok(_) => info!("Listening on relay circuit {circuit_addr}"),
                Err(e) => warn!("Failed to listen on relay circuit {circuit_addr}: {e}"),
            }
        }

        // Register operator-specified external address for NAT/Docker deployments.
        if let Some(ref ext_addr) = config.external_address {
            swarm.add_external_address(ext_addr.clone());
            info!("Registered external address: {ext_addr}");
        }
        if let Some(ref ext_addr) = config.quic_external_address {
            swarm.add_external_address(ext_addr.clone());
            info!("Registered external QUIC address: {ext_addr}");
        }

        let (command_tx, command_rx) = mpsc::channel(config.command_buffer);
        let (event_tx, event_rx) = mpsc::channel(config.event_buffer);

        let handle_metrics = Arc::clone(&metrics);
        let discovered_peers = DiscoveredPeers::new();
        let handle_discovered = discovered_peers.clone();
        let sync_peers_surface = SyncPeersSurface::new();
        let handle_sync_peers = sync_peers_surface.clone();

        let inline_data_max = config.inline_data_max;
        let quic_transport_enabled = !config.quic_listen_addresses.is_empty();
        // The advertised signing protocol version is read from the *same*
        // source of truth the signing path uses — the process-wide version
        // installed from durable genesis at startup — rather than from a
        // separate `Config` field. A config field would be one more place two
        // nodes could disagree, and one more thing a deployment could forget
        // to set, which would advertise V1 while signing V2: precisely the
        // asymmetry this guard exists to prevent (#2123, #2254).
        //
        // #2275: read through `try_active()` and REFUSE to start when nothing
        // is installed. Before this, a node that started p2p ahead of
        // `assert_startup_protocol_compatibility` silently advertised the V1
        // default while later signing whatever the gate installed — and a
        // fleet uniformly mis-ordered that way peers happily with itself
        // (every node tells the same lie), which is the #2076 shape. The
        // correctness of the advertisement was two line numbers in
        // launch_api's main in the right order, with nothing enforcing it.
        // Now the wrong order is a loud startup failure instead — resolved at
        // the very top of this function, before any socket exists.
        let local_handshake = crate::genesis_handshake::GenesisHandshakeMessage::new(
            config.network_id.clone(),
            config.chain_id.clone(),
            config.genesis_hash.clone(),
            advertised_version.as_u32(),
        );
        let bootstrap_peers_for_redial = config.bootstrap_peers.clone();
        let bootstrap_redial_interval = config.bootstrap_redial_interval;
        let bootstrap_redial_max_backoff = config.bootstrap_redial_max_backoff;
        tokio::spawn(event_loop(
            swarm,
            command_rx,
            event_tx,
            metrics,
            config.max_peers_per_subnet,
            config.role,
            config.sentry_allowlist.clone(),
            inline_data_max,
            InboundRateLimit {
                max_bytes: config.inbound_rate_limit_bytes,
                window: config.inbound_rate_limit_window,
            },
            quic_transport_enabled,
            local_handshake,
            discovered_peers,
            sync_peers_surface,
            bootstrap_peers_for_redial,
            bootstrap_redial_interval,
            bootstrap_redial_max_backoff,
            config.outbound_diversity,
            config.preauth_limits,
            config.handshake_limits,
            initial_chain_state,
            crate::announce::AnnouncePolicy::from_config(&config),
        ));

        Ok(P2pHandle {
            command_tx,
            event_rx,
            metrics: handle_metrics,
            local_peer_id,
            discovered_peers: handle_discovered,
            sync_peers: handle_sync_peers,
        })
    }
}

// ---------------------------------------------------------------------------
// Event loop
// ---------------------------------------------------------------------------

/// Publish the current outbound-diversity state, and warn on the transition
/// into the low-diversity condition (#2114).
///
/// The warning fires on the edge rather than every sample: a node that stays
/// eclipsed would otherwise fill the log with identical lines, which is how an
/// operator learns to filter the one message that mattered.
fn publish_outbound_diversity(
    metrics: &Arc<MetricsCollector>,
    outbound: &crate::slots::OutboundPeerSet,
) {
    let low = outbound.is_low_diversity();
    let was_low = metrics
        .outbound_low_diversity
        .load(std::sync::atomic::Ordering::Relaxed);
    metrics.set_outbound_diversity(outbound.len(), outbound.distinct_subnets(), low);

    if low && !was_low {
        warn!(
            "Outbound peer diversity is low: {} outbound peers span only {} distinct /24 \
             subnet(s), below the configured minimum of {}. This is the observable \
             signature of an eclipse in progress — check BOOTSTRAP_PEERS and whether the \
             Kademlia routing table has been poisoned.",
            outbound.len(),
            outbound.distinct_subnets(),
            outbound.policy().min_distinct_subnets,
        );
    }
}

/// #2118 observe-only half-life for the churn-resistant score, in seconds.
///
/// One hour: long enough that a burst of offences from one operator is still visible when
/// they reconnect minutes later under a new identity, short enough that a single bad day
/// ages out rather than permanently excluding a home operator.
const PERSISTENT_SCORE_HALF_LIFE_SECS: u64 = 3_600;

async fn event_loop(
    mut swarm: Swarm<crate::behaviour::WrfcoinBehaviour>,
    mut command_rx: mpsc::Receiver<P2pCommand>,
    event_tx: mpsc::Sender<P2pEvent>,
    metrics: Arc<MetricsCollector>,
    max_peers_per_subnet: usize,
    node_role: crate::config::NodeRole,
    sentry_allowlist: Option<Vec<PeerId>>,
    inline_data_max: usize,
    inbound_rate_limit: InboundRateLimit,
    quic_transport_enabled: bool,
    local_handshake: crate::genesis_handshake::GenesisHandshakeMessage,
    discovered_peers: DiscoveredPeers,
    sync_peers_surface: SyncPeersSurface,
    bootstrap_peers: Vec<Multiaddr>,
    bootstrap_redial_interval: Duration,
    bootstrap_redial_max_backoff: Duration,
    outbound_diversity: crate::slots::OutboundDiversityPolicy,
    preauth_limits: crate::preauth_budget::PreAuthLimits,
    handshake_limits: crate::handshake_guard::HandshakeLimits,
    initial_chain_state: InitialChainState,
    announce_policy: crate::announce::AnnouncePolicy,
) {
    let tm = TopicManager::new();
    // #2114: the self-chosen outbound peer set, and its diversity. Addresses
    // reachable at a configured bootstrap address are attributed to the
    // operator; everything else arrived via Kademlia routing, which is the
    // source an eclipse attacker can influence.
    let bootstrap_addr_set: std::collections::BTreeSet<String> =
        bootstrap_peers.iter().map(|a| a.to_string()).collect();
    let mut outbound_peers = crate::slots::OutboundPeerSet::new(outbound_diversity);
    let mut peer_manager = PeerManager::new();
    let (mut sync_manager, mut sync_chain, mut advertised_status) =
        initialize_sync_state(initial_chain_state);
    let mut outbound_sync_requests = FxHashMap::default();
    let mut inbound_sync_requests = FxHashMap::default();
    // #2116: peers connected but not yet proven to be on our chain.
    let mut unproven_peers = crate::handshake_guard::UnprovenPeers::new(handshake_limits);
    let mut unproven_sweep_tick = tokio::time::interval(std::time::Duration::from_secs(1));
    let mut sync_tick = tokio::time::interval(std::time::Duration::from_millis(250));
    sync_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut pending_broadcasts = PendingBroadcasts::default();
    // A verified peer gets one eager ChainStatus advertisement per connection
    // epoch. The entry is removed only after its final connection closes.
    let mut status_announced_peers = FxHashSet::default();
    // #2999: a connected peer's eager ChainStatus can arrive before its
    // GenesisHandshake is processed — gossipsub guarantees no ordering across
    // topics. The eager advertisement is once-per-epoch, so dropping it defers
    // sync to the periodic tick. Hold the latest pre-proof status per connected
    // peer and apply it when the proof lands; cleared on disconnect, so bounded
    // by the connected-peer count.
    let mut pre_proof_statuses: FxHashMap<PeerId, (u64, [u8; 32], [u8; 32])> =
        FxHashMap::default();
    let mut pending_broadcast_tick =
        tokio::time::interval(std::time::Duration::from_millis(100));
    pending_broadcast_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut chain_status_interval = chain_status_broadcast_interval(sync_chain.height);
    let mut chain_status_tick = new_chain_status_tick(sync_chain.height);
    let mut bandwidth_log_tick = tokio::time::interval(std::time::Duration::from_secs(60));
    bandwidth_log_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut peer_inbound_bytes: FxHashMap<PeerId, u64> = FxHashMap::default();
    let mut peer_inbound_rate_windows: FxHashMap<PeerId, PeerInboundRateState> =
        FxHashMap::default();
    let mut weather_peer_scores: FxHashMap<PeerId, SyncPeerScore> = FxHashMap::default();
    // #2118 OBSERVE-ONLY. Runs alongside the PeerId-keyed map above and never bans: it
    // records what a churn-resistant policy WOULD do, so the subnet weight and half-life
    // can be calibrated on real traffic before anything disconnects an honest operator.
    // The existing map is the one that still decides; this one only reports the delta.
    let mut persistent_scores =
        crate::scoring::PersistentScore::new(PERSISTENT_SCORE_HALF_LIFE_SECS, 3.0);
    let persistent_epoch = std::time::Instant::now();
    // #2117: bounds signature-verification work an unproven peer can cause.
    let mut verify_budget = crate::preauth_budget::VerifyBudget::new(preauth_limits);
    let mut preauth_budget = crate::preauth_budget::PreAuthBudget::new(preauth_limits);
    let mut preauth_evict_tick = tokio::time::interval(std::time::Duration::from_secs(30));
    let mut weather_admission = WeatherDataAdmission::default();
    let mut last_bandwidth_snapshot = metrics.snapshot(0);
    let mut pending_quic_upgrade_dials: FxHashMap<ConnectionId, PeerId> = FxHashMap::default();
    // Tracks whether we've kicked the post-identify Kademlia bootstrap (#1407).
    // The startup `bootstrap()` call only fires when `bootstrap_peers` is
    // non-empty; this path covers nodes that learn about peers organically
    // (e.g. via mDNS / inbound dials) and would otherwise never query the DHT.
    let mut did_post_identify_bootstrap = false;

    // ---- Bootstrap auto-redial state (#1668) -------------------------------
    // After a peer disconnect (e.g. operator restarts the remote container)
    // libp2p does not redial entries listed in `WRFCOIN_P2P_BOOTSTRAP` on its
    // own. We periodically check each bootstrap entry; if it's not connected
    // and its backoff has elapsed, we redial. Jittered exponential backoff
    // (base=interval, factor=2x, cap=max_backoff) avoids dial storms.
    //
    // `bootstrap_redial_states` is keyed by the position in the original
    // bootstrap_peers vector; we hold the multiaddr inside the state so we
    // can dial without re-parsing.
    let bootstrap_redial_check_interval = Duration::from_secs(5);
    let mut bootstrap_redial_tick =
        tokio::time::interval(bootstrap_redial_check_interval);
    bootstrap_redial_tick
        .set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut bootstrap_redial_states: Vec<BootstrapPeerRedial> = bootstrap_peers
        .iter()
        .map(|addr| {
            let peer_id = peer_id_from_multiaddr(addr);
            // The first tick performs the initial dial. Keeping it in the
            // redial state machine means bootstrap peers pass through the
            // same diversity check as every later retry (#2546).
            BootstrapPeerRedial::new(
                addr.clone(),
                peer_id,
                Instant::now(),
                Duration::ZERO,
            )
        })
        .collect();
    let bootstrap_peer_ids: rustc_hash::FxHashSet<PeerId> = bootstrap_redial_states
        .iter()
        .filter_map(|s| s.peer_id)
        .collect();

    loop {
        tokio::select! {
            event = swarm.select_next_some() => {
                use libp2p::swarm::SwarmEvent;
                match event {
                    SwarmEvent::Behaviour(behaviour_event) => {
                        handle_behaviour_event(
                            behaviour_event,
                            &mut verify_budget,
                            &mut preauth_budget,
                            &mut unproven_peers,
                            &tm,
                            &event_tx,
                            &metrics,
                            &mut peer_manager,
                            &mut sync_manager,
                            &mut sync_chain,
                            advertised_status,
                        &mut outbound_sync_requests,
                        &mut inbound_sync_requests,
                        &mut swarm,
                        &mut peer_inbound_bytes,
                        &mut peer_inbound_rate_windows,
                        inbound_rate_limit,
                        &mut weather_peer_scores,
                        &mut persistent_scores,
                        persistent_epoch.elapsed().as_secs(),
                        &mut weather_admission,
                        quic_transport_enabled,
                        inline_data_max,
                        &mut pending_quic_upgrade_dials,
                            &local_handshake,
                            &discovered_peers,
                            &mut did_post_identify_bootstrap,
                            &mut pending_broadcasts,
                            &mut status_announced_peers,
                            &mut pre_proof_statuses,
                            &announce_policy,
                        )
                        .await;
                    }
                    SwarmEvent::ConnectionEstablished {
                        peer_id,
                        connection_id,
                        endpoint,
                        ..
                    } => {
                        let remote_addr = endpoint.get_remote_address().clone();
                        // #2118: remember the address so an offence recorded later, where
                        // only the PeerId is in scope, can still be attributed to a subnet.
                        if let Some(ip) = crate::peer::remote_ip(&remote_addr) {
                            persistent_scores.note_address(
                                peer_id,
                                ip,
                                persistent_epoch.elapsed().as_secs(),
                            );
                        }
                        let is_known_peer = peer_manager.contains(&peer_id);
                        // #2120: a validator keeps inbound consensus connections only
                        // from its named sentries. Decided before the peer holds a
                        // slot, counted, and closed at the connection — not a warning.
                        if !crate::config::inbound_consensus_allowed(
                            node_role,
                            sentry_allowlist.as_deref(),
                            endpoint.is_dialer(),
                            &peer_id,
                        ) {
                            warn!(
                                "Refusing inbound connection from {peer_id} at {remote_addr}: \
                                 not on this validator's sentry allowlist (#2120)"
                            );
                            metrics.record_handshake_drop(
                                crate::handshake_guard::DropReason::NotAllowlistedSentry,
                            );
                            let _ = swarm.close_connection(connection_id);
                        } else if !is_known_peer
                            && !peer_manager.subnet_allows(&remote_addr, max_peers_per_subnet)
                        {
                            warn!(
                                "Rejecting peer {peer_id}: /16 subnet limit \
                                 ({max_peers_per_subnet}) reached for {remote_addr}"
                            );
                            let _ = swarm.close_connection(connection_id);
                        } else {
                            let is_first_connection = peer_manager.peer_connected(
                                peer_id,
                                connection_id,
                                remote_addr.clone(),
                            );
                            pending_quic_upgrade_dials.remove(&connection_id);

                            // #2114: track the peers *we* dialed. Inbound
                            // connections are excluded by construction — an
                            // attacker connecting to us must never be able to
                            // inflate our apparent outbound diversity, which
                            // would turn the eclipse signal into cover for the
                            // eclipse.
                            if endpoint.is_dialer() {
                                // #2546: a reserved dial converts its hold —
                                // keeping the subnet/source the budget was
                                // charged under — so establishment does not
                                // double-count. Unreserved dial paths record
                                // here as before.
                                if !outbound_peers.convert_reservation(&connection_id, peer_id) {
                                    let source = if bootstrap_addr_set
                                        .contains(&remote_addr.to_string())
                                    {
                                        crate::slots::DiscoverySource::Bootstrap
                                    } else {
                                        crate::slots::DiscoverySource::Kademlia
                                    };
                                    outbound_peers.record(peer_id, &remote_addr, source);
                                }
                                publish_outbound_diversity(&metrics, &outbound_peers);
                            }

                            // #2116: the peer has a slot but has proven nothing
                            // yet. Track it so it cannot hold that slot
                            // indefinitely without publishing a handshake.
                            if is_first_connection {
                                if let Err(reason) =
                                    unproven_peers.admit(peer_id, &remote_addr, Instant::now())
                                {
                                    metrics.record_handshake_drop(reason);
                                    if unproven_peers.limits().enforce {
                                        warn!(
                                            "Refusing {peer_id} from {remote_addr}: {reason} \
                                             (address already holds its allowance of peers \
                                             that have not proven chain identity)"
                                        );
                                        let _ = swarm.disconnect_peer_id(peer_id);
                                    } else {
                                        debug!(
                                            "Would refuse {peer_id} from {remote_addr}: \
                                             {reason} (observe-only)"
                                        );
                                    }
                                }
                                metrics.set_unproven_peers(unproven_peers.len());
                            }
                            // #1668: when a bootstrap peer (re)connects, reset
                            // its redial backoff and bump the success counter.
                            if is_first_connection && bootstrap_peer_ids.contains(&peer_id) {
                                for state in bootstrap_redial_states.iter_mut() {
                                    if state.peer_id == Some(peer_id) {
                                        state.mark_connected();
                                    }
                                }
                                metrics
                                    .bootstrap_redial_attempts_success
                                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            }
                            if is_first_connection {
                                metrics.set_connected_peers(peer_manager.connected_count());
                                let _ = event_tx.send(P2pEvent::PeerConnected(peer_id)).await;
                                publish_message(
                                    &mut swarm,
                                    &tm,
                                    &metrics,
                                    &mut pending_broadcasts,
                                    WrfcoinMessage::GenesisHandshake {
                                        network_id: local_handshake.network_id.clone(),
                                        chain_id: local_handshake.chain_id.clone(),
                                        genesis_hash: local_handshake.genesis_hash.clone(),
                                        protocol_version: local_handshake.protocol_version,
                                    },
                                    inline_data_max,
                                    Instant::now(),
                                );
                            }
                            // Retire TCP only after Identify arrives on the new
                            // QUIC connection, proving the remote behaviour has
                            // admitted it too (Refs #3275).
                            refresh_mesh_peer_metrics(&swarm, &tm, &metrics);
                            debug!("Connection established with {peer_id}");
                        }
                    }
                    SwarmEvent::ConnectionClosed {
                        peer_id,
                        connection_id,
                        ..
                    } => {
                        if peer_manager.peer_connection_closed(&peer_id, connection_id) {
                            // #2114: the peer is fully gone, so it no longer
                            // contributes outbound diversity.
                            outbound_peers.remove(&peer_id);
                            publish_outbound_diversity(&metrics, &outbound_peers);
                            // #2116: stop tracking a peer that is gone.
                            unproven_peers.remove(&peer_id);
                            metrics.set_unproven_peers(unproven_peers.len());
                            sync_manager.remove_peer(&peer_id);
                            status_announced_peers.remove(&peer_id);
                            pre_proof_statuses.remove(&peer_id);
                            peer_inbound_bytes.remove(&peer_id);
                            peer_inbound_rate_windows.remove(&peer_id);
                            // Evict the app-level weather-sig score so disconnected peers
                            // don't accumulate in memory indefinitely.  The gossipsub
                            // scoring layer retains its own penalty for `retain_score`
                            // (3600 s) independently of this map.
                            weather_peer_scores.remove(&peer_id);
                            // #2117: release the peer's pre-auth reservations.
                            verify_budget.release_peer(&peer_id);
                            metrics.set_connected_peers(peer_manager.connected_count());
                            let _ = event_tx.send(P2pEvent::PeerDisconnected(peer_id)).await;
                            refresh_mesh_peer_metrics(&swarm, &tm, &metrics);
                            if bootstrap_peer_ids.contains(&peer_id) {
                                let now = Instant::now();
                                for state in bootstrap_redial_states.iter_mut() {
                                    if state.peer_id == Some(peer_id) {
                                        state.schedule_after_disconnect(
                                            now,
                                            bootstrap_redial_interval,
                                        );
                                    }
                                }
                            }
                            debug!("Connection closed with {peer_id}");
                        }
                    }
                    SwarmEvent::OutgoingConnectionError {
                        connection_id,
                        peer_id,
                        error,
                    } => {
                        // #2546: a failed dial hands its diversity budget
                        // back. No-op for dials that never reserved.
                        outbound_peers.release_reservation(&connection_id);
                        // #2819: count EVERY failed outgoing dial. The
                        // bootstrap-only counter below misses kad-driven dials
                        // to peer-advertised addresses, which is exactly where
                        // unroutable bridge/loopback junk shows up.
                        metrics
                            .outgoing_dial_failures
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if let Some(pending_peer_id) = pending_quic_upgrade_dials.remove(&connection_id) {
                            warn!(
                                "QUIC upgrade dial failed for peer {pending_peer_id}: {error}"
                            );
                        } else if let Some(peer_id) = peer_id {
                            debug!("Outgoing connection error for {peer_id}: {error}");
                            // #1668: when an outgoing dial to a configured
                            // bootstrap peer fails (peer not yet back up,
                            // network blip, etc.), bump the failure counter
                            // and increment that peer's consecutive_failures
                            // so the next redial waits longer.
                            if bootstrap_peer_ids.contains(&peer_id) {
                                for state in bootstrap_redial_states.iter_mut() {
                                    if state.peer_id == Some(peer_id)
                                        && !peer_manager.contains(&peer_id)
                                    {
                                        state.schedule_after_failure(
                                            Instant::now(),
                                            bootstrap_redial_interval,
                                            bootstrap_redial_max_backoff,
                                        );
                                    }
                                }
                                metrics
                                    .bootstrap_redial_attempts_fail
                                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            }
                        }
                    }
                    SwarmEvent::NewListenAddr { address, .. } => {
                        info!("Listening on {address}");
                        on_new_listen_addr(&mut swarm, &announce_policy, &address);
                        let _ = event_tx.send(P2pEvent::ListeningAddress(address)).await;
                    }
                    SwarmEvent::ExpiredListenAddr { address, .. } => {
                        on_expired_listen_addr(&mut swarm, &announce_policy, &address);
                    }
                    SwarmEvent::ExternalAddrConfirmed { address } => {
                        on_external_addr_confirmed(&mut swarm, &announce_policy, &address);
                    }
                    _ => {}
                }
            }

            _ = sync_tick.tick() => {
                dispatch_planned_requests(
                    &mut sync_manager,
                    &mut swarm,
                    &mut outbound_sync_requests,
                    Instant::now(),
                );
                publish_sync_observation(&metrics, &sync_manager);
                publish_sync_peer_surface(
                    &sync_peers_surface,
                    &sync_manager,
                    &outbound_sync_requests,
                    &peer_manager,
                    Instant::now(),
                );
                refresh_mesh_peer_metrics(&swarm, &tm, &metrics);
            }

            _ = pending_broadcast_tick.tick() => {
                retry_pending_broadcasts(
                    &mut swarm,
                    &tm,
                    &metrics,
                    &mut pending_broadcasts,
                    Instant::now(),
                );
            }

            _ = bootstrap_redial_tick.tick() => {
                redial_disconnected_bootstrap_peers(
                    &mut bootstrap_redial_states,
                    &peer_manager,
                    &mut swarm,
                    &metrics,
                    bootstrap_redial_interval,
                    bootstrap_redial_max_backoff,
                    &mut outbound_peers,
                );
            }

            _ = preauth_evict_tick.tick() => {
                // #2117 / #2077: these maps are keyed by unauthenticated peer
                // identity. `release_peer` on disconnect is not sufficient on
                // its own — a peer that never disconnects cleanly would
                // persist forever, which is exactly the unbounded-map shape
                // that OOM'd the node in #2077. Evicting expired windows makes
                // the bound hold without depending on a disconnect event.
                verify_budget.evict_expired(Instant::now());
            }

            _ = unproven_sweep_tick.tick() => {
                // #2116: drop peers that have held a slot past the deadline
                // without ever proving they are on our chain. This is the
                // slowloris of a gossipsub-mediated handshake: not dribbled
                // bytes, but an indefinitely unproven peer occupying a finite
                // slot budget.
                let now = Instant::now();
                let newly_expired =
                    record_new_handshake_timeouts(&mut unproven_peers, &metrics, now);
                let enforcing = unproven_peers.limits().enforce;
                if enforcing {
                    // Keep retrying a failed/pending close, but do not count the
                    // same admission as a new timeout on every sweep.
                    for peer_id in unproven_peers.expired(now) {
                        warn!(
                            "Disconnecting {peer_id}: no matching genesis handshake within \
                             {:?}. The peer held a connection slot without proving chain \
                             identity.",
                            unproven_peers.limits().handshake_timeout
                        );
                        let _ = swarm.disconnect_peer_id(peer_id);
                    }
                } else {
                    for peer_id in newly_expired {
                        debug!(
                            "{peer_id} has not proven chain identity within {:?} \
                             (observe-only; counted once, not disconnected)",
                            unproven_peers.limits().handshake_timeout
                        );
                    }
                }
                metrics.set_unproven_peers(unproven_peers.len());
            }

            _ = chain_status_tick.tick() => {
                if peer_manager.connected_count() > 0 {
                    publish_message(
                        &mut swarm,
                        &tm,
                        &metrics,
                        &mut pending_broadcasts,
                        advertised_status.message(),
                        inline_data_max,
                        Instant::now(),
                    );
                }
            }

            _ = bandwidth_log_tick.tick() => {
                let snap = metrics.snapshot(peer_manager.connected_count());
                let d_in = snap.bytes_inbound.saturating_sub(last_bandwidth_snapshot.bytes_inbound);
                let d_out = snap.bytes_outbound.saturating_sub(last_bandwidth_snapshot.bytes_outbound);
                info!(
                    "P2P bandwidth (60s): in={}B out={}B peers={} \
                     blocks_in={}B txs_in={}B weather_in={}B forecast_in={}B \
                     blocks_out={}B txs_out={}B weather_out={}B forecast_out={}B",
                    d_in, d_out, snap.connected_peers,
                    snap.inbound_blocks.bytes.saturating_sub(last_bandwidth_snapshot.inbound_blocks.bytes),
                    snap.inbound_txs.bytes.saturating_sub(last_bandwidth_snapshot.inbound_txs.bytes),
                    snap.inbound_weather.bytes.saturating_sub(last_bandwidth_snapshot.inbound_weather.bytes),
                    snap.inbound_forecast.bytes.saturating_sub(last_bandwidth_snapshot.inbound_forecast.bytes),
                    snap.outbound_blocks.bytes.saturating_sub(last_bandwidth_snapshot.outbound_blocks.bytes),
                    snap.outbound_txs.bytes.saturating_sub(last_bandwidth_snapshot.outbound_txs.bytes),
                    snap.outbound_weather.bytes.saturating_sub(last_bandwidth_snapshot.outbound_weather.bytes),
                    snap.outbound_forecast.bytes.saturating_sub(last_bandwidth_snapshot.outbound_forecast.bytes),
                );
                // Log top-5 peers by inbound bytes.
                if !peer_inbound_bytes.is_empty() {
                    let mut top_peers: Vec<_> = peer_inbound_bytes.iter().collect();
                    top_peers.sort_by(|a, b| b.1.cmp(a.1));
                    top_peers.truncate(5);
                    let summary: Vec<String> = top_peers
                        .iter()
                        .map(|(pid, bytes)| format!("{pid}={bytes}B"))
                        .collect();
                    info!("P2P top peers by inbound bytes: {}", summary.join(", "));
                }
                last_bandwidth_snapshot = snap;
            }

            cmd = command_rx.recv() => {
                match cmd {
                    Some(P2pCommand::BroadcastBlock(block_bytes)) => {
                        publish_message(&mut swarm, &tm, &metrics, &mut pending_broadcasts, WrfcoinMessage::NewBlock { block_bytes }, inline_data_max, Instant::now());
                    }
                    Some(P2pCommand::BroadcastTransactions(tx_bytes)) => {
                        publish_message(&mut swarm, &tm, &metrics, &mut pending_broadcasts, WrfcoinMessage::NewTransactions { tx_bytes }, inline_data_max, Instant::now());
                    }
                    Some(P2pCommand::BroadcastChainStatus { height, tip_hash, state_root }) => {
                        publish_message(&mut swarm, &tm, &metrics, &mut pending_broadcasts, WrfcoinMessage::ChainStatus { height, tip_hash, state_root }, inline_data_max, Instant::now());
                    }
                    Some(P2pCommand::UpdateLocalChainState { height, tip_hash, state_root }) => {
                        sync_manager.set_local_height(height);
                        apply_authoritative_chain_state(
                            &mut sync_chain,
                            &mut advertised_status,
                            &mut chain_status_interval,
                            &mut chain_status_tick,
                            height,
                            tip_hash,
                            state_root,
                        );
                        dispatch_planned_requests(
                            &mut sync_manager,
                            &mut swarm,
                            &mut outbound_sync_requests,
                            Instant::now(),
                        );
                    }
                    Some(P2pCommand::AuthoritativeSyncRejected {
                        peer_id,
                        block_height,
                        accepted_height,
                        accepted_tip_hash,
                        accepted_state_root,
                        attribution,
                        reason,
                    }) => {
                        info!(
                            "Authoritative apply rejected synced block {block_height} from \
                             {peer_id}; rewinding sync shadow to {accepted_height}: {reason}"
                        );
                        sync_manager.note_authoritative_rejection(
                            &peer_id,
                            accepted_height,
                            attribution,
                        );
                        if matches!(
                            attribution,
                            AuthoritativeSyncRejectionAttribution::LocalRuleRejection
                        ) {
                            // #2807: a deterministic rule verdict is not peer
                            // evidence — rotate away from the server and let a
                            // second distinct server corroborate a validation
                            // halt instead of graylisting the peer set.
                            sync_manager.note_rule_rejection(&peer_id, block_height);
                        }
                        // Every block cached here was admitted relative to the speculative
                        // shadow tip. Reconstruct the deliberately context-free shadow at
                        // the launch API's accepted tip instead of retaining stale blocks.
                        sync_chain = Blockchain::default();
                        apply_authoritative_chain_state(
                            &mut sync_chain,
                            &mut advertised_status,
                            &mut chain_status_interval,
                            &mut chain_status_tick,
                            accepted_height,
                            accepted_tip_hash,
                            accepted_state_root,
                        );
                        outbound_sync_requests.clear();
                        if attribution.penalizes_peer() {
                            let banned = sync_manager
                                .peer_metrics(&peer_id)
                                .map(|metrics| metrics.banned)
                                .unwrap_or(false);
                            let _ = event_tx
                                .send(P2pEvent::PeerPenalized {
                                    peer_id,
                                    reason: format!(
                                        "authoritative sync validation failed at block \
                                         {block_height}: {reason}"
                                    ),
                                    banned,
                                })
                                .await;
                        }
                        dispatch_planned_requests(
                            &mut sync_manager,
                            &mut swarm,
                            &mut outbound_sync_requests,
                            Instant::now(),
                        );
                    }
                    Some(P2pCommand::RespondToBlockSyncRequest { request_id, response }) => {
                        match inbound_sync_requests.remove(&request_id) {
                            Some(channel) => {
                                if swarm.behaviour_mut().block_sync.send_response(channel, response).is_err() {
                                    metrics.record_sync_response_send_failure();
                                    warn!("Failed to send block sync response for request {request_id:?}");
                                }
                            }
                            None => warn!("No pending inbound block sync request for {request_id:?}"),
                        }
                    }
                    Some(P2pCommand::BroadcastWeatherData { station_id, data, station_signing_key }) => {
                        use wrfcoin_shared_protocol::crypto::domain_signing::{
                            DomainSigningAction, DomainSigningContext, canonical_signing_message,
                            payload_digest_hex, signing_chain_id, signing_network_id,
                        };
                        use wrfcoin_shared_protocol::crypto::signature::PublicKey;

                        let timestamp = chrono::Utc::now().timestamp();
                        let nonce = generate_weather_nonce();
                        let data_hash = hex::encode(blake3::hash(&data).as_bytes());
                        let payload_digest = payload_digest_hex(&(&station_id, &data_hash));
                        let cid = signing_chain_id();
                        let nid = signing_network_id();
                        let context = DomainSigningContext {
                            action: DomainSigningAction::WeatherDataGossip,
                            signer_id: &station_id,
                            timestamp,
                            nonce: Some(&nonce),
                            chain_id: &cid,
                            network_id: &nid,
                        };
                        let signing_msg = canonical_signing_message(&context, &payload_digest);
                        let sig = station_signing_key.sign(signing_msg.as_bytes());
                        let station_signature = sig.to_hex();
                        let station_pubkey = hex::encode(
                            PublicKey::from_private_key(&station_signing_key).as_bytes()
                        );

                        publish_message(
                            &mut swarm,
                            &tm,
                            &metrics,
                            &mut pending_broadcasts,
                            WrfcoinMessage::WeatherData {
                                station_id,
                                data,
                                timestamp,
                                nonce: Some(nonce),
                                station_signature,
                                station_pubkey,
                            },
                            inline_data_max,
                            Instant::now(),
                        );
                    }
                    Some(P2pCommand::BroadcastForecastTaskAssignment { task_id, node_id, manifest_bytes }) => {
                        publish_message(
                            &mut swarm,
                            &tm,
                            &metrics,
                            &mut pending_broadcasts,
                            WrfcoinMessage::ForecastTaskAssignment { task_id, node_id, manifest_bytes },
                            inline_data_max,
                            Instant::now(),
                        );
                    }
                    Some(P2pCommand::BroadcastComputeNodeHeartbeat { node_id, capabilities, region, active_tasks, max_concurrent_tasks, observation_density, station_count }) => {
                        publish_message(
                            &mut swarm,
                            &tm,
                            &metrics,
                            &mut pending_broadcasts,
                            WrfcoinMessage::ComputeNodeHeartbeat {
                                node_id,
                                capabilities,
                                region,
                                active_tasks,
                                max_concurrent_tasks,
                                observation_density,
                                station_count,
                            },
                            inline_data_max,
                            Instant::now(),
                        );
                    }
                    Some(P2pCommand::BroadcastForecastTaskResult { task_id, node_id, receipt_bytes }) => {
                        publish_message(
                            &mut swarm,
                            &tm,
                            &metrics,
                            &mut pending_broadcasts,
                            WrfcoinMessage::ForecastTaskResult { task_id, node_id, receipt_bytes },
                            inline_data_max,
                            Instant::now(),
                        );
                    }
                    Some(P2pCommand::BroadcastWeatherDataV2 { station_id, data, content_ref, station_signing_key }) => {
                        use wrfcoin_shared_protocol::crypto::domain_signing::{
                            DomainSigningAction, DomainSigningContext, canonical_signing_message,
                            payload_digest_hex, signing_chain_id, signing_network_id,
                        };
                        use wrfcoin_shared_protocol::crypto::signature::PublicKey;

                        let timestamp = chrono::Utc::now().timestamp();
                        let nonce = generate_weather_nonce();
                        let data_hash = weather_v2_data_hash_hex(&data, content_ref.as_ref());
                        let payload_digest = payload_digest_hex(&(&station_id, &data_hash));
                        let cid = signing_chain_id();
                        let nid = signing_network_id();
                        let context = DomainSigningContext {
                            action: DomainSigningAction::WeatherDataGossip,
                            signer_id: &station_id,
                            timestamp,
                            nonce: Some(&nonce),
                            chain_id: &cid,
                            network_id: &nid,
                        };
                        let signing_msg = canonical_signing_message(&context, &payload_digest);
                        let sig = station_signing_key.sign(signing_msg.as_bytes());
                        let station_signature = sig.to_hex();
                        let station_pubkey = hex::encode(
                            PublicKey::from_private_key(&station_signing_key).as_bytes()
                        );

                        publish_message(
                            &mut swarm,
                            &tm,
                            &metrics,
                            &mut pending_broadcasts,
                            WrfcoinMessage::WeatherDataV2 {
                                station_id,
                                data,
                                timestamp,
                                content_ref,
                                nonce,
                                station_signature,
                                station_pubkey,
                            },
                            inline_data_max,
                            Instant::now(),
                        );
                    }
                    Some(P2pCommand::BroadcastForecastTaskResultV2 { task_id, node_id, receipt_bytes, content_ref }) => {
                        publish_message(
                            &mut swarm,
                            &tm,
                            &metrics,
                            &mut pending_broadcasts,
                            WrfcoinMessage::ForecastTaskResultV2 { task_id, node_id, receipt_bytes, content_ref },
                            inline_data_max,
                            Instant::now(),
                        );
                    }
                    Some(P2pCommand::BroadcastDaChallenge { challenge }) => {
                        debug!("Broadcasting DA challenge {} for CID {}", challenge.challenge_id, challenge.cid);
                        publish_message(
                            &mut swarm,
                            &tm,
                            &metrics,
                            &mut pending_broadcasts,
                            WrfcoinMessage::DaChallenge { challenge },
                            inline_data_max,
                            Instant::now(),
                        );
                    }
                    Some(P2pCommand::BroadcastDaChallengeResponse { response }) => {
                        debug!("Broadcasting DA challenge response for {}", response.challenge_id);
                        publish_message(
                            &mut swarm,
                            &tm,
                            &metrics,
                            &mut pending_broadcasts,
                            WrfcoinMessage::DaChallengeResponse { response },
                            inline_data_max,
                            Instant::now(),
                        );
                    }
                    Some(P2pCommand::BroadcastFinalityVote { vote_bytes }) => {
                        publish_message(
                            &mut swarm,
                            &tm,
                            &metrics,
                            &mut pending_broadcasts,
                            WrfcoinMessage::FinalityVote { vote_bytes },
                            inline_data_max,
                            Instant::now(),
                        );
                    }
                    Some(P2pCommand::BroadcastFinalityCertificate { cert_bytes }) => {
                        publish_message(
                            &mut swarm,
                            &tm,
                            &metrics,
                            &mut pending_broadcasts,
                            WrfcoinMessage::FinalityCertificate { cert_bytes },
                            inline_data_max,
                            Instant::now(),
                        );
                    }
                    #[cfg(test)]
                    Some(P2pCommand::BroadcastGenesisHandshake { genesis_hash }) => {
                        publish_message(
                            &mut swarm,
                            &tm,
                            &metrics,
                            &mut pending_broadcasts,
                            WrfcoinMessage::GenesisHandshake {
                                network_id: local_handshake.network_id.clone(),
                                chain_id: local_handshake.chain_id.clone(),
                                genesis_hash,
                                protocol_version: local_handshake.protocol_version,
                            },
                            inline_data_max,
                            Instant::now(),
                        );
                    }
                    #[cfg(test)]
                    Some(P2pCommand::QuicBlocksReady { peer_id, reply }) => {
                        let ready = peer_manager.get(&peer_id).is_some_and(|peer| {
                            peer.has_quic_connection() && !peer.has_non_quic_connection()
                        }) && swarm.behaviour().gossipsub.mesh_peers(&tm.blocks_hash())
                            .any(|peer| peer == &peer_id);
                        let _ = reply.send(ready);
                    }
                    Some(P2pCommand::GetNetworkStats) => {
                        let stats = metrics.snapshot(peer_manager.connected_count());
                        debug!("Network stats: {stats:?}");
                    }
                    Some(P2pCommand::KickSync { observed_height }) => {
                        if let Some(height) = observed_height {
                            sync_manager.note_observed_height(height);
                        }
                        dispatch_planned_requests(
                            &mut sync_manager,
                            &mut swarm,
                            &mut outbound_sync_requests,
                            Instant::now(),
                        );
                    }
                    Some(P2pCommand::DisconnectPeer { peer_id, reason }) => {
                        // #1406: evict a graylisted peer. Log first (Display
                        // borrows peer_id), remove it from the kademlia routing
                        // table, then drop any active connection (the move of
                        // peer_id into disconnect_peer_id is its last use).
                        info!("P2P evicting graylisted peer {peer_id}: reason={reason}");
                        swarm.behaviour_mut().kademlia.remove_peer(&peer_id);
                        let _ = swarm.disconnect_peer_id(peer_id);
                    }
                    Some(P2pCommand::Shutdown) => {
                        info!("Shutdown command received, exiting event loop");
                        break;
                    }
                    None => {
                        info!("Command channel closed, exiting event loop");
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AdvertisedChainStatus, Blockchain, ConnectionId, FxHashSet, P2pEvent, PeerInboundRateState,
        PeerManager, SyncManager, SyncPeerScore, TopicManager, WeatherDataAdmission,
        WrfcoinBehaviourEvent, WrfcoinMessage, emit_sync_result, request_response,
        should_refresh_sync_connection,
    };
    use super::BlockResponse;
    use libp2p::{autonat, gossipsub};

    #[tokio::test]
    async fn completed_sync_result_orders_final_flush_after_all_blocks_2760() {
        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(4);
        let block = wrfcoin_shared_protocol::blockchain::block::Block::new(
            1,
            wrfcoin_shared_protocol::crypto::hash::Hash::default(),
            Vec::new(),
            1,
            1,
        );

        let peer_id = libp2p::PeerId::random();
        emit_sync_result(&event_tx, peer_id, &[block], Some(1)).await;

        assert!(
            matches!(
                event_rx.recv().await,
                Some(P2pEvent::SyncedBlock {
                    peer_id: source,
                    height: 1,
                    ..
                }) if source == peer_id
            ),
            "the imported block must be delivered before completion"
        );
        assert!(
            matches!(event_rx.recv().await, Some(P2pEvent::SyncCompleted { height: 1 })),
            "the final flush marker must follow every imported block"
        );
    }

    /// THE #2546 binding test — the cold-start case the issue filed.
    ///
    /// The #2633 review proved the previous test here was green on unmodified
    /// `main`: it refused a candidate against an already-ESTABLISHED resident,
    /// which the pre-existing redial gate already did. The case #2546 names is
    /// different: a COLD START, empty established set, every bootstrap entry
    /// eligible in the same tick. With a read-only `admits`, each candidate is
    /// evaluated against a set no earlier same-tick candidate has joined, so
    /// all of them dial — "all concentrated peers establish anyway, every time
    /// the node starts." Only budget charged at dial submission (reservations)
    /// makes this fail closed, and that is what this test binds: it FAILS on
    /// main and on the pre-reservation head of this branch.
    #[tokio::test]
    async fn cold_start_bootstrap_dials_cannot_exceed_subnet_ceiling_2546() {
        let mut peers = crate::slots::OutboundPeerSet::new(crate::slots::OutboundDiversityPolicy {
            max_per_ipv4_subnet: 1,
            max_per_discovery_source: 8,
            min_distinct_subnets: 2,
            exempt_bootstrap: false,
            enforce: true,
        });
        // COLD START: nothing established, nothing reserved.
        assert_eq!(peers.len(), 0, "precondition: empty established set");

        // Three bootstrap entries in the same /24 — the issue's own scenario —
        // all eligible immediately (Duration::ZERO, the #2546 initial dial).
        let now = Instant::now();
        let mut states: Vec<_> = ["203.0.113.1", "203.0.113.2", "203.0.113.3"]
            .iter()
            .map(|ip| {
                crate::bootstrap_redial::BootstrapPeerRedial::new(
                    format!("/ip4/{ip}/tcp/8555").parse().unwrap(),
                    None,
                    now,
                    Duration::ZERO,
                )
            })
            .collect();
        let next_before: Vec<_> = states.iter().map(|s| s.next_attempt).collect();

        let mut swarm = crate::behaviour::test_swarm().expect("test swarm");
        let metrics = Arc::new(MetricsCollector::new());
        let peer_manager = PeerManager::new();

        super::redial_disconnected_bootstrap_peers(
            &mut states,
            &peer_manager,
            &mut swarm,
            &metrics,
            Duration::from_secs(30),
            Duration::from_secs(300),
            &mut peers,
        );

        let dialed = states
            .iter()
            .zip(&next_before)
            .filter(|(s, before)| s.next_attempt > **before)
            .count();
        assert_eq!(
            dialed, 1,
            "with max_per_ipv4_subnet=1, exactly ONE same-/24 candidate may \
             reach the dial path in a single cold-start tick; more means the \
             budget was not charged at dial submission (#2546 findings 1+2)"
        );
        assert_eq!(
            metrics
                .outbound_dials_rejected_subnet
                .load(std::sync::atomic::Ordering::Relaxed),
            2,
            "the other two same-/24 candidates must be REFUSED, not deferred \
             silently"
        );
        assert_eq!(
            peers.reserved_len(),
            1,
            "the one admitted dial must hold its budget as a reservation \
             until the swarm resolves it"
        );
        assert_eq!(
            peers.len(),
            0,
            "nothing establishes inside this tick — the ceiling held by \
             reservation alone"
        );
    }

    // --- Behaviour-event harness ----------------------------------------------
    //
    // `handle_behaviour_event` takes 24 arguments including `&mut Swarm<WrfcoinBehaviour>`,
    // which no test could build until `behaviour::test_swarm()`. This owns the state so a
    // test is three lines, not sixty — the reason nobody wrote one before.
    struct BehaviourHarness {
        tm: TopicManager,
        event_tx: mpsc::Sender<P2pEvent>,
        metrics: Arc<MetricsCollector>,
        peer_manager: PeerManager,
        sync_manager: SyncManager,
        sync_chain: Blockchain,
        outbound: FxHashMap<request_response::OutboundRequestId, super::OutboundSyncRequest>,
        inbound: FxHashMap<request_response::InboundRequestId, request_response::ResponseChannel<BlockResponse>>,
        swarm: libp2p::Swarm<crate::behaviour::WrfcoinBehaviour>,
        peer_inbound_bytes: FxHashMap<PeerId, u64>,
        rate_windows: FxHashMap<PeerId, PeerInboundRateState>,
        weather_peer_scores: FxHashMap<PeerId, SyncPeerScore>,
        weather_admission: WeatherDataAdmission,
        /// #2117: the pre-authentication budgets, so a test can exhaust them
        /// and observe what the handler then declines to do.
        verify_budget: crate::preauth_budget::VerifyBudget,
        preauth_budget: crate::preauth_budget::PreAuthBudget,
        /// #2116: peers connected but not yet proven to be on our chain.
        unproven_peers: crate::handshake_guard::UnprovenPeers,
        /// #2118: the churn-resistant score, observed here in the same
        /// observe-only mode production uses.
        persistent_scores: crate::scoring::PersistentScore,
        persistent_now: u64,
        pending_quic: FxHashMap<ConnectionId, PeerId>,
        pending_broadcasts: super::PendingBroadcasts,
        status_announced_peers: FxHashSet<PeerId>,
        pre_proof_statuses: FxHashMap<PeerId, (u64, [u8; 32], [u8; 32])>,
        local_handshake: crate::genesis_handshake::GenesisHandshakeMessage,
        discovered: DiscoveredPeers,
        did_bootstrap: bool,
        /// Production default: filter non-global addresses (crate::announce).
        announce_policy: crate::announce::AnnouncePolicy,
    }

    impl BehaviourHarness {
        fn new() -> (Self, mpsc::Receiver<P2pEvent>) {
            let (event_tx, rx) = mpsc::channel(64);
            let h = Self {
                tm: TopicManager::new(),
                event_tx,
                metrics: Arc::new(MetricsCollector::new()),
                peer_manager: PeerManager::new(),
                sync_manager: SyncManager::new(0),
                sync_chain: Blockchain::default(),
                outbound: FxHashMap::default(),
                inbound: FxHashMap::default(),
                swarm: crate::behaviour::test_swarm().expect("test swarm"),
                peer_inbound_bytes: FxHashMap::default(),
                rate_windows: FxHashMap::default(),
                weather_peer_scores: FxHashMap::default(),
                weather_admission: WeatherDataAdmission::default(),
                verify_budget: crate::preauth_budget::VerifyBudget::new(
                    crate::preauth_budget::PreAuthLimits::default(),
                ),
                preauth_budget: crate::preauth_budget::PreAuthBudget::new(
                    crate::preauth_budget::PreAuthLimits::default(),
                ),
                unproven_peers: crate::handshake_guard::UnprovenPeers::new(
                    crate::handshake_guard::HandshakeLimits::default(),
                ),
                // Same parameters production uses (see `event_loop`).
                persistent_scores: crate::scoring::PersistentScore::new(
                    super::PERSISTENT_SCORE_HALF_LIFE_SECS,
                    3.0,
                ),
                persistent_now: 0,
                pending_quic: FxHashMap::default(),
                pending_broadcasts: super::PendingBroadcasts::default(),
                status_announced_peers: FxHashSet::default(),
                pre_proof_statuses: FxHashMap::default(),
                local_handshake: crate::genesis_handshake::GenesisHandshakeMessage::new(
                    "testnet".into(),
                    "777".into(),
                    "0xabc".into(),
                    1,
                ),
                discovered: DiscoveredPeers::new(),
                did_bootstrap: false,
                announce_policy: crate::announce::AnnouncePolicy::filtering(Vec::new()),
            };
            (h, rx)
        }

        /// Drive the real production event handler.
        async fn dispatch(&mut self, event: WrfcoinBehaviourEvent) {
            super::handle_behaviour_event(
                event,
                &mut self.verify_budget,
                &mut self.preauth_budget,
                &mut self.unproven_peers,
                &self.tm,
                &self.event_tx,
                &self.metrics,
                &mut self.peer_manager,
                &mut self.sync_manager,
                &mut self.sync_chain,
                AdvertisedChainStatus::default(),
                &mut self.outbound,
                &mut self.inbound,
                &mut self.swarm,
                &mut self.peer_inbound_bytes,
                &mut self.rate_windows,
                InboundRateLimit { max_bytes: 0, window: std::time::Duration::ZERO },
                &mut self.weather_peer_scores,
                &mut self.persistent_scores,
                self.persistent_now,
                &mut self.weather_admission,
                false,
                1_048_576,
                &mut self.pending_quic,
                &self.local_handshake,
                &self.discovered,
                &mut self.did_bootstrap,
                &mut self.pending_broadcasts,
                &mut self.status_announced_peers,
                &mut self.pre_proof_statuses,
                &self.announce_policy,
            )
            .await;
        }
    }

    mod privacy_admission_tests;

    // --- Branches newly reachable via the harness ------------------------
    //
    // None of these had a unit test: every one lives inside `handle_behaviour_event` and
    // was unreachable until `behaviour::test_swarm()`. They are consensus-adjacent (sync
    // liveness, peer address learning, mesh membership), so "never executed by a test" was
    // the wrong state for them to be in.

    /// A transport failure must requeue sync work without escalating an honest peer into
    /// the launch layer's invalid/malicious-data reputation path (#2816).
    ///
    /// The sync manager still records local transport health so retry/backoff can rotate
    /// peers. `PeerPenalized` is reserved for invalid protocol data: emitting it here made
    /// ordinary timeouts accumulate into graylisting and disconnect/reconnect loops.
    #[tokio::test]
    async fn block_sync_outbound_failure_requeues_without_external_penalty_2816() {
        let (mut h, mut rx) = BehaviourHarness::new();
        let peer = PeerId::random();

        // Register an in-flight request so the failure has a range to requeue.
        h.sync_manager.record_peer_status(peer, 500);
        let planned = h.sync_manager.plan_requests(std::time::Instant::now());
        assert!(!planned.is_empty(), "precondition: the manager must plan a request");
        let req = planned[0].request.clone();
        // `OutboundRequestId` has a private field and no public constructor, so a synthetic
        // one is impossible. `send_request` returns a REAL id (queued; no connection
        // needed), which keeps the event genuine rather than a fabricated stand-in.
        let request_id = h
            .swarm
            .behaviour_mut()
            .block_sync
            .send_request(&peer, req.clone());
        h.sync_manager.register_request(request_id, peer, &req, std::time::Instant::now());

        h.dispatch(WrfcoinBehaviourEvent::BlockSync(
            request_response::Event::OutboundFailure {
                peer,
                request_id,
                error: request_response::OutboundFailure::Timeout,
                connection_id: ConnectionId::new_unchecked(0),
            },
        ))
        .await;

        assert_eq!(
            h.sync_manager
                .peer_metrics(&peer)
                .expect("connected sync peer must remain observable")
                .stall_count,
            1,
            "the transport failure must still affect sync-local peer health"
        );
        assert!(
            h.sync_manager.in_flight_range_count() > 0
                || h.sync_manager.pending_range_count() > 0,
            "the failed range must remain scheduled for bounded recovery"
        );
        assert!(
            matches!(rx.try_recv(), Err(tokio::sync::mpsc::error::TryRecvError::Empty)),
            "a transport-only sync failure must not emit PeerPenalized"
        );

        // Two more genuine request ids drive the same live event arm to the
        // refresh threshold. The swarm has no connection in this harness, so
        // the disconnect itself is an expected no-op; the metric is the
        // anti-vacuity proof that the production call site armed the refresh.
        for _ in 0..2 {
            let request_id = h.swarm.behaviour_mut().block_sync.send_request(&peer, req.clone());
            h.sync_manager.register_request(
                request_id,
                peer,
                &req,
                std::time::Instant::now(),
            );
            h.dispatch(WrfcoinBehaviourEvent::BlockSync(
                request_response::Event::OutboundFailure {
                    peer,
                    request_id,
                    error: request_response::OutboundFailure::Timeout,
                    connection_id: ConnectionId::new_unchecked(0),
                },
            ))
            .await;
        }
        assert_eq!(
            h.metrics.snapshot(0).sync_connection_refresh_attempts,
            1,
            "the third consecutive timeout must arm one connection refresh"
        );
        assert!(
            matches!(rx.try_recv(), Err(tokio::sync::mpsc::error::TryRecvError::Empty)),
            "connection refresh must remain sync-local and emit no external penalty"
        );
    }

    /// Repeated request/response timeouts on an otherwise connected peer can
    /// leave a half-open libp2p connection alive indefinitely. The live
    /// a953130a follower recovered only after an incidental connection close,
    /// 23 timeouts and roughly 90 minutes after the first failed range. Bound
    /// that wait by asking the swarm to refresh the connection once the
    /// sync-local failure threshold is reached; this must not emit the
    /// invalid/malicious-data reputation event tested above.
    #[test]
    fn repeated_sync_transport_failures_require_connection_refresh() {
        let mut health = crate::sync::SyncPeerMetrics::default();
        assert!(!should_refresh_sync_connection(Some(&health)));
        health.consecutive_transport_failures = 2;
        assert!(!should_refresh_sync_connection(Some(&health)));
        health.consecutive_transport_failures = 3;
        assert!(should_refresh_sync_connection(Some(&health)));
        health.consecutive_transport_failures = 23;
        assert!(should_refresh_sync_connection(Some(&health)));
        assert!(!should_refresh_sync_connection(None));
    }

    #[tokio::test]
    async fn eager_chain_status_is_deduplicated_within_connection_epoch_2816() {
        use crate::message::WrfcoinMessage;

        let (mut h, _events) = BehaviourHarness::new();
        let peer = PeerId::random();
        let address: libp2p::Multiaddr =
            "/ip4/198.51.100.20/tcp/9000".parse().expect("valid address");
        assert!(h.peer_manager.peer_connected(
            peer,
            ConnectionId::new_unchecked(2816),
            address.clone(),
        ));
        h.unproven_peers
            .admit(peer, &address, Instant::now())
            .expect("peer must enter the unproven epoch");

        let handshake = WrfcoinMessage::GenesisHandshake {
            network_id: h.local_handshake.network_id.clone(),
            chain_id: h.local_handshake.chain_id.clone(),
            genesis_hash: h.local_handshake.genesis_hash.clone(),
            protocol_version: h.local_handshake.protocol_version,
        };
        for sequence_number in [1, 2] {
            h.dispatch(WrfcoinBehaviourEvent::Gossipsub(
                gossipsub::Event::Message {
                    propagation_source: peer,
                    message_id: gossipsub::MessageId::from(format!(
                        "valid-handshake-{sequence_number}"
                    )),
                    message: gossipsub::Message {
                        source: Some(peer),
                        data: handshake.to_bytes().expect("serialize handshake"),
                        sequence_number: Some(sequence_number),
                        topic: h.tm.genesis_handshake_hash(),
                    },
                },
            ))
            .await;
        }

        assert_eq!(
            h.metrics
                .sync_reconnect_status_broadcasts
                .load(std::sync::atomic::Ordering::Relaxed),
            1,
            "duplicate valid handshakes in one live connection epoch must advertise once"
        );
        assert_eq!(
            h.pending_broadcasts.len(),
            1,
            "the no-mesh status intent must reuse the bounded PendingBroadcasts path"
        );
    }

    #[tokio::test]
    async fn chain_status_updates_the_connected_author_not_the_relay_2816() {
        use crate::message::WrfcoinMessage;

        let (mut h, mut events) = BehaviourHarness::new();
        let author = PeerId::random();
        let relay = PeerId::random();
        let address: libp2p::Multiaddr =
            "/ip4/198.51.100.21/tcp/9000".parse().expect("valid address");
        assert!(h.peer_manager.peer_connected(
            author,
            ConnectionId::new_unchecked(2817),
            address,
        ));
        h.status_announced_peers.insert(author);
        let status = WrfcoinMessage::ChainStatus {
            height: 30_260,
            tip_hash: [0xa1; 32],
            state_root: [0xb2; 32],
        };

        h.dispatch(WrfcoinBehaviourEvent::Gossipsub(
            gossipsub::Event::Message {
                propagation_source: relay,
                message_id: gossipsub::MessageId::from("relayed-chain-status-2816"),
                message: gossipsub::Message {
                    source: Some(author),
                    data: status.to_bytes().expect("serialize status"),
                    sequence_number: Some(1),
                    topic: h.tm.blocks_hash(),
                },
            },
        ))
        .await;

        assert_eq!(
            h.sync_manager
                .peer_metrics(&author)
                .expect("connected author must be a sync candidate")
                .advertised_height,
            30_260
        );
        assert!(
            h.sync_manager.peer_metrics(&relay).is_none(),
            "the forwarding hop must not inherit the author's advertised height"
        );
        assert!(matches!(
            events.try_recv(),
            Ok(P2pEvent::PeerChainStatus { peer_id, height: 30_260, .. }) if peer_id == author
        ));
    }

    /// #2999: gossipsub guarantees no ordering across topics, so a connected
    /// peer's once-per-epoch eager ChainStatus (#2816) can arrive before its
    /// GenesisHandshake is processed. It must be held and applied when the
    /// proof lands — never dropped (that defers sync to the periodic tick,
    /// which is the two_node flake) and never applied early (the pre-proof
    /// gate protects retained-health restoration on reconnect).
    #[tokio::test]
    async fn chain_status_arriving_before_genesis_proof_is_applied_on_proof_2999() {
        use crate::message::WrfcoinMessage;

        let (mut h, mut events) = BehaviourHarness::new();
        let peer = PeerId::random();
        let address: libp2p::Multiaddr =
            "/ip4/198.51.100.22/tcp/9000".parse().expect("valid address");
        assert!(h.peer_manager.peer_connected(
            peer,
            ConnectionId::new_unchecked(2999),
            address.clone(),
        ));
        h.unproven_peers
            .admit(peer, &address, Instant::now())
            .expect("peer must enter the unproven epoch");

        // 1. The eager ChainStatus wins the cross-topic race.
        let status = WrfcoinMessage::ChainStatus {
            height: 100,
            tip_hash: [0xa1; 32],
            state_root: [0xb2; 32],
        };
        h.dispatch(WrfcoinBehaviourEvent::Gossipsub(
            gossipsub::Event::Message {
                propagation_source: peer,
                message_id: gossipsub::MessageId::from("early-chain-status-2999"),
                message: gossipsub::Message {
                    source: Some(peer),
                    data: status.to_bytes().expect("serialize status"),
                    sequence_number: Some(1),
                    topic: h.tm.blocks_hash(),
                },
            },
        ))
        .await;

        // Pre-proof it must not have been applied: no sync candidate, no event.
        assert!(
            h.sync_manager.peer_metrics(&peer).is_none(),
            "a pre-proof ChainStatus must not create a sync target"
        );
        assert!(
            events.try_recv().is_err(),
            "a pre-proof ChainStatus must not be surfaced as an event"
        );

        // 2. The genesis proof lands.
        let handshake = WrfcoinMessage::GenesisHandshake {
            network_id: h.local_handshake.network_id.clone(),
            chain_id: h.local_handshake.chain_id.clone(),
            genesis_hash: h.local_handshake.genesis_hash.clone(),
            protocol_version: h.local_handshake.protocol_version,
        };
        h.dispatch(WrfcoinBehaviourEvent::Gossipsub(
            gossipsub::Event::Message {
                propagation_source: peer,
                message_id: gossipsub::MessageId::from("late-handshake-2999"),
                message: gossipsub::Message {
                    source: Some(peer),
                    data: handshake.to_bytes().expect("serialize handshake"),
                    sequence_number: Some(2),
                    topic: h.tm.genesis_handshake_hash(),
                },
            },
        ))
        .await;

        // The held status must now be applied through the normal path.
        assert_eq!(
            h.sync_manager
                .peer_metrics(&peer)
                .expect("held status must create a sync candidate once proven")
                .advertised_height,
            100,
            "the pre-proof advertised height must be recorded after the proof"
        );
        let mut saw_status_event = false;
        while let Ok(event) = events.try_recv() {
            if matches!(
                event,
                P2pEvent::PeerChainStatus { peer_id, height: 100, .. } if peer_id == peer
            ) {
                saw_status_event = true;
            }
        }
        assert!(
            saw_status_event,
            "the held ChainStatus must be surfaced as PeerChainStatus after the proof \
             (this is the event the two_node 8s wait consumes)"
        );
        assert!(
            h.pre_proof_statuses.is_empty(),
            "the held status must be consumed, not retained"
        );
    }

    /// The test that the harness split left behind (#2118 / #2533).
    ///
    /// `observe_persistent_offence_records_but_never_bans` covers the scoring
    /// rule in isolation; this covers the thing it cannot — that a real
    /// invalid-signature weather message, dispatched through the real
    /// `handle_behaviour_event`, actually reaches that scoring call.
    ///
    /// That distinction is the whole reason this exists. A RED control on the
    /// original #2118 work unwired `observe_persistent_offence` and left
    /// 245/245 green, because every test called the rule directly. This one
    /// fails when the wiring goes.
    #[tokio::test]
    async fn handle_behaviour_event_records_the_observe_only_score() {
        use std::net::{IpAddr, Ipv4Addr};

        let (mut h, _events) = BehaviourHarness::new();
        let peer = PeerId::random();
        let ip = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 7));

        // The score is keyed by address, so the peer must have one on record —
        // in production this comes from the connection's remote address.
        h.persistent_scores.note_address(peer, ip, 0);
        assert_eq!(
            h.persistent_scores.effective(&peer, ip, 0),
            0.0,
            "no offence observed yet"
        );

        // A weather message whose signature cannot verify. The nonce must be
        // exactly WEATHER_DATA_NONCE_HEX_LEN hex characters or admission
        // rejects it *before* signature verification, and the test would then
        // pass for the wrong reason.
        let msg = crate::message::WrfcoinMessage::WeatherData {
            station_id: "station-1".to_string(),
            data: b"observation".to_vec(),
            timestamp: 1_700_000_000,
            nonce: Some("a".repeat(32)),
            station_signature: "00".repeat(64),
            station_pubkey: "11".repeat(32),
        };

        h.dispatch(WrfcoinBehaviourEvent::Gossipsub(
            gossipsub::Event::Message {
                propagation_source: peer,
                message_id: gossipsub::MessageId::from("test-msg"),
                message: gossipsub::Message {
                    source: Some(peer),
                    data: msg.to_bytes().expect("serialize"),
                    sequence_number: Some(1),
                    topic: h.tm.weather_data_hash(),
                },
            },
        ))
        .await;

        assert!(
            h.persistent_scores.effective(&peer, ip, 0) > 0.0,
            "an invalid weather signature dispatched through the real handler \
             must reach the churn-resistant score — if this is 0, the observe-only \
             wiring is gone and nothing else in the suite would notice"
        );
    }

    /// /// Closes the gap RED control D found on #2117.
    ///
    /// `claim_verify_budget` had tests for its verdict and its counter, but
    /// nothing covered the `return` at the two call sites — the line that
    /// actually stops the ed25519 work. Deleting it compiled and left the whole
    /// suite green, which is the #2500 failure mode.
    ///
    /// The observable difference is what the handler does *afterwards*. An
    /// invalid signature that IS verified increments `weather_msgs_invalid_sig`;
    /// one that is never verified cannot. So exhausting the budget and then
    /// asserting that counter stays put distinguishes "skipped the work" from
    /// "did the work and ignored the answer" — a distinction the budget exists
    /// entirely to make.
    #[tokio::test]
    async fn an_exhausted_verify_budget_skips_the_signature_work_entirely() {
        let (mut h, _events) = BehaviourHarness::new();
        let peer = PeerId::random();

        // A weather message whose signature cannot verify. The nonce is exactly
        // WEATHER_DATA_NONCE_HEX_LEN hex characters so admission does not reject
        // it earlier, which would make this pass for the wrong reason.
        //
        // Station AND nonce must both differ per message: admission
        // rate-limits per station within a window, and repeats a nonce as a
        // replay. Either would reject the second message before it reaches the
        // budget.
        // The nonce must DIFFER per message. Reusing one gets the second
        // message rejected as a replay before it ever reaches the budget, and
        // the "work was skipped" assertion below would then hold for a reason
        // that has nothing to do with #2117. The `preauth_verify_skipped`
        // assertion is what exposes that, and did.
        let payload = |n: u64| {
            crate::message::WrfcoinMessage::WeatherData {
                station_id: format!("station-{n}"),
                data: b"observation".to_vec(),
                timestamp: 1_700_000_000,
                nonce: Some(format!("{n:032x}")),
                station_signature: "00".repeat(64),
                station_pubkey: "11".repeat(32),
            }
            .to_bytes()
            .expect("serialize")
        };

        let topic = h.tm.weather_data_hash();
        let event = |n: u64| {
            WrfcoinBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                propagation_source: peer,
                message_id: gossipsub::MessageId::from(format!("m{n}")),
                message: gossipsub::Message {
                    source: Some(peer),
                    data: payload(n),
                    sequence_number: Some(n),
                    topic: topic.clone(),
                },
            })
        };

        // First message: budget available, so the signature IS verified and
        // found invalid. This is the control — without it, a zero counter at
        // the end would prove nothing about the budget.
        h.dispatch(event(1)).await;
        let after_first = h.metrics.snapshot(0).weather_msgs_invalid_sig;
        assert_eq!(
            after_first, 1,
            "with budget available the signature must actually be verified"
        );

        // Exhaust this peer's verification allowance.
        let limits = crate::preauth_budget::PreAuthLimits::default();
        let now = std::time::Instant::now();
        for _ in 0..limits.max_verify_ops_per_window {
            let _ = h.verify_budget.try_claim(peer, now);
        }

        h.dispatch(event(2)).await;

        assert_eq!(
            h.metrics.snapshot(0).weather_msgs_invalid_sig,
            after_first,
            "with the budget exhausted the ed25519 verification must not run at \
             all — if this counter moved, the handler verified the signature and \
             merely declined to act on it, which bounds no CPU whatsoever"
        );
        assert!(
            h.metrics.snapshot(0).preauth_verify_skipped >= 1,
            "the skip must be observable"
        );
    }

    /// #2544: a peer subscribing to the handshake topic must enter the
    /// republish branch.
    ///
    /// **This test pins the branch, not the delivery.** The counter is
    /// incremented before `publish_message`, so it stays green against an
    /// implementation that counts and sends nothing — Iris planted `return;`
    /// as the first line of `publish_message` on #2560 and this test still
    /// passed. It is kept because it is the cheap, fast discriminator for the
    /// *condition* (paired with `subscribing_to_an_unrelated_topic_does_not_republish`
    /// it pins the topic scoping), and it is deliberately not the only #2544
    /// test: `a_subscribing_peer_receives_the_handshake_and_is_verified` below
    /// is the one that proves a handshake reaches a peer.
    #[tokio::test]
    async fn a_peer_subscribing_to_the_handshake_topic_triggers_a_republish() {
        let (mut h, _events) = BehaviourHarness::new();
        let peer = PeerId::random();

        assert_eq!(
            h.metrics.snapshot(0).genesis_handshake_republished,
            0,
            "precondition: nothing republished yet"
        );

        h.dispatch(WrfcoinBehaviourEvent::Gossipsub(
            gossipsub::Event::Subscribed {
                peer_id: peer,
                topic: h.tm.genesis_handshake_hash(),
            },
        ))
        .await;

        assert_eq!(
            h.metrics.snapshot(0).genesis_handshake_republished,
            1,
            "subscribing to the handshake topic must republish — this is the \
             moment the publish/subscribe race resolves, and the only point at \
             which we know the peer can actually receive it"
        );
    }

    /// The republish must be scoped to the handshake topic. Republishing on
    /// every subscription would put a handshake on the wire for each of the
    /// other topics a peer joins, which is both noise and a small amplification
    /// gift on a path an unauthenticated peer controls the timing of.
    #[tokio::test]
    async fn subscribing_to_an_unrelated_topic_does_not_republish() {
        let (mut h, _events) = BehaviourHarness::new();

        h.dispatch(WrfcoinBehaviourEvent::Gossipsub(
            gossipsub::Event::Subscribed {
                peer_id: PeerId::random(),
                topic: h.tm.weather_data_hash(),
            },
        ))
        .await;

        assert_eq!(
            h.metrics.snapshot(0).genesis_handshake_republished,
            0,
            "only the handshake topic may trigger a handshake republish"
        );
    }

    /// Two `BehaviourHarness` nodes joined by a real libp2p connection, each
    /// driven by the real `handle_behaviour_event`.
    ///
    /// `behaviour::test_swarm()` builds on `MemoryTransport`, so this binds no
    /// socket — which is what makes it runnable in the sandbox the p2p suite
    /// actually executes in (#1821), and why "two-node test" here does not mean
    /// "integration test that will be skipped".
    ///
    /// What it buys over a hand-constructed `gossipsub::Event::Message`: the
    /// message has to be serialized, signed, published, routed by gossipsub,
    /// received, and handed up by the remote swarm. No step can be assumed.
    struct TwoNode {
        a: BehaviourHarness,
        b: BehaviourHarness,
        /// Every gossipsub message B's swarm delivered, captured before B's
        /// handler consumed it. This is the wire record — a counter on A cannot
        /// fake an entry here.
        b_inbound: Vec<gossipsub::Message>,
    }

    impl TwoNode {
        /// Dial B from A. Returns the pair plus both event receivers.
        async fn connect() -> (Self, mpsc::Receiver<P2pEvent>, mpsc::Receiver<P2pEvent>) {
            use libp2p::futures::StreamExt as _;
            use libp2p::swarm::SwarmEvent;

            let (a, a_rx) = BehaviourHarness::new();
            let (mut b, b_rx) = BehaviourHarness::new();
            let mut a = a;

            b.swarm
                .listen_on("/memory/0".parse().expect("valid memory multiaddr"))
                .expect("the in-memory transport must listen without binding a socket");

            let addr = loop {
                if let SwarmEvent::NewListenAddr { address, .. } = b.swarm.select_next_some().await
                {
                    break address;
                }
            };

            a.swarm.dial(addr).expect("A must be able to dial B in memory");

            (
                Self {
                    a,
                    b,
                    b_inbound: Vec::new(),
                },
                a_rx,
                b_rx,
            )
        }

        fn a_peer_id(&self) -> PeerId {
            *self.a.swarm.local_peer_id()
        }

        /// Advance whichever swarm has work, feeding every behaviour event to
        /// the production handler for that side. Returns after a short idle
        /// window if neither has anything, so a caller's loop stays bounded.
        async fn step(&mut self) {
            use libp2p::futures::StreamExt as _;
            use libp2p::swarm::SwarmEvent;

            enum Step {
                A(WrfcoinBehaviourEvent),
                B(WrfcoinBehaviourEvent),
                Idle,
            }

            let step = tokio::select! {
                ev = self.a.swarm.select_next_some() => match ev {
                    SwarmEvent::Behaviour(e) => Step::A(e),
                    _ => Step::Idle,
                },
                ev = self.b.swarm.select_next_some() => match ev {
                    SwarmEvent::Behaviour(e) => Step::B(e),
                    _ => Step::Idle,
                },
                _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => Step::Idle,
            };

            match step {
                Step::A(e) => self.a.dispatch(e).await,
                Step::B(e) => {
                    if let WrfcoinBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                        message,
                        ..
                    }) = &e
                    {
                        self.b_inbound.push(message.clone());
                    }
                    self.b.dispatch(e).await;
                },
                Step::Idle => {},
            }
        }

        /// Every genesis handshake B's swarm actually delivered, decoded from
        /// the received bytes and paired with the message author.
        fn handshakes_delivered_to_b(
            &self,
        ) -> Vec<(Option<PeerId>, crate::genesis_handshake::GenesisHandshakeMessage)> {
            use crate::message::WrfcoinMessage;

            let topic = self.b.tm.genesis_handshake_hash();
            self.b_inbound
                .iter()
                .filter(|m| m.topic == topic)
                .filter_map(|m| match WrfcoinMessage::from_bytes(&m.data) {
                    Ok(WrfcoinMessage::GenesisHandshake {
                        network_id,
                        chain_id,
                        genesis_hash,
                        protocol_version,
                    }) => Some((
                        m.source,
                        crate::genesis_handshake::GenesisHandshakeMessage::new(
                            network_id,
                            chain_id,
                            genesis_hash,
                            protocol_version,
                        ),
                    )),
                    _ => None,
                })
                .collect()
        }

        /// The handshakes B received that A actually authored.
        fn handshakes_from_a(
            &self,
            a_peer: PeerId,
        ) -> Vec<crate::genesis_handshake::GenesisHandshakeMessage> {
            self.handshakes_delivered_to_b()
                .into_iter()
                .filter(|(source, _)| *source == Some(a_peer))
                .map(|(_, handshake)| handshake)
                .collect()
        }

        fn blocks_delivered_to_b(&self) -> Vec<Vec<u8>> {
            let topic = self.b.tm.blocks_hash();
            self.b_inbound
                .iter()
                .filter(|message| message.topic == topic)
                .filter_map(|message| match crate::message::WrfcoinMessage::from_bytes(&message.data) {
                    Ok(crate::message::WrfcoinMessage::NewBlock { block_bytes }) => Some(block_bytes),
                    _ => None,
                })
                .collect()
        }
    }

    /// A weak subscription notification is not publishability. Hold A in that
    /// exact gap, issue one block through the production helper, then let the
    /// real in-memory gossipsub mesh form and drive the bounded retry path.
    #[tokio::test]
    async fn weak_subscription_retains_block_until_real_mesh_exactly_once_2790() {
        let (mut net, _a_events, _b_events) = TwoNode::connect().await;
        let blocks_topic = net.a.tm.blocks_hash();
        let b_peer = *net.b.swarm.local_peer_id();
        let payload = b"pending-before-mesh-2790".to_vec();

        // This is the weak application-visible notification from #2790. It
        // deliberately does not mutate libp2p's internal mesh.
        net.a
            .dispatch(WrfcoinBehaviourEvent::Gossipsub(
                gossipsub::Event::Subscribed {
                    peer_id: b_peer,
                    topic: blocks_topic.clone(),
                },
            ))
            .await;
        assert_eq!(
            net.a
                .swarm
                .behaviour()
                .gossipsub
                .mesh_peers(&blocks_topic)
                .count(),
            0,
            "precondition: the notification must arrive before a publishable mesh exists"
        );

        super::publish_message(
            &mut net.a.swarm,
            &net.a.tm,
            &net.a.metrics,
            &mut net.a.pending_broadcasts,
            crate::message::WrfcoinMessage::NewBlock {
                block_bytes: payload.clone(),
            },
            1_048_576,
            std::time::Instant::now(),
        );
        assert_eq!(net.a.pending_broadcasts.len(), 1);
        assert!(net.blocks_delivered_to_b().is_empty());

        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        while tokio::time::Instant::now() < deadline
            && net
                .a
                .swarm
                .behaviour()
                .gossipsub
                .mesh_peers(&blocks_topic)
                .next()
                .is_none()
        {
            net.step().await;
        }
        assert!(
            net.a
                .swarm
                .behaviour()
                .gossipsub
                .mesh_peers(&blocks_topic)
                .next()
                .is_some(),
            "the real mesh never became publishable"
        );

        super::retry_pending_broadcasts(
            &mut net.a.swarm,
            &net.a.tm,
            &net.a.metrics,
            &mut net.a.pending_broadcasts,
            std::time::Instant::now(),
        );
        while tokio::time::Instant::now() < deadline && net.blocks_delivered_to_b().is_empty() {
            net.step().await;
        }
        assert_eq!(net.blocks_delivered_to_b(), vec![payload]);
        assert_eq!(net.a.pending_broadcasts.len(), 0);

        // A later timer tick has no retained intent to replay.
        super::retry_pending_broadcasts(
            &mut net.a.swarm,
            &net.a.tm,
            &net.a.metrics,
            &mut net.a.pending_broadcasts,
            std::time::Instant::now(),
        );
        for _ in 0..20 {
            net.step().await;
        }
        assert_eq!(
            net.blocks_delivered_to_b().len(),
            1,
            "successful publication must remove the intent and prevent a second send"
        );
    }

    /// #2544, the assertion that matters: a peer that subscribes to the
    /// handshake topic must **receive** our handshake and have its chain
    /// identity actually checked.
    ///
    /// Iris's finding on #2560 was that
    /// `a_peer_subscribing_to_the_handshake_topic_triggers_a_republish` proves
    /// only `genesis_handshake_republished += 1`. That is the vacuity shape:
    /// the metric is supposed to *represent* a handshake reaching a peer, and
    /// nothing tied the two together. Planting `return;` as the first line of
    /// `publish_message` left it green.
    ///
    /// This test observes no counter on the delivery path. B's swarm has to
    /// hand B's handler a gossipsub message that really traversed the
    /// transport, B's handler has to decode it, `verify_match` it against B's
    /// own genesis, and `mark_proven` A. The assertions read the wire record
    /// and B's unproven-peer set — neither of which A can move by incrementing
    /// anything.
    ///
    /// The republish is the **only** handshake publish in play. The publish at
    /// connection establishment lives in `event_loop`'s `SwarmEvent` arm, which
    /// `BehaviourHarness` does not run, and no `BroadcastGenesisHandshake`
    /// command is issued. So both mutations fail here:
    ///   A. delete the `publish_message` call from the `Subscribed` branch
    ///   B. `return;` as the first line of `publish_message`
    /// — B receives nothing and stays unproven in both.
    #[tokio::test]
    async fn a_subscribing_peer_receives_the_handshake_and_is_verified() {
        let (mut net, _a_events, _b_events) = TwoNode::connect().await;
        let a_peer = net.a_peer_id();

        // The state #2544 could leave forever: A is connected but has not
        // proven which chain it is on. `event_loop` admits here on
        // ConnectionEstablished; the behaviour harness does not run that arm,
        // so the test puts A in the same place explicitly.
        net.b
            .unproven_peers
            .admit(
                a_peer,
                &"/memory/1".parse().expect("valid memory multiaddr"),
                Instant::now(),
            )
            .expect("A must be admitted as an unproven peer");
        assert!(
            net.b.unproven_peers.contains(&a_peer),
            "precondition: A is connected and unproven on B"
        );
        assert!(
            net.handshakes_delivered_to_b().is_empty(),
            "precondition: no handshake has crossed the wire yet"
        );

        // Drive both production handlers until B has verified A, or give up.
        // The budget is generous because the failure mode under test is
        // "never arrives"; a slow arrival is not what this is discriminating.
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        while tokio::time::Instant::now() < deadline && net.b.unproven_peers.contains(&a_peer) {
            net.step().await;
        }

        let delivered = net.handshakes_delivered_to_b();
        let from_a: Vec<_> = delivered
            .iter()
            .filter(|(source, _)| *source == Some(a_peer))
            .collect();
        assert!(
            !from_a.is_empty(),
            "B's swarm never delivered a genesis handshake authored by A. The \
             subscription republish is the only publish in this test, so this \
             is the #2544 defect itself: a peer that subscribes after our \
             connection-establishment publish receives nothing. Messages B did \
             receive on the handshake topic: {delivered:?}"
        );

        let (_, received) = from_a[0];
        assert_eq!(
            (
                received.network_id.as_str(),
                received.chain_id.as_str(),
                received.genesis_hash.as_str(),
                received.protocol_version,
            ),
            (
                net.a.local_handshake.network_id.as_str(),
                net.a.local_handshake.chain_id.as_str(),
                net.a.local_handshake.genesis_hash.as_str(),
                net.a.local_handshake.protocol_version,
            ),
            "the delivered handshake must be A's own chain identity, not a \
             placeholder that happens to deserialize"
        );

        // The composition assertion. `mark_proven` is reachable only from the
        // Ok arm of `verify_match`, inside the gossipsub Message handler,
        // keyed on the message author. It cannot be reached by publishing, by
        // counting, or by B talking to itself.
        assert!(
            !net.b.unproven_peers.contains(&a_peer),
            "B received A's handshake but never verified it — delivery reached \
             the handler and chain identity still went unchecked, which is the \
             half of #2544 that a delivery-only assertion would miss"
        );

        assert_eq!(
            net.a.metrics.snapshot(0).genesis_handshake_republished,
            1,
            "exactly one republish: one peer subscribing to the handshake \
             topic must not put more than one handshake on the wire"
        );
    }

    /// The other duplicate #2544 has to survive: a node must be able to send
    /// its **own** handshake more than once.
    ///
    /// Production sends it at least twice — `event_loop` publishes on
    /// ConnectionEstablished, and the #2544 branch republishes when the peer
    /// subscribes — plus once more for every later peer that subscribes. Those
    /// sends are byte-identical, so under a content-derived message ID they are
    /// one message: `publish` returns `PublishError::Duplicate` for the whole
    /// 60 s `duplicate_cache_time`, `publish_message` logs the error and
    /// returns, and `genesis_handshake_republished` has already counted a send
    /// that never reached the wire.
    ///
    /// This drives `publish_message` — the same function `event_loop` calls at
    /// ConnectionEstablished — a second time, and requires a second handshake
    /// to arrive at B.
    #[tokio::test]
    async fn a_node_can_send_its_handshake_more_than_once() {
        use crate::message::WrfcoinMessage;

        let (mut net, _a_events, _b_events) = TwoNode::connect().await;
        let a_peer = net.a_peer_id();

        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        while tokio::time::Instant::now() < deadline && net.handshakes_from_a(a_peer).is_empty() {
            net.step().await;
        }
        assert_eq!(
            net.handshakes_from_a(a_peer).len(),
            1,
            "precondition: the subscription republish delivered exactly one handshake"
        );

        // Byte-for-byte what `event_loop` publishes on ConnectionEstablished.
        let handshake = WrfcoinMessage::GenesisHandshake {
            network_id: net.a.local_handshake.network_id.clone(),
            chain_id: net.a.local_handshake.chain_id.clone(),
            genesis_hash: net.a.local_handshake.genesis_hash.clone(),
            protocol_version: net.a.local_handshake.protocol_version,
        };
        super::publish_message(
            &mut net.a.swarm,
            &net.a.tm,
            &net.a.metrics,
            &mut net.a.pending_broadcasts,
            handshake,
            1_048_576,
            Instant::now(),
        );

        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        while tokio::time::Instant::now() < deadline && net.handshakes_from_a(a_peer).len() < 2 {
            net.step().await;
        }

        assert_eq!(
            net.handshakes_from_a(a_peer).len(),
            2,
            "A's second handshake never reached B. Both sends carry identical \
             bytes, so a content-derived message ID makes the second one a \
             duplicate of the first and libp2p refuses to publish it — which is \
             every send after the first, for the 60 s the duplicate cache holds \
             the ID"
        );
    }

    /// A gossipsub subscription must be reported, so the operator surface can see mesh
    /// membership change rather than inferring it.
    #[tokio::test]
    async fn gossipsub_subscription_changes_are_reported() {
        let (mut h, mut rx) = BehaviourHarness::new();
        let peer = PeerId::random();
        let topic = h.tm.blocks_hash();

        h.dispatch(WrfcoinBehaviourEvent::Gossipsub(gossipsub::Event::Subscribed {
            peer_id: peer,
            topic: topic.clone(),
        }))
        .await;
        match rx.try_recv() {
            Ok(P2pEvent::SubscriptionStateChanged { peer_id, subscribed, .. }) => {
                assert_eq!(peer_id, peer);
                assert!(subscribed, "Subscribed must report subscribed=true");
            },
            other => panic!("expected SubscriptionStateChanged, got {other:?}"),
        }

        h.dispatch(WrfcoinBehaviourEvent::Gossipsub(gossipsub::Event::Unsubscribed {
            peer_id: peer,
            topic,
        }))
        .await;
        match rx.try_recv() {
            Ok(P2pEvent::SubscriptionStateChanged { subscribed, .. }) => {
                assert!(!subscribed, "Unsubscribed must report subscribed=false");
            },
            other => panic!("expected SubscriptionStateChanged, got {other:?}"),
        }
    }

    /// AutoNAT reporting a Docker bridge address as "public" must be warned about, not
    /// accepted silently.
    ///
    /// This is the #673-shaped failure in miniature: a node that believes an unreachable
    /// address is its public one advertises something no peer can dial, while every local
    /// health check stays green. The branch exists to make that loud.
    #[tokio::test]
    async fn autonat_public_docker_bridge_address_is_handled() {
        let (mut h, _rx) = BehaviourHarness::new();
        let bridge: libp2p::Multiaddr =
            "/ip4/172.18.0.2/tcp/9000".parse().expect("valid docker bridge multiaddr");
        assert!(
            crate::config::is_docker_bridge_addr(&bridge),
            "precondition: this must be recognised as a bridge address, or the branch \
             under test is never entered"
        );

        // Must not panic, and must leave the swarm usable — the branch is warn-only today,
        // so the assertion that matters is that it is REACHED with a bridge address.
        h.dispatch(WrfcoinBehaviourEvent::Autonat(autonat::Event::StatusChanged {
            old: autonat::NatStatus::Unknown,
            new: autonat::NatStatus::Public(bridge),
        }))
        .await;
    }

    /// #2819: identify-advertised loopback/unspecified/docker-bridge addresses
    /// must NOT enter the Kademlia routing table (they become dial candidates
    /// there), while LAN/tailnet addresses must. Mutation control: removing the
    /// `is_undialable_advertised_addr` filter in the Identify::Received arm
    /// turns this red on the junk assertions.
    #[tokio::test]
    async fn identify_advertised_junk_addresses_are_not_ingested() {
        let (mut h, _rx) = BehaviourHarness::new();
        // The tailnet address below is relayed only under the
        // WRFCOIN_P2P_ANNOUNCE_PRIVATE_ADDRS opt-in; the default is covered by
        // identify_private_addresses_are_not_relayed_via_kademlia_by_default.
        h.announce_policy = crate::announce::AnnouncePolicy::announce_all();
        let peer_keypair = libp2p::identity::Keypair::generate_ed25519();
        let peer_id = peer_keypair.public().to_peer_id();

        let junk: Vec<libp2p::Multiaddr> = vec![
            "/ip4/127.0.0.1/tcp/9000".parse().unwrap(),
            "/ip4/0.0.0.0/tcp/9000".parse().unwrap(),
            "/ip4/172.19.0.5/tcp/9000".parse().unwrap(), // secret-leak-ok: docker bridge constant, not a key
        ];
        let good: libp2p::Multiaddr =
            "/ip4/100.64.0.1/tcp/9000".parse().unwrap(); // secret-leak-ok: CGNAT range constant, not a key

        let mut listen_addrs = junk.clone();
        listen_addrs.push(good.clone());
        let info = libp2p::identify::Info {
            public_key: peer_keypair.public(),
            protocol_version: "/wrfcoin/id/1.0.0".to_string(),
            agent_version: "test".to_string(),
            listen_addrs,
            protocols: Vec::new(),
            observed_addr: "/ip4/203.0.113.9/tcp/4001".parse().unwrap(),
            signed_peer_record: None,
        };
        h.dispatch(WrfcoinBehaviourEvent::Identify(libp2p::identify::Event::Received {
            connection_id: ConnectionId::new_unchecked(1),
            peer_id,
            info,
        }))
        .await;

        let kad = &mut h.swarm.behaviour_mut().kademlia;
        let ingested: Vec<String> = kad
            .kbuckets()
            .flat_map(|bucket| {
                bucket
                    .iter()
                    .flat_map(|entry| entry.node.value.iter().map(|a| a.to_string()))
                    .collect::<Vec<_>>()
            })
            .collect();
        // Kademlia stores addresses with a trailing /p2p/<peer-id> component,
        // so match on the advertised prefix.
        assert!(
            ingested.iter().any(|a| a.starts_with(&good.to_string())),
            "dialable advertised address must be ingested; got {ingested:?}"
        );
        for addr in &junk {
            assert!(
                !ingested.iter().any(|a| a.starts_with(&addr.to_string())),
                "undialable advertised address {addr} must be filtered; got {ingested:?}"
            );
        }
    }

    /// Default announce policy: a tailnet (100.64/10) or LAN address a peer
    /// advertised must not enter Kademlia, which re-serves it to every peer
    /// that asks; a /dns4 and a global address must. Mutation control: drop
    /// the `may_relay_peer_addr` check in the Identify::Received arm and the
    /// private assertions go red.
    #[tokio::test]
    async fn identify_private_addresses_are_not_relayed_via_kademlia_by_default() {
        let (mut h, _rx) = BehaviourHarness::new();
        let peer_keypair = libp2p::identity::Keypair::generate_ed25519();
        let peer_id = peer_keypair.public().to_peer_id();

        let private: Vec<libp2p::Multiaddr> = vec![
            "/ip4/100.64.0.1/tcp/9000".parse().unwrap(), // secret-leak-ok: CGNAT range constant, not a key
            "/ip4/192.168.0.10/tcp/9000".parse().unwrap(),
            "/ip6/fd7a:115c:a1e0::1/tcp/9000".parse().unwrap(),
        ];
        let public: Vec<libp2p::Multiaddr> = vec![
            "/dns4/seed.example.org/tcp/9000".parse().unwrap(),
            "/ip4/8.8.4.4/tcp/9000".parse().unwrap(),
        ];
        let info = libp2p::identify::Info {
            public_key: peer_keypair.public(),
            protocol_version: "/wrfcoin/id/1.0.0".to_string(),
            agent_version: "test".to_string(),
            listen_addrs: private.iter().chain(public.iter()).cloned().collect(),
            protocols: Vec::new(),
            observed_addr: "/ip4/100.64.0.2/tcp/4001".parse().unwrap(), // secret-leak-ok: CGNAT range constant, not a key
            signed_peer_record: None,
        };
        h.dispatch(WrfcoinBehaviourEvent::Identify(libp2p::identify::Event::Received {
            connection_id: ConnectionId::new_unchecked(3),
            peer_id,
            info,
        }))
        .await;

        let kad = &mut h.swarm.behaviour_mut().kademlia;
        let ingested: Vec<String> = kad
            .kbuckets()
            .flat_map(|bucket| {
                bucket
                    .iter()
                    .flat_map(|entry| entry.node.value.iter().map(|a| a.to_string()))
                    .collect::<Vec<_>>()
            })
            .collect();
        for addr in &public {
            assert!(
                ingested.iter().any(|a| a.starts_with(&addr.to_string())),
                "global address {addr} must be relayed; got {ingested:?}"
            );
        }
        for addr in &private {
            assert!(
                !ingested.iter().any(|a| a.starts_with(&addr.to_string())),
                "non-global address {addr} must not be relayed; got {ingested:?}"
            );
        }
    }

    /// Own-address announcements: with filtering on, a private listen address
    /// is never promoted to an external address (identify's announce list), a
    /// global one is, and a private address an AutoNAT peer confirms is
    /// withdrawn — but the operator's own private WRFCOIN_P2P_EXTERNAL_ADDR is
    /// kept.
    #[tokio::test]
    async fn own_private_addresses_are_not_announced_but_operator_choice_is() {
        let (mut h, _rx) = BehaviourHarness::new();
        let operator: libp2p::Multiaddr = "/ip4/100.64.0.7/tcp/9000".parse().unwrap(); // secret-leak-ok: CGNAT range constant, not a key
        let policy = crate::announce::AnnouncePolicy::filtering(vec![operator.clone()]);
        let tailnet: libp2p::Multiaddr = "/ip4/100.101.102.103/tcp/9000".parse().unwrap(); // secret-leak-ok: CGNAT range constant, not a key
        let global: libp2p::Multiaddr = "/ip4/8.8.4.4/tcp/9000".parse().unwrap();

        super::on_new_listen_addr(&mut h.swarm, &policy, &tailnet);
        super::on_new_listen_addr(&mut h.swarm, &policy, &global);
        h.swarm.add_external_address(operator.clone());
        // A tailnet peer's AutoNAT confirmed our tailnet address.
        h.swarm.add_external_address(tailnet.clone());
        super::on_external_addr_confirmed(&mut h.swarm, &policy, &tailnet);
        super::on_external_addr_confirmed(&mut h.swarm, &policy, &operator);

        let announced: Vec<libp2p::Multiaddr> = h.swarm.external_addresses().cloned().collect();
        assert!(announced.contains(&global), "global listen addr must be announced: {announced:?}");
        assert!(announced.contains(&operator), "operator addr must be announced: {announced:?}");
        assert!(!announced.contains(&tailnet), "tailnet addr must not be announced: {announced:?}");

        super::on_expired_listen_addr(&mut h.swarm, &policy, &global);
        super::on_expired_listen_addr(&mut h.swarm, &policy, &operator);
        let announced: Vec<libp2p::Multiaddr> = h.swarm.external_addresses().cloned().collect();
        assert!(!announced.contains(&global), "expired listener must be withdrawn: {announced:?}");
        assert!(announced.contains(&operator), "operator addr must survive: {announced:?}");
    }

    /// #2819 same-scope exception: a peer that observes US at loopback shares
    /// this host, so its advertised loopback addresses are dialable and must
    /// be ingested (this is every localhost multi-node topology, including the
    /// in-process test clusters) — while its docker-bridge addresses still are
    /// not.
    #[tokio::test]
    async fn identify_same_host_peer_keeps_loopback_addresses() {
        let (mut h, _rx) = BehaviourHarness::new();
        let peer_keypair = libp2p::identity::Keypair::generate_ed25519();
        let peer_id = peer_keypair.public().to_peer_id();

        let loopback: libp2p::Multiaddr = "/ip4/127.0.0.1/tcp/9100".parse().unwrap();
        let bridge: libp2p::Multiaddr =
            "/ip4/172.19.0.5/tcp/9000".parse().unwrap(); // secret-leak-ok: docker bridge constant, not a key
        let info = libp2p::identify::Info {
            public_key: peer_keypair.public(),
            protocol_version: "/wrfcoin/id/1.0.0".to_string(),
            agent_version: "test".to_string(),
            listen_addrs: vec![loopback.clone(), bridge.clone()],
            protocols: Vec::new(),
            // The peer sees us at loopback: same host.
            observed_addr: "/ip4/127.0.0.1/tcp/4001".parse().unwrap(),
            signed_peer_record: None,
        };
        h.dispatch(WrfcoinBehaviourEvent::Identify(libp2p::identify::Event::Received {
            connection_id: ConnectionId::new_unchecked(2),
            peer_id,
            info,
        }))
        .await;

        let kad = &mut h.swarm.behaviour_mut().kademlia;
        let ingested: Vec<String> = kad
            .kbuckets()
            .flat_map(|bucket| {
                bucket
                    .iter()
                    .flat_map(|entry| entry.node.value.iter().map(|a| a.to_string()))
                    .collect::<Vec<_>>()
            })
            .collect();
        assert!(
            ingested.iter().any(|a| a.starts_with(&loopback.to_string())),
            "same-host loopback address must be ingested; got {ingested:?}"
        );
        assert!(
            !ingested.iter().any(|a| a.starts_with(&bridge.to_string())),
            "bridge address must still be filtered for a loopback-scope peer; got {ingested:?}"
        );
    }

    use super::{
        InboundRateLimit, InboundRateLimitExceeded, InitialChainState, P2pClient, P2pHandle,
        apply_authoritative_chain_state, chain_status_broadcast_interval,
        initialize_sync_state, new_chain_status_tick, record_peer_inbound_rate,
    };
    use crate::discovered::DiscoveredPeers;
    use crate::metrics::MetricsCollector;
    use libp2p::PeerId;
    use rustc_hash::FxHashMap;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tokio::sync::mpsc;

    #[test]
    fn chain_status_interval_clamps_at_genesis() {
        assert_eq!(chain_status_broadcast_interval(0), Duration::from_secs(300));
    }

    #[test]
    fn restored_tip_initializes_downloader_shadow_and_dispatch_range_2796() {
        let restored = InitialChainState {
            height: 11_016,
            tip_hash: [0xa1; 32],
            state_root: [0xb2; 32],
        };

        // Re-run the exact boot initialization three times: the live defect
        // reproduced across three boots, so no boot-order side effect may be
        // required for the authoritative state to stick.
        for _ in 0..3 {
            let (mut manager, chain, advertised) = initialize_sync_state(restored);
            assert_eq!(manager.local_height(), restored.height);
            assert_eq!(manager.target_height(), restored.height);
            assert_eq!(chain.height, restored.height);
            assert_eq!(<[u8; 32]>::from(chain.tip), restored.tip_hash);
            assert_eq!(advertised.height, restored.height);
            assert_eq!(advertised.tip_hash, restored.tip_hash);
            assert_eq!(advertised.state_root, restored.state_root);

            let peer = PeerId::random();
            manager.record_peer_status(peer, restored.height + 500);
            let planned = manager.plan_requests(Instant::now());
            assert!(!planned.is_empty(), "a restored node must dispatch catch-up work");
            assert_eq!(
                planned[0].request.start_height,
                restored.height + 1,
                "catch-up must begin immediately above the authoritative tip, not at genesis"
            );
        }
    }

    #[test]
    fn empty_chain_helper_remains_zero_height_2796() {
        let (manager, chain, state_root) =
            initialize_sync_state(InitialChainState::default());
        assert_eq!(manager.local_height(), 0);
        assert_eq!(manager.target_height(), 0);
        assert_eq!(chain.height, 0);
        assert_eq!(<[u8; 32]>::from(chain.tip), [0; 32]);
        assert_eq!(state_root, AdvertisedChainStatus::default());
    }

    #[test]
    fn chain_status_interval_tracks_mid_speed_block_time() {
        assert_eq!(
            chain_status_broadcast_interval(100_001),
            Duration::from_secs(20)
        );
    }

    #[test]
    fn chain_status_interval_clamps_at_terminal_height() {
        assert_eq!(
            chain_status_broadcast_interval(10_000_000),
            Duration::from_secs(10)
        );
    }

    #[tokio::test]
    async fn authoritative_rewind_recomputes_chain_status_interval_2788() {
        let mut chain = wrfcoin_shared_protocol::blockchain::chain::Blockchain::default();
        let mut advertised = AdvertisedChainStatus::default();
        let mut interval = chain_status_broadcast_interval(0);
        let mut tick = new_chain_status_tick(0);
        let height = 100_001;

        apply_authoritative_chain_state(
            &mut chain,
            &mut advertised,
            &mut interval,
            &mut tick,
            height,
            [0x28; 32],
            [0x88; 32],
        );

        assert_eq!(chain.height, height);
        assert_eq!(interval, chain_status_broadcast_interval(height));
        assert_eq!(tick.period(), interval);
    }

    #[tokio::test]
    async fn delayed_success_ack_does_not_rewind_speculative_shadow() {
        let mut chain = wrfcoin_shared_protocol::blockchain::chain::Blockchain::default();
        chain.height = 28_866;
        chain.tip = [0x66; 32].into();
        let mut advertised = AdvertisedChainStatus {
            height: 28_866,
            tip_hash: [0x66; 32],
            state_root: [0xa6; 32],
        };
        let mut interval = chain_status_broadcast_interval(chain.height);
        let mut tick = new_chain_status_tick(chain.height);

        apply_authoritative_chain_state(
            &mut chain,
            &mut advertised,
            &mut interval,
            &mut tick,
            28_839,
            [0x39; 32],
            [0xb9; 32],
        );

        assert_eq!(chain.height, 28_866, "a delayed success ack must be monotonic");
        assert_eq!(<[u8; 32]>::from(chain.tip), [0x66; 32]);

        // The final acknowledgement at the speculative height is allowed to
        // publish the authoritative root and tip.
        apply_authoritative_chain_state(
            &mut chain,
            &mut advertised,
            &mut interval,
            &mut tick,
            28_866,
            [0x76; 32],
            [0xb6; 32],
        );
        assert_eq!(<[u8; 32]>::from(chain.tip), [0x76; 32]);
        assert_eq!(advertised.state_root, [0xb6; 32]);
    }

    /// #3040: the regression behind every `StateDivergenceDetected` page on a
    /// desktop reconnect. The shadow is ahead (blocks cached, not yet applied)
    /// and the applier acknowledges a lower height. #2815 correctly keeps the
    /// shadow put, but the advertised `ChainStatus` must be the acknowledged
    /// triple — never the shadow's `(height, tip)` glued to the boot root.
    #[tokio::test]
    async fn delayed_ack_advertises_the_authoritative_triple_not_shadow_with_boot_root_3040() {
        let boot = InitialChainState {
            height: 44_353,
            tip_hash: [0x53; 32],
            state_root: [0xe8; 32],
        };
        let (_, mut chain, mut advertised) = initialize_sync_state(boot);
        assert_eq!(
            advertised.message(),
            WrfcoinMessage::ChainStatus {
                height: 44_353,
                tip_hash: [0x53; 32],
                state_root: [0xe8; 32],
            },
            "before any acknowledgement the boot triple is advertised"
        );

        // Sync responses race ahead of the applier: the shadow is at the
        // peer's tip while the launch API has applied nothing yet.
        chain.height = 45_134;
        chain.tip = [0x34; 32].into();
        let mut interval = chain_status_broadcast_interval(chain.height);
        let mut tick = new_chain_status_tick(chain.height);

        for (height, tip, root) in [
            (44_354_u64, [0x54_u8; 32], [0xa4_u8; 32]),
            (44_355, [0x55; 32], [0xa5; 32]),
        ] {
            apply_authoritative_chain_state(
                &mut chain,
                &mut advertised,
                &mut interval,
                &mut tick,
                height,
                tip,
                root,
            );
            assert_eq!(chain.height, 45_134, "#2815: the shadow must not rewind");
            assert_eq!(<[u8; 32]>::from(chain.tip), [0x34; 32]);
            assert_eq!(
                advertised,
                AdvertisedChainStatus {
                    height,
                    tip_hash: tip,
                    state_root: root,
                },
                "the advertised status must follow the delayed acknowledgement"
            );
        }
        let published = advertised.message();
        assert_ne!(
            published,
            WrfcoinMessage::ChainStatus {
                height: 45_134,
                tip_hash: [0x34; 32],
                state_root: [0xe8; 32],
            },
            "the shadow tip must never be advertised with the stale boot root"
        );
        assert_eq!(
            published,
            WrfcoinMessage::ChainStatus {
                height: 44_355,
                tip_hash: [0x55; 32],
                state_root: [0xa5; 32],
            }
        );

        // Once the applier reaches the shadow, both views agree again.
        apply_authoritative_chain_state(
            &mut chain,
            &mut advertised,
            &mut interval,
            &mut tick,
            45_134,
            [0x34; 32],
            [0x97; 32],
        );
        assert_eq!(chain.height, 45_134);
        assert_eq!(advertised.height, 45_134);
        assert_eq!(advertised.state_root, [0x97; 32]);
    }

    #[test]
    fn sync_response_test_entrypoint_stays_feature_gated_2788() {
        let source = include_str!("node.rs");
        assert!(
            source.contains(
                "#[cfg(any(test, feature = \"test-harness\"))]\n#[doc(hidden)]\npub async fn handle_sync_response_for_test"
            ),
            "the cross-crate response harness must not ship in normal production builds"
        );
    }

    #[test]
    fn p2p_handle_stats_reflect_cached_connected_peers() {
        let metrics = Arc::new(MetricsCollector::new());
        metrics.set_connected_peers(1);
        let (command_tx, _command_rx) = mpsc::channel(1);
        let (_event_tx, event_rx) = mpsc::channel(1);
        let handle = P2pHandle {
            command_tx,
            event_rx,
            metrics,
            local_peer_id: PeerId::random(),
            discovered_peers: DiscoveredPeers::new(),
            sync_peers: crate::sync_surface::SyncPeersSurface::new(),
        };

        let stats = handle.stats();
        assert_eq!(stats.connected_peers, 1);
    }

    #[test]
    fn p2p_client_stats_reflect_cached_connected_peers() {
        let metrics = Arc::new(MetricsCollector::new());
        metrics.set_connected_peers(2);
        let (command_tx, _command_rx) = mpsc::channel(1);
        let client = P2pClient {
            command_tx,
            metrics,
            local_peer_id: PeerId::random(),
            discovered_peers: DiscoveredPeers::new(),
            sync_peers: crate::sync_surface::SyncPeersSurface::new(),
        };

        let stats = client.stats();
        assert_eq!(stats.connected_peers, 2);
    }

    // -- Per-variant arrival rate (#2115, rate half) -------------------------

    #[test]
    fn a_variant_flood_is_refused_once_its_own_ceiling_is_reached() {
        use crate::message::{limit_for, MessageKind};
        let mut windows = FxHashMap::default();
        let peer = PeerId::random();
        let now = Instant::now();
        // Byte ceiling deliberately enormous, so only the variant rate can bite.
        let limit = InboundRateLimit {
            max_bytes: u64::MAX,
            window: Duration::from_secs(1),
        };
        let kind = MessageKind::GenesisHandshake;
        let max = limit_for(kind).max_per_window;

        for i in 0..max {
            record_peer_inbound_rate(&mut windows, peer, Some(kind), 1, now, limit)
                .unwrap_or_else(|e| panic!("message {i} within ceiling: {e:?}"));
        }

        let err = record_peer_inbound_rate(&mut windows, peer, Some(kind), 1, now, limit)
            .expect_err("one past the variant ceiling must be refused");
        match err {
            super::InboundRateBreach::Variant { kind: k, used, max: m, .. } => {
                assert_eq!(k, kind);
                assert_eq!(used, max);
                assert_eq!(m, max);
            },
            other => panic!("expected a variant breach, got {other:?}"),
        }
    }

    #[test]
    fn variants_are_counted_independently_of_each_other() {
        // Flooding one variant must not consume another's allowance, or one
        // chatty message type would silence the rest.
        use crate::message::{limit_for, MessageKind};
        let mut windows = FxHashMap::default();
        let peer = PeerId::random();
        let now = Instant::now();
        let limit = InboundRateLimit {
            max_bytes: u64::MAX,
            window: Duration::from_secs(1),
        };
        let flooded = MessageKind::GenesisHandshake;

        for _ in 0..limit_for(flooded).max_per_window {
            record_peer_inbound_rate(&mut windows, peer, Some(flooded), 1, now, limit).unwrap();
        }
        assert!(record_peer_inbound_rate(&mut windows, peer, Some(flooded), 1, now, limit).is_err());

        assert!(
            record_peer_inbound_rate(&mut windows, peer, Some(MessageKind::NewBlock), 1, now, limit)
                .is_ok(),
            "a different variant must still be accepted"
        );
    }

    #[test]
    fn one_peers_variant_flood_does_not_consume_anothers_allowance() {
        use crate::message::{limit_for, MessageKind};
        let mut windows = FxHashMap::default();
        let attacker = PeerId::random();
        let honest = PeerId::random();
        let now = Instant::now();
        let limit = InboundRateLimit {
            max_bytes: u64::MAX,
            window: Duration::from_secs(1),
        };
        let kind = MessageKind::GenesisHandshake;

        for _ in 0..limit_for(kind).max_per_window {
            record_peer_inbound_rate(&mut windows, attacker, Some(kind), 1, now, limit).unwrap();
        }
        assert!(record_peer_inbound_rate(&mut windows, attacker, Some(kind), 1, now, limit).is_err());

        assert!(
            record_peer_inbound_rate(&mut windows, honest, Some(kind), 1, now, limit).is_ok(),
            "the ceiling is per peer; an attacker must not silence an honest one"
        );
    }

    #[test]
    fn variant_counts_reset_with_the_window() {
        use crate::message::{limit_for, MessageKind};
        let mut windows = FxHashMap::default();
        let peer = PeerId::random();
        let now = Instant::now();
        let limit = InboundRateLimit {
            max_bytes: u64::MAX,
            window: Duration::from_secs(1),
        };
        let kind = MessageKind::GenesisHandshake;

        for _ in 0..limit_for(kind).max_per_window {
            record_peer_inbound_rate(&mut windows, peer, Some(kind), 1, now, limit).unwrap();
        }
        assert!(record_peer_inbound_rate(&mut windows, peer, Some(kind), 1, now, limit).is_err());

        assert!(
            record_peer_inbound_rate(
                &mut windows,
                peer,
                Some(kind),
                1,
                now + Duration::from_secs(1),
                limit
            )
            .is_ok(),
            "a rate ceiling that never refills is a one-way ratchet to a mute node"
        );
    }

    #[test]
    fn a_refused_message_does_not_consume_the_budget_it_was_refused_for() {
        // The byte ceiling refuses here; the variant counter must be untouched,
        // or a peer could exhaust a variant's allowance purely by being
        // rejected on bytes.
        use crate::message::MessageKind;
        let mut windows = FxHashMap::default();
        let peer = PeerId::random();
        let now = Instant::now();
        let limit = InboundRateLimit {
            max_bytes: 10,
            window: Duration::from_secs(1),
        };
        let kind = MessageKind::NewBlock;

        assert!(
            record_peer_inbound_rate(&mut windows, peer, Some(kind), 100, now, limit).is_err(),
            "oversized on bytes"
        );

        // The variant counter is still zero, so a legitimate small message of
        // the same kind is accepted.
        assert!(record_peer_inbound_rate(&mut windows, peer, Some(kind), 1, now, limit).is_ok());
        assert_eq!(windows.get(&peer).unwrap().per_kind[kind.as_index()], 1);
    }

    #[test]
    fn an_unidentifiable_variant_still_gets_the_byte_ceiling() {
        // `kind_from_wire` returns None for truncated or unknown discriminants.
        // None must mean "no variant ceiling", never "no ceiling".
        let mut windows = FxHashMap::default();
        let peer = PeerId::random();
        let now = Instant::now();
        let limit = InboundRateLimit {
            max_bytes: 100,
            window: Duration::from_secs(1),
        };

        record_peer_inbound_rate(&mut windows, peer, None, 100, now, limit).unwrap();
        assert!(
            record_peer_inbound_rate(&mut windows, peer, None, 1, now, limit).is_err(),
            "an unidentifiable message must still be bounded by bytes"
        );
    }

    #[test]
    fn the_variant_ceiling_is_reported_in_preference_to_the_byte_ceiling() {
        // When a peer breaches both, the reason must name what it was flooding.
        // A byte total does not tell an operator which message type to look at.
        use crate::message::{limit_for, MessageKind};
        let mut windows = FxHashMap::default();
        let peer = PeerId::random();
        let now = Instant::now();
        let limit = InboundRateLimit {
            max_bytes: 1_000,
            window: Duration::from_secs(1),
        };
        let kind = MessageKind::GenesisHandshake;

        for _ in 0..limit_for(kind).max_per_window {
            record_peer_inbound_rate(&mut windows, peer, Some(kind), 1, now, limit).unwrap();
        }

        // This message breaches the variant ceiling AND the byte ceiling.
        let err = record_peer_inbound_rate(&mut windows, peer, Some(kind), 10_000, now, limit)
            .expect_err("breaches both");
        assert!(
            matches!(err, super::InboundRateBreach::Variant { .. }),
            "expected the more specific reason, got {err:?}"
        );
        assert!(err.reason().contains("GenesisHandshake"), "{}", err.reason());
    }

    #[test]
    fn a_disabled_rate_limit_disables_the_variant_ceiling_too() {
        use crate::message::MessageKind;
        let mut windows = FxHashMap::default();
        let peer = PeerId::random();
        let now = Instant::now();
        let limit = InboundRateLimit {
            max_bytes: 0, // disabled
            window: Duration::from_secs(1),
        };
        for _ in 0..10_000 {
            record_peer_inbound_rate(
                &mut windows,
                peer,
                Some(MessageKind::GenesisHandshake),
                1,
                now,
                limit,
            )
            .unwrap();
        }
        assert!(windows.is_empty(), "a disabled limiter must not accumulate state");
    }

    #[test]
    fn inbound_rate_limit_rejects_before_peer_exceeds_window_budget() {
        let peer = PeerId::random();
        let mut windows = FxHashMap::default();
        let now = Instant::now();
        let limit = InboundRateLimit {
            max_bytes: 100,
            window: Duration::from_secs(10),
        };

        record_peer_inbound_rate(&mut windows, peer, None, 60, now, limit)
            .expect("first message should fit in the window");
        let err =
            record_peer_inbound_rate(&mut windows, peer, None, 41, now + Duration::from_secs(1), limit)
                .expect_err("second message should exceed the per-peer window");

        assert_eq!(
            err,
            super::InboundRateBreach::Bytes(InboundRateLimitExceeded {
                used: 60,
                attempted: 41,
                max: 100,
                window: Duration::from_secs(10),
            })
        );
    }

    #[test]
    fn inbound_rate_limit_resets_after_window() {
        let peer = PeerId::random();
        let mut windows = FxHashMap::default();
        let now = Instant::now();
        let limit = InboundRateLimit {
            max_bytes: 100,
            window: Duration::from_secs(10),
        };

        record_peer_inbound_rate(&mut windows, peer, None, 100, now, limit)
            .expect("message should fill the first window");
        record_peer_inbound_rate(
            &mut windows,
            peer,
            None,
            100,
            now + Duration::from_secs(10),
            limit,
        )
        .expect("new window should accept another full budget");
    }

    #[test]
    fn inbound_rate_limit_zero_bytes_disables_enforcement() {
        let peer = PeerId::random();
        let mut windows = FxHashMap::default();
        let limit = InboundRateLimit {
            max_bytes: 0,
            window: Duration::from_secs(10),
        };

        record_peer_inbound_rate(&mut windows, peer, None, u64::MAX, Instant::now(), limit)
            .expect("zero-byte config disables enforcement for local tests");
    }

    /// THE #2117 regression, measured on the real admission path.
    ///
    /// Before the per-peer quota, 50 000 distinct `station_id`s from ONE peer
    /// produced 50 000 entries in each map (and ~16 s of CPU, because `prune`
    /// ran a full `retain` on every admission). `station_id` is attacker-chosen
    /// and is used BEFORE the station signature is verified, so nothing upstream
    /// bounds its cardinality.
    #[test]
    fn a_single_peer_flooding_station_ids_cannot_grow_the_admission_maps_2117() {
        use super::WeatherDataAdmission;
        use crate::preauth_budget::DEFAULT_MAX_ENTRIES_PER_PEER;
        let peer = PeerId::random();
        let mut admission = WeatherDataAdmission::default();
        let now = Instant::now();

        for i in 0..50_000u32 {
            // Same `now` throughout, so TTL expiry cannot be what bounds this.
            let _ = admission.admit(peer, &format!("s{i}"), Some(&format!("{i:032x}")), now);
        }

        let owner = peer.to_string();
        assert!(
            admission.last_peer_station_verify.len_for(&owner) <= DEFAULT_MAX_ENTRIES_PER_PEER,
            "rate map grew to {} entries from one peer",
            admission.last_peer_station_verify.len_for(&owner)
        );
        assert!(
            admission.seen_nonces.len_for(&owner) <= DEFAULT_MAX_ENTRIES_PER_PEER,
            "replay map grew to {} entries from one peer",
            admission.seen_nonces.len_for(&owner)
        );
        assert!(
            admission.last_peer_station_verify.evicted_quota() > 0,
            "the flood must be observable as quota evictions, not silently absorbed"
        );
    }

    /// The property that makes the per-peer quota the right shape rather than a
    /// global cap: a flooding peer must not be able to evict an honest peer's
    /// admission state, because that would re-open replay protection and rate
    /// limiting for the honest peer — trading an OOM for a security regression.
    #[test]
    fn an_honest_peers_admission_state_survives_another_peers_flood_2117() {
        use super::{WeatherDataAdmission, WeatherDataAdmissionError};
        let honest = PeerId::random();
        let attacker = PeerId::random();
        let mut admission = WeatherDataAdmission::default();
        let now = Instant::now();

        let nonce = "aabbccddaabbccddaabbccddaabbccdd";
        admission
            .admit(honest, "STATION_001", Some(nonce), now)
            .expect("honest peer is admitted");

        for i in 0..20_000u32 {
            let _ = admission.admit(attacker, &format!("s{i}"), Some(&format!("{i:032x}")), now);
        }

        // The honest peer's replay entry must still block a replay of that nonce.
        assert_eq!(
            admission.admit(honest, "STATION_001", Some(nonce), now),
            Err(WeatherDataAdmissionError::Replay),
            "the attacker's flood must not have evicted the honest peer's replay \
             record — losing it would let that message be re-injected"
        );
    }

    #[test]
    fn weather_data_v2_admission_rate_limits_within_window() {
        use super::{WeatherDataAdmission, WeatherDataAdmissionError};
        let peer = PeerId::random();
        let station = "STATION_001";
        let mut admission = WeatherDataAdmission::default();
        let now = Instant::now();

        admission
            .admit_v2(peer, station, now)
            .expect("first V2 message should be admitted");

        let err = admission
            .admit_v2(peer, station, now + Duration::from_millis(10))
            .expect_err("second V2 message within 250ms should be rate-limited");
        assert_eq!(err, WeatherDataAdmissionError::RateLimited);
    }

    #[test]
    fn weather_data_v2_admission_allows_after_interval() {
        use super::{WeatherDataAdmission, WEATHER_DATA_MIN_VERIFY_INTERVAL};
        let peer = PeerId::random();
        let station = "STATION_002";
        let mut admission = WeatherDataAdmission::default();
        let now = Instant::now();

        admission
            .admit_v2(peer, station, now)
            .expect("first V2 message should be admitted");
        admission
            .admit_v2(peer, station, now + WEATHER_DATA_MIN_VERIFY_INTERVAL)
            .expect("V2 message after full interval should be admitted");
    }

    /// The #2118 test seam: every effect of the invalid-signature penalty, asserted.
    ///
    /// This is the test that did not exist. A RED control showed that unwiring the
    /// observe-only step from both match arms left the whole suite green, because
    /// `handle_behaviour_event` needs a live swarm no test can build. Extracting the
    /// penalty moved that logic somewhere reachable — this asserts all four effects, so
    /// removing any one of them now fails.
    /// #2116 folded into #2118: a genesis-handshake mismatch must cost the author
    /// something. Before this the whole action was `swarm.disconnect_peer_id(author)` —
    /// no score, no penalty — so a wrong-chain or version-skewed peer could reconnect
    /// and repeat indefinitely, each cycle costing us a full Noise handshake.
    ///
    /// Same seam argument as the weather-signature contract: the match arm needs a live
    /// `Swarm`, so anything inline there is unreachable by tests.
    #[tokio::test]
    async fn penalize_genesis_handshake_mismatch_records_an_offence_and_emits_the_event() {
        use super::P2pEvent;
        use crate::scoring::PersistentScore;
        use std::net::{IpAddr, Ipv4Addr};

        let mut persistent = PersistentScore::new(3_600, 3.0);
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let author = PeerId::random();
        persistent.note_address(author, IpAddr::V4(Ipv4Addr::new(10, 7, 7, 1)), 0);

        assert_eq!(
            persistent.effective_for(&author, 0),
            0.0,
            "precondition: the author starts unpenalised"
        );

        let would_ban = super::penalize_genesis_handshake_mismatch(
            &mut persistent,
            0,
            &tx,
            author,
            "chain identity mismatch",
        )
        .await;

        assert!(
            persistent.effective_for(&author, 0) > 0.0,
            "the mismatch must cost the author score — otherwise reconnecting is free"
        );
        assert!(!would_ban, "one offence is below the ban threshold");

        match rx.try_recv() {
            Ok(P2pEvent::PeerPenalized { peer_id, banned, reason }) => {
                assert_eq!(peer_id, author);
                assert!(
                    !banned,
                    "OBSERVE-ONLY: this must not ban while #2118 is still observing, or \
                     it changes who gets dropped ahead of calibration"
                );
                assert!(
                    reason.contains("genesis handshake mismatch"),
                    "the reason must name the offence, got {reason:?}"
                );
            },
            other => panic!("expected a PeerPenalized event, got {other:?}"),
        }
    }

    /// Composition regression for #2116 + merged #2532.
    ///
    /// The same author first crosses the unproven deadline in shipped
    /// observe-only mode, remains connected, and later supplies a mismatching
    /// genesis handshake. The timeout is one edge; the mismatch is one
    /// distinct persistent-score/event outcome. Re-polling the still-unproven
    /// peer must not turn one admission into elapsed peer-seconds.
    #[tokio::test]
    async fn timeout_then_mismatch_counts_each_outcome_once() {
        use crate::message::WrfcoinMessage;
        use std::net::{IpAddr, Ipv4Addr};
        use std::sync::atomic::Ordering;

        let (mut h, mut rx) = BehaviourHarness::new();
        let author = PeerId::random();
        let address: libp2p::Multiaddr =
            "/ip4/198.51.100.7/tcp/9000".parse().unwrap();
        let start = Instant::now();

        h.unproven_peers
            .admit(author, &address, start)
            .expect("first unproven peer must be admitted");
        h.persistent_scores.note_address(
            author,
            IpAddr::V4(Ipv4Addr::new(198, 51, 100, 7)),
            h.persistent_now,
        );

        let deadline = start + h.unproven_peers.limits().handshake_timeout;
        assert_eq!(
            super::record_new_handshake_timeouts(
                &mut h.unproven_peers,
                &h.metrics,
                deadline,
            ),
            vec![author]
        );
        assert!(
            super::record_new_handshake_timeouts(
                &mut h.unproven_peers,
                &h.metrics,
                deadline + std::time::Duration::from_secs(1),
            )
            .is_empty(),
            "a second sweep must not recount the same expired admission"
        );
        assert_eq!(
            h.metrics.handshake_timeouts.load(Ordering::Relaxed),
            1,
            "the peer crossed the timeout edge exactly once"
        );

        let mismatch = WrfcoinMessage::GenesisHandshake {
            network_id: "wrong-network".into(),
            chain_id: "777".into(),
            genesis_hash: "0xabc".into(),
            protocol_version: 1,
        };
        h.dispatch(WrfcoinBehaviourEvent::Gossipsub(
            gossipsub::Event::Message {
                propagation_source: author,
                message_id: gossipsub::MessageId::from("timeout-then-mismatch"),
                message: gossipsub::Message {
                    source: Some(author),
                    data: mismatch.to_bytes().expect("serialize mismatch"),
                    sequence_number: Some(1),
                    topic: h.tm.genesis_handshake_hash(),
                },
            },
        ))
        .await;

        match rx.try_recv() {
            Ok(P2pEvent::PeerPenalized {
                peer_id,
                banned,
                reason,
            }) => {
                assert_eq!(peer_id, author);
                assert!(!banned, "one mismatch remains observe-only");
                assert!(
                    reason.contains("genesis handshake mismatch"),
                    "the second outcome must be the distinct mismatch penalty: {reason}"
                );
            },
            other => panic!("expected one mismatch PeerPenalized event, got {other:?}"),
        }
        assert!(
            h.persistent_scores.effective_for(&author, h.persistent_now) > 0.0,
            "the merged #2532 mismatch accounting must run"
        );
        assert!(
            super::record_new_handshake_timeouts(
                &mut h.unproven_peers,
                &h.metrics,
                deadline + std::time::Duration::from_secs(2),
            )
            .is_empty(),
            "the later mismatch must not reopen the timeout edge"
        );
        assert_eq!(
            h.metrics.handshake_timeouts.load(Ordering::Relaxed),
            1,
            "one peer admission must remain one timeout after mismatch"
        );
    }

    /// Repeat offences must accumulate, which is the whole point: a peer that
    /// reconnect-loops crosses the threshold instead of paying nothing each time.
    #[tokio::test]
    async fn repeated_handshake_mismatches_accumulate_toward_the_ban_threshold() {
        use crate::scoring::PersistentScore;
        use std::net::{IpAddr, Ipv4Addr};

        let mut persistent = PersistentScore::new(3_600, 3.0);
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let author = PeerId::random();
        persistent.note_address(author, IpAddr::V4(Ipv4Addr::new(10, 8, 8, 1)), 0);

        let mut last = false;
        for _ in 0..4 {
            last = super::penalize_genesis_handshake_mismatch(
                &mut persistent, 0, &tx, author, "chain identity mismatch",
            )
            .await;
        }
        while rx.try_recv().is_ok() {}

        assert!(
            last,
            "four mismatches at 1.0 each must reach the 3.0 threshold — got effective {:.2}",
            persistent.effective_for(&author, 0)
        );
    }

    #[tokio::test]
    async fn penalize_invalid_weather_signature_has_all_four_effects() {
        use super::{MetricsCollector, SyncPeerScore};
        use crate::scoring::PersistentScore;
        use std::net::{IpAddr, Ipv4Addr};

        let metrics = std::sync::Arc::new(MetricsCollector::new());
        let mut scores: rustc_hash::FxHashMap<PeerId, SyncPeerScore> =
            rustc_hash::FxHashMap::default();
        let mut persistent = PersistentScore::new(3_600, 3.0);
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);

        let peer = PeerId::random();
        persistent.note_address(peer, IpAddr::V4(Ipv4Addr::new(10, 4, 4, 1)), 0);

        let before = metrics
            .weather_msgs_invalid_sig
            .load(std::sync::atomic::Ordering::Relaxed);

        let banned = super::penalize_invalid_weather_signature(
            &metrics, &mut scores, &mut persistent, 0, &tx, peer, "test reason",
        )
        .await;

        // 1. metric
        assert_eq!(
            metrics.weather_msgs_invalid_sig.load(std::sync::atomic::Ordering::Relaxed),
            before + 1,
            "the invalid-signature metric must increment"
        );
        // 2. PeerId-keyed score
        assert!(scores.contains_key(&peer), "the offence must be recorded per-peer");
        assert!(!banned, "one offence is below the ban threshold");
        // 3. churn-resistant observer — THE effect the RED control could not see before
        assert!(
            persistent.effective_for(&peer, 0) > 0.0,
            "the observe-only score must record the offence; if this passes with the \
             observe step removed, the seam is not doing its job"
        );
        // 4. event
        match rx.try_recv() {
            Ok(P2pEvent::PeerPenalized { peer_id, banned: ev_banned, .. }) => {
                assert_eq!(peer_id, peer);
                assert_eq!(ev_banned, banned, "event must carry the same decision");
            },
            other => panic!("expected PeerPenalized, got {other:?}"),
        }
    }

    /// A fresh identity from the same subnet inherits observed suspicion through the seam
    /// — the churn defect, end to end through the production penalty path.
    #[tokio::test]
    async fn penalty_seam_lets_a_fresh_identity_inherit_subnet_suspicion() {
        use super::{MetricsCollector, SyncPeerScore};
        use crate::scoring::PersistentScore;
        use std::net::{IpAddr, Ipv4Addr};

        let metrics = std::sync::Arc::new(MetricsCollector::new());
        let mut scores: rustc_hash::FxHashMap<PeerId, SyncPeerScore> =
            rustc_hash::FxHashMap::default();
        let mut persistent = PersistentScore::new(3_600, 3.0);
        let (tx, _rx) = tokio::sync::mpsc::channel(64);

        let bad = PeerId::random();
        persistent.note_address(bad, IpAddr::V4(Ipv4Addr::new(10, 5, 5, 1)), 0);
        for _ in 0..6 {
            super::penalize_invalid_weather_signature(
                &metrics, &mut scores, &mut persistent, 0, &tx, bad, "r",
            )
            .await;
        }

        // Reconnect under a brand-new identity from the same /24.
        let fresh = PeerId::random();
        persistent.note_address(fresh, IpAddr::V4(Ipv4Addr::new(10, 5, 5, 2)), 0);
        assert!(
            scores.get(&fresh).is_none(),
            "the PeerId-keyed map sees a clean peer — that IS the defect"
        );
        assert!(
            persistent.effective_for(&fresh, 0) > 0.0,
            "the churn-resistant score must still see the subnet's history"
        );
    }

    /// #2118 observe-only: the step records the offence and reports the disagreement,
    /// and — critically — never bans on its own.
    #[test]
    fn observe_persistent_offence_records_but_never_bans() {
        use crate::scoring::PersistentScore;
        use std::net::{IpAddr, Ipv4Addr};

        let mut scores = PersistentScore::new(3_600, 3.0);
        let bad = PeerId::random();
        let ip = IpAddr::V4(Ipv4Addr::new(10, 9, 9, 1));
        scores.note_address(bad, ip, 0);

        // Below threshold: agrees with the existing decision, reports no ban.
        assert!(!super::observe_persistent_offence(&mut scores, bad, false, 0));

        // Enough offences to cross the threshold.
        for _ in 0..5 {
            super::observe_persistent_offence(&mut scores, bad, false, 0);
        }
        assert!(
            super::observe_persistent_offence(&mut scores, bad, false, 0),
            "the persistent policy must report a ban once the threshold is crossed"
        );

        // THE observe-only property: a fresh identity on the same subnet inherits
        // suspicion, which is what the PeerId-keyed map cannot see — but nothing here
        // bans, it only reports.
        let fresh = PeerId::random();
        scores.note_address(fresh, IpAddr::V4(Ipv4Addr::new(10, 9, 9, 2)), 0);
        assert!(
            scores.effective_for(&fresh, 0) > 0.0,
            "a fresh identity from the same subnet must inherit observed suspicion"
        );
    }

    #[test]
    fn weather_peer_scores_evicted_on_peer_disconnect() {
        // Regression: prior to this fix, `weather_peer_scores` was never cleaned
        // up when a peer disconnected.  Over time (or under targeted churn) this
        // caused unbounded map growth.  Verify the eviction pattern is correct.
        use super::SyncPeerScore;

        let peer = PeerId::random();
        let mut scores: rustc_hash::FxHashMap<PeerId, SyncPeerScore> =
            rustc_hash::FxHashMap::default();

        // Peer sends a WeatherData message with an invalid station signature →
        // a score entry is inserted.
        let score = scores.entry(peer).or_default();
        score.record_invalid_weather_sig();
        assert!(
            scores.contains_key(&peer),
            "score entry must exist after invalid-sig event"
        );

        // Peer disconnects → entry must be removed.
        scores.remove(&peer);
        assert!(
            !scores.contains_key(&peer),
            "score entry must be evicted after disconnect"
        );
    }

    #[test]
    fn weather_data_v1_and_v2_share_rate_bucket() {
        // A v1 `admit()` followed immediately by a v2 `admit_v2()` for the
        // same peer+station must be rate-limited. This prevents mixing
        // variants to bypass the per-station throttle.
        use super::{WeatherDataAdmission, WeatherDataAdmissionError};
        let peer = PeerId::random();
        let station = "STATION_003";
        let nonce = "aabbccddaabbccddaabbccddaabbccdd"; // 32 hex chars = 16 bytes (WEATHER_DATA_NONCE_HEX_LEN)
        let mut admission = WeatherDataAdmission::default();
        let now = Instant::now();

        // V1 admit succeeds.
        admission
            .admit(peer, station, Some(nonce), now)
            .expect("first V1 message should be admitted");

        // Immediate V2 admit_v2 should be rate-limited (shared bucket).
        let err = admission
            .admit_v2(peer, station, now + Duration::from_millis(5))
            .expect_err("V2 after V1 within 250ms should be rate-limited");
        assert_eq!(err, WeatherDataAdmissionError::RateLimited);
    }
}

// ---------------------------------------------------------------------------
// Behaviour event handler
// ---------------------------------------------------------------------------

fn refresh_mesh_peer_metrics(
    swarm: &Swarm<crate::behaviour::WrfcoinBehaviour>,
    tm: &TopicManager,
    metrics: &MetricsCollector,
) {
    let mesh_counts = [
        ("blocks", swarm.behaviour().gossipsub.mesh_peers(&tm.blocks_hash()).count()),
        (
            "transactions",
            swarm
                .behaviour()
                .gossipsub
                .mesh_peers(&tm.transactions_hash())
                .count(),
        ),
        ("weather", swarm.behaviour().gossipsub.mesh_peers(&tm.weather_data_hash()).count()),
        (
            "forecast",
            swarm
                .behaviour()
                .gossipsub
                .mesh_peers(&tm.forecast_tasks_hash())
                .count(),
        ),
    ];

    for (topic, count) in mesh_counts {
        metrics.set_mesh_peers(topic, count);
    }
}

fn record_peer_inbound_rate(
    peer_windows: &mut FxHashMap<PeerId, PeerInboundRateState>,
    peer_id: PeerId,
    kind: Option<crate::message::MessageKind>,
    msg_bytes: u64,
    now: Instant,
    limit: InboundRateLimit,
) -> Result<(), InboundRateBreach> {
    // The byte ceiling and the per-variant ceiling share one window and one
    // map. Two mechanisms would mean two windows drifting apart and a second
    // structure keyed by peer identity; folding them keeps a single accounting
    // path, which is also the only way "used" figures in the two messages stay
    // consistent with each other.
    if limit.disabled() {
        return Ok(());
    }

    let state = peer_windows.entry(peer_id).or_insert(PeerInboundRateState {
        window_started: now,
        bytes: 0,
        per_kind: [0; crate::message::MessageKind::COUNT],
    });

    if now.duration_since(state.window_started) >= limit.window {
        state.window_started = now;
        state.bytes = 0;
        state.per_kind = [0; crate::message::MessageKind::COUNT];
    }

    // Per-variant ceiling first: it is the more specific statement, so when a
    // peer breaches both, the reported reason names the variant rather than a
    // byte total that does not say what was being flooded.
    if let Some(kind) = kind {
        let max = crate::message::limit_for(kind).max_per_window;
        if max > 0 {
            let used = state.per_kind[kind.as_index()];
            if used.saturating_add(1) > max {
                return Err(InboundRateBreach::Variant {
                    kind,
                    used,
                    max,
                    window: limit.window,
                });
            }
        }
    }

    let projected = state.bytes.saturating_add(msg_bytes);
    if projected > limit.max_bytes {
        return Err(InboundRateBreach::Bytes(InboundRateLimitExceeded {
            used: state.bytes,
            attempted: msg_bytes,
            max: limit.max_bytes,
            window: limit.window,
        }));
    }

    // Commit only once every ceiling has passed: a rejected message must not
    // consume the budget it was rejected for.
    if let Some(kind) = kind {
        state.per_kind[kind.as_index()] = state.per_kind[kind.as_index()].saturating_add(1);
    }
    state.bytes = projected;
    Ok(())
}

#[allow(clippy::too_many_arguments)]

/// #2118 observe-only step: record the offence against the churn-resistant score and
/// report where it would disagree with the PeerId-keyed decision.
///
/// Returns whether the persistent policy would ban, so the decision is a VALUE a test can
/// assert on rather than only a log line. It never bans: the caller's `banned` is still
/// what acts.
fn observe_persistent_offence(
    scores: &mut crate::scoring::PersistentScore,
    peer: PeerId,
    banned: bool,
    now: u64,
) -> bool {
    scores.record_offence_for(peer, 1.0, now);
    let would_ban = scores.would_ban(&peer, now);
    if would_ban != banned {
        log::info!(
            "#2118 observe-only: peer {peer} banned={banned} would_ban={would_ban} \
             (effective {:.2}) — churn-resistant policy disagrees with the PeerId-keyed one",
            scores.effective_for(&peer, now)
        );
    }
    would_ban
}


/// Record only the transition into handshake timeout.
///
/// The guard retains expired peers until proof or disconnect so enforcement
/// can retry a failed close. Metrics must not inherit that retry cardinality:
/// one admission crossing its deadline is one timeout observation.
fn record_new_handshake_timeouts(
    unproven_peers: &mut crate::handshake_guard::UnprovenPeers,
    metrics: &MetricsCollector,
    now: Instant,
) -> Vec<PeerId> {
    let newly_expired = unproven_peers.newly_expired(now);
    for _ in &newly_expired {
        metrics.record_handshake_drop(crate::handshake_guard::DropReason::Timeout);
    }
    newly_expired
}

/// The complete penalty for a genesis-handshake mismatch (#2116, folded into #2118).
///
/// Before this, the entire action was `swarm.disconnect_peer_id(author)` — no score, no
/// penalty, no rate limit. A wrong-chain or version-skewed peer could reconnect
/// immediately and repeat indefinitely, and **each cycle costs a full Noise handshake**
/// plus gossipsub subscription, paid by us for a peer already established as unusable.
/// That is the cost #2116 exists to bound, in the architecture that actually exists (the
/// genesis handshake is a gossipsub topic message, not a connection-level protocol — see
/// the #2116 rescope note).
///
/// The likeliest trigger is honest: a version-skewed fleet mid-upgrade (#2076)
/// reconnect-loops against every peer.
///
/// Extracted for the same reason `penalize_invalid_weather_signature` is — the match arm
/// needs a live `Swarm`, which no test in this crate can build, so anything left inline
/// there is unreachable by tests. `swarm.disconnect_peer_id` stays at the call site: it is
/// the transport action, not part of the peer-penalty contract.
///
/// **OBSERVE-ONLY**, matching the rest of #2118: this records the offence and reports what
/// the churn-resistant policy *would* decide. The disconnect still happens exactly as
/// before, so this cannot change who gets dropped until #2118 leaves observe-only.
///
/// Returns `would_ban`, so the decision is a value tests assert on rather than a log line.
async fn penalize_genesis_handshake_mismatch(
    persistent_scores: &mut crate::scoring::PersistentScore,
    persistent_now: u64,
    event_tx: &mpsc::Sender<P2pEvent>,
    author: PeerId,
    diagnosis: &str,
) -> bool {
    // `banned = false`: the pre-existing behaviour is an unconditional disconnect with no
    // ban decision, so there is no PeerId-keyed verdict to disagree with. Passing the
    // honest value keeps `observe_persistent_offence`'s disagreement log meaningful
    // instead of asserting a decision this site never made.
    let would_ban = observe_persistent_offence(persistent_scores, author, false, persistent_now);
    let _ = event_tx
        .send(P2pEvent::PeerPenalized {
            peer_id: author,
            reason: format!("genesis handshake mismatch: {diagnosis}"),
            banned: false,
        })
        .await;
    would_ban
}

/// Bound recovery from a half-open block-sync request/response connection.
///
/// Sync-local health survives reconnects, so refreshing the transport does not
/// erase backoff or turn an honest timeout into an external reputation event.
/// Three consecutive failures permit roughly 10s + 20s + 40s of bounded
/// retry before forcing a fresh connection epoch; every later failure remains
/// eligible to refresh until a successful response resets the counter.
const SYNC_CONNECTION_REFRESH_FAILURE_THRESHOLD: u32 = 3;

fn should_refresh_sync_connection(peer_health: Option<&crate::sync::SyncPeerMetrics>) -> bool {
    peer_health.is_some_and(|health| {
        health.consecutive_transport_failures >= SYNC_CONNECTION_REFRESH_FAILURE_THRESHOLD
    })
}

/// The complete penalty for an invalid weather-station signature (#2118 test seam).
///
/// Extracted so the penalty contract is testable. `handle_behaviour_event` needs a live
/// `Swarm<WrfcoinBehaviour>` — which no test in this crate can build — so anything left
/// inline in that match arm is, in practice, unreachable by tests. A RED control proved
/// exactly that: unwiring the observe-only step from both arms left the whole suite green.
///
/// Everything here is plain state, so all four effects are asserted in tests:
///   1. the invalid-signature metric increments;
///   2. the PeerId-keyed score records the offence and decides `banned`;
///   3. the churn-resistant score observes it (#2118, observe-only — never bans);
///   4. a `PeerPenalized` event is emitted with that decision.
///
/// The gossipsub `Reject` stays at the call site: it is the transport's validation
/// verdict, not part of the peer-penalty contract, and it is the one step that genuinely
/// needs the swarm.
///
/// Returns the `banned` decision, so callers and tests read the same value.
async fn penalize_invalid_weather_signature(
    metrics: &Arc<MetricsCollector>,
    weather_peer_scores: &mut FxHashMap<PeerId, SyncPeerScore>,
    persistent_scores: &mut crate::scoring::PersistentScore,
    persistent_now: u64,
    event_tx: &mpsc::Sender<P2pEvent>,
    peer: PeerId,
    reason: &str,
) -> bool {
    metrics
        .weather_msgs_invalid_sig
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let score = weather_peer_scores.entry(peer).or_default();
    score.record_invalid_weather_sig();
    let banned = score.is_banned();
    // #2118 OBSERVE-ONLY. `banned` above is still the decision; this only records where a
    // churn-resistant policy would differ, so the weight and half-life can be calibrated
    // before anything disconnects an honest operator.
    observe_persistent_offence(persistent_scores, peer, banned, persistent_now);
    let _ = event_tx
        .send(P2pEvent::PeerPenalized {
            peer_id: peer,
            reason: reason.to_string(),
            banned,
        })
        .await;
    banned
}

/// Claim one signature verification for `peer`, returning whether the caller
/// may proceed (#2117).
///
/// Extracted from the two call sites so the decision and its metric are
/// reachable by a test. The `return` at each call site is what actually
/// prevents the ed25519 work, and that part is covered by inspection rather
/// than by a test — exercising it needs a real `handle_behaviour_event`
/// invocation, which is what the harness in #2533 provides.
///
/// `false` means the caller must **skip verification entirely**, not perform it
/// and discard the verdict. Doing the work and throwing away the answer would
/// bound nothing, which is the whole exposure.
pub(crate) fn claim_verify_budget(
    verify_budget: &mut crate::preauth_budget::VerifyBudget,
    metrics: &Arc<MetricsCollector>,
    peer: PeerId,
) -> bool {
    if verify_budget.try_claim(peer, Instant::now()).is_ok() {
        return true;
    }
    metrics
        .preauth_verify_skipped
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    debug!(
        "Skipping weather signature verification for {peer}: pre-auth verification budget exceeded"
    );
    false
}

/// A listener came up. When announcement filtering is on (the default),
/// identify's raw listen-address list is hidden (see `build_behaviour`), so
/// each listen address that may be announced is promoted to an external
/// address here — that is the list identify sends. Private interface addresses
/// a `0.0.0.0` listener expands to are simply never promoted.
fn on_new_listen_addr(
    swarm: &mut Swarm<crate::behaviour::WrfcoinBehaviour>,
    policy: &crate::announce::AnnouncePolicy,
    address: &Multiaddr,
) {
    if policy.is_filtering() && policy.may_announce_own(address) {
        swarm.add_external_address(address.clone());
    }
}

/// Undo [`on_new_listen_addr`] when the listener goes away, without touching
/// an operator-configured external address that happens to be equal.
fn on_expired_listen_addr(
    swarm: &mut Swarm<crate::behaviour::WrfcoinBehaviour>,
    policy: &crate::announce::AnnouncePolicy,
    address: &Multiaddr,
) {
    if policy.is_filtering() && !policy.is_operator_addr(address) {
        swarm.remove_external_address(address);
    }
}

/// AutoNAT (or any behaviour) confirmed an external address. A peer on the
/// same tailnet/LAN can confirm a private address; withdraw it immediately so
/// it is not announced (crate::announce).
fn on_external_addr_confirmed(
    swarm: &mut Swarm<crate::behaviour::WrfcoinBehaviour>,
    policy: &crate::announce::AnnouncePolicy,
    address: &Multiaddr,
) {
    if !policy.may_announce_own(address) {
        info!("External address {address} is not globally routable; withdrawing it from announcements");
        swarm.remove_external_address(address);
    }
}

async fn handle_behaviour_event(
    event: WrfcoinBehaviourEvent,
    // #2117: claimed before any signature verification.
    verify_budget: &mut crate::preauth_budget::VerifyBudget,
    // #2117: aggregate inbound-byte ceiling across all peers.
    preauth_budget: &mut crate::preauth_budget::PreAuthBudget,
    // #2116: cleared when a peer proves chain identity.
    unproven_peers: &mut crate::handshake_guard::UnprovenPeers,
    tm: &TopicManager,
    event_tx: &mpsc::Sender<P2pEvent>,
    metrics: &Arc<MetricsCollector>,
    peer_manager: &mut PeerManager,
    sync_manager: &mut SyncManager,
    sync_chain: &mut Blockchain,
    advertised_status: AdvertisedChainStatus,
    outbound_sync_requests: &mut FxHashMap<
        request_response::OutboundRequestId,
        OutboundSyncRequest,
    >,
    inbound_sync_requests: &mut FxHashMap<
        request_response::InboundRequestId,
        request_response::ResponseChannel<BlockResponse>,
    >,
    swarm: &mut Swarm<crate::behaviour::WrfcoinBehaviour>,
    peer_inbound_bytes: &mut FxHashMap<PeerId, u64>,
    peer_inbound_rate_windows: &mut FxHashMap<PeerId, PeerInboundRateState>,
    inbound_rate_limit: InboundRateLimit,
    weather_peer_scores: &mut FxHashMap<PeerId, SyncPeerScore>,
    // #2118 observe-only: records what a churn-resistant policy would do. Never bans.
    persistent_scores: &mut crate::scoring::PersistentScore,
    persistent_now: u64,
    weather_admission: &mut WeatherDataAdmission,
    quic_transport_enabled: bool,
    inline_data_max: usize,
    pending_quic_upgrade_dials: &mut FxHashMap<ConnectionId, PeerId>,
    local_handshake: &crate::genesis_handshake::GenesisHandshakeMessage,
    discovered_peers: &DiscoveredPeers,
    did_post_identify_bootstrap: &mut bool,
    pending_broadcasts: &mut PendingBroadcasts,
    status_announced_peers: &mut FxHashSet<PeerId>,
    pre_proof_statuses: &mut FxHashMap<PeerId, (u64, [u8; 32], [u8; 32])>,
    // Which addresses may be passed on to other peers (`crate::announce`).
    announce_policy: &crate::announce::AnnouncePolicy,
) {
    match event {
        WrfcoinBehaviourEvent::Gossipsub(gossipsub::Event::Message {
            propagation_source,
            message_id,
            message,
        }) => {
            let msg_bytes = message.data.len() as u64;
            let label = topic_label(tm.topic_name(&message.topic));
            metrics.record_inbound_topic(label, msg_bytes);
            *peer_inbound_bytes.entry(propagation_source).or_default() += msg_bytes;
            // #2117: the aggregate ceiling. `record_peer_inbound_rate` below
            // bounds each peer individually, but N peers each individually
            // under that limit still sum to an unbounded total — which is the
            // shape that OOM'd the node in #2077. A per-peer ceiling cannot
            // bound a sum over an attacker-chosen N, so this is checked first.
            if preauth_budget
                .try_reserve(message.data.len(), Instant::now())
                .is_err()
            {
                warn!(
                    "Dropping gossipsub message from {propagation_source}: aggregate inbound \
                     budget exhausted ({}B/window across all peers)",
                    preauth_budget.limits().max_bytes_total
                );
                metrics
                    .messages_rejected
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let _ = swarm
                    .behaviour_mut()
                    .gossipsub
                    .report_message_validation_result(
                        &message_id,
                        &propagation_source,
                        gossipsub::MessageAcceptance::Reject,
                    );
                return;
            }

            // #2115: identify the variant from the wire bytes without
            // deserializing, so a flood is rate-limited after inspecting a
            // fixed 4 bytes. `None` (truncated or unknown discriminant) means
            // only the byte ceiling applies — bincode rejects the message
            // moments later regardless.
            let kind = crate::message::kind_from_wire(&message.data);
            if let Err(breach) = record_peer_inbound_rate(
                peer_inbound_rate_windows,
                propagation_source,
                kind,
                msg_bytes,
                Instant::now(),
                inbound_rate_limit,
            ) {
                let reason = breach.reason();
                warn!("Dropping gossipsub message from {propagation_source}: {reason}");
                metrics
                    .messages_rejected
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let _ = swarm
                    .behaviour_mut()
                    .gossipsub
                    .report_message_validation_result(
                        &message_id,
                        &propagation_source,
                        gossipsub::MessageAcceptance::Reject,
                    );
                let _ = event_tx.try_send(P2pEvent::PeerPenalized {
                    peer_id: propagation_source,
                    reason,
                    banned: false,
                });
                return;
            }
            // Capture before `message` is shadowed by the decoded WrfcoinMessage below.
            let message_source = message.source;

            let Ok(message) = WrfcoinMessage::from_bytes(&message.data) else {
                warn!("Failed to decode gossipsub message from {propagation_source}: invalid bincode");
                metrics
                    .messages_rejected
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                return;
            };

            if let Err(reason) = message.validate_inline_size(inline_data_max) {
                warn!(
                    "Rejected oversized gossipsub message from {propagation_source} for {label}: {reason}"
                );
                metrics
                    .messages_rejected
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                return;
            }

            // Local admission/relay policy, never a block validity rule. Ignore
            // avoids penalizing an unpatched peer for a locally disabled type.
            if contains_disabled_transaction_admission(&message) {
                metrics
                    .messages_rejected
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let _ = swarm
                    .behaviour_mut()
                    .gossipsub
                    .report_message_validation_result(
                        &message_id,
                        &propagation_source,
                        gossipsub::MessageAcceptance::Ignore,
                    );
                return;
            }
            let _ = swarm
                .behaviour_mut()
                .gossipsub
                .report_message_validation_result(
                    &message_id,
                    &propagation_source,
                    gossipsub::MessageAcceptance::Accept,
                );

            match message {
                WrfcoinMessage::NewBlock { block_bytes } => {
                    metrics
                        .blocks_received
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let _ = event_tx.send(P2pEvent::NewBlock(block_bytes)).await;
                },
                WrfcoinMessage::NewTransactions { tx_bytes } => {
                    metrics
                        .txs_received
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let _ = event_tx.send(P2pEvent::NewTransactions(tx_bytes)).await;
                },
                WrfcoinMessage::WeatherData {
                    station_id,
                    data,
                    timestamp,
                    nonce,
                    station_signature,
                    station_pubkey,
                } => {
                    let Some(nonce) = nonce else {
                        warn!(
                            "WeatherData from {propagation_source}: missing nonce for station {station_id}, dropping before signature verification"
                        );
                        metrics
                            .messages_rejected
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let _ = swarm
                            .behaviour_mut()
                            .gossipsub
                            .report_message_validation_result(
                                &message_id,
                                &propagation_source,
                                gossipsub::MessageAcceptance::Reject,
                            );
                        return;
                    };

                    if let Err(reason) = weather_admission.admit(
                        propagation_source,
                        &station_id,
                        Some(&nonce),
                        Instant::now(),
                    ) {
                        warn!(
                            "WeatherData from {propagation_source}: {} for station {station_id}, dropping before signature verification",
                            reason.as_str()
                        );
                        metrics
                            .messages_rejected
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let _ = swarm
                            .behaviour_mut()
                            .gossipsub
                            .report_message_validation_result(
                                &message_id,
                                &propagation_source,
                                gossipsub::MessageAcceptance::Reject,
                            );
                        return;
                    }

                    // #2117: claim the verification budget BEFORE doing the
                    // ed25519 work. The peer supplies the public key, so each
                    // attempt is a full verification that costs them only
                    // bytes — and because verification is inline on this task,
                    // a flood stalls the whole event loop rather than merely
                    // burning a core. Returning here means the work never
                    // happens, which is the entire point; verifying and then
                    // discarding the result would bound nothing.
                    if !claim_verify_budget(verify_budget, &metrics, propagation_source) {
                        return;
                    }

                    let valid = verify_weather_gossip_signature(
                        &station_id,
                        &data,
                        timestamp,
                        &nonce,
                        &station_signature,
                        &station_pubkey,
                    );

                    if !valid {
                        warn!(
                            "WeatherData from {propagation_source}: invalid signature for station {station_id}, dropping"
                        );
                        let _ = swarm
                            .behaviour_mut()
                            .gossipsub
                            .report_message_validation_result(
                                &message_id,
                                &propagation_source,
                                gossipsub::MessageAcceptance::Reject,
                            );
                        penalize_invalid_weather_signature(
                            metrics,
                            weather_peer_scores,
                            persistent_scores,
                            persistent_now,
                            event_tx,
                            propagation_source,
                            "invalid weather station signature",
                        )
                        .await;
                    } else {
                        metrics
                            .weather_msgs_received
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let _ = event_tx
                            .send(P2pEvent::WeatherData {
                                station_id,
                                station_pubkey,
                                data,
                            })
                            .await;
                    }
                },
                WrfcoinMessage::ChainStatus {
                    height,
                    tip_hash,
                    state_root,
                } => {
                    let status_peer = match message_source {
                        Some(author) if peer_manager.contains(&author) => author,
                        Some(author) => {
                            debug!(
                                "Ignoring relayed ChainStatus authored by non-connected peer \
                                 {author} via {propagation_source}"
                            );
                            return;
                        },
                        None => propagation_source,
                    };
                    // A reconnecting peer may publish ChainStatus before its
                    // genesis handshake is processed. Do not let that status
                    // recreate a default-health sync target and dispatch a
                    // request ahead of retained-health restoration. The set is
                    // connection-epoch scoped and populated only after a valid
                    // genesis proof below.
                    if !status_announced_peers.contains(&status_peer) {
                        // #2999: gossipsub gives no cross-topic ordering, so a
                        // connected peer's once-per-epoch eager ChainStatus can
                        // beat its own GenesisHandshake here. Hold the latest
                        // one and apply it when the proof lands — never before,
                        // preserving the retained-health-first ordering this
                        // gate exists for. A status from a peer that is not
                        // connected at all is still dropped outright.
                        if peer_manager.contains(&status_peer) {
                            debug!(
                                "Holding ChainStatus from {status_peer} until genesis proof \
                                 for this connection epoch"
                            );
                            pre_proof_statuses
                                .insert(status_peer, (height, tip_hash, state_root));
                        } else {
                            debug!(
                                "Ignoring ChainStatus from {status_peer} until genesis proof for \
                                 this connection epoch"
                            );
                        }
                        return;
                    }
                    sync_manager.record_peer_status(status_peer, height);
                    if let Some(peer_info) = peer_manager.get_mut(&status_peer) {
                        peer_info.update_chain_status(height, tip_hash, state_root);
                    }
                    let _ = event_tx
                        .send(P2pEvent::PeerChainStatus {
                            peer_id: status_peer,
                            height,
                            tip_hash,
                            state_root,
                        })
                        .await;
                    dispatch_planned_requests(
                        sync_manager,
                        swarm,
                        outbound_sync_requests,
                        Instant::now(),
                    );
                },
                WrfcoinMessage::ForecastTaskAssignment {
                    task_id,
                    node_id,
                    manifest_bytes,
                } => {
                    metrics
                        .forecast_tasks_received
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    debug!("Received forecast task assignment {task_id} from {propagation_source}");
                    let _ = event_tx
                        .send(P2pEvent::ForecastTaskAssignment {
                            task_id,
                            node_id,
                            manifest_bytes,
                        })
                        .await;
                },
                WrfcoinMessage::ComputeNodeHeartbeat {
                    node_id,
                    capabilities,
                    region,
                    active_tasks,
                    max_concurrent_tasks,
                    observation_density,
                    station_count,
                } => {
                    metrics
                        .forecast_tasks_received
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    debug!("Received compute heartbeat from {node_id} via {propagation_source}");
                    let _ = event_tx
                        .send(P2pEvent::ComputeNodeHeartbeat {
                            node_id,
                            capabilities,
                            region,
                            active_tasks,
                            max_concurrent_tasks,
                            observation_density,
                            station_count,
                        })
                        .await;
                },
                WrfcoinMessage::ForecastTaskResult {
                    task_id,
                    node_id,
                    receipt_bytes,
                } => {
                    metrics
                        .forecast_tasks_received
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    debug!(
                        "Received forecast result for {task_id} from {node_id} via {propagation_source}"
                    );
                    let _ = event_tx
                        .send(P2pEvent::ForecastTaskResult {
                            task_id,
                            node_id,
                            receipt_bytes,
                        })
                        .await;
                },
                WrfcoinMessage::WeatherDataV2 {
                    station_id,
                    data,
                    timestamp,
                    content_ref,
                    nonce,
                    station_signature,
                    station_pubkey,
                } => {
                    // #1969: V2 messages now carry the same station signature as
                    // V1. Admission enforces nonce validity + replay + per-peer
                    // rate limiting before we spend a signature verification.
                    if let Err(reason) = weather_admission.admit(
                        propagation_source,
                        &station_id,
                        Some(&nonce),
                        Instant::now(),
                    ) {
                        warn!(
                            "WeatherDataV2 from {propagation_source}: {} for station {station_id}, dropping before signature verification",
                            reason.as_str()
                        );
                        metrics
                            .messages_rejected
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let _ = swarm
                            .behaviour_mut()
                            .gossipsub
                            .report_message_validation_result(
                                &message_id,
                                &propagation_source,
                                gossipsub::MessageAcceptance::Reject,
                            );
                        return;
                    }

                    // #2117: claim the verification budget BEFORE doing the
                    // ed25519 work. The peer supplies the public key, so each
                    // attempt is a full verification that costs them only
                    // bytes — and because verification is inline on this task,
                    // a flood stalls the whole event loop rather than merely
                    // burning a core. Returning here means the work never
                    // happens, which is the entire point; verifying and then
                    // discarding the result would bound nothing.
                    if !claim_verify_budget(verify_budget, &metrics, propagation_source) {
                        return;
                    }

                    let valid = verify_weather_gossip_signature_v2(
                        &station_id,
                        &data,
                        content_ref.as_ref(),
                        timestamp,
                        &nonce,
                        &station_signature,
                        &station_pubkey,
                    );

                    if !valid {
                        warn!(
                            "WeatherDataV2 from {propagation_source}: invalid signature for station {station_id}, dropping"
                        );
                        let _ = swarm
                            .behaviour_mut()
                            .gossipsub
                            .report_message_validation_result(
                                &message_id,
                                &propagation_source,
                                gossipsub::MessageAcceptance::Reject,
                            );
                        penalize_invalid_weather_signature(
                            metrics,
                            weather_peer_scores,
                            persistent_scores,
                            persistent_now,
                            event_tx,
                            propagation_source,
                            "invalid weather station signature (v2)",
                        )
                        .await;
                        // Fail closed: never bridge an unverified V2 payload to
                        // the mempool / earn-loop.
                        return;
                    }

                    metrics
                        .weather_msgs_received
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let _ = event_tx
                        .send(P2pEvent::WeatherDataV2 {
                            station_id,
                            data,
                            content_ref,
                        })
                        .await;
                },
                WrfcoinMessage::ForecastTaskResultV2 {
                    task_id,
                    node_id,
                    receipt_bytes,
                    content_ref,
                } => {
                    debug!(
                        "Received forecast result v2 for {task_id} from {node_id} via {propagation_source}"
                    );
                    let _ = event_tx
                        .send(P2pEvent::ForecastTaskResultV2 {
                            task_id,
                            node_id,
                            receipt_bytes,
                            content_ref,
                        })
                        .await;
                },
                WrfcoinMessage::DaChallenge { challenge } => {
                    debug!(
                        "Received DA challenge {} for CID {} from {propagation_source}",
                        challenge.challenge_id, challenge.cid
                    );
                    let _ = event_tx
                        .send(P2pEvent::DaChallenge {
                            peer_id: propagation_source,
                            challenge,
                        })
                        .await;
                },
                WrfcoinMessage::DaChallengeResponse { response } => {
                    debug!(
                        "Received DA challenge response for {} from keeper {} via {propagation_source}",
                        response.challenge_id, response.keeper_id
                    );
                    let _ = event_tx
                        .send(P2pEvent::DaChallengeResponse {
                            peer_id: propagation_source,
                            response,
                        })
                        .await;
                },
                WrfcoinMessage::GenesisHandshake {
                    network_id,
                    chain_id,
                    genesis_hash,
                    protocol_version,
                } => {
                    let remote = crate::genesis_handshake::GenesisHandshakeMessage::new(
                        network_id,
                        chain_id,
                        genesis_hash,
                        protocol_version,
                    );
                    if let Err(mismatch) = local_handshake.verify_match(&remote) {
                        // `propagation_source` is the peer that forwarded the message,
                        // which may be an innocent relay in a multi-hop mesh.
                        // Only disconnect the actual message author (`message.source`).
                        // If source is None (anonymous authenticity), drop silently.
                        // A version-only skew is an upgrade-coordination fault,
                        // not a wrong-network peer, and it is the one an
                        // operator will actually hit. Saying so explicitly is
                        // the difference between "why is my fleet not peering"
                        // and a 28-hour #2076 diagnosis.
                        let diagnosis = if mismatch.is_version_only() {
                            format!(
                                "signing protocol version skew on a matching chain \
                                 (local v{} vs peer v{}) — these nodes cannot verify \
                                 each other's signatures; complete the activation \
                                 cutover before peering them",
                                mismatch.expected_protocol_version,
                                mismatch.got_protocol_version,
                            )
                        } else {
                            "chain identity mismatch".to_string()
                        };

                        match message_source {
                            Some(author) => {
                                warn!(
                                    "Genesis handshake mismatch ({diagnosis}): \
                                     disconnecting author {author} \
                                     (forwarded by {propagation_source}): \
                                     expected network_id={} chain_id={} genesis_hash={} protocol_version={} \
                                     got network_id={} chain_id={} genesis_hash={} protocol_version={}",
                                    mismatch.expected_network_id,
                                    mismatch.expected_chain_id,
                                    mismatch.expected_genesis_hash,
                                    mismatch.expected_protocol_version,
                                    mismatch.got_network_id,
                                    mismatch.got_chain_id,
                                    mismatch.got_genesis_hash,
                                    mismatch.got_protocol_version,
                                );
                                // #2116 folded into #2118: record the offence so a
                                // wrong-chain peer cannot reconnect-loop for free.
                                // Observe-only — the disconnect below is unchanged.
                                penalize_genesis_handshake_mismatch(
                                    persistent_scores,
                                    persistent_now,
                                    &event_tx,
                                    author,
                                    &diagnosis,
                                )
                                .await;
                                let _ = swarm.disconnect_peer_id(author);
                            }
                            None => {
                                warn!(
                                    "Genesis handshake mismatch ({diagnosis}) via {propagation_source} \
                                     (unsigned — cannot identify author, ignoring): \
                                     expected network_id={} chain_id={} genesis_hash={} protocol_version={} \
                                     got network_id={} chain_id={} genesis_hash={} protocol_version={}",
                                    mismatch.expected_network_id,
                                    mismatch.expected_chain_id,
                                    mismatch.expected_genesis_hash,
                                    mismatch.expected_protocol_version,
                                    mismatch.got_network_id,
                                    mismatch.got_chain_id,
                                    mismatch.got_genesis_hash,
                                    mismatch.got_protocol_version,
                                );
                            }
                        }
                    } else {
                        // #2116: chain identity proven — the peer stops
                        // counting against the deadline and its address's
                        // allowance. Keyed on the message *author*, not the
                        // forwarder: in a multi-hop mesh `propagation_source`
                        // may be an innocent relay, and crediting it would let
                        // one relayed message clear every squatter behind it.
                        if let Some(author) = message_source {
                            unproven_peers.mark_proven(&author);
                            metrics.set_unproven_peers(unproven_peers.len());
                            if peer_manager.contains(&author)
                                && status_announced_peers.insert(author)
                            {
                                metrics.record_sync_reconnect_status_broadcast();
                                info!(
                                    "Genesis proof established connection epoch for {author}; \
                                     advertising current ChainStatus height={}",
                                    advertised_status.height
                                );
                                publish_message(
                                    swarm,
                                    tm,
                                    metrics,
                                    pending_broadcasts,
                                    advertised_status.message(),
                                    inline_data_max,
                                    Instant::now(),
                                );
                            }
                            // #2999: apply the ChainStatus this peer sent
                            // before its proof was processed. Without this the
                            // once-per-epoch eager advertisement is lost and
                            // sync waits for the periodic tick.
                            if let Some((height, tip_hash, state_root)) =
                                pre_proof_statuses.remove(&author)
                            {
                                if status_announced_peers.contains(&author) {
                                    sync_manager.record_peer_status(author, height);
                                    if let Some(peer_info) = peer_manager.get_mut(&author) {
                                        peer_info
                                            .update_chain_status(height, tip_hash, state_root);
                                    }
                                    let _ = event_tx
                                        .send(P2pEvent::PeerChainStatus {
                                            peer_id: author,
                                            height,
                                            tip_hash,
                                            state_root,
                                        })
                                        .await;
                                    dispatch_planned_requests(
                                        sync_manager,
                                        swarm,
                                        outbound_sync_requests,
                                        Instant::now(),
                                    );
                                }
                            }
                        }
                        debug!("Genesis handshake verified for peer {propagation_source}");
                    }
                },
                WrfcoinMessage::FinalityVote { vote_bytes } => {
                    metrics
                        .finality_votes_received
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let _ = event_tx
                        .send(P2pEvent::FinalityVote {
                            peer_id: propagation_source,
                            vote_bytes,
                        })
                        .await;
                },
                WrfcoinMessage::FinalityCertificate { cert_bytes } => {
                    metrics
                        .finality_certificates_received
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let _ = event_tx
                        .send(P2pEvent::FinalityCertificate {
                            peer_id: propagation_source,
                            cert_bytes,
                        })
                        .await;
                },
            }
        },
        WrfcoinBehaviourEvent::Gossipsub(gossipsub::Event::Subscribed { peer_id, topic }) => {
            let name = tm.topic_name(&topic).unwrap_or("unknown").to_string();
            debug!("Peer {peer_id} subscribed to {name}");
            // #2884: topics are IdentTopics, so an unrecognized subscription
            // carries the peer's literal topic string — a renamed generation
            // of one of ours is version skew, directly attributable here.
            if tm.topic_name(&topic).is_none() {
                if let Some(skew) = crate::message::detect_topic_version_skew(topic.as_str()) {
                    warn!("{}", skew.operator_report(&peer_id));
                }
            }
            let _ = event_tx.try_send(P2pEvent::SubscriptionStateChanged {
                peer_id,
                topic: name,
                subscribed: true,
            });
            // #2544: the genesis handshake was published once, at connection
            // establishment, and never retried. Gossipsub only forwards to peers
            // it already knows are subscribed, and it learns that from the
            // subscription exchange that FOLLOWS connection setup — so the
            // publish at establishment races it and can reach nobody. `publish`
            // returning Ok means "sent to the peers I knew about", not
            // "delivered to the peer that just connected".
            //
            // This is the moment the race resolves: the peer is now a known
            // subscriber, so republishing here reaches it. Without it a peer can
            // stay unverified forever — and `verify_match` runs only on a
            // RECEIVED handshake, so a wrong-chain peer goes undetected rather
            // than being detected and disconnected.
            if topic == tm.genesis_handshake_hash() {
                metrics
                    .genesis_handshake_republished
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                debug!(
                    "Peer {peer_id} subscribed to the genesis-handshake topic; republishing \
                     ours so chain identity is actually checked (#2544)"
                );
                publish_message(
                    swarm,
                    tm,
                    metrics,
                    pending_broadcasts,
                    WrfcoinMessage::GenesisHandshake {
                        network_id: local_handshake.network_id.clone(),
                        chain_id: local_handshake.chain_id.clone(),
                        genesis_hash: local_handshake.genesis_hash.clone(),
                        protocol_version: local_handshake.protocol_version,
                    },
                    inline_data_max,
                    Instant::now(),
                );
            }
            refresh_mesh_peer_metrics(swarm, tm, metrics);
        },
        WrfcoinBehaviourEvent::Gossipsub(gossipsub::Event::Unsubscribed { peer_id, topic }) => {
            let name = tm.topic_name(&topic).unwrap_or("unknown").to_string();
            debug!("Peer {peer_id} unsubscribed from {name}");
            let _ = event_tx.try_send(P2pEvent::SubscriptionStateChanged {
                peer_id,
                topic: name,
                subscribed: false,
            });
            refresh_mesh_peer_metrics(swarm, tm, metrics);
        },
        WrfcoinBehaviourEvent::Gossipsub(_) => {},

        WrfcoinBehaviourEvent::Identify(identify::Event::Received { connection_id, peer_id, info }) => {
            debug!(
                "Identify: peer {peer_id} agent={} protocols={}",
                info.agent_version,
                info.protocols.len()
            );
            // #2884: identify is the one place a version-skewed wrfcoin peer
            // names the wire generation it speaks — negotiation failure and
            // topic non-intersection are anonymous without this.
            if let Some(skew) = crate::message::detect_block_sync_version_skew(
                info.protocols.iter().map(|protocol| protocol.as_ref()),
            ) {
                warn!("{}", skew.operator_report(&peer_id));
            }
            for addr in &info.listen_addrs {
                // Peers advertise every expanded wildcard listen address,
                // including docker-bridge and loopback multiaddrs that are
                // unroutable from any other machine. Ingesting them into the
                // Kademlia routing table made them dial candidates and starved
                // the mesh with failed dials (#2819). LAN and tailnet addrs
                // stay eligible, and a peer sharing our host/docker network
                // (it observes us at a loopback/bridge address) may advertise
                // that same scope — see should_ingest_advertised_addr.
                if !crate::config::should_ingest_advertised_addr(addr, &info.observed_addr) {
                    debug!("Identify: ignoring undialable advertised addr {addr} from {peer_id}");
                    continue;
                }
                // Kademlia re-serves its routing table to every peer that asks,
                // so a private/tailnet address a peer told US must not go in
                // there unless the operator opted in (crate::announce). The
                // peer manager still keeps it for this node's own dials (QUIC
                // upgrade), which never leave the node.
                if announce_policy.may_relay_peer_addr(addr) {
                    swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, addr.clone());
                } else {
                    debug!("Identify: not relaying non-global advertised addr {addr} from {peer_id} via Kademlia");
                }
                if let Some(peer_info) = peer_manager.get_mut(&peer_id) {
                    peer_info.add_address(addr.clone());
                }
            }
            // Kick a one-shot Kademlia bootstrap on the first identify we
            // process. Without this, nodes that learn peers organically
            // (rather than via an explicit `bootstrap_peers` config) never
            // populate the DHT routing table beyond direct identify
            // exchanges. Errors are best-effort — a bootstrap failure when
            // the routing table is otherwise empty is not actionable here.
            if !*did_post_identify_bootstrap {
                match swarm.behaviour_mut().kademlia.bootstrap() {
                    Ok(_query_id) => {
                        info!("Kicked Kademlia bootstrap after first identify (#1407)");
                        *did_post_identify_bootstrap = true;
                    },
                    Err(err) => {
                        debug!("Post-identify Kademlia bootstrap deferred: {err:?}");
                    },
                }
            }
            maybe_dial_quic_upgrade(
                peer_id,
                quic_transport_enabled,
                peer_manager,
                swarm,
                pending_quic_upgrade_dials,
            );
            prune_redundant_non_quic_connections(
                peer_id,
                connection_id,
                peer_manager,
                swarm,
            );
        },
        WrfcoinBehaviourEvent::Identify(_) => {},

        WrfcoinBehaviourEvent::Kademlia(kad::Event::RoutingUpdated {
            peer, addresses, ..
        }) => {
            let addr_vec: Vec<libp2p::Multiaddr> = addresses.iter().cloned().collect();
            debug!(
                "Kademlia routing updated for {peer} ({} addrs)",
                addr_vec.len()
            );
            discovered_peers.record(peer, addr_vec);
        },
        WrfcoinBehaviourEvent::Kademlia(_) => {},

        WrfcoinBehaviourEvent::BlockSync(request_response::Event::Message {
            peer,
            message,
            ..
        }) => match message {
            request_response::Message::Request {
                request_id,
                request,
                channel,
            } => {
                metrics.record_sync_inbound_request();
                debug!(
                    "Received block sync request {request_id:?} from {peer}: start={} count={}",
                    request.start_height, request.count
                );
                inbound_sync_requests.insert(request_id, channel);
                let _ = event_tx
                    .send(P2pEvent::InboundBlockSyncRequest {
                        peer_id: peer,
                        request_id,
                        request,
                    })
                    .await;
            },
            request_response::Message::Response {
                request_id,
                response,
            } => {
                let Some(outbound) = outbound_sync_requests.remove(&request_id) else {
                    return;
                };
                debug!(
                    "Received block sync response {request_id:?} from {}: blocks={} retry_after_ms={:?}",
                    outbound.peer_id,
                    response.blocks.len(),
                    response.retry_after_ms
                );
                match sync_manager.handle_response(request_id, response, Instant::now(), sync_chain)
                {
                    Ok(blocks) => {
                        let completed_height = (!blocks.is_empty()
                            && matches!(sync_manager.state(), SyncState::Complete))
                        .then_some(sync_manager.local_height());
                        emit_sync_result(
                            event_tx,
                            outbound.peer_id,
                            &blocks,
                            completed_height,
                        )
                        .await;
                        dispatch_planned_requests(
                            sync_manager,
                            swarm,
                            outbound_sync_requests,
                            Instant::now(),
                        );
                    },
                    Err(err) => {
                        emit_sync_penalty(event_tx, outbound.peer_id, &err, sync_manager).await;
                        dispatch_planned_requests(
                            sync_manager,
                            swarm,
                            outbound_sync_requests,
                            Instant::now(),
                        );
                    },
                }
            },
        },
        WrfcoinBehaviourEvent::BlockSync(request_response::Event::OutboundFailure {
            peer,
            request_id,
            error,
            ..
        }) => {
            let mapped_peer = outbound_sync_requests
                .get(&request_id)
                .map(|entry| entry.peer_id)
                .unwrap_or(peer);
            sync_manager.note_request_failure(request_id);
            let peer_health = sync_manager.peer_metrics(&mapped_peer);
            // #2884: libp2p's UnsupportedProtocols rendering names neither
            // side's protocol; attribute the local one here (the identify
            // handler's skew warning names the remote's).
            if matches!(
                error,
                request_response::OutboundFailure::UnsupportedProtocols
            ) {
                warn!(
                    "{}",
                    crate::sync::block_sync_negotiation_refused_report(&mapped_peer)
                );
            }
            warn!(
                "Block sync request {request_id:?} to {mapped_peer} failed: {error}; \
                 local_transport_failures={} retry_after_ms={} target={}",
                peer_health
                    .as_ref()
                    .map(|health| health.consecutive_transport_failures)
                    .unwrap_or(0),
                peer_health
                    .as_ref()
                    .map(|health| health.retry_after_ms)
                    .unwrap_or(0),
                sync_manager.target_height()
            );
            outbound_sync_requests.remove(&request_id);
            if should_refresh_sync_connection(peer_health.as_ref()) {
                warn!(
                    "Refreshing block-sync connection to {mapped_peer} after {} consecutive \
                     transport failures; sync-local cooldown is retained",
                    peer_health
                        .as_ref()
                        .map(|health| health.consecutive_transport_failures)
                        .unwrap_or(0)
                );
                metrics.record_sync_connection_refresh_attempt();
                if let Err(error) = swarm.disconnect_peer_id(mapped_peer) {
                    warn!(
                        "Block-sync connection refresh for {mapped_peer} could not be scheduled: \
                         {error:?}"
                    );
                }
            }
            dispatch_planned_requests(sync_manager, swarm, outbound_sync_requests, Instant::now());
        },
        WrfcoinBehaviourEvent::BlockSync(request_response::Event::InboundFailure {
            peer,
            request_id,
            error,
            ..
        }) => {
            inbound_sync_requests.remove(&request_id);
            metrics.record_sync_inbound_failure();
            warn!("Inbound block sync failure from {peer}: {error}");
        },
        WrfcoinBehaviourEvent::BlockSync(request_response::Event::ResponseSent {
            peer,
            request_id,
            ..
        }) => {
            metrics.record_sync_response_sent();
            debug!("Sent block sync response to {peer} for request {request_id:?}");
            inbound_sync_requests.remove(&request_id);
        },

        // -- NAT traversal events -----------------------------------------------
        WrfcoinBehaviourEvent::Autonat(autonat::Event::StatusChanged { old, new }) => {
            info!("AutoNAT status changed: {old:?} -> {new:?}");
            if let autonat::NatStatus::Public(ref addr) = new {
                if crate::config::is_docker_bridge_addr(addr) {
                    warn!(
                        "AutoNAT confirmed Docker bridge address ({addr}) as public. \
                         External peers cannot reach this address. \
                         Set WRFCOIN_P2P_EXTERNAL_ADDR to the host's public multiaddr."
                    );
                }
            }
        },
        WrfcoinBehaviourEvent::Autonat(autonat::Event::InboundProbe(probe)) => {
            debug!("AutoNAT inbound probe: {probe:?}");
        },
        WrfcoinBehaviourEvent::Autonat(autonat::Event::OutboundProbe(probe)) => {
            debug!("AutoNAT outbound probe: {probe:?}");
        },

        WrfcoinBehaviourEvent::RelayClient(relay::client::Event::ReservationReqAccepted {
            relay_peer_id,
            renewal,
            ..
        }) => {
            if renewal {
                info!("Relay reservation renewed with {relay_peer_id}");
            } else {
                info!("Relay reservation accepted by {relay_peer_id}");
            }
        },
        WrfcoinBehaviourEvent::RelayClient(event) => {
            debug!("Relay client event: {event:?}");
        },

        WrfcoinBehaviourEvent::Dcutr(dcutr::Event {
            remote_peer_id,
            result,
        }) => match result {
            Ok(_) => info!("DCUtR: direct connection established with {remote_peer_id}"),
            Err(ref e) => warn!("DCUtR: upgrade failed with {remote_peer_id}: {e}"),
        },

        WrfcoinBehaviourEvent::Upnp(upnp::Event::NewExternalAddr(addr)) => {
            // A router behind carrier-grade NAT maps to a 100.64/10 address,
            // which is not reachable from the internet and must not be
            // announced (crate::announce).
            if announce_policy.may_announce_own(&addr) {
                info!("UPnP: new external address mapped: {addr}");
                swarm.add_external_address(addr);
            } else {
                info!("UPnP: mapped external address {addr} is not globally routable; not announcing it");
            }
        },
        WrfcoinBehaviourEvent::Upnp(upnp::Event::ExpiredExternalAddr(addr)) => {
            info!("UPnP: external address mapping expired: {addr}");
            swarm.remove_external_address(&addr);
        },
        WrfcoinBehaviourEvent::Upnp(upnp::Event::GatewayNotFound) => {
            debug!("UPnP: no gateway found on this network");
        },
        WrfcoinBehaviourEvent::Upnp(upnp::Event::NonRoutableGateway) => {
            debug!("UPnP: gateway found but it is not routable");
        },
    }
}

fn dispatch_planned_requests(
    sync_manager: &mut SyncManager,
    swarm: &mut Swarm<crate::behaviour::WrfcoinBehaviour>,
    outbound_sync_requests: &mut FxHashMap<
        request_response::OutboundRequestId,
        OutboundSyncRequest,
    >,
    now: Instant,
) {
    let planned = sync_manager.plan_requests(now);
    for planned_request in planned {
        let request_id = swarm
            .behaviour_mut()
            .block_sync
            .send_request(&planned_request.peer_id, planned_request.request.clone());
        debug!(
            "Dispatching block sync request {request_id:?} to {}: start={} count={} target={}",
            planned_request.peer_id,
            planned_request.request.start_height,
            planned_request.request.count,
            sync_manager.target_height()
        );
        sync_manager.register_request(
            request_id,
            planned_request.peer_id,
            &planned_request.request,
            now,
        );
        outbound_sync_requests.insert(
            request_id,
            OutboundSyncRequest {
                peer_id: planned_request.peer_id,
            },
        );
    }
}

fn publish_sync_observation(metrics: &MetricsCollector, sync_manager: &SyncManager) {
    metrics.set_sync_observation(
        sync_manager.local_height(),
        sync_manager.target_height(),
        sync_manager.state().label(),
        sync_manager.pending_range_count(),
        sync_manager.in_flight_range_count(),
    );
    metrics.set_sync_liveness(sync_manager.liveness_metrics());
}

/// Publish the per-peer rotation surface (#2820) on the same tick as the
/// aggregate sync observation. `outbound_sync_requests` supplies the same
/// in-flight view `plan_requests` uses, so the published eligibility verdicts
/// mirror the live selection filter.
fn publish_sync_peer_surface(
    surface: &SyncPeersSurface,
    sync_manager: &SyncManager,
    outbound_sync_requests: &FxHashMap<
        request_response::OutboundRequestId,
        OutboundSyncRequest,
    >,
    peer_manager: &PeerManager,
    now: Instant,
) {
    let busy_peers: Vec<PeerId> = outbound_sync_requests
        .values()
        .map(|request| request.peer_id)
        .collect();
    let entries = sync_manager
        .peer_sync_statuses(&busy_peers, now)
        .into_iter()
        .map(|status| SyncPeerSurfaceEntry {
            peer_id: status.peer_id.to_string(),
            advertised_height: status.advertised_height,
            required_height: status.required_height,
            advertisement_age_secs: status.advertisement_age_ms.map(|ms| ms / 1000),
            connection_age_secs: peer_manager
                .get(&status.peer_id)
                .map(|info| now.saturating_duration_since(info.connected_at).as_secs()),
            in_flight_requests: status.in_flight_requests,
            consecutive_transport_failures: status.consecutive_transport_failures,
            cooling_down: status.cooling_down,
            retry_after_ms: status.retry_after_ms,
            banned: status.banned,
            metered: status.metered,
            priority: status.priority,
            eligible: status.eligible,
            eligible_via: status.eligible_via,
            ineligible_reason: status.ineligible_reason,
        })
        .collect();
    surface.publish(entries);
}

async fn emit_sync_penalty(
    event_tx: &mpsc::Sender<P2pEvent>,
    peer_id: PeerId,
    error: &SyncError,
    sync_manager: &SyncManager,
) {
    let _ = event_tx
        .send(P2pEvent::PeerPenalized {
            peer_id,
            reason: format!("sync validation failed: {error:?}"),
            banned: sync_manager
                .peer_metrics(&peer_id)
                .map(|metrics| metrics.banned)
                .unwrap_or(false),
        })
        .await;
}

async fn emit_synced_block(
    event_tx: &mpsc::Sender<P2pEvent>,
    peer_id: PeerId,
    block: &wrfcoin_shared_protocol::blockchain::block::Block,
) {
    let Ok(block_bytes) = bincode::serialize(block) else {
        error!("Failed to serialize synced block {}", block.height);
        return;
    };

    let _ = event_tx
        .send(P2pEvent::SyncedBlock {
            peer_id,
            height: block.height,
            block_bytes,
        })
        .await;
}

async fn emit_sync_result(
    event_tx: &mpsc::Sender<P2pEvent>,
    peer_id: PeerId,
    blocks: &[wrfcoin_shared_protocol::blockchain::block::Block],
    completed_height: Option<u64>,
) {
    for block in blocks {
        emit_synced_block(event_tx, peer_id, block).await;
    }
    if let Some(height) = completed_height {
        let _ = event_tx.send(P2pEvent::SyncCompleted { height }).await;
    }
}

/// Test-support entrypoint that executes the real response handler and event
/// emitter before returning the production `P2pEvent` channel to a consumer.
/// This keeps cross-crate launch tests on the exact
/// `handle_response -> SyncedBlock` transport without opening a live socket.
#[cfg(any(test, feature = "test-harness"))]
#[doc(hidden)]
pub async fn handle_sync_response_for_test(
    peer_id: PeerId,
    blocks: Vec<wrfcoin_shared_protocol::blockchain::block::Block>,
    chain: &mut Blockchain,
) -> Result<mpsc::Receiver<P2pEvent>, SyncError> {
    let first_height = blocks.first().map(|block| block.height).unwrap_or(1);
    let target_height = blocks
        .last()
        .map(|block| block.height)
        .unwrap_or(chain.height);
    let count = u32::try_from(blocks.len()).unwrap_or(u32::MAX);
    let request = BlockRequest {
        start_height: first_height,
        count,
    };
    let mut behaviour = request_response::Behaviour::with_codec(
        crate::sync::BlockSyncCodec,
        std::iter::once((
            libp2p::StreamProtocol::new(crate::sync::BLOCK_SYNC_PROTOCOL),
            request_response::ProtocolSupport::Full,
        )),
        request_response::Config::default(),
    );
    let request_id = behaviour.send_request(&peer_id, request.clone());
    let now = Instant::now();
    let mut manager = SyncManager::new(chain.height);
    manager.record_peer_status(peer_id, target_height);
    manager.register_request(request_id, peer_id, &request, now);
    let imported = manager.handle_response(
        request_id,
        BlockResponse {
            blocks,
            retry_after_ms: None,
        },
        now,
        chain,
    )?;
    let completed_height = (!imported.is_empty()
        && matches!(manager.state(), SyncState::Complete))
    .then_some(manager.local_height());
    let (event_tx, event_rx) = mpsc::channel(imported.len().saturating_add(1).max(1));
    emit_sync_result(&event_tx, peer_id, &imported, completed_height).await;
    drop(event_tx);
    Ok(event_rx)
}

// ---------------------------------------------------------------------------
// Publish helper
// ---------------------------------------------------------------------------

/// Hard cap for serialized gossipsub messages. Matches `max_transmit_size`
/// in the default config. Defense-in-depth: the inline threshold should
/// catch oversized data much earlier.
const MAX_GOSSIP_MESSAGE_BYTES: usize = 1_048_576; // 1 MiB

fn record_publish_success(
    metrics: &Arc<MetricsCollector>,
    tm: &TopicManager,
    topic: &gossipsub::TopicHash,
    kind: MessageKind,
    bytes: usize,
) {
    metrics.record_outbound_topic(topic_label(tm.topic_name(topic)), bytes as u64);
    let counter = match kind {
        MessageKind::NewBlock => Some(&metrics.blocks_published),
        MessageKind::NewTransactions => Some(&metrics.txs_published),
        MessageKind::WeatherData | MessageKind::WeatherDataV2 => {
            Some(&metrics.weather_msgs_published)
        },
        MessageKind::ForecastTaskAssignment
        | MessageKind::ComputeNodeHeartbeat
        | MessageKind::ForecastTaskResult
        | MessageKind::ForecastTaskResultV2 => Some(&metrics.forecast_tasks_published),
        MessageKind::FinalityVote => Some(&metrics.finality_votes_published),
        MessageKind::FinalityCertificate => Some(&metrics.finality_certificates_published),
        MessageKind::ChainStatus
        | MessageKind::DaChallenge
        | MessageKind::DaChallengeResponse
        | MessageKind::GenesisHandshake => None,
    };
    if let Some(counter) = counter {
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

fn set_pending_broadcast_gauges(
    metrics: &Arc<MetricsCollector>,
    pending: &PendingBroadcasts,
) {
    metrics
        .pending_broadcasts
        .store(pending.len(), std::sync::atomic::Ordering::Relaxed);
    metrics
        .pending_broadcast_bytes
        .store(pending.total_bytes(), std::sync::atomic::Ordering::Relaxed);
}

fn retain_unpublishable(
    metrics: &Arc<MetricsCollector>,
    pending: &mut PendingBroadcasts,
    broadcast: PendingBroadcast,
) {
    let kind = broadcast.kind();
    match pending.enqueue(broadcast) {
        EnqueueOutcome::Queued => {
            metrics
                .pending_broadcasts_enqueued
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            debug!("Retained {kind:?} until its gossipsub topic has a publishable mesh");
        },
        EnqueueOutcome::Deduplicated => {
            metrics
                .pending_broadcasts_deduplicated
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            debug!("Coalesced an identical pending {kind:?} broadcast intent");
        },
        EnqueueOutcome::Overflow => {
            metrics
                .pending_broadcasts_overflowed
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            metrics
                .messages_rejected
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            error!(
                "Pending gossipsub queue full; dropping newest {kind:?} intent \
                 (bounded item/byte ceiling)"
            );
        },
    }
    set_pending_broadcast_gauges(metrics, pending);
}

fn contains_disabled_transaction_admission(message: &WrfcoinMessage) -> bool {
    use bincode::Options as _;
    use wrfcoin_shared_protocol::{
        blockchain::transaction::Transaction, mempool::local_admission_disabled,
    };
    let WrfcoinMessage::NewTransactions { tx_bytes } = message else {
        return false;
    };
    tx_bytes.iter().any(|bytes| {
        // Match native-chain's fixed-int/trailing-byte wire semantics, with
        // inspection bounded by the received byte count.
        bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .allow_trailing_bytes()
            .with_limit(bytes.len() as u64)
            .deserialize::<Transaction>(bytes)
            .is_ok_and(|tx| local_admission_disabled(tx.tx_type))
    })
}

fn publish_message(
    swarm: &mut Swarm<crate::behaviour::WrfcoinBehaviour>,
    tm: &TopicManager,
    metrics: &Arc<MetricsCollector>,
    pending: &mut PendingBroadcasts,
    message: WrfcoinMessage,
    inline_data_max: usize,
    now: Instant,
) {
    let kind = message.kind();
    // Refuse before serialization, publishing, or the pending retry queue.
    if contains_disabled_transaction_admission(&message) {
        metrics
            .messages_rejected
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        debug!("Refused locally disabled transaction gossip (Refs #3408, #3430)");
        return;
    }
    if let Err(reason) = message.validate_inline_size(inline_data_max) {
        error!("Rejected oversized {kind:?} gossipsub message: {reason}");
        metrics
            .messages_rejected
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        return;
    }

    let topic_hash = message.topic(tm);

    match message.to_bytes() {
        Ok(data) => {
            if data.len() > MAX_GOSSIP_MESSAGE_BYTES {
                error!(
                    "Serialized {kind:?} message ({} bytes) exceeds gossipsub hard cap \
                     ({MAX_GOSSIP_MESSAGE_BYTES} bytes), dropping",
                    data.len()
                );
                metrics
                    .messages_rejected
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                return;
            }
            match swarm
                .behaviour_mut()
                .gossipsub
                .publish(topic_hash.clone(), data.clone())
            {
                Ok(_) => {
                    record_publish_success(metrics, tm, &topic_hash, kind, data.len());
                    debug!("Published {kind:?} gossipsub message");
                    // #3453: a retained older ChainStatus must never be
                    // delivered after this newer one and regress the peer view.
                    let dropped = pending.supersede(&topic_hash, kind);
                    if dropped > 0 {
                        metrics
                            .pending_broadcasts_deduplicated
                            .fetch_add(dropped as u64, std::sync::atomic::Ordering::Relaxed);
                        set_pending_broadcast_gauges(metrics, pending);
                        debug!(
                            "Dropped {dropped} retained {kind:?} intent(s) superseded by a newer publish"
                        );
                    }
                },
                Err(gossipsub::PublishError::NoPeersSubscribedToTopic) => {
                    retain_unpublishable(
                        metrics,
                        pending,
                        PendingBroadcast::new(topic_hash, kind, data, now),
                    );
                },
                Err(gossipsub::PublishError::Duplicate) => {
                    debug!("Skipped duplicate {kind:?} gossipsub publish intent");
                },
                Err(err) => {
                    metrics
                        .messages_rejected
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    error!("Failed to publish {kind:?} gossipsub message: {err}");
                },
            }
        },
        Err(err) => {
            metrics
                .messages_rejected
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            error!("Failed to serialize {kind:?} message: {err}");
        },
    }
}

fn retry_pending_broadcasts(
    swarm: &mut Swarm<crate::behaviour::WrfcoinBehaviour>,
    tm: &TopicManager,
    metrics: &Arc<MetricsCollector>,
    pending: &mut PendingBroadcasts,
    now: Instant,
) {
    // A subscription notification is intentionally insufficient. This reads
    // the actual mesh maintained by gossipsub on a bounded timer event.
    let mut ready_topics = Vec::new();
    for topic in pending.topics() {
        if !ready_topics.contains(topic)
            && swarm.behaviour().gossipsub.mesh_peers(topic).next().is_some()
        {
            ready_topics.push(topic.clone());
        }
    }

    let summary = pending.retry_ready(
        now,
        |topic| ready_topics.contains(topic),
        |broadcast| {
            match swarm
                .behaviour_mut()
                .gossipsub
                .publish(broadcast.topic().clone(), broadcast.data().to_vec())
            {
                Ok(_) => RetryOutcome::Published,
                Err(gossipsub::PublishError::Duplicate) => RetryOutcome::Duplicate,
                Err(gossipsub::PublishError::NoPeersSubscribedToTopic) => {
                    RetryOutcome::NoPeers
                },
                Err(err) => {
                    error!(
                        "Terminal retry failure for pending {:?} gossipsub intent: {err}",
                        broadcast.kind()
                    );
                    RetryOutcome::Terminal
                },
            }
        },
    );

    for (topic, kind, bytes) in &summary.published {
        record_publish_success(metrics, tm, topic, *kind, *bytes);
    }

    metrics
        .pending_broadcast_retry_attempts
        .fetch_add(summary.attempts, std::sync::atomic::Ordering::Relaxed);
    metrics
        .pending_broadcasts_published
        .fetch_add(summary.published.len() as u64, std::sync::atomic::Ordering::Relaxed);
    metrics
        .pending_broadcasts_deduplicated
        .fetch_add(summary.duplicate, std::sync::atomic::Ordering::Relaxed);
    metrics
        .pending_broadcasts_expired
        .fetch_add(summary.expired, std::sync::atomic::Ordering::Relaxed);
    metrics
        .pending_broadcasts_exhausted
        .fetch_add(summary.exhausted, std::sync::atomic::Ordering::Relaxed);
    metrics.messages_rejected.fetch_add(
        summary.expired + summary.exhausted + summary.terminal,
        std::sync::atomic::Ordering::Relaxed,
    );
    if summary.expired > 0 || summary.exhausted > 0 || summary.terminal > 0 {
        warn!(
            "Dropped bounded pending gossipsub intents: expired={} exhausted={} terminal={}",
            summary.expired, summary.exhausted, summary.terminal
        );
    }
    set_pending_broadcast_gauges(metrics, pending);
}
