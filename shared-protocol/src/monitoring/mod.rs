pub mod types;

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{LazyLock, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};

use prometheus::{
    Gauge, Histogram, HistogramOpts, HistogramVec, IntCounter, IntCounterVec, IntGauge, IntGaugeVec, Opts,
    Registry,
};

use crate::security::slashing::SlashingReason;
use crate::{blockchain::block::Block, crypto::hash::Hash};

pub use types::{
    ColorCode, Heatmap, HeatmapMetrics, HeatmapNode, HeatmapSection, Location, NetworkHealthSummary,
    NodeMetricPoint, NodeMetrics, NodeStatus, NodeType, PerformanceScore, Region,
};

/// Topic labels used for P2P bandwidth Prometheus metrics.
pub const P2P_TOPICS: [&str; 4] = ["blocks", "transactions", "weather", "forecast"];

/// Ceiling on live `peer_id` series in `wrfcoin_state_divergence_total` (#2589).
///
/// Derived from the P2P connection ceiling: `max_established_total = 256`, so a
/// healthy node never approaches this and the cap does **not** shape normal
/// operation. The 2x headroom exists so that reaching it is a genuine signal —
/// it means teardown events are being lost, which
/// `wrfcoin_state_divergence_peer_evictions_total` then makes observable.
pub const MAX_STATE_DIVERGENCE_PEER_SERIES: usize = 512;

/// #2734: permanent sentinel series in `wrfcoin_state_divergence_total`.
///
/// A labeled family with zero live series is omitted from the Prometheus text
/// exposition entirely, and per-peer series only exist after a peer reports
/// chain status — so on a freshly restarted, fully healthy node the container
/// healthcheck's `grep wrfcoin_state_divergence_total` found nothing and held
/// the node `unhealthy` while `/readyz` was 200 (measured in #2734; eight
/// services gate on that health). This sentinel is created at registration
/// time, stays zero forever, and never collides with a real peer (libp2p peer
/// ids are multihashes, never `"none"`), so the family is always present and
/// the grep degrades to a genuine "metrics endpoint is serving" probe.
pub const STATE_DIVERGENCE_SENTINEL_PEER: &str = "none";

/// Insertion-ordered registry of peers currently holding a `peer_id` series.
///
/// `order` gives deterministic FIFO eviction; `present` makes the membership
/// test O(1) because `admit_state_divergence_peer` runs on every
/// `PeerChainStatus`, not once per peer.
#[derive(Default)]
struct AdmittedDivergencePeers {
    order: VecDeque<String>,
    present: HashSet<String>,
}

/// Per-topic P2P bandwidth data passed to [`CoreMetrics::apply_p2p_snapshot`].
#[derive(Debug, Clone, Default)]
pub struct P2pBandwidthSnapshot {
    /// Per-topic received bytes: \[blocks, transactions, weather, forecast\].
    pub bytes_received: [u64; 4],
    /// Per-topic sent bytes.
    pub bytes_sent: [u64; 4],
    /// Per-topic received message counts.
    pub messages_received: [u64; 4],
    /// Per-topic sent message counts.
    pub messages_sent: [u64; 4],
}

/// Per-topic gossipsub mesh peer counts passed to [`CoreMetrics::apply_p2p_mesh_snapshot`].
#[derive(Debug, Clone, Default)]
pub struct P2pMeshSnapshot {
    /// Per-topic mesh peers: [blocks, transactions, weather, forecast].
    pub mesh_peers: [u64; 4],
}

/// A locally observed mempool entry immediately before candidate assembly.
/// This is telemetry, not consensus evidence: peers may have seen a different pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderingMempoolEntry {
    pub tx_id: Hash,
    pub fee: u64,
    pub size: usize,
    pub arrival_rank: u64,
}

/// A local pre-assembly view. An empty view means no assessment, not fair ordering.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrderingMempoolSnapshot {
    pub entries: Vec<OrderingMempoolEntry>,
}

#[derive(Debug, Clone, Default)]
pub struct CoreMetricSnapshot {
    pub block_height: u64,
    pub mempool_size: u64,
    pub connected_peers: u64,
    pub transactions_processed_total: u64,
    pub transactions_failed_total: u64,
    pub zk_proofs_observed_total: u64,
    /// Per-reason slashing event counts keyed by [`SlashingReason::as_str()`].
    pub slashing_events_by_reason: HashMap<String, u64>,
    /// 0/1 flag for same-height state-root divergence observed by launch_api.
    pub state_diverged: bool,
    pub validator_participation_rate: f64,
    pub weather_active_sensors_count: u64,
    pub weather_data_quality_score: f64,
    /// Height of the most recently finalized block. The chain layer computes
    /// this as `chain_tip_height.saturating_sub(MAX_REORG_DEPTH)` — any block
    /// deeper than `MAX_REORG_DEPTH` is no longer reorganizable and therefore
    /// considered final under the current consensus rules. Fork/reorg event
    /// counters (`chain_fork_count`, `chain_reorg_total`, etc.) are updated
    /// directly by the reorg engine via `inc_fork_observed`/`inc_reorg` — they
    /// are not carried on the snapshot because they are event-driven, not
    /// state-derived.
    pub chain_finalized_height: u64,
    /// Height at which state pruning last executed (#1202).
    pub state_pruning_last_height: u64,
}

pub struct CoreMetrics {
    pub registry: Registry,
    pub block_height: IntGauge,
    pub chain_height: IntGauge,
    pub mempool_size: IntGauge,
    /// Current transaction-pool size under the launch-readiness metric name
    /// (`wrfcoin_tx_pool_size`, #1620). Mirrors `mempool_size`; both are kept
    /// so the legacy `wrfcoin_blockchain_mempool_size` dashboards and the new
    /// launch-gate alerts can coexist during the testnet rollout.
    pub tx_pool_size: IntGauge,
    /// Cumulative blocks produced by this node, labelled by `consensus`
    /// mechanism (#1620). Incremented at the consensus block-creation site so
    /// every producer path (HTTP mine route, P2P auto-production) is counted.
    pub block_production_total: IntCounterVec,
    /// Wall-clock duration of block production in seconds (#1620). Observed
    /// once per successfully produced block.
    pub block_production_duration_seconds: Histogram,
    /// Cumulative PoP blocks accepted per liveness `path`
    /// (`publishing_score` / `useful_work` / `empty_liveness`, #1620). N5 takes
    /// the `useful_work` path every block, so a flat `useful_work` series is
    /// the canonical chain-liveness signal pre-launch.
    pub pop_liveness_path_total: IntCounterVec,
    /// 1 if the most recent WAL replay at startup completed cleanly; 0 after a
    /// replay error (#1620). Initialised to 1 (healthy) so a node that never
    /// replays a WAL — fresh node or clean checkpoint — reads as healthy, and
    /// only a genuine replay failure drives it to 0.
    pub wal_replay_status: IntGauge,
    /// Duration of WAL replay at node startup in seconds (#1620). Has at least
    /// one observation after every startup that reaches the replay stage.
    pub wal_replay_duration_seconds: Histogram,
    /// Cumulative count of WAL entries replayed at node startup (#1620).
    pub wal_entries_replayed_total: IntCounter,
    pub connected_peers: IntGauge,
    pub transactions_processed_total: IntCounter,
    pub transactions_failed_total: IntCounter,
    pub zk_proofs_observed_total: IntCounter,
    pub slashing_events_total: IntCounterVec,
    pub privacy_key_image_double_spend_total: IntCounter,
    /// 1 when the node's last `/readyz` evaluation reported ready (HTTP 200);
    /// 0 when not_ready (HTTP 503). Lets Prometheus alert on readiness (#2057).
    pub node_ready: IntGauge,
    /// Contributions whose oracle quorum arrived after their one-time reward
    /// decision had already closed, summed over the blocks currently held in
    /// memory (#2464). Non-zero means accepted, revealed observations are
    /// reaching quorum but will never be paid — the production gap behind the
    /// 5,578 observations that earned zero WRF. Refreshed on every scrape from
    /// the `oracle_reward_late_quorum_count` block-metadata key, so it never
    /// double-counts a block that is replayed or re-applied.
    pub oracle_late_quorum_unpaid: IntGauge,
    /// Real-provenance contribution reveals the #2779 fence deferred from the
    /// most recent locally-produced block for lack of oracle quorum. Sustained
    /// non-zero = quorum starvation (a dead attester ran 10.5h invisible on
    /// 2026-08-28, #2833); until now this count lived only in hash-bearing
    /// block metadata that nothing scrapes.
    pub pop_deferred_real_contribution_reveals: IntGauge,
    pub state_diverged: IntGauge,
    pub state_diverged_total: IntCounter,
    pub state_diverged_cleared_total: IntCounter,
    /// Same-height divergence events attributed to the reporting libp2p peer.
    ///
    /// **Cardinality is bounded by admission, not by trust in the peer.** A
    /// `peer_id` series is created only by
    /// [`CoreMetrics::admit_state_divergence_peer`], which the event loop calls
    /// on a peer's first `PeerChainStatus` — i.e. after the peer has proven
    /// chain identity — and is removed by
    /// [`CoreMetrics::release_state_divergence_peer`] on disconnect. Live series
    /// therefore track *currently admitted* peers, which the node's connection
    /// limits already bound.
    ///
    /// Raw pre-admission connections are counted by
    /// [`CoreMetrics::p2p_preadmission_connections_total`], which carries **no
    /// `peer_id` label**, so unauthenticated connection churn cannot allocate
    /// series (#2589).
    pub state_divergence_total: IntCounterVec,
    /// Raw `PeerConnected` events observed before admission, aggregate and
    /// **unlabelled** by design.
    ///
    /// This is the pre-admission counterpart to [`Self::state_divergence_total`].
    /// A remote peer choosing fresh libp2p identities can drive this counter but
    /// cannot grow the metric family, which is the whole point: connection churn
    /// costs one increment, not one persistent time series (#2589).
    pub p2p_preadmission_connections_total: IntCounter,
    /// Series evicted because [`MAX_STATE_DIVERGENCE_PEER_SERIES`] was reached.
    ///
    /// Non-zero means teardown events are being lost — the cap is doing work it
    /// should never need to do on a healthy node. Alert on any increase.
    pub state_divergence_peer_evictions_total: IntCounter,
    /// Insertion-ordered set of peers currently holding a `peer_id` series, so
    /// the cap can evict deterministically without consulting the event path.
    admitted_divergence_peers: Mutex<AdmittedDivergencePeers>,
    pub validator_participation_rate: Gauge,
    pub weather_active_sensors_count: IntGauge,
    pub weather_data_quality_score: Gauge,
    // Chain fork / reorg / finality — referenced by alert-rules-blockchain-health.yml
    // (#1148). `chain_fork_count` and `chain_reorg_total` are event-driven
    // cumulative counters bumped directly from the reorg engine via
    // [`inc_fork_observed`] / [`inc_reorg`]. `chain_fork_active` and
    // `chain_reorg_depth` are gauges reflecting the most recently observed
    // state. `chain_finalized_height` is a state-derived gauge refreshed on
    // every monitoring snapshot.
    pub chain_fork_count: IntCounter,
    pub chain_fork_active: IntGauge,
    pub chain_reorg_depth: IntGauge,
    pub chain_reorg_total: IntCounter,
    pub chain_finalized_height: IntGauge,
    /// 0/1 gauge indicating whether *any* BFT source has emitted a
    /// [`FinalitySignal`](crate::consensus::finality::FinalitySignal) on this
    /// node since boot (#1388). `wrfcoin_chain_finalized_height` falls back to
    /// the probabilistic floor `tip - MAX_REORG_DEPTH` when the BFT guard is
    /// inert, so on its own it cannot distinguish "BFT fired and advanced
    /// finality" from "BFT never fired and we're just showing a rolling
    /// window." This gauge exposes the distinction so alert rules can detect
    /// nodes where BFT has silently never activated.
    pub bft_finality_active: IntGauge,
    /// Raw BFT-only finalized height — never clamped to the probabilistic
    /// floor (#1399). Zero until the first FinalitySignal arrives. Unlike
    /// `chain_finalized_height`, this gauge reflects whether the BFT gadget
    /// has actually advanced finality, not just the probabilistic fallback.
    pub bft_finalized_height: IntGauge,
    /// Unix timestamp (seconds) of the most recent BFT-finalized height
    /// advance (#1399). Zero until the first FinalitySignal arrives. Pair
    /// with `time() - wrfcoin_chain_bft_last_advance_timestamp_seconds > 600`
    /// to page on stalled BFT finality even when the gauge has a non-zero
    /// value (e.g. BFT fired once then went silent).
    pub bft_last_advance_timestamp: IntGauge,
    /// Cumulative count of `FinalityCertificate`s emitted by the BFT
    /// finality gadget on this node since boot (#1388). Incremented every
    /// time a block first crosses the 2/3+1 supermajority threshold.
    pub bft_finality_certificates_emitted_total: IntCounter,
    /// Cumulative count of BFT finality votes ingested by the gadget on
    /// this node since boot (#1388). Counts both locally-produced votes
    /// and votes received over the gossipsub `wrfcoin/finality-votes/1`
    /// topic.
    pub bft_finality_votes_received_total: IntCounter,
    pub state_pruning_last_height: IntGauge,
    /// Cumulative count of equivocation / malicious-validator detections
    /// surfaced by the consensus layer (e.g. `ProofOfStake::record_double_sign`
    /// returning `MaliciousValidator`). Incremented at the detection site,
    /// not at state-transition apply — so operators see the misbehavior
    /// immediately, even while #1393 (on-chain slashing tx) is still
    /// outstanding and the evidence is only durable in `DurableEvidenceStore`.
    /// Pair with a `rate(...) > 0` alert rule to page oncall on any detected
    /// but unenforceable slashing. Refs #1393.
    pub malicious_validator_detected_total: IntCounter,
    /// Cumulative count of peers that crossed the graylist threshold due to
    /// accumulated `PeerPenalized` events (#1789). Incremented exactly once per
    /// peer per graylist *transition* (not on every subsequent penalty), so the
    /// counter tracks unique ban/graylist events. Pair with
    /// `rate(wrfcoin_p2p_peers_graylisted_total[5m]) > 0` to alert on
    /// sustained peer misbehavior.
    pub peers_graylisted_total: IntCounter,
    /// Block-sync serve requests refused by the #2119 serve-side governor,
    /// labelled by reason (`too_soon` / `bandwidth_exhausted`).
    ///
    /// **This is the observation in "observe before enforce".** The ceilings ship
    /// with defaults chosen for a ~12-node testnet and no measurement behind
    /// them; without this counter the rejection reasons exist only in debug logs,
    /// which is the same as not having them. Alert on
    /// `rate(wrfcoin_p2p_sync_serve_rejected_total[5m])` before tightening any
    /// limit.
    pub sync_serve_rejected_total: IntCounterVec,
    /// Peer messages whose PAYLOAD failed bincode decode after the gossip
    /// envelope itself parsed, labelled by payload kind
    /// (`block` / `transaction` / `forecast_manifest` / `forecast_receipt` /
    /// `finality_vote` / `finality_certificate`) (#2884).
    ///
    /// The #2727-class audit showed a version-skewed peer fails exactly here —
    /// its `Block` / `FinalityVote` layout differs, decode errors, and the
    /// handler drops the message — so skew presents as silently lost gossip,
    /// sync ranges, and finality votes (quorum starvation, the 2026-08-28
    /// silent-starvation family) with nothing naming the cause. A sustained
    /// nonzero `rate()` here, especially concentrated on one kind, is the
    /// version-skew signature; sporadic single counts are line noise or a
    /// hostile peer. Alert on `rate(...[5m]) > 0` sustained.
    pub p2p_payload_decode_failed_total: IntCounterVec,
    /// Transaction-gossip commands that could not be queued, labelled by the
    /// bounded failure set `serialize`, `full`, or `closed` (#3225).
    pub tx_gossip_enqueue_failures_total: IntCounterVec,
    /// Consecutive transaction-gossip enqueue failures. Reset by the next
    /// successful enqueue; sustained failures also gate readiness.
    pub tx_gossip_backpressure_streak: IntGauge,
    /// Cumulative count of on-chain slashing applies (#1393), labelled by
    /// reason string mirroring `SlashingEvidenceType`
    /// (`double_sign` / `invalid_zk_proof` / `repeated_low_quality_data`).
    pub slashing_applied_total: IntCounterVec,
    /// Cumulative base units of stake debited by the on-chain apply path (#1393).
    /// Mirrors the `u128` return of `ProofOfStake::debit_validator_stake`
    /// saturated into `u64` for Prometheus (`IntCounter` upper bound is `u63::MAX`).
    pub slashing_stake_debited_total: IntCounter,
    /// Cumulative transactions dropped during block assembly between mempool
    /// admission and block application (#2056), labelled by drop `reason`.
    /// Incremented at the `governance_transitions_for_block` filter when an
    /// admitted transaction fails to apply and is silently excluded from the
    /// block. Before #2056 this drop was invisible (no log, no metric).
    pub tx_dropped_total: IntCounterVec,
    pub ordering_blocks_assessed_total: IntCounter,
    pub ordering_no_data_total: IntCounter,
    pub ordering_assessment_available: IntGauge,
    pub ordering_reorder_distance: Histogram,
    pub ordering_excluded_sufficient_fee_total: IntCounterVec,
    pub observation_inclusion_delay_arrival_rank: HistogramVec,
    last_transactions_processed_total: AtomicU64,
    last_transactions_failed_total: AtomicU64,
    last_zk_proofs_observed_total: AtomicU64,
    /// Per-reason last-seen counters, indexed by position in [`SlashingReason::ALL`].
    last_slashing_events: [AtomicU64; 5],
    // P2P bandwidth counters
    pub p2p_bytes_received_total: IntCounterVec,
    pub p2p_bytes_sent_total: IntCounterVec,
    pub p2p_messages_total: IntCounterVec,
    pub p2p_mesh_peers: IntGaugeVec,
    /// Downloader heights labelled `local` and `target` (#2796).
    pub p2p_sync_height: IntGaugeVec,
    /// One-hot downloader state over a fixed, bounded label set.
    pub p2p_sync_state: IntGaugeVec,
    /// Downloader range counts labelled `pending`, `in_flight`, `outstanding`.
    pub p2p_sync_ranges: IntGaugeVec,
    last_p2p_bytes_received: [AtomicU64; 4],
    last_p2p_bytes_sent: [AtomicU64; 4],
    last_p2p_msgs_received: [AtomicU64; 4],
    last_p2p_msgs_sent: [AtomicU64; 4],
}

impl CoreMetrics {
    pub fn new() -> Self {
        let registry = Registry::new();
        let block_height = IntGauge::new("wrfcoin_blockchain_block_height", "Current block height").expect("metric creation failed");
        let chain_height = IntGauge::new(
            "wrfcoin_chain_height",
            "Current chain tip height exposed for testnet launch-readiness gates (#1620)",
        ).expect("metric creation failed");
        let mempool_size = IntGauge::new("wrfcoin_blockchain_mempool_size", "Current mempool size").expect("metric creation failed");
        let tx_pool_size = IntGauge::new(
            "wrfcoin_tx_pool_size",
            "Current transaction pool size exposed for testnet launch-readiness gates (#1620)",
        ).expect("metric creation failed");
        let block_production_total = IntCounterVec::new(
            Opts::new(
                "wrfcoin_block_production_total",
                "Total blocks produced by this node, labelled by consensus mechanism (#1620)",
            ),
            &["consensus"],
        ).expect("metric creation failed");
        // Pre-initialize the PoP label so the series appears as 0 before the
        // first block is produced.
        block_production_total.with_label_values(&["pop"]);
        let block_production_duration_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "wrfcoin_block_production_duration_seconds",
                "Wall-clock duration of block production in seconds (#1620)",
            ),
        ).expect("metric creation failed");
        let pop_liveness_path_total = IntCounterVec::new(
            Opts::new(
                "wrfcoin_pop_liveness_path_total",
                "Total PoP blocks accepted per liveness path (#1620)",
            ),
            &["path"],
        ).expect("metric creation failed");
        // Pre-initialize all known liveness paths so they appear as 0 in /metrics.
        for path in ["publishing_score", "useful_work", "empty_liveness"] {
            pop_liveness_path_total.with_label_values(&[path]);
        }
        let wal_replay_status = IntGauge::new(
            "wrfcoin_wal_replay_status",
            "1 if the most recent WAL replay at startup completed cleanly; 0 after a replay error (#1620)",
        ).expect("metric creation failed");
        // Healthy by default: a node that performs no replay (fresh node, or a
        // clean checkpoint with an empty WAL window) is not in an error state.
        wal_replay_status.set(1);
        let wal_replay_duration_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "wrfcoin_wal_replay_duration_seconds",
                "Duration of WAL replay at node startup in seconds (#1620)",
            ),
        ).expect("metric creation failed");
        let wal_entries_replayed_total = IntCounter::new(
            "wrfcoin_wal_entries_replayed_total",
            "Cumulative count of WAL entries replayed at node startup (#1620)",
        ).expect("metric creation failed");
        let connected_peers = IntGauge::new("wrfcoin_network_peer_count", "Number of connected P2P peers").expect("metric creation failed");
        let transactions_processed_total = IntCounter::new("wrfcoin_blockchain_transactions_processed_total", "Total number of transactions processed").expect("metric creation failed");
        let transactions_failed_total = IntCounter::new("wrfcoin_blockchain_transactions_failed_total", "Total number of transactions that failed").expect("metric creation failed");
        let zk_proofs_observed_total = IntCounter::new(
            "wrfcoin_blockchain_zk_proofs_observed_total",
            "Total number of privacy proofs structurally present, not cryptographically verified",
        ).expect("metric creation failed");
        let slashing_events_total = IntCounterVec::new(
            Opts::new("wrfcoin_blockchain_slashing_events_total", "Total number of slashing events"),
            &["reason"],
        ).expect("metric creation failed");
        // Pre-initialize all reason label combinations so they appear as 0 in /metrics.
        for reason in SlashingReason::ALL {
            slashing_events_total.with_label_values(&[reason.as_str()]);
        }
        let privacy_key_image_double_spend_total = IntCounter::new("wrfcoin_privacy_key_image_double_spend_total", "Total key image double-spend rejections").expect("metric creation failed");
        let node_ready = IntGauge::new(
            "wrfcoin_node_ready",
            "1 when the node's /readyz evaluation reports ready (HTTP 200); 0 when not_ready (HTTP 503). Set from the same evaluation that answers /readyz — block-staleness (NODE_SYNC_STALE_AFTER_SECS), state divergence, WAN failover, and the peer-count gate (#2057). Defaults to 0 (fail-closed) until the first /readyz evaluation runs.",
        ).expect("metric creation failed");
        let pop_deferred_real_contribution_reveals = IntGauge::new(
            "wrfcoin_pop_deferred_real_contribution_reveals",
            "Real-provenance contribution reveals deferred from the most recent locally-produced block pending oracle quorum (#2779 fence); sustained non-zero means quorum starvation (#2833)",
        )
        .unwrap();
        let oracle_late_quorum_unpaid = IntGauge::new(
            "wrfcoin_oracle_late_quorum_unpaid",
            "Contributions whose oracle quorum arrived AFTER their one-time reward decision closed, summed over the in-memory block window (#2464). Non-zero means revealed observations reached quorum too late and will never be paid. Refreshed per scrape from block metadata, so replayed blocks do not double-count.",
        ).expect("metric creation failed");
        let state_diverged = IntGauge::new(
            "wrfcoin_chain_state_diverged",
            "1 when the node has observed a same-height peer with a different state root; 0 otherwise (#1528)",
        ).expect("metric creation failed");
        let state_diverged_total = IntCounter::new(
            "wrfcoin_chain_state_diverged_total",
            "Total number of times this node detected its chain tip diverged from a connected peer at the same height (fork / partition)",
        ).expect("metric creation failed");
        let state_diverged_cleared_total = IntCounter::new(
            "wrfcoin_chain_state_diverged_cleared_total",
            "Total number of times the state-divergence latch was cleared after a same-height peer re-confirmed a matching tip and non-zero state root (#1713 recovery)",
        ).expect("metric creation failed");
        let state_divergence_total = IntCounterVec::new(
            Opts::new(
                "wrfcoin_state_divergence_total",
                "Total same-height state divergence events attributed to the reporting libp2p peer (#1620, #2589)",
            ),
            &["peer_id"],
        ).expect("metric creation failed");
        let p2p_preadmission_connections_total = IntCounter::new(
            "wrfcoin_p2p_preadmission_connections_total",
            "Total raw libp2p PeerConnected events observed before the peer reported chain status; deliberately unlabelled so connection churn cannot allocate per-peer series (#2589)",
        ).expect("metric creation failed");
        let state_divergence_peer_evictions_total = IntCounter::new(
            "wrfcoin_state_divergence_peer_evictions_total",
            "Per-peer divergence series evicted because the MAX_STATE_DIVERGENCE_PEER_SERIES ceiling was reached; non-zero means teardown events are being lost (#2589)",
        ).expect("metric creation failed");
        let validator_participation_rate = Gauge::new("wrfcoin_consensus_validator_participation_rate", "Fraction of active validators participating in consensus (0.0-1.0)").expect("metric creation failed");
        let weather_active_sensors_count = IntGauge::new("wrfcoin_weather_active_sensors_count", "Number of active weather sensors reporting data").expect("metric creation failed");
        let weather_data_quality_score = Gauge::new("wrfcoin_weather_data_quality_score", "Aggregate weather data quality score (0.0-1.0)").expect("metric creation failed");
        let chain_fork_count = IntCounter::new(
            "wrfcoin_chain_fork_count",
            "Cumulative count of candidate forks observed (a reorg plan was produced)",
        )
        .expect("metric creation failed");
        let chain_fork_active = IntGauge::new(
            "wrfcoin_chain_fork_active",
            "1 when the node is aware of a persistent non-canonical fork it cannot auto-resolve (depth exceeds MAX_REORG_DEPTH); 0 otherwise",
        )
        .expect("metric creation failed");
        let chain_reorg_depth = IntGauge::new(
            "wrfcoin_chain_reorg_depth",
            "Depth (in blocks) of the most recently executed chain reorganization",
        )
        .expect("metric creation failed");
        let chain_reorg_total = IntCounter::new(
            "wrfcoin_chain_reorg_total",
            "Cumulative count of chain reorganizations executed",
        )
        .expect("metric creation failed");
        let chain_finalized_height = IntGauge::new(
            "wrfcoin_chain_finalized_height",
            "Height of the most recently finalized block (chain tip minus MAX_REORG_DEPTH)",
        )
        .expect("metric creation failed");
        let state_pruning_last_height = IntGauge::new(
            "wrfcoin_state_pruning_last_height",
            "Block height at which state pruning last executed (#1202)",
        )
        .expect("metric creation failed");
        let slashing_applied_total = IntCounterVec::new(
            Opts::new(
                "wrfcoin_slashing_applied_total",
                "Cumulative on-chain slashing applies, labelled by evidence reason (#1393)",
            ),
            &["reason"],
        )
        .expect("metric creation failed");
        // Pre-initialize the three known reasons so they appear as 0 in /metrics.
        for reason in ["double_sign", "invalid_zk_proof", "repeated_low_quality_data"] {
            slashing_applied_total.with_label_values(&[reason]);
        }
        let slashing_stake_debited_total = IntCounter::new(
            "wrfcoin_slashing_stake_debited_total",
            "Cumulative base units of stake debited by on-chain slashing (#1393)",
        )
        .expect("metric creation failed");
        let tx_dropped_total = IntCounterVec::new(
            Opts::new(
                "wrfcoin_blockchain_tx_dropped_total",
                "Cumulative transactions dropped during block assembly, labelled by reason (#2056)",
            ),
            &["reason"],
        )
        .expect("metric creation failed");
        // Pre-initialize the known drop reasons so they appear as 0 in /metrics.
        for reason in [
            "invalid_nonce",
            "nonce_overflow",
            "insufficient_spendable_balance",
            "insufficient_balance",
            "account_not_found",
            "privacy_rejected",
            "apply_failed",
        ] {
            tx_dropped_total.with_label_values(&[reason]);
        }
        let ordering_blocks_assessed_total = IntCounter::new("wrfcoin_ordering_blocks_assessed_total", "Blocks assessed against a non-empty local mempool view (#2199)").expect("metric creation failed");
        let ordering_no_data_total = IntCounter::new("wrfcoin_ordering_no_data_total", "Block observations with an empty local mempool; not fairness assessments (#2199)").expect("metric creation failed");
        let ordering_assessment_available = IntGauge::new("wrfcoin_ordering_assessment_available", "1 when the latest ordering observation had local candidates; 0 means no data (#2199)").expect("metric creation failed");
        let ordering_reorder_distance = Histogram::with_opts(HistogramOpts::new("wrfcoin_ordering_reorder_distance", "Absolute local arrival-rank versus in-block-position distance (#2199)")).expect("metric creation failed");
        let ordering_excluded_sufficient_fee_total = IntCounterVec::new(Opts::new("wrfcoin_ordering_excluded_sufficient_fee_total", "Locally visible candidates omitted despite fee at least the block minimum; probabilistic only (#2199)"), &["proposer"]).expect("metric creation failed");
        let observation_inclusion_delay_arrival_rank = HistogramVec::new(HistogramOpts::new("wrfcoin_observation_inclusion_delay_arrival_rank", "Local arrival-rank delay for included station observations (#2199)"), &["station"]).expect("metric creation failed");

        let bft_finality_active = IntGauge::new(
            "wrfcoin_chain_bft_finality_active",
            "1 if a BFT finality source has emitted at least one FinalitySignal \
             on this node since boot; 0 otherwise. Distinguishes a live BFT \
             guard from the probabilistic (tip - MAX_REORG_DEPTH) fallback \
             shown by wrfcoin_chain_finalized_height (#1388).",
        )
        .expect("metric creation failed");
        let bft_finalized_height = IntGauge::new(
            "wrfcoin_chain_bft_finalized_height",
            "BFT-only finalized block height — never clamped to the probabilistic \
             floor (tip - MAX_REORG_DEPTH). Zero until the first FinalitySignal \
             arrives. Use alongside wrfcoin_chain_finalized_height to distinguish \
             a live BFT gadget from the probabilistic fallback (#1399).",
        )
        .expect("metric creation failed");
        let bft_last_advance_timestamp = IntGauge::new(
            "wrfcoin_chain_bft_last_advance_timestamp_seconds",
            "Unix timestamp (seconds) of the last time the BFT-finalized height \
             advanced. Zero until the first FinalitySignal arrives. Alert via \
             `time() - wrfcoin_chain_bft_last_advance_timestamp_seconds > 600` \
             to detect stalled BFT even when the height gauge has a value (#1399).",
        )
        .expect("metric creation failed");
        let bft_finality_certificates_emitted_total = IntCounter::new(
            "wrfcoin_chain_bft_finality_certificates_emitted_total",
            "Cumulative count of FinalityCertificates emitted by the BFT \
             gadget on this node since boot — every supermajority crossing \
             produces one (#1388).",
        )
        .expect("metric creation failed");
        let bft_finality_votes_received_total = IntCounter::new(
            "wrfcoin_chain_bft_finality_votes_received_total",
            "Cumulative count of BFT finality votes ingested by the gadget \
             on this node since boot — counts both locally-produced votes \
             and votes received via gossipsub (#1388).",
        )
        .expect("metric creation failed");
        let malicious_validator_detected_total = IntCounter::new(
            "wrfcoin_chain_malicious_validator_detected_total",
            "Cumulative count of malicious-validator detections observed by \
             consensus (e.g. double-sign equivocation). Incremented at the \
             detection site; independent of whether the corresponding \
             slashing transaction has been applied on-chain yet (#1393).",
        )
        .expect("metric creation failed");
        let sync_serve_rejected_total = IntCounterVec::new(
            prometheus::Opts::new(
                "wrfcoin_p2p_sync_serve_rejected_total",
                "Block-sync serve requests refused by the serve-side governor, \
                 labelled by reason (too_soon / bandwidth_exhausted) (#2119).",
            ),
            &["reason"],
        )
        .expect("metric creation failed");
        let p2p_payload_decode_failed_total = IntCounterVec::new(
            prometheus::Opts::new(
                "wrfcoin_p2p_payload_decode_failed_total",
                "Peer message payloads that failed bincode decode after the \
                 envelope parsed, labelled by payload kind. Sustained nonzero \
                 rate concentrated on one kind is the version-skew signature \
                 (#2884/#2727).",
            ),
            &["kind"],
        )
        .expect("metric creation failed");
        let tx_gossip_enqueue_failures_total = IntCounterVec::new(
            prometheus::Opts::new(
                "wrfcoin_tx_gossip_enqueue_failures_total",
                "Transaction gossip commands that could not be queued, by bounded reason (#3225)",
            ),
            &["reason"],
        )
        .expect("metric creation failed");
        for reason in ["serialize", "full", "closed"] {
            tx_gossip_enqueue_failures_total.with_label_values(&[reason]);
        }
        let tx_gossip_backpressure_streak = IntGauge::new(
            "wrfcoin_tx_gossip_backpressure_streak",
            "Consecutive failed transaction-gossip enqueue attempts; reset by successful enqueue (#3225)",
        )
        .expect("metric creation failed");
        let peers_graylisted_total = IntCounter::new(
            "wrfcoin_p2p_peers_graylisted_total",
            "Cumulative count of peers that crossed the graylist threshold \
             (accumulated PeerPenalized events). Incremented once per graylist \
             transition in the native-chain P2P event loop (#1789).",
        )
        .expect("metric creation failed");

        registry.register(Box::new(block_height.clone())).unwrap();
        registry.register(Box::new(chain_height.clone())).unwrap();
        registry.register(Box::new(mempool_size.clone())).unwrap();
        registry.register(Box::new(connected_peers.clone())).unwrap();
        registry.register(Box::new(transactions_processed_total.clone())).unwrap();
        registry.register(Box::new(transactions_failed_total.clone())).unwrap();
        registry.register(Box::new(zk_proofs_observed_total.clone())).unwrap();
        registry.register(Box::new(slashing_events_total.clone())).unwrap();
        registry.register(Box::new(privacy_key_image_double_spend_total.clone())).unwrap();
        registry.register(Box::new(node_ready.clone())).unwrap();
        registry.register(Box::new(oracle_late_quorum_unpaid.clone())).unwrap();
        registry
            .register(Box::new(pop_deferred_real_contribution_reveals.clone()))
            .unwrap();
        registry.register(Box::new(state_diverged.clone())).unwrap();
        registry.register(Box::new(state_diverged_total.clone())).unwrap();
        registry.register(Box::new(state_diverged_cleared_total.clone())).unwrap();
        registry.register(Box::new(state_divergence_total.clone())).unwrap();
        // #2734: the family must exist in the exposition from the first scrape,
        // not only after the first PeerChainStatus. See the sentinel's doc.
        state_divergence_total.with_label_values(&[STATE_DIVERGENCE_SENTINEL_PEER]);
        registry.register(Box::new(p2p_preadmission_connections_total.clone())).unwrap();
        registry.register(Box::new(state_divergence_peer_evictions_total.clone())).unwrap();
        registry.register(Box::new(validator_participation_rate.clone())).unwrap();
        registry.register(Box::new(weather_active_sensors_count.clone())).unwrap();
        registry.register(Box::new(weather_data_quality_score.clone())).unwrap();
        registry.register(Box::new(chain_fork_count.clone())).unwrap();
        registry.register(Box::new(chain_fork_active.clone())).unwrap();
        registry.register(Box::new(chain_reorg_depth.clone())).unwrap();
        registry.register(Box::new(chain_reorg_total.clone())).unwrap();
        registry.register(Box::new(chain_finalized_height.clone())).unwrap();
        registry.register(Box::new(bft_finality_active.clone())).unwrap();
        registry.register(Box::new(bft_finalized_height.clone())).unwrap();
        registry.register(Box::new(bft_last_advance_timestamp.clone())).unwrap();
        registry.register(Box::new(bft_finality_certificates_emitted_total.clone())).unwrap();
        registry.register(Box::new(bft_finality_votes_received_total.clone())).unwrap();
        registry.register(Box::new(state_pruning_last_height.clone())).unwrap();
        registry.register(Box::new(malicious_validator_detected_total.clone())).unwrap();
        registry.register(Box::new(peers_graylisted_total.clone())).unwrap();
        registry.register(Box::new(sync_serve_rejected_total.clone())).unwrap();
        registry.register(Box::new(p2p_payload_decode_failed_total.clone())).unwrap();
        registry.register(Box::new(tx_gossip_enqueue_failures_total.clone())).unwrap();
        registry.register(Box::new(tx_gossip_backpressure_streak.clone())).unwrap();
        registry.register(Box::new(slashing_applied_total.clone())).unwrap();
        registry.register(Box::new(slashing_stake_debited_total.clone())).unwrap();
        registry.register(Box::new(tx_dropped_total.clone())).unwrap();
        registry.register(Box::new(ordering_blocks_assessed_total.clone())).unwrap();
        registry.register(Box::new(ordering_no_data_total.clone())).unwrap();
        registry.register(Box::new(ordering_assessment_available.clone())).unwrap();
        registry.register(Box::new(ordering_reorder_distance.clone())).unwrap();
        registry.register(Box::new(ordering_excluded_sufficient_fee_total.clone())).unwrap();
        registry.register(Box::new(observation_inclusion_delay_arrival_rank.clone())).unwrap();
        registry.register(Box::new(tx_pool_size.clone())).unwrap();
        registry.register(Box::new(block_production_total.clone())).unwrap();
        registry.register(Box::new(block_production_duration_seconds.clone())).unwrap();
        registry.register(Box::new(pop_liveness_path_total.clone())).unwrap();
        registry.register(Box::new(wal_replay_status.clone())).unwrap();
        registry.register(Box::new(wal_replay_duration_seconds.clone())).unwrap();
        registry.register(Box::new(wal_entries_replayed_total.clone())).unwrap();

        let p2p_bytes_received_total = IntCounterVec::new(
            Opts::new("wrfcoin_p2p_bytes_received_total", "Total bytes received per gossipsub topic"),
            &["topic"],
        ).expect("metric creation failed");
        let p2p_bytes_sent_total = IntCounterVec::new(
            Opts::new("wrfcoin_p2p_bytes_sent_total", "Total bytes sent per gossipsub topic"),
            &["topic"],
        ).expect("metric creation failed");
        let p2p_messages_total = IntCounterVec::new(
            Opts::new("wrfcoin_p2p_messages_total", "Total messages per gossipsub topic and direction"),
            &["topic", "direction"],
        ).expect("metric creation failed");
        let p2p_mesh_peers = IntGaugeVec::new(
            Opts::new(
                "wrfcoin_p2p_gossipsub_mesh_peers",
                "Current gossipsub mesh peer count per topic",
            ),
            &["topic"],
        ).expect("metric creation failed");
        let p2p_sync_height = IntGaugeVec::new(
            Opts::new(
                "wrfcoin_p2p_sync_height",
                "Block downloader height by kind (local / target) (#2796)",
            ),
            &["kind"],
        ).expect("metric creation failed");
        let p2p_sync_state = IntGaugeVec::new(
            Opts::new(
                "wrfcoin_p2p_sync_state",
                "One-hot block downloader state (#2796)",
            ),
            &["state"],
        ).expect("metric creation failed");
        let p2p_sync_ranges = IntGaugeVec::new(
            Opts::new(
                "wrfcoin_p2p_sync_ranges",
                "Block downloader range count by kind (pending / in_flight / outstanding) (#2796)",
            ),
            &["kind"],
        ).expect("metric creation failed");

        registry.register(Box::new(p2p_bytes_received_total.clone())).unwrap();
        registry.register(Box::new(p2p_bytes_sent_total.clone())).unwrap();
        registry.register(Box::new(p2p_messages_total.clone())).unwrap();
        registry.register(Box::new(p2p_mesh_peers.clone())).unwrap();
        registry.register(Box::new(p2p_sync_height.clone())).unwrap();
        registry.register(Box::new(p2p_sync_state.clone())).unwrap();
        registry.register(Box::new(p2p_sync_ranges.clone())).unwrap();
        for topic in P2P_TOPICS {
            p2p_mesh_peers.with_label_values(&[topic]);
        }
        for kind in ["local", "target"] {
            p2p_sync_height.with_label_values(&[kind]);
        }
        for state in ["idle", "discovering", "downloading", "stalled", "complete"] {
            p2p_sync_state.with_label_values(&[state]);
        }
        p2p_sync_state.with_label_values(&["idle"]).set(1);
        for kind in ["pending", "in_flight", "outstanding"] {
            p2p_sync_ranges.with_label_values(&[kind]);
        }

        Self {
            registry, block_height, chain_height, mempool_size, connected_peers,
            transactions_processed_total, transactions_failed_total,
            zk_proofs_observed_total, slashing_events_total,
            privacy_key_image_double_spend_total,
            node_ready,
            oracle_late_quorum_unpaid,
            pop_deferred_real_contribution_reveals,
            state_diverged,
            state_diverged_cleared_total,
            state_diverged_total,
            state_divergence_total,
            p2p_preadmission_connections_total,
            state_divergence_peer_evictions_total,
            admitted_divergence_peers: Mutex::new(AdmittedDivergencePeers::default()),
            validator_participation_rate,
            weather_active_sensors_count,
            weather_data_quality_score,
            chain_fork_count,
            chain_fork_active,
            chain_reorg_depth,
            chain_reorg_total,
            chain_finalized_height,
            bft_finality_active,
            bft_finalized_height,
            bft_last_advance_timestamp,
            bft_finality_certificates_emitted_total,
            bft_finality_votes_received_total,
            state_pruning_last_height,
            malicious_validator_detected_total,
            peers_graylisted_total,
            sync_serve_rejected_total,
            p2p_payload_decode_failed_total,
            tx_gossip_enqueue_failures_total,
            tx_gossip_backpressure_streak,
            slashing_applied_total,
            slashing_stake_debited_total,
            tx_dropped_total,
            ordering_blocks_assessed_total,
            ordering_no_data_total,
            ordering_assessment_available,
            ordering_reorder_distance,
            ordering_excluded_sufficient_fee_total,
            observation_inclusion_delay_arrival_rank,
            tx_pool_size,
            block_production_total,
            block_production_duration_seconds,
            pop_liveness_path_total,
            wal_replay_status,
            wal_replay_duration_seconds,
            wal_entries_replayed_total,
            last_transactions_processed_total: AtomicU64::new(0),
            last_transactions_failed_total: AtomicU64::new(0),
            last_zk_proofs_observed_total: AtomicU64::new(0),
            last_slashing_events: core::array::from_fn(|_| AtomicU64::new(0)),
            p2p_bytes_received_total,
            p2p_bytes_sent_total,
            p2p_messages_total,
            p2p_mesh_peers,
            p2p_sync_height,
            p2p_sync_state,
            p2p_sync_ranges,
            last_p2p_bytes_received: core::array::from_fn(|_| AtomicU64::new(0)),
            last_p2p_bytes_sent: core::array::from_fn(|_| AtomicU64::new(0)),
            last_p2p_msgs_received: core::array::from_fn(|_| AtomicU64::new(0)),
            last_p2p_msgs_sent: core::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    /// Record observation of a candidate fork (a reorg plan was produced).
    /// Bumps the cumulative fork counter monotonically.
    pub fn inc_fork_observed(&self) {
        self.chain_fork_count.inc();
    }

    /// Record execution of a chain reorganization of the given depth.
    /// Bumps `chain_reorg_total`, sets `chain_reorg_depth` to the new value,
    /// and clears `chain_fork_active` (the node has converged onto a new tip).
    pub fn inc_reorg(&self, depth: u64) {
        self.chain_reorg_total.inc();
        self.chain_reorg_depth.set(depth as i64);
        self.chain_fork_active.set(0);
    }

    /// Set the fork-active indicator. Set to true when the node has observed
    /// a candidate fork it cannot auto-resolve (depth exceeds MAX_REORG_DEPTH);
    /// set to false on successful reorg or when the fork vanishes.
    pub fn set_fork_active(&self, active: bool) {
        self.chain_fork_active.set(if active { 1 } else { 0 });
    }

    /// Mark the BFT finality source as active (#1388). Called once by the
    /// finality signal consumer the first time a `FinalitySignal` arrives.
    /// Latches at 1; subsequent calls are no-ops.
    pub fn mark_bft_finality_active(&self) {
        self.bft_finality_active.set(1);
    }

    /// Record a BFT-finalized height advance (#1399). Updates the raw BFT
    /// height gauge (never clamped) and stamps the current wall-clock time
    /// so operators can detect stalled finality via
    /// `time() - wrfcoin_chain_bft_last_advance_timestamp_seconds > 600`.
    pub fn record_bft_advance(&self, height: u64) {
        self.bft_finalized_height.set(height as i64);
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.bft_last_advance_timestamp.set(ts as i64);
    }

    pub fn inc_key_image_double_spend(&self) {
        self.privacy_key_image_double_spend_total.inc();
    }

    /// Set the node-readiness gauge from a `/readyz` evaluation result.
    /// `ready` mirrors the HTTP status the handler returns (`true` → 200/ready,
    /// `false` → 503/not_ready), so Prometheus can alert on
    /// `wrfcoin_node_ready == 0 for: 2m` (#2057).
    pub fn set_node_ready(&self, ready: bool) {
        self.node_ready.set(if ready { 1 } else { 0 });
    }

    /// Set the late-quorum-unpaid gauge (#2464).
    ///
    /// A gauge, not a counter: the caller recomputes the value from block
    /// metadata on every scrape, so a block that is replayed, re-applied after a
    /// reorg, or restored from the WAL contributes exactly once.
    pub fn set_oracle_late_quorum_unpaid(&self, count: u64) {
        self.oracle_late_quorum_unpaid.set(count as i64);
    }

    /// Record how many real-provenance reveals the quorum fence deferred from
    /// the block just produced (#2833). Set on every local block creation, so
    /// the gauge tracks the newest block rather than latching a stale spike.
    pub fn set_pop_deferred_real_contribution_reveals(&self, count: u64) {
        self.pop_deferred_real_contribution_reveals.set(count as i64);
    }

    /// Record one local ordering observation. Different nodes have different
    /// mempools, so an exclusion is a signal for investigation, never proof.
    /// An empty snapshot is exported as unavailable rather than as fair zero.
    pub fn record_block_ordering(
        &self,
        block: &Block,
        snapshot: &OrderingMempoolSnapshot,
        proposer: &str,
    ) {
        if snapshot.entries.is_empty() {
            self.ordering_assessment_available.set(0);
            self.ordering_no_data_total.inc();
            return;
        }
        self.ordering_assessment_available.set(1);
        self.ordering_blocks_assessed_total.inc();
        let by_id: HashMap<Hash, &OrderingMempoolEntry> = snapshot.entries.iter().map(|e| (e.tx_id, e)).collect();
        let included: HashSet<Hash> = block.transactions.iter().filter_map(|tx| tx.id).collect();
        let min_fee_rate_milli = block.transactions.iter()
            .filter(|tx| tx.tx_type != crate::blockchain::transaction::TransactionType::Coinbase)
            .map(|tx| crate::mempool::pool::Mempool::fee_rate_milli(tx.fee, tx.size()))
            .min();
        for (position, tx) in block.transactions.iter().enumerate() {
            if let Some(entry) = tx.id.and_then(|id| by_id.get(&id)) {
                // determinism-ok: Prometheus telemetry only; this value never enters consensus state, block bytes, or signatures.
                self.ordering_reorder_distance.observe(entry.arrival_rank.abs_diff(position as u64) as f64);
                if let Some(station) = observation_station_id(tx) {
                    // determinism-ok: Prometheus telemetry only; this value never enters consensus state, block bytes, or signatures.
                    self.observation_inclusion_delay_arrival_rank.with_label_values(&[&station]).observe(entry.arrival_rank as f64);
                }
            }
        }
        if let Some(min_fee_rate_milli) = min_fee_rate_milli {
            let excluded = snapshot.entries.iter().filter(|entry| {
                crate::mempool::pool::Mempool::fee_rate_milli(entry.fee, entry.size)
                    >= min_fee_rate_milli
                    && !included.contains(&entry.tx_id)
            }).count() as u64;
            if excluded != 0 {
                self.ordering_excluded_sufficient_fee_total.with_label_values(&[proposer]).inc_by(excluded);
            }
        }
    }


    /// Count a raw pre-admission `PeerConnected` event.
    ///
    /// Deliberately takes **no peer identity**. An unauthenticated peer cycling
    /// libp2p identities can drive this counter arbitrarily, and that is the
    /// intended cost model: churn buys one increment, never a new time series.
    pub fn record_preadmission_peer_connection(&self) {
        self.p2p_preadmission_connections_total.inc();
    }

    /// Admit a peer into the per-peer divergence family, creating its series at
    /// zero so a healthy admitted peer is visible in `/metrics` before it ever
    /// diverges.
    ///
    /// **Call this only past the admission boundary** — i.e. once the peer has
    /// reported chain status. Calling it on a raw connection reintroduces the
    /// unbounded-cardinality vector this exists to close (#2589). Idempotent:
    /// re-admitting an already-admitted peer leaves its counter untouched.
    ///
    /// **Hard-bounded, independently of teardown.** The registry evicts the
    /// oldest admitted peer once [`MAX_STATE_DIVERGENCE_PEER_SERIES`] is
    /// reached, so the live series count can never exceed that ceiling **even if
    /// no release event ever arrives**. That independence is the point: the
    /// `PeerDisconnected` send in `p2p/src/node.rs` is `let _ = …send(…)` and its
    /// consumer is a detached `tokio::spawn`, so a dropped teardown is reachable
    /// at the event boundary and an admission-plus-release scheme alone would
    /// leak a series every time one is lost.
    pub fn admit_state_divergence_peer(&self, peer_id: &str) {
        // #2734: the sentinel already exists and must never enter the admitted
        // set — eviction would then be able to remove it.
        if peer_id == STATE_DIVERGENCE_SENTINEL_PEER {
            return;
        }
        let mut admitted = self
            .admitted_divergence_peers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if admitted.present.contains(peer_id) {
            return;
        }
        while admitted.order.len() >= MAX_STATE_DIVERGENCE_PEER_SERIES {
            let Some(evicted) = admitted.order.pop_front() else {
                break;
            };
            admitted.present.remove(&evicted);
            let _ = self.state_divergence_total.remove_label_values(&[evicted.as_str()]);
            self.state_divergence_peer_evictions_total.inc();
        }
        admitted.order.push_back(peer_id.to_string());
        admitted.present.insert(peer_id.to_string());
        self.state_divergence_total.with_label_values(&[peer_id]);
    }

    /// Release an admitted peer's series on disconnect.
    ///
    /// Without this the family only ever grows: `IntCounterVec` has no eviction,
    /// so a series outlives the connection that created it for the process
    /// lifetime. A peer that is not currently admitted is not an error — the
    /// event loop calls this for every disconnect, including peers that
    /// disconnected before ever reporting chain status.
    ///
    /// This is the *prompt* path. It is not the bound — see
    /// [`Self::admit_state_divergence_peer`] for the ceiling that holds when this
    /// is never called.
    pub fn release_state_divergence_peer(&self, peer_id: &str) {
        // #2734: the startup sentinel is not a peer and must survive every
        // release path — removing it would re-open the empty-family window the
        // healthcheck trips over.
        if peer_id == STATE_DIVERGENCE_SENTINEL_PEER {
            return;
        }
        let mut admitted = self
            .admitted_divergence_peers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if admitted.present.remove(peer_id) {
            admitted.order.retain(|candidate| candidate != peer_id);
        }
        drop(admitted);
        let _ = self.state_divergence_total.remove_label_values(&[peer_id]);
    }

    /// Number of peers currently holding a `peer_id` series. Test-facing view of
    /// the bound; the authoritative evidence is always the rendered exposition.
    pub fn admitted_state_divergence_peer_count(&self) -> usize {
        self.admitted_divergence_peers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .order
            .len()
    }

    /// Record a same-height divergence event detected from peer gossip.
    pub fn inc_state_divergence_detected(&self, peer_id: &str) {
        self.state_diverged_total.inc();
        self.state_divergence_total
            .with_label_values(&[peer_id])
            .inc();
    }

    /// Record that the state-divergence latch was cleared after a same-height
    /// peer re-confirmed a matching tip and non-zero state root (#1713).
    pub fn inc_state_divergence_cleared(&self) {
        self.state_diverged_cleared_total.inc();
    }

    /// Increment the slashing events counter for the given reason.
    pub fn inc_slashing_event(&self, reason: &SlashingReason) {
        self.slashing_events_total
            .with_label_values(&[reason.as_str()])
            .inc();
    }

    /// Record a consensus-layer detection of malicious validator behavior
    /// (e.g. double-sign equivocation) — independent of whether the
    /// corresponding slashing transaction has been applied on-chain.
    /// Intended to be called at the detection site (#1393).
    pub fn inc_malicious_validator_detected(&self) {
        self.malicious_validator_detected_total.inc();
    }

    /// Record a peer graylist transition triggered by accumulated `PeerPenalized`
    /// events (#1789). Called at most once per peer per transition (not on every
    /// subsequent penalty after the peer is already graylisted).
    pub fn inc_peer_graylisted(&self) {
        self.peers_graylisted_total.inc();
    }

    /// Record a block-sync serve refusal by reason (#2119).
    ///
    /// `reason` comes from `ServeRejection::label()`, which is a fixed small set
    /// (`too_soon` / `bandwidth_exhausted`) — deliberately not a free-form string,
    /// because a peer-influenced label would make this an unbounded-cardinality
    /// metric and turn a resource guard into a memory vector.
    pub fn inc_sync_serve_rejected(&self, reason: &str) {
        self.sync_serve_rejected_total.with_label_values(&[reason]).inc();
    }

    /// Record a peer message payload that failed bincode decode after the
    /// gossip envelope parsed (#2884). `kind` is a fixed label from the
    /// handler site (`block`, `transaction`, …) — never peer-controlled data,
    /// which would mint unbounded label series.
    pub fn inc_p2p_payload_decode_failed(&self, kind: &str) {
        self.p2p_payload_decode_failed_total
            .with_label_values(&[kind])
            .inc();
    }

    /// Current count for one payload-decode-failure kind — test/diagnostic
    /// read hook for [`Self::inc_p2p_payload_decode_failed`].
    pub fn p2p_payload_decode_failed_count(&self, kind: &str) -> u64 {
        self.p2p_payload_decode_failed_total
            .with_label_values(&[kind])
            .get()
    }

    /// Record an on-chain slashing apply (#1393). Bumps the per-reason apply
    /// counter and the cumulative stake-debited total.
    ///
    /// `debited_base_units` is `u128` to match `debit_validator_stake`'s
    /// return type; it is saturated into `u64` before incrementing the
    /// Prometheus `IntCounter`, whose effective upper bound is `u63::MAX`.
    pub fn record_slashing_applied(&self, reason: &str, debited_base_units: u128) {
        self.slashing_applied_total
            .with_label_values(&[reason])
            .inc();
        let debited_u64 = u64::try_from(debited_base_units).unwrap_or(u64::MAX);
        self.slashing_stake_debited_total.inc_by(debited_u64);
    }

    /// Record a transaction dropped during block assembly (#2056). `reason` is
    /// a bounded, low-cardinality label (see the pre-initialized set in
    /// `CoreMetrics::new`) derived from the underlying `StateError`.
    pub fn inc_tx_dropped(&self, reason: &str) {
        self.tx_dropped_total.with_label_values(&[reason]).inc();
    }

    /// Sync P2P bandwidth counters from the P2P layer's atomic snapshot into
    /// the Prometheus registry.
    pub fn apply_p2p_snapshot(&self, snap: &P2pBandwidthSnapshot) {
        for (i, topic) in P2P_TOPICS.iter().enumerate() {
            sync_counter(
                &self.p2p_bytes_received_total.with_label_values(&[topic]),
                &self.last_p2p_bytes_received[i],
                snap.bytes_received[i],
            );
            sync_counter(
                &self.p2p_bytes_sent_total.with_label_values(&[topic]),
                &self.last_p2p_bytes_sent[i],
                snap.bytes_sent[i],
            );
            sync_counter(
                &self.p2p_messages_total.with_label_values(&[topic, &"received"]),
                &self.last_p2p_msgs_received[i],
                snap.messages_received[i],
            );
            sync_counter(
                &self.p2p_messages_total.with_label_values(&[topic, &"sent"]),
                &self.last_p2p_msgs_sent[i],
                snap.messages_sent[i],
            );
        }
    }

    /// Sync gossipsub mesh peer counts into the Prometheus registry.
    pub fn apply_p2p_mesh_snapshot(&self, snap: &P2pMeshSnapshot) {
        for (i, topic) in P2P_TOPICS.iter().enumerate() {
            self.p2p_mesh_peers
                .with_label_values(&[topic])
                .set(snap.mesh_peers[i] as i64);
        }
    }

    /// Publish the P2P downloader's bounded operator-facing gauges (#2796).
    pub fn set_p2p_sync(
        &self,
        local_height: u64,
        target_height: u64,
        state: &str,
        pending_ranges: usize,
        in_flight_ranges: usize,
        outstanding_ranges: usize,
    ) {
        let bounded_i64 = |value: u64| i64::try_from(value).unwrap_or(i64::MAX);
        self.p2p_sync_height
            .with_label_values(&["local"])
            .set(bounded_i64(local_height));
        self.p2p_sync_height
            .with_label_values(&["target"])
            .set(bounded_i64(target_height));

        let state = match state {
            "idle" | "discovering" | "downloading" | "stalled" | "complete" => state,
            _ => "idle",
        };
        for candidate in ["idle", "discovering", "downloading", "stalled", "complete"] {
            self.p2p_sync_state
                .with_label_values(&[candidate])
                .set(i64::from(candidate == state));
        }

        for (kind, value) in [
            ("pending", pending_ranges),
            ("in_flight", in_flight_ranges),
            ("outstanding", outstanding_ranges),
        ] {
            self.p2p_sync_ranges
                .with_label_values(&[kind])
                .set(i64::try_from(value).unwrap_or(i64::MAX));
        }
    }

    pub fn apply_snapshot(&self, snapshot: &CoreMetricSnapshot) {
        self.block_height.set(snapshot.block_height as i64);
        self.chain_height.set(snapshot.block_height as i64);
        self.mempool_size.set(snapshot.mempool_size as i64);
        self.tx_pool_size.set(snapshot.mempool_size as i64);
        self.connected_peers.set(snapshot.connected_peers as i64);
        self.validator_participation_rate.set(snapshot.validator_participation_rate);
        self.weather_active_sensors_count.set(snapshot.weather_active_sensors_count as i64);
        self.weather_data_quality_score.set(snapshot.weather_data_quality_score);
        self.state_diverged.set(if snapshot.state_diverged { 1 } else { 0 });
        self.chain_finalized_height.set(snapshot.chain_finalized_height as i64);
        self.state_pruning_last_height.set(snapshot.state_pruning_last_height as i64);
        sync_counter(&self.transactions_processed_total, &self.last_transactions_processed_total, snapshot.transactions_processed_total);
        sync_counter(&self.transactions_failed_total, &self.last_transactions_failed_total, snapshot.transactions_failed_total);
        sync_counter(&self.zk_proofs_observed_total, &self.last_zk_proofs_observed_total, snapshot.zk_proofs_observed_total);
        for (i, reason) in SlashingReason::ALL.iter().enumerate() {
            let count = snapshot
                .slashing_events_by_reason
                .get(reason.as_str())
                .copied()
                .unwrap_or(0);
            sync_counter(
                &self.slashing_events_total.with_label_values(&[reason.as_str()]),
                &self.last_slashing_events[i],
                count,
            );
        }
    }
}

fn observation_station_id(tx: &crate::blockchain::transaction::Transaction) -> Option<String> {
    use crate::blockchain::transaction::TransactionType;
    match tx.tx_type {
        TransactionType::ContributionCommit | TransactionType::ForecastCommit => tx.parse_commit_payload().map(|payload| payload.station_id),
        TransactionType::ContributionReveal | TransactionType::ForecastReveal => tx.parse_reveal_payload().map(|payload| payload.station_id),
        _ => None,
    }
}

fn sync_counter(counter: &IntCounter, last_seen: &AtomicU64, target: u64) {
    let mut previous = last_seen.load(Ordering::Relaxed);
    loop {
        if target <= previous { return; }
        match last_seen.compare_exchange_weak(previous, target, Ordering::AcqRel, Ordering::Relaxed) {
            Ok(_) => { counter.inc_by(target - previous); return; }
            Err(actual) => previous = actual,
        }
    }
}

pub static CORE_METRICS: LazyLock<CoreMetrics> = LazyLock::new(CoreMetrics::new);

#[cfg(test)]
mod tests {
    use super::{Block, CoreMetricSnapshot, CoreMetrics, Hash, MAX_STATE_DIVERGENCE_PEER_SERIES, OrderingMempoolEntry, OrderingMempoolSnapshot, P2pBandwidthSnapshot, P2pMeshSnapshot, STATE_DIVERGENCE_SENTINEL_PEER};
    use prometheus::{Encoder, TextEncoder};
    use std::collections::HashMap;

    fn slashing_map(pairs: &[(&str, u64)]) -> HashMap<String, u64> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn apply_snapshot_updates_gauges_and_counters_monotonically() {
        let metrics = CoreMetrics::new();
        metrics.apply_snapshot(&CoreMetricSnapshot {
            block_height: 12, mempool_size: 4, connected_peers: 2,
            transactions_processed_total: 10, transactions_failed_total: 1,
            zk_proofs_observed_total: 3,
            slashing_events_by_reason: slashing_map(&[("invalid_reveal", 1), ("oracle_attestation_fraud", 1)]),
            ..Default::default()
        });
        metrics.apply_snapshot(&CoreMetricSnapshot {
            block_height: 15, mempool_size: 1, connected_peers: 5,
            transactions_processed_total: 14, transactions_failed_total: 1,
            zk_proofs_observed_total: 4,
            slashing_events_by_reason: slashing_map(&[("invalid_reveal", 2), ("oracle_attestation_fraud", 1)]),
            ..Default::default()
        });
        assert_eq!(metrics.block_height.get(), 15);
        assert_eq!(metrics.mempool_size.get(), 1);
        assert_eq!(metrics.connected_peers.get(), 5);
        assert_eq!(metrics.transactions_processed_total.get(), 14);
        assert_eq!(metrics.transactions_failed_total.get(), 1);
        assert_eq!(metrics.zk_proofs_observed_total.get(), 4);
        assert_eq!(metrics.slashing_events_total.with_label_values(&["invalid_reveal"]).get(), 2);
        assert_eq!(metrics.slashing_events_total.with_label_values(&["oracle_attestation_fraud"]).get(), 1);
        // Apply smaller snapshot — counters must NOT decrease.
        metrics.apply_snapshot(&CoreMetricSnapshot {
            block_height: 9, mempool_size: 0, connected_peers: 1,
            transactions_processed_total: 4, transactions_failed_total: 0,
            zk_proofs_observed_total: 2,
            slashing_events_by_reason: slashing_map(&[("invalid_reveal", 1)]),
            ..Default::default()
        });
        assert_eq!(metrics.transactions_processed_total.get(), 14);
        assert_eq!(metrics.transactions_failed_total.get(), 1);
        assert_eq!(metrics.zk_proofs_observed_total.get(), 4);
        assert_eq!(metrics.slashing_events_total.with_label_values(&["invalid_reveal"]).get(), 2);
        assert_eq!(metrics.slashing_events_total.with_label_values(&["oracle_attestation_fraud"]).get(), 1);
    }

    #[test]
    fn inc_slashing_event_increments_correct_reason_label() {
        use crate::security::slashing::SlashingReason;
        let metrics = CoreMetrics::new();
        metrics.inc_slashing_event(&SlashingReason::InvalidZKProof);
        metrics.inc_slashing_event(&SlashingReason::InvalidZKProof);
        metrics.inc_slashing_event(&SlashingReason::DataQualityViolation);
        assert_eq!(metrics.slashing_events_total.with_label_values(&["invalid_zk_proof"]).get(), 2);
        assert_eq!(metrics.slashing_events_total.with_label_values(&["data_quality_violation"]).get(), 1);
        assert_eq!(metrics.slashing_events_total.with_label_values(&["invalid_reveal"]).get(), 0);
    }

    // ── #1148: chain fork / reorg / finality metric contract ────────────────
    //
    // These tests guard the 5 metrics referenced by
    // `infra/testnet/alert-rules-blockchain-health.yml`. A silent rename here
    // would break six operator alerts. The tests assert both registration
    // (the metric exists with the exact name) and semantics (the helper
    // methods update the right counters/gauges in the expected direction).

    #[test]
    fn chain_fork_and_reorg_metrics_are_registered_with_exact_names() {
        let metrics = CoreMetrics::new();
        metrics.admit_state_divergence_peer("registration-test-peer");
        let families: Vec<String> = metrics
            .registry
            .gather()
            .into_iter()
            .map(|mf| mf.name().to_string())
            .collect();
        // These exact names are the contract with
        // `infra/testnet/alert-rules-blockchain-health.yml`. Do NOT rename
        // without also updating the alert rules (and preferably a coordinated
        // operator migration).
        for required in [
            "wrfcoin_chain_height",
            "wrfcoin_chain_fork_count",
            "wrfcoin_chain_fork_active",
            "wrfcoin_chain_reorg_depth",
            "wrfcoin_chain_reorg_total",
            "wrfcoin_chain_finalized_height",
            "wrfcoin_chain_bft_finality_active",
            "wrfcoin_chain_bft_finalized_height",
            "wrfcoin_chain_bft_last_advance_timestamp_seconds",
            "wrfcoin_chain_malicious_validator_detected_total",
            "wrfcoin_chain_state_diverged",
            "wrfcoin_state_divergence_total",
        ] {
            assert!(
                families.iter().any(|name| name == required),
                "metric {required} must be registered in CoreMetrics — alert rules depend on the exact name"
            );
        }
    }

    #[test]
    fn inc_fork_observed_increments_cumulative_fork_counter() {
        let metrics = CoreMetrics::new();
        assert_eq!(metrics.chain_fork_count.get(), 0);
        metrics.inc_fork_observed();
        metrics.inc_fork_observed();
        metrics.inc_fork_observed();
        assert_eq!(metrics.chain_fork_count.get(), 3);
    }

    #[test]
    fn inc_reorg_bumps_total_sets_depth_and_clears_fork_active() {
        let metrics = CoreMetrics::new();
        // Start with an active fork (e.g., rejected for exceeding MAX_REORG_DEPTH).
        metrics.set_fork_active(true);
        assert_eq!(metrics.chain_fork_active.get(), 1);

        metrics.inc_reorg(3);
        assert_eq!(metrics.chain_reorg_total.get(), 1);
        assert_eq!(metrics.chain_reorg_depth.get(), 3);
        assert_eq!(
            metrics.chain_fork_active.get(),
            0,
            "successful reorg must clear chain_fork_active"
        );

        metrics.inc_reorg(7);
        assert_eq!(metrics.chain_reorg_total.get(), 2);
        assert_eq!(
            metrics.chain_reorg_depth.get(),
            7,
            "reorg_depth should track the MOST RECENT reorg, not the max"
        );

        // Depth is allowed to DECREASE (it reflects the last reorg only),
        // but reorg_total is monotonic.
        metrics.inc_reorg(1);
        assert_eq!(metrics.chain_reorg_total.get(), 3);
        assert_eq!(metrics.chain_reorg_depth.get(), 1);
    }

    #[test]
    fn bft_finality_active_starts_at_zero_and_latches_to_one() {
        // #1388 — the gauge is the canonical signal for "BFT has ever fired
        // on this node." It must start at 0 (so a silent inert node is
        // detectable by alerting) and latch at 1 once any FinalitySignal is
        // consumed. It must not decay back to 0.
        let metrics = CoreMetrics::new();
        assert_eq!(
            metrics.bft_finality_active.get(),
            0,
            "must start at 0 at node boot so the alert rule can detect inert BFT"
        );
        metrics.mark_bft_finality_active();
        assert_eq!(metrics.bft_finality_active.get(), 1);
        metrics.mark_bft_finality_active();
        metrics.mark_bft_finality_active();
        assert_eq!(
            metrics.bft_finality_active.get(),
            1,
            "mark_bft_finality_active must latch idempotently at 1"
        );
    }

    #[test]
    fn record_bft_advance_sets_height_and_timestamps_both_gauges() {
        // #1399 — bft_finalized_height must reflect the raw BFT height, and
        // bft_last_advance_timestamp must be set to a non-zero wall-clock value.
        let metrics = CoreMetrics::new();
        assert_eq!(metrics.bft_finalized_height.get(), 0, "must start at zero");
        assert_eq!(
            metrics.bft_last_advance_timestamp.get(),
            0,
            "must start at zero — alert rule uses time() - timestamp > 600"
        );

        let before = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        metrics.record_bft_advance(500);
        let after = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        assert_eq!(metrics.bft_finalized_height.get(), 500);
        let ts = metrics.bft_last_advance_timestamp.get() as u64;
        assert!(
            ts >= before && ts <= after,
            "timestamp must be within the current second window: got {ts}, before={before}, after={after}"
        );

        // Advancing again must overwrite both gauges.
        metrics.record_bft_advance(750);
        assert_eq!(metrics.bft_finalized_height.get(), 750);
    }

    #[test]
    fn inc_malicious_validator_detected_increments_counter() {
        // #1393 — the counter must be strictly cumulative. Operators alert
        // on `rate(...) > 0`, so a reset would mask a second attack within
        // the alert window.
        let metrics = CoreMetrics::new();
        assert_eq!(metrics.malicious_validator_detected_total.get(), 0);
        metrics.inc_malicious_validator_detected();
        metrics.inc_malicious_validator_detected();
        metrics.inc_malicious_validator_detected();
        assert_eq!(
            metrics.malicious_validator_detected_total.get(),
            3,
            "inc_malicious_validator_detected must be cumulative"
        );
    }

    #[test]
    fn set_fork_active_toggles_gauge_between_zero_and_one() {
        let metrics = CoreMetrics::new();
        assert_eq!(metrics.chain_fork_active.get(), 0);
        metrics.set_fork_active(true);
        assert_eq!(metrics.chain_fork_active.get(), 1);
        metrics.set_fork_active(false);
        assert_eq!(metrics.chain_fork_active.get(), 0);
        metrics.set_fork_active(true);
        metrics.set_fork_active(true);
        assert_eq!(metrics.chain_fork_active.get(), 1, "idempotent on repeat");
    }

    #[test]
    fn apply_snapshot_sets_chain_finalized_height_and_allows_it_to_advance() {
        let metrics = CoreMetrics::new();
        metrics.apply_snapshot(&CoreMetricSnapshot {
            block_height: 150,
            chain_finalized_height: 50, // 150 - 100 (MAX_REORG_DEPTH)
            ..Default::default()
        });
        assert_eq!(metrics.chain_finalized_height.get(), 50);

        // Gauge advances with the tip.
        metrics.apply_snapshot(&CoreMetricSnapshot {
            block_height: 200,
            chain_finalized_height: 100,
            ..Default::default()
        });
        assert_eq!(metrics.chain_finalized_height.get(), 100);

        // Unlike counters, gauges are allowed to report a smaller value on a
        // later snapshot (e.g., if the chain is rolled back or the definition
        // of finality changes). Pin this behavior so nobody accidentally adds
        // monotonic clamping to a gauge.
        metrics.apply_snapshot(&CoreMetricSnapshot {
            block_height: 190,
            chain_finalized_height: 90,
            ..Default::default()
        });
        assert_eq!(metrics.chain_finalized_height.get(), 90);
    }

    #[test]
    fn chain_fork_and_reorg_counters_are_monotonic_under_concurrent_use() {
        use std::sync::Arc;
        use std::thread;

        let metrics = Arc::new(CoreMetrics::new());
        let mut handles = Vec::new();
        for _ in 0..8 {
            let m = Arc::clone(&metrics);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    m.inc_fork_observed();
                    m.inc_reorg(5);
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(metrics.chain_fork_count.get(), 800);
        assert_eq!(metrics.chain_reorg_total.get(), 800);
        // Depth is the LAST setter's value — concurrent setters race, but the
        // important invariant is that the counter stays correct.
        assert_eq!(metrics.chain_reorg_depth.get(), 5);
    }

    #[test]
    fn state_pruning_last_height_metric_is_registered_and_set() {
        let metrics = CoreMetrics::new();
        // Verify the metric is registered with the exact name.
        let families: Vec<String> = metrics
            .registry
            .gather()
            .into_iter()
            .map(|mf| mf.name().to_string())
            .collect();
        assert!(
            families.iter().any(|n| n == "wrfcoin_state_pruning_last_height"),
            "wrfcoin_state_pruning_last_height must be registered"
        );

        // Verify snapshot sets it.
        metrics.apply_snapshot(&CoreMetricSnapshot {
            state_pruning_last_height: 42_000,
            ..Default::default()
        });
        assert_eq!(metrics.state_pruning_last_height.get(), 42_000);
    }

    #[test]
    fn state_diverged_metric_is_registered_and_tracks_snapshot() {
        let metrics = CoreMetrics::new();
        let families: Vec<String> = metrics
            .registry
            .gather()
            .into_iter()
            .map(|mf| mf.name().to_string())
            .collect();
        assert!(
            families.iter().any(|n| n == "wrfcoin_chain_state_diverged"),
            "wrfcoin_chain_state_diverged must be registered"
        );
        assert!(
            families.iter().any(|n| n == "wrfcoin_chain_height"),
            "wrfcoin_chain_height must be registered"
        );

        metrics.apply_snapshot(&CoreMetricSnapshot {
            state_diverged: true,
            block_height: 123,
            ..Default::default()
        });
        assert_eq!(metrics.state_diverged.get(), 1);
        assert_eq!(metrics.chain_height.get(), 123);

        metrics.apply_snapshot(&CoreMetricSnapshot {
            state_diverged: false,
            block_height: 124,
            ..Default::default()
        });
        assert_eq!(metrics.state_diverged.get(), 0);
        assert_eq!(metrics.chain_height.get(), 124);
    }

    /// #2464: the late-quorum-unpaid gauge must be registered so a scrape shows
    /// an explicit `0` rather than an absent series — an absent series is
    /// indistinguishable from "the detector never ran", which is exactly the
    /// observability defect the issue reports.
    #[test]
    fn oracle_late_quorum_unpaid_gauge_is_registered_and_settable() {
        let metrics = CoreMetrics::new();
        let families: Vec<String> = metrics
            .registry
            .gather()
            .into_iter()
            .map(|mf| mf.name().to_string())
            .collect();
        assert!(
            families
                .iter()
                .any(|n| n == "wrfcoin_oracle_late_quorum_unpaid"),
            "wrfcoin_oracle_late_quorum_unpaid must be registered so a healthy \
             chain reports an explicit 0"
        );
        assert_eq!(metrics.oracle_late_quorum_unpaid.get(), 0);

        metrics.set_oracle_late_quorum_unpaid(3);
        assert_eq!(metrics.oracle_late_quorum_unpaid.get(), 3);
        metrics.set_pop_deferred_real_contribution_reveals(12);
        assert_eq!(metrics.pop_deferred_real_contribution_reveals.get(), 12);
        metrics.set_pop_deferred_real_contribution_reveals(0);
        assert_eq!(metrics.pop_deferred_real_contribution_reveals.get(), 0);
        // Gauge semantics: a re-scrape that recomputes the same window must
        // overwrite, never accumulate.
        metrics.set_oracle_late_quorum_unpaid(3);
        assert_eq!(metrics.oracle_late_quorum_unpaid.get(), 3);
        metrics.set_oracle_late_quorum_unpaid(0);
        assert_eq!(metrics.oracle_late_quorum_unpaid.get(), 0);
    }

    #[test]
    fn node_ready_gauge_is_registered_and_defaults_to_not_ready() {
        let metrics = CoreMetrics::new();
        let families: Vec<String> = metrics
            .registry
            .gather()
            .into_iter()
            .map(|mf| mf.name().to_string())
            .collect();
        assert!(
            families.iter().any(|n| n == "wrfcoin_node_ready"),
            "wrfcoin_node_ready must be registered"
        );
        // Fail-closed default until the first /readyz evaluation runs.
        assert_eq!(metrics.node_ready.get(), 0);

        metrics.set_node_ready(true);
        assert_eq!(metrics.node_ready.get(), 1);
        metrics.set_node_ready(false);
        assert_eq!(metrics.node_ready.get(), 0);
    }

    /// Renders `metrics` to Prometheus exposition text — the same bytes a
    /// `/metrics` scrape returns.
    ///
    /// Every assertion below reads this text rather than the counter objects.
    /// That is deliberate: `IntCounterVec::with_label_values` **creates** a
    /// missing series as a side effect, so using it to test for absence always
    /// passes. Only the rendered output can prove a series is gone.
    fn render(metrics: &CoreMetrics) -> String {
        let mut encoded = Vec::new();
        TextEncoder::new()
            .encode(&metrics.registry.gather(), &mut encoded)
            .expect("metrics should encode");
        String::from_utf8(encoded).expect("metrics must be UTF-8")
    }

    /// **The #2734 defect-2 test.** The container healthcheck greps the
    /// exposition for `wrfcoin_state_divergence_total`. A labeled family with
    /// no series is omitted from the text encoding entirely, and per-peer
    /// series exist only after a `PeerChainStatus` — so a freshly restarted
    /// healthy node rendered NO such line and sat `unhealthy` while `/readyz`
    /// was 200. The family must be present from the very first scrape.
    #[test]
    fn state_divergence_family_is_present_before_any_peer_reports_2734() {
        let metrics = CoreMetrics::new();

        let exposition = render(&metrics);
        assert!(
            exposition.contains("wrfcoin_state_divergence_total"),
            "the family must appear in the exposition before any PeerChainStatus (#2734); got:\n{}",
            exposition
                .lines()
                .filter(|l| l.contains("divergence"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        assert!(
            exposition.contains(&format!(
                "wrfcoin_state_divergence_total{{peer_id=\"{STATE_DIVERGENCE_SENTINEL_PEER}\"}} 0"
            )),
            "the startup sentinel series must render as a stable zero"
        );
    }

    /// The sentinel must survive admission churn, ceiling evictions, and every
    /// release path — otherwise the empty-family window reopens.
    #[test]
    fn state_divergence_sentinel_survives_churn_eviction_and_release_2734() {
        let metrics = CoreMetrics::new();

        // Direct attempts to admit/release the sentinel label are no-ops.
        metrics.admit_state_divergence_peer(STATE_DIVERGENCE_SENTINEL_PEER);
        metrics.release_state_divergence_peer(STATE_DIVERGENCE_SENTINEL_PEER);
        assert_eq!(
            metrics.admitted_state_divergence_peer_count(),
            0,
            "the sentinel must never enter the admitted set"
        );

        // Blow past the eviction ceiling, then release everything.
        for i in 0..(MAX_STATE_DIVERGENCE_PEER_SERIES + 8) {
            metrics.admit_state_divergence_peer(&format!("churn-peer-{i}"));
        }
        for i in 0..(MAX_STATE_DIVERGENCE_PEER_SERIES + 8) {
            metrics.release_state_divergence_peer(&format!("churn-peer-{i}"));
        }

        let exposition = render(&metrics);
        assert!(
            exposition.contains(&format!(
                "wrfcoin_state_divergence_total{{peer_id=\"{STATE_DIVERGENCE_SENTINEL_PEER}\"}} 0"
            )),
            "sentinel must still render after churn + evictions + full release (#2734)"
        );
    }

    #[test]
    fn state_diverged_total_metric_is_registered_and_counts_events() {
        let metrics = CoreMetrics::new();
        metrics.admit_state_divergence_peer("peer-a");
        metrics.admit_state_divergence_peer("peer-b");

        let exposition = render(&metrics);
        assert!(
            exposition.contains("wrfcoin_state_divergence_total{peer_id=\"peer-a\"} 0"),
            "admitted peer-a must be exported at zero"
        );
        assert!(
            exposition.contains("wrfcoin_state_divergence_total{peer_id=\"peer-b\"} 0"),
            "admitted peer-b must be exported at zero"
        );

        let families: Vec<String> = metrics
            .registry
            .gather()
            .into_iter()
            .map(|mf| mf.name().to_string())
            .collect();
        assert!(
            families.iter().any(|n| n == "wrfcoin_chain_state_diverged_total"),
            "wrfcoin_chain_state_diverged_total must be registered"
        );
        assert!(
            families.iter().any(|n| n == "wrfcoin_state_divergence_total"),
            "wrfcoin_state_divergence_total must be registered"
        );

        metrics.inc_state_divergence_detected("peer-a");
        metrics.inc_state_divergence_detected("peer-a");
        metrics.inc_state_divergence_detected("peer-b");
        assert_eq!(metrics.state_diverged_total.get(), 3);

        let exposition = render(&metrics);
        assert!(
            exposition.contains("wrfcoin_state_divergence_total{peer_id=\"peer-a\"} 2"),
            "peer-a's two events must render against its own label, got:\n{exposition}"
        );
        assert!(
            exposition.contains("wrfcoin_state_divergence_total{peer_id=\"peer-b\"} 1"),
            "peer-b's single event must render against its own label, got:\n{exposition}"
        );
    }

    /// #2589: raw connection churn must not allocate per-peer series, and an
    /// admitted peer's series must be RELEASED on disconnect.
    ///
    /// Without release, `IntCounterVec` grows for the process lifetime and a
    /// peer reconnecting under fresh identities is a memory-exhaustion vector.
    #[test]
    fn preadmission_churn_allocates_no_series_and_disconnect_releases_them() {
        let metrics = CoreMetrics::new();

        // 1. Pre-admission churn: 50 distinct identities connect and are counted,
        //    but none is admitted.
        for _ in 0..50 {
            metrics.record_preadmission_peer_connection();
        }
        let exposition = render(&metrics);
        // #2734: the permanent startup sentinel is the one series allowed to
        // exist here — it is created at registration, not by churn.
        let churn_series = exposition
            .lines()
            .filter(|line| line.starts_with("wrfcoin_state_divergence_total{peer_id="))
            .filter(|line| !line.contains(&format!("peer_id=\"{STATE_DIVERGENCE_SENTINEL_PEER}\"")))
            .count();
        assert_eq!(
            churn_series, 0,
            "unadmitted connection churn must not allocate ANY peer_id series, got:\n{exposition}"
        );
        assert!(
            exposition.contains("wrfcoin_p2p_preadmission_connections_total 50"),
            "pre-admission connections must be counted in aggregate, got:\n{exposition}"
        );

        // 2. Admission creates exactly one series, at zero.
        metrics.admit_state_divergence_peer("admitted-peer");
        metrics.admit_state_divergence_peer("other-peer");
        let exposition = render(&metrics);
        assert!(
            exposition.contains("wrfcoin_state_divergence_total{peer_id=\"admitted-peer\"} 0"),
            "admission must create the series at zero, got:\n{exposition}"
        );

        // 3. Release removes it from the RENDERED output, leaving peers alone.
        metrics.release_state_divergence_peer("admitted-peer");
        let exposition = render(&metrics);
        assert!(
            !exposition.contains("peer_id=\"admitted-peer\""),
            "disconnect must remove the series from rendered /metrics, got:\n{exposition}"
        );
        assert!(
            exposition.contains("wrfcoin_state_divergence_total{peer_id=\"other-peer\"} 0"),
            "releasing one peer must not disturb another, got:\n{exposition}"
        );

        // 4. Releasing a never-admitted peer is a no-op, not a panic: the event
        //    loop calls release for every disconnect, admitted or not.
        metrics.release_state_divergence_peer("never-admitted");
    }

    /// #2589 hard bound: live per-peer series stay under the ceiling **when no
    /// release ever arrives**.
    ///
    /// This is the missed-teardown case, not the happy path. `PeerDisconnected`
    /// is sent with `let _ = event_tx.send(…)` in `p2p/src/node.rs` and consumed
    /// by a detached `tokio::spawn`, so a lost teardown is reachable at the event
    /// boundary. Admission-plus-release alone would leak one series every time
    /// one is dropped; the cap is what makes the bound independent of that path.
    #[test]
    fn admission_is_hard_capped_even_when_release_never_arrives() {
        let metrics = CoreMetrics::new();
        let overshoot = 25usize;

        // Admit past the ceiling. Not one release is issued — every teardown is
        // "lost", which is exactly the scenario the cap has to survive.
        for i in 0..MAX_STATE_DIVERGENCE_PEER_SERIES + overshoot {
            metrics.admit_state_divergence_peer(&format!("peer-{i:05}"));
        }

        let exposition = render(&metrics);
        // #2734: discount the permanent startup sentinel — the ceiling bounds
        // PEER series; the sentinel is a registration-time constant.
        let rendered_series = exposition
            .lines()
            .filter(|line| line.starts_with("wrfcoin_state_divergence_total{peer_id="))
            .filter(|line| !line.contains(&format!("peer_id=\"{STATE_DIVERGENCE_SENTINEL_PEER}\"")))
            .count();

        assert!(
            rendered_series <= MAX_STATE_DIVERGENCE_PEER_SERIES,
            "live series must never exceed the ceiling ({MAX_STATE_DIVERGENCE_PEER_SERIES}); \
             rendered {rendered_series} -- the bound is not independent of teardown"
        );
        assert_eq!(
            metrics.admitted_state_divergence_peer_count(),
            MAX_STATE_DIVERGENCE_PEER_SERIES,
            "registry must saturate at the ceiling, not grow"
        );

        // The eviction counter is the operator's signal that teardowns are being
        // lost. It must be non-zero here and must itself be exported.
        assert_eq!(
            metrics.state_divergence_peer_evictions_total.get(),
            overshoot as u64,
            "every admission past the ceiling must evict exactly one older peer"
        );
        assert!(
            exposition.contains("wrfcoin_state_divergence_peer_evictions_total"),
            "the eviction counter must be exported so the cap engaging is observable"
        );

        // Both directions, in rendered text: the oldest is gone, the newest kept.
        assert!(
            !exposition.contains("peer_id=\"peer-00000\""),
            "the oldest admitted peer must have been evicted; got exposition of {rendered_series} series"
        );
        let newest = format!("peer-{:05}", MAX_STATE_DIVERGENCE_PEER_SERIES + overshoot - 1);
        assert!(
            exposition.contains(&format!("wrfcoin_state_divergence_total{{peer_id=\"{newest}\"}} 0")),
            "the most recently admitted peer must still be rendered"
        );
    }

    /// Re-admitting an already-admitted peer must not consume a slot or evict.
    /// `admit_state_divergence_peer` runs on EVERY `PeerChainStatus`, so a
    /// non-idempotent implementation would churn the whole registry under a
    /// single chatty peer.
    #[test]
    fn readmitting_a_known_peer_neither_evicts_nor_grows_the_registry() {
        let metrics = CoreMetrics::new();
        metrics.admit_state_divergence_peer("stable-peer");
        metrics.admit_state_divergence_peer("second-peer");

        for _ in 0..(MAX_STATE_DIVERGENCE_PEER_SERIES * 2) {
            metrics.admit_state_divergence_peer("stable-peer");
        }

        assert_eq!(metrics.admitted_state_divergence_peer_count(), 2);
        assert_eq!(
            metrics.state_divergence_peer_evictions_total.get(),
            0,
            "re-admission must never evict"
        );
        let exposition = render(&metrics);
        assert!(exposition.contains("peer_id=\"second-peer\""), "got:\n{exposition}");
    }

    #[test]
    fn apply_p2p_snapshot_syncs_counters_monotonically() {
        let metrics = CoreMetrics::new();

        metrics.apply_p2p_snapshot(&P2pBandwidthSnapshot {
            bytes_received: [100, 200, 300, 50],
            bytes_sent: [80, 150, 250, 40],
            messages_received: [10, 20, 30, 5],
            messages_sent: [8, 15, 25, 4],
        });
        assert_eq!(metrics.p2p_bytes_received_total.with_label_values(&["blocks"]).get(), 100);
        assert_eq!(metrics.p2p_bytes_received_total.with_label_values(&["weather"]).get(), 300);
        assert_eq!(metrics.p2p_bytes_sent_total.with_label_values(&["transactions"]).get(), 150);
        assert_eq!(metrics.p2p_messages_total.with_label_values(&["forecast", "received"]).get(), 5);
        assert_eq!(metrics.p2p_messages_total.with_label_values(&["blocks", "sent"]).get(), 8);

        // Apply larger snapshot — counters should increase.
        metrics.apply_p2p_snapshot(&P2pBandwidthSnapshot {
            bytes_received: [200, 400, 600, 100],
            bytes_sent: [160, 300, 500, 80],
            messages_received: [20, 40, 60, 10],
            messages_sent: [16, 30, 50, 8],
        });
        assert_eq!(metrics.p2p_bytes_received_total.with_label_values(&["blocks"]).get(), 200);
        assert_eq!(metrics.p2p_bytes_sent_total.with_label_values(&["weather"]).get(), 500);
        assert_eq!(metrics.p2p_messages_total.with_label_values(&["transactions", "sent"]).get(), 30);

        // Apply smaller snapshot — counters must NOT decrease.
        metrics.apply_p2p_snapshot(&P2pBandwidthSnapshot {
            bytes_received: [50, 100, 150, 25],
            bytes_sent: [40, 75, 125, 20],
            messages_received: [5, 10, 15, 3],
            messages_sent: [4, 8, 13, 2],
        });
        assert_eq!(metrics.p2p_bytes_received_total.with_label_values(&["blocks"]).get(), 200);
        assert_eq!(metrics.p2p_bytes_sent_total.with_label_values(&["weather"]).get(), 500);
    }

    #[test]
    fn p2p_mesh_metric_is_registered_with_exact_name() {
        let metrics = CoreMetrics::new();
        let families: Vec<String> = metrics
            .registry
            .gather()
            .into_iter()
            .map(|mf| mf.name().to_string())
            .collect();
        assert!(
            families
                .iter()
                .any(|name| name == "wrfcoin_p2p_gossipsub_mesh_peers"),
            "mesh peers metric must be registered under the exact contract name"
        );
    }

    #[test]
    fn apply_p2p_mesh_snapshot_sets_gauges_per_topic() {
        let metrics = CoreMetrics::new();

        metrics.apply_p2p_mesh_snapshot(&P2pMeshSnapshot {
            mesh_peers: [1, 2, 3, 4],
        });
        assert_eq!(metrics.p2p_mesh_peers.with_label_values(&["blocks"]).get(), 1);
        assert_eq!(metrics.p2p_mesh_peers.with_label_values(&["transactions"]).get(), 2);
        assert_eq!(metrics.p2p_mesh_peers.with_label_values(&["weather"]).get(), 3);
        assert_eq!(metrics.p2p_mesh_peers.with_label_values(&["forecast"]).get(), 4);
    }

    #[test]
    fn p2p_sync_gauges_expose_heights_state_and_outstanding_ranges_2796() {
        let metrics = CoreMetrics::new();
        metrics.set_p2p_sync(11_016, 12_801, "downloading", 2, 3, 5);

        assert_eq!(metrics.p2p_sync_height.with_label_values(&["local"]).get(), 11_016);
        assert_eq!(metrics.p2p_sync_height.with_label_values(&["target"]).get(), 12_801);
        assert_eq!(metrics.p2p_sync_state.with_label_values(&["downloading"]).get(), 1);
        assert_eq!(metrics.p2p_sync_state.with_label_values(&["idle"]).get(), 0);
        assert_eq!(metrics.p2p_sync_ranges.with_label_values(&["outstanding"]).get(), 5);

        let exposition = render(&metrics);
        for name in [
            "wrfcoin_p2p_sync_height",
            "wrfcoin_p2p_sync_state",
            "wrfcoin_p2p_sync_ranges",
        ] {
            assert!(exposition.contains(name), "missing {name} from:\n{exposition}");
        }
    }

    // ── #1622 Chunk 1: Chain + WAL metrics ──────────────────────────────────

    #[test]
    fn chunk1_metrics_are_registered_with_exact_names() {
        let metrics = CoreMetrics::new();
        let families: Vec<String> = metrics
            .registry
            .gather()
            .into_iter()
            .map(|mf| mf.name().to_string())
            .collect();
        for required in [
            "wrfcoin_tx_pool_size",
            "wrfcoin_block_production_total",
            "wrfcoin_block_production_duration_seconds",
            "wrfcoin_pop_liveness_path_total",
            "wrfcoin_wal_replay_status",
            "wrfcoin_wal_replay_duration_seconds",
            "wrfcoin_wal_entries_replayed_total",
        ] {
            assert!(
                families.iter().any(|n| n == required),
                "metric {required} must be registered (#1622 chunk 1)"
            );
        }
    }

    #[test]
    fn tx_pool_size_mirrors_mempool_size_in_snapshot() {
        let metrics = CoreMetrics::new();
        metrics.apply_snapshot(&CoreMetricSnapshot {
            mempool_size: 42,
            ..Default::default()
        });
        assert_eq!(metrics.tx_pool_size.get(), 42);
        assert_eq!(metrics.mempool_size.get(), 42);

        metrics.apply_snapshot(&CoreMetricSnapshot {
            mempool_size: 0,
            ..Default::default()
        });
        assert_eq!(metrics.tx_pool_size.get(), 0);
    }

    #[test]
    fn wal_replay_status_starts_healthy_and_can_be_set_to_error() {
        let metrics = CoreMetrics::new();
        assert_eq!(
            metrics.wal_replay_status.get(),
            1,
            "wal_replay_status must default to 1 (healthy) — fresh/clean-checkpoint node should not read as error"
        );
        metrics.wal_replay_status.set(0);
        assert_eq!(metrics.wal_replay_status.get(), 0);
        metrics.wal_replay_status.set(1);
        assert_eq!(metrics.wal_replay_status.get(), 1);
    }

    #[test]
    fn wal_replay_duration_and_entries_are_observable() {
        let metrics = CoreMetrics::new();
        assert_eq!(metrics.wal_entries_replayed_total.get(), 0);
        metrics.wal_entries_replayed_total.inc_by(137);
        assert_eq!(metrics.wal_entries_replayed_total.get(), 137);

        metrics.wal_replay_duration_seconds.observe(0.42);
        let sample_count = metrics
            .wal_replay_duration_seconds
            .get_sample_count();
        assert_eq!(sample_count, 1, "wal_replay_duration_seconds must record observations");
    }

    #[test]
    fn block_production_total_pre_initialized_and_incrementable() {
        let metrics = CoreMetrics::new();
        // pop label must be pre-initialized at zero.
        assert_eq!(
            metrics.block_production_total.with_label_values(&["pop"]).get(),
            0
        );
        metrics.block_production_total.with_label_values(&["pop"]).inc();
        metrics.block_production_total.with_label_values(&["pop"]).inc();
        assert_eq!(
            metrics.block_production_total.with_label_values(&["pop"]).get(),
            2
        );
    }

    #[test]
    fn pop_liveness_path_total_pre_initialized_for_all_known_paths() {
        let metrics = CoreMetrics::new();
        for path in ["publishing_score", "useful_work", "empty_liveness"] {
            assert_eq!(
                metrics.pop_liveness_path_total.with_label_values(&[path]).get(),
                0,
                "path={path} must be pre-initialized to 0 in /metrics before first block"
            );
        }
        metrics.pop_liveness_path_total.with_label_values(&["useful_work"]).inc();
        assert_eq!(
            metrics.pop_liveness_path_total.with_label_values(&["useful_work"]).get(),
            1
        );
    }

    #[test]
    fn block_production_duration_seconds_records_observations() {
        let metrics = CoreMetrics::new();
        assert_eq!(metrics.block_production_duration_seconds.get_sample_count(), 0);
        metrics.block_production_duration_seconds.observe(0.123);
        metrics.block_production_duration_seconds.observe(0.456);
        assert_eq!(
            metrics.block_production_duration_seconds.get_sample_count(),
            2
        );
    }

    #[test]
    fn ordering_metrics_distinguish_no_data_from_censorship_signal_2199() {
        let metrics = CoreMetrics::new();
        let empty = Block::new(3, Hash::default(), vec![], 1, 1);
        metrics.record_block_ordering(&empty, &OrderingMempoolSnapshot::default(), "p");
        assert_eq!(metrics.ordering_assessment_available.get(), 0, "empty is no data, never fair");
        assert_eq!(metrics.ordering_no_data_total.get(), 1);
        let included_id = Hash::hash_bytes(b"included");
        let excluded_id = Hash::hash_bytes(b"excluded");
        let mut tx = crate::blockchain::transaction::Transaction::new(vec![], vec![]);
        tx.id = Some(included_id);
        tx.fee = 10;
        let block = Block::new(3, Hash::default(), vec![tx], 1, 1);
        let snapshot = OrderingMempoolSnapshot { entries: vec![
            OrderingMempoolEntry { tx_id: included_id, fee: 10, size: 1, arrival_rank: 0 },
            OrderingMempoolEntry { tx_id: excluded_id, fee: 10, size: 1, arrival_rank: 1 },
        ]};
        metrics.record_block_ordering(&block, &snapshot, "producer-a");
        assert_eq!(metrics.ordering_assessment_available.get(), 1);
        assert_eq!(metrics.ordering_reorder_distance.get_sample_count(), 1);
        assert_eq!(metrics.ordering_excluded_sufficient_fee_total.with_label_values(&["producer-a"]).get(), 1);
    }

    #[test]
    fn ordering_metrics_use_fee_rate_for_unequal_transaction_sizes_2593() {
        fn tx_with_padding(padding: usize, fee: u64) -> crate::blockchain::transaction::Transaction {
            let mut tx = crate::blockchain::transaction::Transaction::new(
                vec![],
                vec![crate::blockchain::transaction::TransactionOutput {
                    amount: 1,
                    address: "a".repeat(padding),
                    script: "OP_TRUE".to_string(),
                    nft_data: None,
                }],
            );
            tx.fee = fee;
            tx.set_id();
            tx
        }

        let included_low_rate = tx_with_padding(600, 100);
        let included_high_rate = tx_with_padding(0, 200);
        let excluded_lower_rate = tx_with_padding(2_000, 101);
        let low_rate = crate::mempool::pool::Mempool::fee_rate_milli(
            included_low_rate.fee,
            included_low_rate.size(),
        );
        let high_rate = crate::mempool::pool::Mempool::fee_rate_milli(
            included_high_rate.fee,
            included_high_rate.size(),
        );
        let excluded_rate = crate::mempool::pool::Mempool::fee_rate_milli(
            excluded_lower_rate.fee,
            excluded_lower_rate.size(),
        );
        assert!(low_rate < high_rate, "block transactions must have unequal fee rates");
        assert!(excluded_lower_rate.fee >= included_low_rate.fee);
        assert!(excluded_rate < low_rate, "raw-fee and fee-rate ordering must disagree");

        let included_low_rate_size = included_low_rate.size();
        let included_high_rate_size = included_high_rate.size();
        let included_low_rate_id = included_low_rate.id.expect("test tx id");
        let included_high_rate_id = included_high_rate.id.expect("test tx id");
        let excluded_lower_rate_id = excluded_lower_rate.id.expect("test tx id");
        let block = Block::new(
            3,
            Hash::default(),
            vec![included_low_rate, included_high_rate],
            1,
            1,
        );
        let snapshot = OrderingMempoolSnapshot {
            entries: vec![
                OrderingMempoolEntry {
                    tx_id: included_low_rate_id,
                    fee: 100,
                    size: included_low_rate_size,
                    arrival_rank: 0,
                },
                OrderingMempoolEntry {
                    tx_id: included_high_rate_id,
                    fee: 200,
                    size: included_high_rate_size,
                    arrival_rank: 1,
                },
                OrderingMempoolEntry {
                    tx_id: excluded_lower_rate_id,
                    fee: excluded_lower_rate.fee,
                    size: excluded_lower_rate.size(),
                    arrival_rank: 2,
                },
            ],
        };
        let metrics = CoreMetrics::new();
        metrics.record_block_ordering(&block, &snapshot, "producer-size-test");

        assert_eq!(
            metrics
                .ordering_excluded_sufficient_fee_total
                .with_label_values(&["producer-size-test"])
                .get(),
            0,
            "a lower fee-rate candidate must not be counted as sufficient-fee exclusion"
        );
    }
}
