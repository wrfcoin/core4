//! Wire messages exchanged over the gossipsub overlay and topic management.

use libp2p::gossipsub::{self, IdentTopic, TopicHash};
use serde::{Deserialize, Serialize};

use crate::da_challenge::{DataAvailabilityChallenge, DataAvailabilityChallengeResponse};

// ---------------------------------------------------------------------------
// Wire format version (#2884)
// ---------------------------------------------------------------------------

/// The version of every bincode payload layout carried on this node's wire
/// identifiers — the gossipsub topics below and [`crate::sync::BLOCK_SYNC_PROTOCOL`].
///
/// **Bump this whenever the serialized layout of any wire-carried type
/// changes** — `WrfcoinMessage` variant payloads, `Block`, `Transaction`,
/// `FinalityVote`/`FinalityCertificate`, `BlockRequest`/`BlockResponse`, or
/// anything they transitively contain. bincode is positional and ignores
/// `#[serde(default)]`, so ANY field insertion, removal, or reorder is a wire
/// break (the #2727 hazard class on the network — #2884). Three such breaks
/// shipped on unchanged identifiers in one month: `Block` gained two
/// mid-struct producer fields (`ae0a22833`, #2226/#2371), `FinalityVote`
/// gained two mid-struct fields (`927e239ed`, #2227/#2366), and
/// `BlockResponse` appended `retry_after_ms` (`0bb151ff9`, #2539/#2645) whose
/// own doc comment says bincode makes it breaking — all absorbed only because
/// full-fleet flag-day redeploys happened to follow.
///
/// The `wire_identifiers_carry_the_wire_format_version` test binds every
/// topic string and the block-sync protocol string to this constant: bumping
/// it without renaming the identifiers is a RED test. A renamed identifier
/// moves the failure from payload decode to the identifier layer — but note
/// precisely what that looks like, because libp2p does not name it for you:
/// block-sync requests to a skewed peer fail protocol negotiation, surfaced
/// as a generic `OutboundFailure::UnsupportedProtocols` (the connection stays
/// open), and renamed gossip topics simply never intersect, so the peer stays
/// connected but exchanges no wrfcoin gossip at all. Neither event names
/// either side's identifier. The attribution therefore lives in this crate:
/// [`detect_block_sync_version_skew`] (run on every identify exchange) and
/// [`detect_topic_version_skew`] (run on every unrecognized gossipsub
/// subscription) emit warnings naming the local and remote identifier
/// generations, and the block-sync failure handler names the local protocol
/// on `UnsupportedProtocols`. #2892's decode counters cover same-identifier
/// skew; these warnings cover renamed-identifier skew, where no payload ever
/// reaches decode and #2892 counts nothing.
///
/// **On bump, choose the rollout explicitly**: either ship one release that
/// speaks both versions (list both protocols in the request_response
/// behaviour's `protocols` iterator; subscribe both topic generations,
/// publish to the new one) and drop the old identifiers in the release after
/// — or declare a flag-day full-fleet redeploy in the deploy plan. Either is
/// acceptable; what is not acceptable is the bump happening by accident or
/// not happening at all. The #2883 golden-fixture tripwire (planned) is what
/// will make a layout change impossible to miss; this constant is where the
/// acknowledgement lands.
///
/// Version 1 == the layout deployed by the 2026-08-28 mesh-cut. The
/// identifiers below already carried a `/1` (and `/1.0.0`) suffix, so pinning
/// version 1 changes no byte on the wire.
pub const WIRE_FORMAT_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Gossipsub topic constants
// ---------------------------------------------------------------------------
//
// Every topic name ends in the wire-format version and is test-bound to
// [`WIRE_FORMAT_VERSION`] — see its doc for the bump policy (#2884).

/// Topic string for block announcements.
pub const BLOCKS_TOPIC: &str = "wrfcoin/blocks/1";
/// Topic string for transaction broadcasts.
pub const TRANSACTIONS_TOPIC: &str = "wrfcoin/transactions/1";
/// Topic string for weather-data messages.
pub const WEATHER_DATA_TOPIC: &str = "wrfcoin/weather-data/1";
/// Topic string for forecast task distribution messages.
pub const FORECAST_TASKS_TOPIC: &str = "wrfcoin/forecast-tasks/1";
/// Topic string for Data Availability challenges and responses.
pub const DA_CHALLENGES_TOPIC: &str = "wrfcoin/da-challenges/1";
/// Topic string for genesis handshake messages exchanged on connect.
pub const GENESIS_HANDSHAKE_TOPIC: &str = "wrfcoin/genesis-handshake/1";
/// Topic string for BFT finality votes and certificates (#1388).
///
/// Carries [`WrfcoinMessage::FinalityVote`] and
/// [`WrfcoinMessage::FinalityCertificate`] payloads. Both are bincode-serialized
/// opaque bytes whose schema lives in `consensus-engines/src/selection/bft_finality.rs`,
/// so the p2p layer does not depend on the consensus crate.
pub const FINALITY_VOTES_TOPIC: &str = "wrfcoin/finality-votes/1";

/// Every gossip topic bound to [`WIRE_FORMAT_VERSION`], in one place so the
/// skew detectors below track a future bump automatically.
pub const WIRE_TOPICS: [&str; 7] = [
    BLOCKS_TOPIC,
    TRANSACTIONS_TOPIC,
    WEATHER_DATA_TOPIC,
    FORECAST_TASKS_TOPIC,
    DA_CHALLENGES_TOPIC,
    GENESIS_HANDSHAKE_TOPIC,
    FINALITY_VOTES_TOPIC,
];

// ---------------------------------------------------------------------------
// Wire-format version-skew attribution (#2884)
// ---------------------------------------------------------------------------
//
// A renamed identifier makes version skew fail early, but libp2p's own events
// never name either side's identifier: block-sync negotiation failure renders
// as a bare "the remote supports none of the requested protocols", and a
// renamed gossip topic is just a topic we don't recognize. These detectors
// are what turn those anonymous events into an operator-readable statement of
// WHICH versions disagree. Both are pure functions so the exact surfaced text
// is unit-testable without a live swarm.

/// Prefix shared by every generation of
/// [`crate::sync::BLOCK_SYNC_PROTOCOL`].
const BLOCK_SYNC_PROTOCOL_PREFIX: &str = "/wrfcoin/block-sync/";

/// Wire-format version skew inferred from a peer's identify-advertised
/// protocol list: the peer speaks a *different generation* of the block-sync
/// protocol and not ours.
///
/// Because the `wire_identifiers_carry_the_wire_format_version` test binds
/// the block-sync protocol and all seven gossip topics to the same
/// [`WIRE_FORMAT_VERSION`], a block-sync generation mismatch seen in identify
/// attributes BOTH failure surfaces at once: block-sync negotiation refusals
/// and the absence of shared gossip topics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockSyncVersionSkew {
    /// The block-sync protocol this node speaks.
    pub local_protocol: &'static str,
    /// The `/wrfcoin/block-sync/…` protocol the peer advertised instead.
    pub remote_protocol: String,
}

impl BlockSyncVersionSkew {
    /// The wire-format version the peer advertises, when its protocol id
    /// parses as `/wrfcoin/block-sync/<major>[.…]`.
    pub fn remote_wire_version(&self) -> Option<u32> {
        let rest = self.remote_protocol.strip_prefix(BLOCK_SYNC_PROTOCOL_PREFIX)?;
        rest.split('.').next()?.parse().ok()
    }

    /// The operator-facing warning: names both identifiers, both versions,
    /// and what the mismatch does to each wire surface.
    pub fn operator_report(&self, peer: &impl std::fmt::Display) -> String {
        let remote_version = self
            .remote_wire_version()
            .map(|v| format!("wire format {v}"))
            .unwrap_or_else(|| "unparseable wire format".to_string());
        format!(
            "wire-format version skew with peer {peer}: it advertises {remote} ({remote_version}) \
             but this node speaks {local} (wire format {local_version}). Block-sync requests \
             between the two nodes fail protocol negotiation (surfaced as UnsupportedProtocols; \
             the connection stays OPEN), and every wrfcoin gossip topic carries the same version, \
             so the nodes share no gossip topics either: the peer stays connected but exchanges \
             no wrfcoin traffic. Upgrade one side, or ship a dual-version release (#2884)",
            remote = self.remote_protocol,
            local = self.local_protocol,
            local_version = WIRE_FORMAT_VERSION,
        )
    }
}

/// Scan a peer's identify-advertised protocols for block-sync generation
/// skew. Returns `None` when the peer advertises our exact block-sync
/// protocol (even alongside other generations — a dual-version peer
/// mid-rollout interoperates with us), and `None` when it advertises no
/// `/wrfcoin/block-sync/…` protocol at all (a non-wrfcoin peer is not
/// version skew).
///
/// During an intentional dual-version rollout on OUR side this still fires
/// for every peer that only speaks the previous generation — which is the
/// upgrade inventory the deploy plan needs, not noise.
pub fn detect_block_sync_version_skew<'a, I>(remote_protocols: I) -> Option<BlockSyncVersionSkew>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut candidate: Option<String> = None;
    for protocol in remote_protocols {
        if protocol == crate::sync::BLOCK_SYNC_PROTOCOL {
            return None;
        }
        if candidate.is_none() && protocol.starts_with(BLOCK_SYNC_PROTOCOL_PREFIX) {
            candidate = Some(protocol.to_string());
        }
    }
    candidate.map(|remote_protocol| BlockSyncVersionSkew {
        local_protocol: crate::sync::BLOCK_SYNC_PROTOCOL,
        remote_protocol,
    })
}

/// Wire-format version skew inferred from a gossipsub subscription: a peer
/// subscribed to a *different generation* of one of this node's topics.
///
/// Topics are [`IdentTopic`]s, so the subscription event carries the peer's
/// literal topic string — a renamed generation is directly readable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicVersionSkew {
    /// This node's generation of the topic.
    pub local_topic: &'static str,
    /// The generation the peer subscribed to.
    pub remote_topic: String,
}

impl TopicVersionSkew {
    /// The operator-facing warning: names both topic generations and states
    /// that gossip mismatch is starvation, not refusal.
    pub fn operator_report(&self, peer: &impl std::fmt::Display) -> String {
        format!(
            "wire-format version skew with peer {peer}: it subscribed to gossip topic \
             {remote}, a different generation of this node's {local} (wire format \
             {local_version}). Gossipsub is not a negotiation: the peer stays connected and \
             simply shares no topic with us, so this traffic is silently absent between the \
             two nodes. Upgrade one side, or ship a dual-version release (#2884)",
            remote = self.remote_topic,
            local = self.local_topic,
            local_version = WIRE_FORMAT_VERSION,
        )
    }
}

/// Check whether a topic string this node does NOT use is a different
/// generation of one of [`WIRE_TOPICS`] — i.e. it shares a topic's base name
/// (`wrfcoin/blocks`, …) with a different version segment. Returns `None`
/// for our own topics and for unrelated topics.
pub fn detect_topic_version_skew(remote_topic: &str) -> Option<TopicVersionSkew> {
    for local_topic in WIRE_TOPICS {
        if remote_topic == local_topic {
            return None;
        }
        let Some((base, _version)) = local_topic.rsplit_once('/') else {
            continue;
        };
        let Some(remote_rest) = remote_topic.strip_prefix(base) else {
            continue;
        };
        if remote_rest.starts_with('/') {
            return Some(TopicVersionSkew {
                local_topic,
                remote_topic: remote_topic.to_string(),
            });
        }
    }
    None
}

// ---------------------------------------------------------------------------
// TopicManager
// ---------------------------------------------------------------------------

/// Manages the set of gossipsub topics used by WRFCoin.
///
/// Each topic is represented as an [`IdentTopic`] (unhashed topic string) so
/// that human-readable topic names are sent on the wire.
#[derive(Debug, Clone)]
pub struct TopicManager {
    blocks: IdentTopic,
    transactions: IdentTopic,
    weather_data: IdentTopic,
    forecast_tasks: IdentTopic,
    da_challenges: IdentTopic,
    genesis_handshake: IdentTopic,
    finality_votes: IdentTopic,
}

impl Default for TopicManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TopicManager {
    /// Create a new `TopicManager` with the canonical WRFCoin topics.
    pub fn new() -> Self {
        Self {
            blocks: IdentTopic::new(BLOCKS_TOPIC),
            transactions: IdentTopic::new(TRANSACTIONS_TOPIC),
            weather_data: IdentTopic::new(WEATHER_DATA_TOPIC),
            forecast_tasks: IdentTopic::new(FORECAST_TASKS_TOPIC),
            da_challenges: IdentTopic::new(DA_CHALLENGES_TOPIC),
            genesis_handshake: IdentTopic::new(GENESIS_HANDSHAKE_TOPIC),
            finality_votes: IdentTopic::new(FINALITY_VOTES_TOPIC),
        }
    }

    /// The block-announcement topic.
    pub fn blocks(&self) -> &IdentTopic {
        &self.blocks
    }

    /// The transaction-broadcast topic.
    pub fn transactions(&self) -> &IdentTopic {
        &self.transactions
    }

    /// The weather-data topic.
    pub fn weather_data(&self) -> &IdentTopic {
        &self.weather_data
    }

    /// The forecast-tasks topic.
    pub fn forecast_tasks(&self) -> &IdentTopic {
        &self.forecast_tasks
    }

    /// The DA-challenges topic.
    pub fn da_challenges(&self) -> &IdentTopic {
        &self.da_challenges
    }

    /// Return the [`TopicHash`] for blocks.
    pub fn blocks_hash(&self) -> TopicHash {
        self.blocks.hash()
    }

    /// Return the [`TopicHash`] for transactions.
    pub fn transactions_hash(&self) -> TopicHash {
        self.transactions.hash()
    }

    /// Return the [`TopicHash`] for weather data.
    pub fn weather_data_hash(&self) -> TopicHash {
        self.weather_data.hash()
    }

    /// Return the [`TopicHash`] for forecast tasks.
    pub fn forecast_tasks_hash(&self) -> TopicHash {
        self.forecast_tasks.hash()
    }

    /// Return the [`TopicHash`] for DA challenges.
    pub fn da_challenges_hash(&self) -> TopicHash {
        self.da_challenges.hash()
    }

    /// Return the genesis-handshake topic.
    pub fn genesis_handshake(&self) -> &IdentTopic {
        &self.genesis_handshake
    }

    /// Return the [`TopicHash`] for genesis handshake messages.
    pub fn genesis_handshake_hash(&self) -> TopicHash {
        self.genesis_handshake.hash()
    }

    /// The BFT finality-votes topic (#1388).
    pub fn finality_votes(&self) -> &IdentTopic {
        &self.finality_votes
    }

    /// Return the [`TopicHash`] for BFT finality vote/cert messages.
    pub fn finality_votes_hash(&self) -> TopicHash {
        self.finality_votes.hash()
    }

    /// Return all topics as a slice-friendly vec of references.
    pub fn all_topics(&self) -> Vec<&IdentTopic> {
        vec![
            &self.blocks,
            &self.transactions,
            &self.weather_data,
            &self.forecast_tasks,
            &self.da_challenges,
            &self.genesis_handshake,
            &self.finality_votes,
        ]
    }

    /// Map a [`TopicHash`] back to a human-readable topic name, if known.
    pub fn topic_name(&self, hash: &TopicHash) -> Option<&'static str> {
        if *hash == self.blocks.hash() {
            Some(BLOCKS_TOPIC)
        } else if *hash == self.transactions.hash() {
            Some(TRANSACTIONS_TOPIC)
        } else if *hash == self.weather_data.hash() {
            Some(WEATHER_DATA_TOPIC)
        } else if *hash == self.forecast_tasks.hash() {
            Some(FORECAST_TASKS_TOPIC)
        } else if *hash == self.da_challenges.hash() {
            Some(DA_CHALLENGES_TOPIC)
        } else if *hash == self.genesis_handshake.hash() {
            Some(GENESIS_HANDSHAKE_TOPIC)
        } else if *hash == self.finality_votes.hash() {
            Some(FINALITY_VOTES_TOPIC)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// WrfcoinMessage -- the envelope that travels inside gossipsub payloads
// ---------------------------------------------------------------------------

/// Application-level messages exchanged over gossipsub.
///
/// Payloads are kept as opaque byte vectors so the P2P layer does not depend
/// on the concrete `Block` / `Transaction` types from `shared-protocol`
/// beyond what is needed for routing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WrfcoinMessage {
    /// A newly mined / validated block (serialized bytes).
    NewBlock {
        /// The block serialized to bytes by the caller.
        block_bytes: Vec<u8>,
    },

    /// One or more new transactions (each serialized independently).
    NewTransactions {
        /// Each entry is a single serialized transaction.
        tx_bytes: Vec<Vec<u8>>,
    },

    /// Weather observation data from an IoT station.
    WeatherData {
        /// The originating weather station identifier.
        station_id: String,
        /// Raw observation payload.
        data: Vec<u8>,
        /// Unix timestamp (seconds) when the observation was taken.
        timestamp: i64,
        /// Per-message replay nonce bound into the station signature.
        #[serde(default)]
        nonce: Option<String>,
        /// Hex-encoded ed25519 signature over the domain-separated signing message.
        station_signature: String,
        /// Hex-encoded ed25519 public key (32 bytes) of the signing station.
        station_pubkey: String,
    },

    /// A peer's current chain status, used during initial block download.
    ChainStatus {
        /// Block height of the peer's best chain.
        height: u64,
        /// Hash of the tip block.
        tip_hash: [u8; 32],
        /// Merkle root of the account/state trie at this height.
        #[serde(default)]
        state_root: [u8; 32],
    },

    /// Assign a forecast computation task to a specific compute node.
    ForecastTaskAssignment {
        task_id: String,
        node_id: String,
        manifest_bytes: Vec<u8>,
    },

    /// Heartbeat from a compute node reporting its availability.
    ComputeNodeHeartbeat {
        node_id: String,
        capabilities: Vec<String>,
        region: String,
        active_tasks: u32,
        max_concurrent_tasks: u32,
        /// Observation density (observations per km^2 per day) for the node's region.
        /// Used by data-desert gamification to boost rewards in under-observed areas.
        observation_density: Option<f64>,
        /// Number of active weather stations contributing to this heartbeat.
        /// Distinct from `observation_density` which is a normalized [0,1] float.
        #[serde(default)]
        station_count: Option<u32>,
    },

    /// Result of a forecast computation task.
    ForecastTaskResult {
        task_id: String,
        node_id: String,
        receipt_bytes: Vec<u8>,
    },

    /// Weather observation with content-addressed offloading support.
    ///
    /// Small observations (under the inline threshold) carry data inline;
    /// large ones carry a [`ContentRef`] to off-chain storage.
    WeatherDataV2 {
        station_id: String,
        /// Inline observation payload. Empty when data is offloaded.
        data: Vec<u8>,
        timestamp: i64,
        /// Content reference for offloaded data. `None` when data is inline.
        content_ref: Option<ContentRef>,
        /// Per-message replay nonce bound into the station signature.
        ///
        /// Required (fail-closed): unlike the original unsigned V2 format, a
        /// V2 message now carries the same domain-separated station signature
        /// as V1 (#1969). Empty/garbage nonces are rejected at admission.
        nonce: String,
        /// Hex-encoded ed25519 signature over the domain-separated signing
        /// message, exactly as for [`WrfcoinMessage::WeatherData`].
        station_signature: String,
        /// Hex-encoded ed25519 public key (32 bytes) of the signing station.
        station_pubkey: String,
    },

    /// Forecast task result with content-addressed offloading support.
    ForecastTaskResultV2 {
        task_id: String,
        node_id: String,
        /// Inline receipt bytes. Empty when offloaded.
        receipt_bytes: Vec<u8>,
        /// Content reference for offloaded receipt data.
        content_ref: Option<ContentRef>,
    },

    /// A Data Availability challenge broadcast to keeper nodes.
    ///
    /// Challengers broadcast this on the `wrfcoin/da-challenges/1` topic to
    /// ask any keeper holding `cid` to prove possession of the specified byte
    /// range. See [`crate::da_challenge`] for the full protocol.
    DaChallenge {
        challenge: DataAvailabilityChallenge,
    },

    /// A keeper's response to a [`WrfcoinMessage::DaChallenge`].
    DaChallengeResponse {
        response: DataAvailabilityChallengeResponse,
    },

    /// Genesis handshake broadcast by each node immediately after connecting.
    ///
    /// Peers that receive a [`GenesisHandshake`] with a mismatched
    /// `network_id`, `chain_id`, `genesis_hash`, or `protocol_version` MUST
    /// disconnect the sender.
    GenesisHandshake {
        /// Network identifier (e.g., "mainnet", "testnet").
        network_id: String,
        /// Chain identifier (e.g., "wrfcoin-main").
        chain_id: String,
        /// SHA-256 hash of the genesis block (hex-encoded, 64 hex chars).
        genesis_hash: String,
        /// Active signing protocol version (#2123).
        ///
        /// Identity alone does not imply interoperability: a pre-#2123 binary
        /// loads a V2 genesis, ignores the `protocol_version` metadata key it
        /// does not understand, and advertises identical identity while signing
        /// V1. Carrying the active version here is what makes that detectable
        /// instead of a silent halt (#2076).
        protocol_version: u32,
    },

    /// A single BFT finality vote from a validator (#1388).
    ///
    /// `vote_bytes` is the bincode-serialized
    /// `consensus-engines::selection::bft_finality::FinalityVote`. The p2p
    /// crate intentionally treats it as an opaque blob so that consensus
    /// types are not pulled into the networking layer.
    FinalityVote {
        /// Bincode-serialized `FinalityVote`.
        vote_bytes: Vec<u8>,
    },

    /// A BFT finality certificate (2/3+1 vote-set) for a finalized block (#1388).
    ///
    /// `cert_bytes` is the bincode-serialized
    /// `consensus-engines::selection::bft_finality::FinalityCertificate`.
    /// Gossiped so peers that missed individual votes can still adopt the
    /// finality decision and arm their reorg guard.
    FinalityCertificate {
        /// Bincode-serialized `FinalityCertificate`.
        cert_bytes: Vec<u8>,
    },
}

// ---------------------------------------------------------------------------
// Content-addressed storage reference
// ---------------------------------------------------------------------------

/// Valid storage provider names, matching `VALID_STORAGE_PROVIDERS` in
/// `shared-protocol/src/blockchain/transaction.rs`.
const VALID_STORAGE_PROVIDERS: &[&str] = &["ipfs", "filecoin", "arweave"];
const MAX_DATA_CID_LEN: usize = 256;

/// Content-addressed reference for data that has been offloaded to
/// external storage. Mirrors the CID pattern from `RevealPayload`
/// in `shared-protocol`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContentRef {
    /// CID reference (e.g. "ipfs:QmXyz...", "arweave:abc123...")
    pub data_cid: String,
    /// SHA-256 hex digest (64 lowercase hex chars) of the original payload.
    pub data_hash: String,
    /// Storage provider: "ipfs", "filecoin", or "arweave".
    pub storage_provider: String,
    /// Size in bytes of the original payload (informational, for fetch planning).
    pub data_size: u64,
}

impl ContentRef {
    /// Validate CID, hash, and provider format.
    pub fn validate(&self) -> Result<(), String> {
        if self.data_cid.is_empty() {
            return Err("data_cid must not be empty".into());
        }
        if self.data_cid.len() > MAX_DATA_CID_LEN {
            return Err(format!(
                "data_cid length {} exceeds maximum {MAX_DATA_CID_LEN}",
                self.data_cid.len()
            ));
        }
        if self.data_cid.trim() != self.data_cid {
            return Err("data_cid must not have surrounding whitespace".into());
        }
        if self.data_hash.len() != 64 || !self.data_hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err("data_hash must be a 64-character hex string".into());
        }
        if !VALID_STORAGE_PROVIDERS.contains(&self.storage_provider.as_str()) {
            return Err(format!(
                "storage_provider '{}' not in {VALID_STORAGE_PROVIDERS:?}",
                self.storage_provider
            ));
        }
        Ok(())
    }
}

/// Hard cap on the total wire-message size accepted by [`WrfcoinMessage::from_bytes`].
///
/// Matches the `max_transmit_size` default in [`crate::config::P2pConfig`] (1 MiB).
/// Defense-in-depth: libp2p gossipsub rejects messages exceeding `max_transmit_size`
/// before they reach our code, but an explicit deserialization limit ensures that
/// a misconfigured or future transport path can never exhaust allocator memory by
/// replaying a crafted oversized blob.
const MAX_MESSAGE_BYTES: u64 = 1_048_576;

/// Which wire variant a message is, for limit lookup (#2115).
///
/// Mirrors [`WrfcoinMessage`] one-for-one. Kept as a separate enum so [`limit_for`] can be
/// an exhaustive `match` with **no wildcard arm**: adding a variant to `WrfcoinMessage`
/// without adding one here fails to compile, and adding one here without a limit fails to
/// compile too. That is the "every variant has a limit" acceptance criterion enforced by
/// the type system rather than by review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    NewBlock,
    NewTransactions,
    WeatherData,
    ChainStatus,
    ForecastTaskAssignment,
    ComputeNodeHeartbeat,
    ForecastTaskResult,
    WeatherDataV2,
    ForecastTaskResultV2,
    DaChallenge,
    DaChallengeResponse,
    GenesisHandshake,
    FinalityVote,
    FinalityCertificate,
}

impl MessageKind {
    /// Number of variants. Used to size the per-variant counters in the
    /// inbound rate window.
    pub const COUNT: usize = 14;

    /// Dense index for this variant.
    ///
    /// NO wildcard arm: a new variant fails to compile here, the same
    /// discipline `limit_for` uses. Paired with `COUNT` via
    /// `every_variant_has_a_distinct_index`, so adding a variant without
    /// bumping `COUNT` fails a test rather than silently aliasing two variants
    /// onto one counter.
    pub fn as_index(self) -> usize {
        match self {
            MessageKind::NewBlock => 0,
            MessageKind::NewTransactions => 1,
            MessageKind::WeatherData => 2,
            MessageKind::ChainStatus => 3,
            MessageKind::ForecastTaskAssignment => 4,
            MessageKind::ComputeNodeHeartbeat => 5,
            MessageKind::ForecastTaskResult => 6,
            MessageKind::WeatherDataV2 => 7,
            MessageKind::ForecastTaskResultV2 => 8,
            MessageKind::DaChallenge => 9,
            MessageKind::DaChallengeResponse => 10,
            MessageKind::GenesisHandshake => 11,
            MessageKind::FinalityVote => 12,
            MessageKind::FinalityCertificate => 13,
        }
    }

    /// Every variant, for exhaustive iteration in tests.
    pub const ALL: [MessageKind; Self::COUNT] = [
        MessageKind::NewBlock,
        MessageKind::NewTransactions,
        MessageKind::WeatherData,
        MessageKind::ChainStatus,
        MessageKind::ForecastTaskAssignment,
        MessageKind::ComputeNodeHeartbeat,
        MessageKind::ForecastTaskResult,
        MessageKind::WeatherDataV2,
        MessageKind::ForecastTaskResultV2,
        MessageKind::DaChallenge,
        MessageKind::DaChallengeResponse,
        MessageKind::GenesisHandshake,
        MessageKind::FinalityVote,
        MessageKind::FinalityCertificate,
    ];
}

/// Per-variant ceiling (#2115).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageLimit {
    pub max_bytes: usize,
    /// Maximum messages of this variant accepted from one peer per inbound rate
    /// window (#2115, rate half).
    ///
    /// `0` disables the rate ceiling for the variant.
    ///
    /// This field is added *together with* its enforcement, deliberately. The
    /// size half of #2115 left it out rather than shipping an unenforced field,
    /// because a limit nobody reads looks exactly like a guard while doing
    /// nothing — which is the failure mode the whole epic kept turning up.
    pub max_per_window: u32,
}

/// Ceiling for one variant.
///
/// # Sizing, and why nothing honest is affected
///
/// Before this, ONE global cap ([`MAX_MESSAGE_BYTES`], 1 MiB) covered every variant: a
/// `ChainStatus` — a handful of scalars — was allowed exactly as many bytes as a full
/// block. The cap did not discriminate by what was actually sent, the same shape as the
/// #2119 block-sync defect.
///
/// The block and transaction variants keep the full 1 MiB, so the paths that carry real
/// payloads are **unchanged**. Everything else gets a ceiling that is still orders of
/// magnitude above any honest instance — 64 KiB for control messages that are tens of
/// bytes, 256 KiB for the ones that can legitimately carry a payload. A limit only binds
/// on a peer sending, say, a 1 MiB heartbeat, which is not a shape honest traffic has.
///
/// `message_round_trips_within_its_own_limit` asserts a representative instance of every
/// variant is still accepted, so an over-tightening here fails a test rather than silently
/// partitioning the network.
pub fn limit_for(kind: MessageKind) -> MessageLimit {
    // NO wildcard arm. See MessageKind's docs.
    let max_bytes = match kind {
        MessageKind::NewBlock => MAX_MESSAGE_BYTES as usize,
        MessageKind::NewTransactions => MAX_MESSAGE_BYTES as usize,
        MessageKind::WeatherData => 262_144,
        MessageKind::ChainStatus => 65_536,
        MessageKind::ForecastTaskAssignment => 65_536,
        MessageKind::ComputeNodeHeartbeat => 65_536,
        MessageKind::ForecastTaskResult => 262_144,
        MessageKind::WeatherDataV2 => 262_144,
        MessageKind::ForecastTaskResultV2 => 262_144,
        MessageKind::DaChallenge => 65_536,
        MessageKind::DaChallengeResponse => 262_144,
        MessageKind::GenesisHandshake => 65_536,
        MessageKind::FinalityVote => 65_536,
        MessageKind::FinalityCertificate => 262_144,
    };

    // Per-peer arrival rate per window. Sized generously against plausible
    // legitimate traffic on a ~12-node network, because an over-rate peer is
    // *penalised*, not merely dropped — a ceiling that clips honest traffic
    // would score honest peers down.
    //
    // Reference points: gossip carries one `NewBlock` per new block (bulk
    // history moves over the `block_sync` request/response protocol, not
    // here), one `GenesisHandshake` per connection, and one `FinalityVote` per
    // validator per height. Each ceiling below is one to two orders of
    // magnitude above that.
    let max_per_window = match kind {
        MessageKind::NewBlock => 64,
        MessageKind::NewTransactions => 256,
        MessageKind::WeatherData => 128,
        MessageKind::ChainStatus => 64,
        MessageKind::ForecastTaskAssignment => 64,
        MessageKind::ComputeNodeHeartbeat => 32,
        MessageKind::ForecastTaskResult => 64,
        MessageKind::WeatherDataV2 => 128,
        MessageKind::ForecastTaskResultV2 => 64,
        MessageKind::DaChallenge => 64,
        MessageKind::DaChallengeResponse => 64,
        // Published once per connection establishment (#2544), so a peer
        // sending these repeatedly is doing something other than handshaking.
        MessageKind::GenesisHandshake => 16,
        MessageKind::FinalityVote => 128,
        MessageKind::FinalityCertificate => 64,
    };

    MessageLimit {
        max_bytes,
        max_per_window,
    }
}

impl WrfcoinMessage {
    /// This message's variant, for limit lookup.
    pub fn kind(&self) -> MessageKind {
        // NO wildcard arm, deliberately — see MessageKind.
        match self {
            WrfcoinMessage::NewBlock { .. } => MessageKind::NewBlock,
            WrfcoinMessage::NewTransactions { .. } => MessageKind::NewTransactions,
            WrfcoinMessage::WeatherData { .. } => MessageKind::WeatherData,
            WrfcoinMessage::ChainStatus { .. } => MessageKind::ChainStatus,
            WrfcoinMessage::ForecastTaskAssignment { .. } => MessageKind::ForecastTaskAssignment,
            WrfcoinMessage::ComputeNodeHeartbeat { .. } => MessageKind::ComputeNodeHeartbeat,
            WrfcoinMessage::ForecastTaskResult { .. } => MessageKind::ForecastTaskResult,
            WrfcoinMessage::WeatherDataV2 { .. } => MessageKind::WeatherDataV2,
            WrfcoinMessage::ForecastTaskResultV2 { .. } => MessageKind::ForecastTaskResultV2,
            WrfcoinMessage::DaChallenge { .. } => MessageKind::DaChallenge,
            WrfcoinMessage::DaChallengeResponse { .. } => MessageKind::DaChallengeResponse,
            WrfcoinMessage::GenesisHandshake { .. } => MessageKind::GenesisHandshake,
            WrfcoinMessage::FinalityVote { .. } => MessageKind::FinalityVote,
            WrfcoinMessage::FinalityCertificate { .. } => MessageKind::FinalityCertificate,
        }
    }
}

/// Read the variant discriminant from wire bytes **without deserializing** (#2115).
///
/// `bincode::serialize` uses `FixintEncoding`, so an enum is encoded as a little-endian
/// `u32` discriminant followed by the payload. Reading four bytes is enough to select the
/// variant's ceiling, so an oversized message is rejected after inspecting a fixed 4 bytes
/// and allocating nothing.
///
/// Returns `None` for a truncated or unknown discriminant. `None` is NOT "allow": callers
/// fall back to the global cap and let bincode reject the malformed input, which it will —
/// but no unknown discriminant ever earns a *larger* allowance than the global cap.
pub fn kind_from_wire(bytes: &[u8]) -> Option<MessageKind> {
    let raw = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?);
    Some(match raw {
        0 => MessageKind::NewBlock,
        1 => MessageKind::NewTransactions,
        2 => MessageKind::WeatherData,
        3 => MessageKind::ChainStatus,
        4 => MessageKind::ForecastTaskAssignment,
        5 => MessageKind::ComputeNodeHeartbeat,
        6 => MessageKind::ForecastTaskResult,
        7 => MessageKind::WeatherDataV2,
        8 => MessageKind::ForecastTaskResultV2,
        9 => MessageKind::DaChallenge,
        10 => MessageKind::DaChallengeResponse,
        11 => MessageKind::GenesisHandshake,
        12 => MessageKind::FinalityVote,
        13 => MessageKind::FinalityCertificate,
        _ => return None,
    })
}

impl WrfcoinMessage {
    /// Serialize to bincode bytes for transmission on the wire.
    pub fn to_bytes(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    /// Deserialize from bincode bytes received on the wire.
    ///
    /// Enforces a hard [`MAX_MESSAGE_BYTES`] (1 MiB) length guard before
    /// handing the slice to bincode, so that a crafted peer message cannot
    /// drive unbounded heap allocation during deserialization.
    ///
    /// Note: we use an upfront length check rather than
    /// `bincode::options().with_limit(…)` because the global
    /// `bincode::serialize` / `bincode::deserialize` pair uses `FixintEncoding`
    /// while `bincode::options()` defaults to `VarintEncoding`, making them
    /// wire-incompatible.  The length check achieves the same security property
    /// with zero encoding risk.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, bincode::Error> {
        if bytes.len() as u64 > MAX_MESSAGE_BYTES {
            return Err(Box::new(bincode::ErrorKind::Custom(format!(
                "message {} bytes exceeds {MAX_MESSAGE_BYTES} byte hard cap",
                bytes.len()
            ))));
        }
        // #2115: then the PER-VARIANT ceiling, still before deserialization. The
        // discriminant is the first four wire bytes, so this costs a fixed 4-byte read and
        // allocates nothing. An unknown/truncated discriminant yields None and keeps the
        // global cap — it never earns a larger allowance.
        if let Some(kind) = kind_from_wire(bytes) {
            let limit = limit_for(kind);
            if bytes.len() > limit.max_bytes {
                return Err(Box::new(bincode::ErrorKind::Custom(format!(
                    "{kind:?} message {} bytes exceeds its {} byte ceiling (#2115)",
                    bytes.len(),
                    limit.max_bytes
                ))));
            }
        }
        bincode::deserialize(bytes)
    }

    /// Determine which gossipsub topic this message belongs to.
    pub fn topic(&self, tm: &TopicManager) -> TopicHash {
        match self {
            WrfcoinMessage::NewBlock { .. } | WrfcoinMessage::ChainStatus { .. } => {
                tm.blocks_hash()
            }
            WrfcoinMessage::NewTransactions { .. } => tm.transactions_hash(),
            WrfcoinMessage::WeatherData { .. }
            | WrfcoinMessage::WeatherDataV2 { .. } => tm.weather_data_hash(),
            WrfcoinMessage::ForecastTaskAssignment { .. }
            | WrfcoinMessage::ComputeNodeHeartbeat { .. }
            | WrfcoinMessage::ForecastTaskResult { .. }
            | WrfcoinMessage::ForecastTaskResultV2 { .. } => tm.forecast_tasks_hash(),
            WrfcoinMessage::DaChallenge { .. }
            | WrfcoinMessage::DaChallengeResponse { .. } => tm.da_challenges_hash(),
            WrfcoinMessage::GenesisHandshake { .. } => tm.genesis_handshake_hash(),
            WrfcoinMessage::FinalityVote { .. }
            | WrfcoinMessage::FinalityCertificate { .. } => tm.finality_votes_hash(),
        }
    }

    /// Check that inline data does not exceed the given threshold.
    ///
    /// Returns `Ok(())` for messages that do not carry variable-size data
    /// payloads, or for messages that use CID references. Returns `Err` if
    /// the message carries inline data exceeding `inline_data_max` bytes.
    pub fn validate_inline_size(&self, inline_data_max: usize) -> Result<(), String> {
        match self {
            WrfcoinMessage::WeatherData { data, station_id, .. } => {
                if data.len() > inline_data_max {
                    return Err(format!(
                        "WeatherData from station '{station_id}': inline data {} bytes exceeds \
                         {inline_data_max} byte threshold -- use WeatherDataV2 with \
                         content_ref for large payloads",
                        data.len()
                    ));
                }
            }
            WrfcoinMessage::WeatherDataV2 { data, content_ref, station_id, .. } => {
                if content_ref.is_none() && data.len() > inline_data_max {
                    return Err(format!(
                        "WeatherDataV2 from station '{}': inline data {} bytes exceeds \
                         {inline_data_max} byte threshold -- provide content_ref",
                        station_id,
                        data.len()
                    ));
                }
                if let Some(cr) = content_ref {
                    cr.validate()?;
                }
            }
            WrfcoinMessage::ForecastTaskResult { receipt_bytes, task_id, .. } => {
                if receipt_bytes.len() > inline_data_max {
                    return Err(format!(
                        "ForecastTaskResult for task '{}': receipt_bytes {} bytes exceeds \
                         {inline_data_max} byte threshold -- use ForecastTaskResultV2 with \
                         content_ref for large payloads",
                        task_id,
                        receipt_bytes.len()
                    ));
                }
            }
            WrfcoinMessage::ForecastTaskResultV2 { receipt_bytes, content_ref, task_id, .. } => {
                if content_ref.is_none() && receipt_bytes.len() > inline_data_max {
                    return Err(format!(
                        "ForecastTaskResultV2 for task '{}': receipt_bytes {} bytes exceeds \
                         {inline_data_max} byte threshold -- provide content_ref",
                        task_id,
                        receipt_bytes.len()
                    ));
                }
                if let Some(cr) = content_ref {
                    cr.validate()?;
                }
            }
            // Opaque-blob consensus and chain messages: block bytes, raw transactions,
            // and BFT vote/certificate payloads are all bounded by `inline_data_max`.
            // Finality votes and certificates are small cryptographic structures (a few
            // hundred bytes in practice); bounding them here prevents a misbehaving peer
            // from gossiping synthetic oversized blobs before the outer 1 MiB hard cap
            // in `from_bytes` would fire.
            WrfcoinMessage::NewBlock { block_bytes } => {
                if block_bytes.len() > inline_data_max {
                    return Err(format!(
                        "NewBlock: block_bytes {} bytes exceeds {inline_data_max} byte threshold",
                        block_bytes.len()
                    ));
                }
            }
            WrfcoinMessage::NewTransactions { tx_bytes } => {
                let total: usize = tx_bytes.iter().map(|t| t.len()).sum();
                if total > inline_data_max {
                    return Err(format!(
                        "NewTransactions: total tx payload {total} bytes exceeds \
                         {inline_data_max} byte threshold",
                    ));
                }
            }
            WrfcoinMessage::FinalityVote { vote_bytes } => {
                if vote_bytes.len() > inline_data_max {
                    return Err(format!(
                        "FinalityVote: vote_bytes {} bytes exceeds {inline_data_max} byte threshold",
                        vote_bytes.len()
                    ));
                }
            }
            WrfcoinMessage::FinalityCertificate { cert_bytes } => {
                if cert_bytes.len() > inline_data_max {
                    return Err(format!(
                        "FinalityCertificate: cert_bytes {} bytes exceeds \
                         {inline_data_max} byte threshold",
                        cert_bytes.len()
                    ));
                }
            }
            // DA challenge messages are bounded by CHALLENGE_FRAGMENT_MAX_BYTES (4 KiB),
            // well under the inline threshold in all practical configurations.
            WrfcoinMessage::DaChallenge { .. } | WrfcoinMessage::DaChallengeResponse { .. } => {}
            _ => {}
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Gossipsub message-ID function
// ---------------------------------------------------------------------------

/// Produces a deterministic [`gossipsub::MessageId`].
///
/// For propagated content (blocks, transactions, weather data, votes) the ID is
/// a blake3 hash of the raw payload, so the same logical message relayed by
/// several peers is recognised as one message however the metadata differs.
///
/// # Why the genesis handshake is excluded (#2544)
///
/// The handshake is an *identity announcement*, not propagated content, and
/// content-addressing it is not merely suboptimal — it silently disables it:
///
/// - Every node on one chain publishes **byte-identical** handshake bytes. Under
///   a content-derived ID they all share one `MessageId`, so the first one a
///   node sees or publishes poisons its 60 s `duplicate_cache` against every
///   other node's handshake. Two honest peers then never verify each other.
/// - A node must legitimately re-send *its own* handshake — that is the whole
///   of #2544, sending again once a peer has actually subscribed. `publish`
///   refuses an ID already in the duplicate cache with `PublishError::Duplicate`
///   (libp2p-gossipsub `behaviour.rs`), so under a content-derived ID the
///   republish never reaches the wire. It increments a counter and returns.
///
/// So handshakes are keyed on `(source, sequence_number)` instead — which is
/// exactly gossipsub's own default message-ID function, applied to messages
/// whose meaning is per-sender state rather than shared content. Each node's
/// handshake and ChainStatus is distinct from every other node's, and each send
/// is distinct from the same node's previous send.
///
/// Unsigned handshakes (no `source`/`sequence_number`) fall back to the content
/// hash. `MessageAuthenticity::Signed` means production never takes that path;
/// it exists so this stays a total function rather than panicking.
pub fn message_id_fn(message: &gossipsub::Message) -> gossipsub::MessageId {
    let sender_scoped = message.topic.as_str() == GENESIS_HANDSHAKE_TOPIC
        || kind_from_wire(&message.data) == Some(MessageKind::ChainStatus);
    if sender_scoped {
        if let (Some(source), Some(sequence_number)) = (message.source, message.sequence_number) {
            let mut hasher = blake3::Hasher::new();
            hasher.update(message.topic.as_str().as_bytes());
            hasher.update(&source.to_bytes());
            hasher.update(&sequence_number.to_be_bytes());
            return gossipsub::MessageId::from(hasher.finalize().as_bytes().to_vec());
        }
    }

    let hash = blake3::hash(&message.data);
    gossipsub::MessageId::from(hash.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- #2544 message-ID contract ----------------------------------------------------

    fn raw(topic: &str, data: &[u8], source: Option<u8>, seqno: Option<u64>) -> gossipsub::Message {
        // A deterministic peer id per `source` byte, so the cases below differ
        // in exactly the field under test and nothing else.
        let peer = source.map(|b| {
            let key = libp2p::identity::Keypair::ed25519_from_bytes([b; 32])
                .expect("32 bytes is a valid ed25519 secret");
            libp2p::PeerId::from(key.public())
        });
        gossipsub::Message {
            source: peer,
            data: data.to_vec(),
            sequence_number: seqno,
            topic: IdentTopic::new(topic).hash(),
        }
    }

    /// Two nodes on the same chain publish byte-identical handshakes. If those
    /// share a `MessageId`, the first one a node sees or publishes poisons its
    /// 60 s duplicate cache against every other node's — so two honest peers
    /// never verify each other's chain identity (#2544).
    #[test]
    fn two_nodes_identical_handshakes_get_distinct_message_ids() {
        let payload = b"identical handshake bytes";
        let from_a = message_id_fn(&raw(GENESIS_HANDSHAKE_TOPIC, payload, Some(1), Some(7)));
        let from_b = message_id_fn(&raw(GENESIS_HANDSHAKE_TOPIC, payload, Some(2), Some(7)));

        assert_ne!(
            from_a, from_b,
            "identical handshake bytes from different peers must be different \
             messages — they are two peers announcing themselves, not one \
             message relayed twice"
        );
    }

    /// A node must be able to send its own handshake repeatedly: once at
    /// connection establishment, once per peer that subscribes afterwards.
    /// Equal IDs make every send after the first a `PublishError::Duplicate`.
    #[test]
    fn one_node_repeating_its_handshake_gets_distinct_message_ids() {
        let payload = b"identical handshake bytes";
        let first = message_id_fn(&raw(GENESIS_HANDSHAKE_TOPIC, payload, Some(1), Some(7)));
        let second = message_id_fn(&raw(GENESIS_HANDSHAKE_TOPIC, payload, Some(1), Some(8)));

        assert_ne!(
            first, second,
            "a node re-sending its handshake must produce a new message, or \
             gossipsub refuses the send for the whole duplicate-cache window"
        );
        assert_eq!(
            message_id_fn(&raw(GENESIS_HANDSHAKE_TOPIC, payload, Some(1), Some(7))),
            first,
            "the ID must still be a deterministic function of the message — \
             one send is one message, seen the same way by every node"
        );
    }

    /// The exemption is scoped to the handshake topic. Everything else stays
    /// content-addressed: propagation dedup is what stops a block or a
    /// transaction from being re-processed once per relaying peer, and keying
    /// those on the sender would defeat it.
    #[test]
    fn propagated_topics_stay_content_addressed() {
        let payload = b"one block, relayed by two peers";
        for topic in [
            BLOCKS_TOPIC,
            TRANSACTIONS_TOPIC,
            WEATHER_DATA_TOPIC,
            FORECAST_TASKS_TOPIC,
            DA_CHALLENGES_TOPIC,
            FINALITY_VOTES_TOPIC,
        ] {
            assert_eq!(
                message_id_fn(&raw(topic, payload, Some(1), Some(7))),
                message_id_fn(&raw(topic, payload, Some(2), Some(9))),
                "{topic}: the same content from two relays must be one message"
            );
        }
    }

    #[test]
    fn identical_chain_status_from_distinct_peers_remains_peer_scoped_2816() {
        let status = WrfcoinMessage::ChainStatus {
            height: 30_248,
            tip_hash: [0xa1; 32],
            state_root: [0xb2; 32],
        }
        .to_bytes()
        .expect("serialize chain status");
        let from_a = message_id_fn(&raw(BLOCKS_TOPIC, &status, Some(1), Some(7)));
        let from_b = message_id_fn(&raw(BLOCKS_TOPIC, &status, Some(2), Some(7)));
        assert_ne!(
            from_a, from_b,
            "identical heights from two peers must create two scheduler candidates"
        );

        let block = WrfcoinMessage::NewBlock {
            block_bytes: b"same-block".to_vec(),
        }
        .to_bytes()
        .expect("serialize block");
        assert_eq!(
            message_id_fn(&raw(BLOCKS_TOPIC, &block, Some(1), Some(8))),
            message_id_fn(&raw(BLOCKS_TOPIC, &block, Some(2), Some(9))),
            "ordinary propagated block content must remain globally deduplicated"
        );
    }

    /// An unsigned handshake carries neither source nor sequence number.
    /// Production signs every message, so this is only about staying total.
    #[test]
    fn an_unsigned_handshake_falls_back_to_the_content_hash() {
        let payload = b"unsigned handshake";
        assert_eq!(
            message_id_fn(&raw(GENESIS_HANDSHAKE_TOPIC, payload, None, None)),
            message_id_fn(&raw(BLOCKS_TOPIC, payload, None, None)),
            "with no author to key on, the content hash is all there is"
        );
    }

    // --- #2115 per-variant ceilings ---------------------------------------------------

    /// One representative instance of every variant, used to prove both that the wire
    /// discriminant mapping is right and that no honest message is rejected.
    fn one_of_each() -> Vec<WrfcoinMessage> {
        use crate::da_challenge::{
            DataAvailabilityChallenge, DataAvailabilityChallengeResponse,
        };
        // EVERY variant, in declaration order. Partial coverage here is worse than none:
        // a fixture set that stops at index 3 cannot catch a discriminant shift at 7-9,
        // which is exactly the bug this suite exists to catch (and exactly the one I made).
        vec![
            WrfcoinMessage::NewBlock { block_bytes: vec![1, 2, 3] },
            WrfcoinMessage::NewTransactions { tx_bytes: vec![vec![1], vec![2]] },
            WrfcoinMessage::WeatherData {
                station_id: "s".into(), data: vec![7], timestamp: 1,
                nonce: None, station_signature: "ab".into(), station_pubkey: "cd".into(),
            },
            WrfcoinMessage::ChainStatus { height: 1, tip_hash: [7u8; 32], state_root: [9u8; 32] },
            WrfcoinMessage::ForecastTaskAssignment {
                task_id: "t".into(), node_id: "n".into(), manifest_bytes: vec![1],
            },
            WrfcoinMessage::ComputeNodeHeartbeat {
                node_id: "n".into(), capabilities: vec!["c".into()], region: "r".into(),
                active_tasks: 1, max_concurrent_tasks: 2,
                observation_density: None, station_count: None,
            },
            WrfcoinMessage::ForecastTaskResult {
                task_id: "t".into(), node_id: "n".into(), receipt_bytes: vec![1],
            },
            WrfcoinMessage::WeatherDataV2 {
                station_id: "s".into(), data: vec![7], timestamp: 1, content_ref: None,
                nonce: "n".into(), station_signature: "ab".into(), station_pubkey: "cd".into(),
            },
            WrfcoinMessage::ForecastTaskResultV2 {
                task_id: "t".into(), node_id: "n".into(), receipt_bytes: vec![1],
                content_ref: None,
            },
            WrfcoinMessage::DaChallenge {
                challenge: DataAvailabilityChallenge {
                    challenge_id: "c".into(), cid: "cid".into(), nonce: [1u8; 32],
                    offset: 0, length: 4, issued_at: 1,
                },
            },
            WrfcoinMessage::DaChallengeResponse {
                response: DataAvailabilityChallengeResponse {
                    challenge_id: "c".into(), data_fragment: vec![1, 2],
                    proof: [2u8; 32], keeper_id: "k".into(), responded_at: 1,
                },
            },
            WrfcoinMessage::GenesisHandshake {
                network_id: "testnet".into(), chain_id: "777".into(),
                genesis_hash: "0xabc".into(), protocol_version: 2,
            },
            WrfcoinMessage::FinalityVote { vote_bytes: vec![1] },
            WrfcoinMessage::FinalityCertificate { cert_bytes: vec![1] },
        ]
    }

    /// The fixture set must cover EVERY variant, or the discriminant test below is vacuous
    /// for whatever it omits.
    #[test]
    fn fixture_covers_every_variant() {
        let kinds: std::collections::BTreeSet<_> =
            one_of_each().iter().map(|m| format!("{:?}", m.kind())).collect();
        assert_eq!(
            kinds.len(),
            14,
            "one_of_each must contain all 14 variants; got {kinds:?}"
        );
    }

    /// THE test that matters: the discriminant read from the wire must agree with the
    /// variant's own `kind()`. A mismatch silently applies another variant's ceiling.
    ///
    /// I got this wrong on the first attempt — `WeatherDataV2` and `ForecastTaskResultV2`
    /// sit at indices 7 and 8, shifting everything after them, so `DaChallenge` is 9 and
    /// not 7. The compiler's exhaustiveness check caught the missing variants; only this
    /// test catches a wrong ORDER, because both sides still compile.
    #[test]
    fn wire_discriminant_agrees_with_kind() {
        for msg in one_of_each() {
            let bytes = msg.to_bytes().expect("serialize");
            let from_wire = kind_from_wire(&bytes)
                .unwrap_or_else(|| panic!("no discriminant for {:?}", msg.kind()));
            assert_eq!(
                from_wire,
                msg.kind(),
                "wire discriminant disagrees with kind() — a wrong mapping applies another \
                 variant's ceiling"
            );
        }
    }

    /// No honest message is rejected by its own ceiling. Guards against over-tightening,
    /// which would partition the network rather than harden it.
    #[test]
    fn message_round_trips_within_its_own_limit() {
        for msg in one_of_each() {
            let bytes = msg.to_bytes().expect("serialize");
            let limit = limit_for(msg.kind());
            assert!(
                bytes.len() <= limit.max_bytes,
                "{:?} is {} bytes, over its own {} byte ceiling",
                msg.kind(),
                bytes.len(),
                limit.max_bytes
            );
            WrfcoinMessage::from_bytes(&bytes).expect("an honest message must decode");
        }
    }

    /// A control message padded past its ceiling is refused — even though it is well under
    /// the 1 MiB global cap that used to be the only bound.
    #[test]
    fn control_message_over_its_ceiling_is_refused_below_the_global_cap() {
        let limit = limit_for(MessageKind::WeatherData).max_bytes;
        assert!(
            (limit as u64) < MAX_MESSAGE_BYTES,
            "this test is only meaningful while the variant ceiling is below the global cap"
        );
        // ChainStatus is all fixed-size fields, so pad a variable-length variant instead:
        // WeatherData shares the 65_536-or-larger class only if we pick a control variant.
        // DaChallenge is the control-sized variant with a variable field.
        let msg = WrfcoinMessage::WeatherData {
            station_id: "x".repeat(limit + 1024),
            data: Vec::new(),
            timestamp: 0,
            nonce: None,
            station_signature: String::new(),
            station_pubkey: String::new(),
        };
        let bytes = msg.to_bytes().expect("serialize");
        assert!(bytes.len() > limit && (bytes.len() as u64) < MAX_MESSAGE_BYTES);
        let err = WrfcoinMessage::from_bytes(&bytes).expect_err("must be refused by #2115");
        assert!(format!("{err}").contains("ceiling"), "got {err}");
    }

    /// An unknown discriminant must not earn a LARGER allowance than the global cap.
    #[test]
    fn unknown_discriminant_does_not_widen_the_bound() {
        let mut bytes = vec![0u8; 16];
        bytes[..4].copy_from_slice(&9999u32.to_le_bytes());
        assert!(kind_from_wire(&bytes).is_none(), "unknown discriminant must be None");
        // Still rejected — by bincode, not by a widened ceiling.
        assert!(WrfcoinMessage::from_bytes(&bytes).is_err());
    }

    #[test]
    fn truncated_prefix_is_not_a_kind() {
        assert!(kind_from_wire(&[0u8; 3]).is_none(), "fewer than 4 bytes has no discriminant");
        assert!(kind_from_wire(&[]).is_none());
    }

    #[test]
    fn topic_manager_names_roundtrip() {
        let tm = TopicManager::new();
        assert_eq!(tm.topic_name(&tm.blocks_hash()), Some(BLOCKS_TOPIC));
        assert_eq!(
            tm.topic_name(&tm.transactions_hash()),
            Some(TRANSACTIONS_TOPIC)
        );
        assert_eq!(
            tm.topic_name(&tm.weather_data_hash()),
            Some(WEATHER_DATA_TOPIC)
        );
    }

    #[test]
    fn unknown_topic_returns_none() {
        let tm = TopicManager::new();
        let fake = TopicHash::from_raw("nonexistent");
        assert!(tm.topic_name(&fake).is_none());
    }

    #[test]
    fn message_serialization_roundtrip() {
        let msg = WrfcoinMessage::NewBlock {
            block_bytes: vec![1, 2, 3, 4],
        };
        let bytes = msg.to_bytes().unwrap();
        let decoded = WrfcoinMessage::from_bytes(&bytes).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn forecast_task_messages_roundtrip() {
        let messages = vec![
            WrfcoinMessage::ForecastTaskAssignment {
                task_id: "task-001".to_string(),
                node_id: "node-abc".to_string(),
                manifest_bytes: vec![10, 20, 30],
            },
            WrfcoinMessage::ComputeNodeHeartbeat {
                node_id: "node-abc".to_string(),
                capabilities: vec!["wrf".to_string(), "gfs".to_string()],
                region: "us-east".to_string(),
                active_tasks: 2,
                max_concurrent_tasks: 4,
                observation_density: Some(0.5),
                station_count: Some(42),
            },
            WrfcoinMessage::ForecastTaskResult {
                task_id: "task-001".to_string(),
                node_id: "node-abc".to_string(),
                receipt_bytes: vec![99, 88, 77],
            },
        ];

        for msg in messages {
            let bytes = msg.to_bytes().unwrap();
            let decoded = WrfcoinMessage::from_bytes(&bytes).unwrap();
            assert_eq!(msg, decoded);
        }
    }

    #[test]
    fn forecast_messages_use_correct_topic() {
        let tm = TopicManager::new();
        let expected = tm.forecast_tasks_hash();

        let assignment = WrfcoinMessage::ForecastTaskAssignment {
            task_id: "t1".into(),
            node_id: "n1".into(),
            manifest_bytes: vec![],
        };
        assert_eq!(assignment.topic(&tm), expected);

        let heartbeat = WrfcoinMessage::ComputeNodeHeartbeat {
            node_id: "n1".into(),
            capabilities: vec![],
            region: "us-east".into(),
            active_tasks: 0,
            max_concurrent_tasks: 4,
            observation_density: None,
            station_count: None,
        };
        assert_eq!(heartbeat.topic(&tm), expected);

        let result = WrfcoinMessage::ForecastTaskResult {
            task_id: "t1".into(),
            node_id: "n1".into(),
            receipt_bytes: vec![],
        };
        assert_eq!(result.topic(&tm), expected);
    }

    #[test]
    fn forecast_tasks_topic_in_all_topics() {
        let tm = TopicManager::new();
        let all = tm.all_topics();
        let hashes: Vec<TopicHash> = all.iter().map(|t| t.hash()).collect();
        assert!(hashes.contains(&tm.forecast_tasks_hash()));
    }

    #[test]
    fn forecast_tasks_topic_name_resolves() {
        let tm = TopicManager::new();
        assert_eq!(
            tm.topic_name(&tm.forecast_tasks_hash()),
            Some(FORECAST_TASKS_TOPIC)
        );
    }

    #[test]
    fn station_count_field_roundtrips_through_serde() {
        let heartbeat = WrfcoinMessage::ComputeNodeHeartbeat {
            node_id: "node-42".to_string(),
            capabilities: vec!["wrf".to_string()],
            region: "us-west".to_string(),
            active_tasks: 1,
            max_concurrent_tasks: 8,
            observation_density: Some(0.25),
            station_count: Some(42),
        };

        let json = serde_json::to_string(&heartbeat).unwrap();
        let decoded: WrfcoinMessage = serde_json::from_str(&json).unwrap();

        match decoded {
            WrfcoinMessage::ComputeNodeHeartbeat { station_count, .. } => {
                assert_eq!(station_count, Some(42));
            }
            _ => panic!("expected ComputeNodeHeartbeat variant"),
        }
    }

    #[test]
    fn station_count_defaults_to_none_on_missing_field() {
        let json = r#"{
            "ComputeNodeHeartbeat": {
                "node_id": "node-42",
                "capabilities": ["wrf"],
                "region": "us-west",
                "active_tasks": 1,
                "max_concurrent_tasks": 8,
                "observation_density": 0.25
            }
        }"#;
        let decoded: WrfcoinMessage = serde_json::from_str(json).unwrap();

        match decoded {
            WrfcoinMessage::ComputeNodeHeartbeat { station_count, .. } => {
                assert_eq!(station_count, None);
            }
            _ => panic!("expected ComputeNodeHeartbeat variant"),
        }
    }
}

#[cfg(test)]
mod variant_rate_tests {
    use super::{limit_for, MessageKind};

    /// Pairs `as_index` with `COUNT`. Adding a variant without bumping `COUNT`
    /// would silently alias two variants onto one rate counter — they would
    /// share an allowance, and a flood of one would throttle the other.
    #[test]
    fn every_variant_has_a_distinct_index_inside_count() {
        let mut seen = vec![false; MessageKind::COUNT];
        for kind in MessageKind::ALL {
            let idx = kind.as_index();
            assert!(
                idx < MessageKind::COUNT,
                "{kind:?} indexes {idx}, outside COUNT={}",
                MessageKind::COUNT
            );
            assert!(!seen[idx], "{kind:?} shares index {idx} with another variant");
            seen[idx] = true;
        }
        assert!(
            seen.iter().all(|s| *s),
            "COUNT={} exceeds the number of variants in ALL",
            MessageKind::COUNT
        );
    }

    #[test]
    fn all_matches_the_variant_count() {
        assert_eq!(MessageKind::ALL.len(), MessageKind::COUNT);
    }

    /// Every variant must carry a rate ceiling. A zero here would disable the
    /// ceiling for that variant, which is a legitimate configuration but never
    /// a legitimate default — it would be an unbounded arrival rate shipped by
    /// omission.
    #[test]
    fn every_variant_ships_a_nonzero_rate_ceiling() {
        for kind in MessageKind::ALL {
            assert!(
                limit_for(kind).max_per_window > 0,
                "{kind:?} ships an unbounded arrival rate"
            );
        }
    }

    /// The handshake is published once per connection (#2544), so its ceiling
    /// should be the tightest. If it ever drifts up to match bulk traffic, the
    /// reasoning behind it has been lost.
    #[test]
    fn the_handshake_ceiling_is_tighter_than_bulk_traffic() {
        let handshake = limit_for(MessageKind::GenesisHandshake).max_per_window;
        let txs = limit_for(MessageKind::NewTransactions).max_per_window;
        assert!(handshake < txs, "handshake={handshake} txs={txs}");
    }
}

/// Guard for the conditional half of #2115's compression-bomb requirement.
///
/// #2115 asks for a decompression ratio cap **"if compression is used on any
/// path"**. It is not: `p2p` pulls no compression crate (verified through the
/// full transitive graph, not just direct dependencies), nothing in the decode
/// path decompresses, and the gossipsub behaviour installs no `DataTransform`
/// — libp2p's designated hook for exactly this — so it runs the identity
/// transform.
///
/// Writing a ratio cap today would therefore produce a guard that can never
/// fire, and a test asserting properties of a code path that does not exist
/// passes no matter what anyone later writes. That is the vacuous-guard shape
/// catalogued in #2549.
///
/// What is worth having is making the *condition* self-enforcing, so "we
/// checked once" becomes "it stays checked": if someone adds compression, this
/// fails and points at the requirement rather than letting it ship
/// unbounded.
#[cfg(test)]
mod compression_guard {
    /// Crates that would introduce a decompressor into this crate.
    const COMPRESSION_CRATES: &[&str] = &[
        "flate2",
        "zstd",
        "snap",
        "lz4",
        "brotli",
        "libflate",
        "miniz_oxide",
        "async-compression",
        "xz2",
        "bzip2",
    ];

    /// If this fails, you are adding compression to the p2p decode path.
    /// #2115 requires a **maximum decompression ratio and an absolute output
    /// size** before that is safe: without both, a small compressed payload
    /// expands into an arbitrarily large allocation, which is a
    /// compression-bomb DoS.
    ///
    /// Implement the cap, then extend this test to assert the cap holds rather
    /// than deleting it.
    ///
    /// **Limit, stated rather than implied:** this reads `p2p/Cargo.toml`, so
    /// it catches a compression crate being added as a direct dependency. It
    /// cannot catch a decompressor reached transitively through another crate's
    /// public API. It is a tripwire on the likely path, not a proof.
    #[test]
    fn adding_compression_requires_a_ratio_cap_first() {
        let manifest = include_str!("../Cargo.toml");

        // Only the dependency sections; a crate named in a comment or in
        // package metadata is not a dependency. This mirrors the lesson from
        // #2414, where a grep matched a name inside a comment recording its
        // removal — a grep cannot tell a definition from an epitaph.
        let deps: String = manifest
            .lines()
            .skip_while(|l| !l.trim_start().starts_with("[dependencies"))
            .filter(|l| !l.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");

        for crate_name in COMPRESSION_CRATES {
            let declared = deps.lines().any(|line| {
                let name = line.split_once('=').map(|(n, _)| n.trim()).unwrap_or("");
                name == *crate_name || name.trim_matches('"') == *crate_name
            });
            assert!(
                !declared,
                "`{crate_name}` was added to p2p/Cargo.toml. #2115 requires a maximum \
                 decompression ratio AND an absolute output size before any compression \
                 reaches the decode path — otherwise a small payload expands into an \
                 arbitrarily large allocation. Implement the cap, then extend this test to \
                 assert it rather than deleting this test."
            );
        }
    }
}
