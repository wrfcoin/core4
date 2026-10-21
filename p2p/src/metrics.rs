//! Lightweight in-process network metrics.
//!
//! These counters are updated on every gossipsub publish / receive and can be
//! polled by the host application via [`P2pHandle`](crate::node::P2pHandle).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Per-topic byte and message counts in a snapshot.
#[derive(Debug, Clone, Default)]
pub struct TopicStats {
    pub bytes: u64,
    pub messages: u64,
}

/// Aggregate network statistics for a running P2P node.
#[derive(Debug, Clone, Default)]
pub struct NetworkStats {
    /// Number of peers currently connected.
    pub connected_peers: usize,
    /// Per-topic gossipsub mesh peer counts.
    pub mesh_peers_by_topic: BTreeMap<String, usize>,
    /// Total blocks received from the gossipsub overlay.
    pub blocks_received: u64,
    /// Total blocks we published.
    pub blocks_published: u64,
    /// Total transactions received.
    pub txs_received: u64,
    /// Total transactions we published.
    pub txs_published: u64,
    /// Total weather-data messages received.
    pub weather_msgs_received: u64,
    /// Total weather-data messages we published.
    pub weather_msgs_published: u64,
    /// Total weather-data messages dropped due to invalid station signature.
    pub weather_msgs_invalid_sig: u64,
    /// Total forecast-task messages received.
    pub forecast_tasks_received: u64,
    /// Total forecast-task messages we published.
    pub forecast_tasks_published: u64,
    /// Total BFT finality votes received from peers (#1388).
    pub finality_votes_received: u64,
    /// Total BFT finality votes we published (#1388).
    pub finality_votes_published: u64,
    /// Total BFT finality certificates received from peers (#1388).
    pub finality_certificates_received: u64,
    /// Total BFT finality certificates we published (#1388).
    pub finality_certificates_published: u64,
    /// Approximate total inbound bytes processed.
    pub bytes_inbound: u64,
    /// Approximate total outbound bytes produced.
    pub bytes_outbound: u64,
    /// Per-topic inbound stats (blocks, transactions, weather, forecast).
    pub inbound_blocks: TopicStats,
    pub inbound_txs: TopicStats,
    pub inbound_weather: TopicStats,
    pub inbound_forecast: TopicStats,
    /// Per-topic outbound stats.
    pub outbound_blocks: TopicStats,
    pub outbound_txs: TopicStats,
    pub outbound_weather: TopicStats,
    pub outbound_forecast: TopicStats,
    /// Messages rejected by validation or a terminal bounded publish outcome.
    pub messages_rejected: u64,
    /// Outbound gossip intents retained until their topic has a publishable mesh (#2790).
    pub pending_broadcasts: usize,
    /// Serialized bytes held by the bounded pending-broadcast queue (#2790).
    pub pending_broadcast_bytes: usize,
    /// New intents admitted to the pending-broadcast queue (#2790).
    pub pending_broadcasts_enqueued: u64,
    /// Identical pending intents coalesced rather than allocated twice (#2790).
    pub pending_broadcasts_deduplicated: u64,
    /// Pending publish attempts made after an actual mesh-readiness check (#2790).
    pub pending_broadcast_retry_attempts: u64,
    /// Pending intents eventually published after mesh readiness (#2790).
    pub pending_broadcasts_published: u64,
    /// New intents rejected by the deterministic item/byte ceiling (#2790).
    pub pending_broadcasts_overflowed: u64,
    /// Pending intents dropped after reaching their retry ceiling (#2790).
    pub pending_broadcasts_exhausted: u64,
    /// Pending intents dropped after reaching their age ceiling (#2790).
    pub pending_broadcasts_expired: u64,
    /// Successful bootstrap-peer redial attempts (#1668).
    pub bootstrap_redial_attempts_success: u64,
    /// Failed bootstrap-peer redial attempts (#1668). A failure is a libp2p
    /// `Swarm::dial` error OR an `OutgoingConnectionError` for a bootstrap-peer
    /// dial we initiated via the redial loop.
    pub bootstrap_redial_attempts_fail: u64,
    /// Every failed outgoing dial, whatever the target (#2819). Before this
    /// counter, only bootstrap-peer dial failures were counted, so the
    /// unroutable-advertised-address churn (44 failed dials/hour on the
    /// workstation node) was invisible outside debug logs.
    pub outgoing_dial_failures: u64,
    /// Genesis handshakes republished when a peer subscribed to the handshake
    /// topic, closing the publish/subscribe race (#2544).
    pub genesis_handshake_republished: u64,
    /// Outbound dial candidates refused for concentrating the outbound peer set
    /// in one IPv4 `/24` (#2114).
    pub outbound_dials_rejected_subnet: u64,
    /// Outbound dial candidates refused for over-reliance on one discovery
    /// source (#2114).
    pub outbound_dials_rejected_source: u64,
    /// Current number of outbound (self-dialed) peers (#2114).
    pub outbound_peers: usize,
    /// Distinct IPv4 `/24`s represented in the current outbound peer set. The
    /// observable quantity an eclipse drives toward 1 (#2114).
    pub outbound_distinct_subnets: usize,
    /// Whether the outbound set is currently below `min_distinct_subnets`.
    pub outbound_low_diversity: bool,
    /// Number of times the outbound set has *entered* the low-diversity
    /// condition. A counter rather than only a flag, so a flapping eclipse is
    /// still visible after it clears (#2114).
    pub outbound_low_diversity_events: u64,
    /// Pending inbound connections refused for exceeding the per-source-IP
    /// concurrent-connection quota (#2113).
    pub conn_rejected_per_ip: u64,
    /// Pending inbound connections refused for exceeding the per-IPv4-`/24`
    /// concurrent-connection quota (#2113).
    pub conn_rejected_ipv4_subnet: u64,
    /// Pending inbound connections refused for exceeding the per-IPv6-`/64`
    /// concurrent-connection quota (#2113).
    pub conn_rejected_ipv6_subnet: u64,
    /// Signature verifications skipped because the peer exceeded its
    /// pre-authentication verification budget (#2117).
    pub preauth_verify_skipped: u64,
    /// Peer admissions first observed crossing the handshake deadline (#2116).
    /// Repeated sweeps of the same still-connected peer do not increment it.
    pub handshake_timeouts: u64,
    /// Connections refused because the source address already held its
    /// allowance of concurrently-unproven peers (#2116).
    pub handshake_concurrent_per_ip_drops: u64,
    /// #2120: inbound peers refused by a validator's sentry allowlist.
    pub sentry_allowlist_refusals: u64,
    /// Peers currently connected but not yet proven to be on our chain (#2116).
    pub unproven_peers: usize,
    /// Seconds since the node started.
    pub uptime_secs: u64,
    /// Authoritative local height used by the block downloader (#2796).
    pub sync_local_height: u64,
    /// Highest bounded peer height currently targeted by the downloader.
    pub sync_target_height: u64,
    /// Stable downloader state: idle/discovering/downloading/stalled/complete.
    pub sync_state: String,
    /// Queued or retry-deferred block ranges not yet dispatched.
    pub sync_pending_ranges: usize,
    /// Block ranges currently awaiting peer responses.
    pub sync_in_flight_ranges: usize,
    /// Total queued/deferred plus in-flight ranges.
    pub sync_outstanding_ranges: usize,
    /// Outbound block-range requests registered with the sync manager.
    pub sync_requests_dispatched: u64,
    /// Registered block-range responses received from peers.
    pub sync_responses_received: u64,
    /// Requester-side transport failures, including local stall expiry.
    pub sync_transport_failures: u64,
    /// Failed or partial ranges returned to the pending queue.
    pub sync_ranges_requeued: u64,
    /// Failed ranges deliberately reassigned to a different peer.
    pub sync_peer_rotations: u64,
    /// Forced connection-refresh attempts after repeated block-sync timeouts.
    pub sync_connection_refresh_attempts: u64,
    /// Planning attempts deferred because eligible peers were cooling down.
    pub sync_cooldown_deferrals: u64,
    /// Last-resort dispatches to a banned peer because no un-banned
    /// candidate existed for a needed range (#2807 starvation floor).
    pub sync_last_resort_dispatches: u64,
    /// Current ChainStatus broadcasts triggered after reconnect proof.
    pub sync_reconnect_status_broadcasts: u64,
    /// Block-range requests received by the responder path.
    pub sync_inbound_requests: u64,
    /// Block-range responses handed back to libp2p successfully.
    pub sync_responses_sent: u64,
    /// Responder-side inbound request failures.
    pub sync_inbound_failures: u64,
    /// Responses the responder path failed to hand back to libp2p.
    pub sync_response_send_failures: u64,
}

/// Internal atomic counters used by the event loop.
///
/// The event loop bumps these atomics; a snapshot can be taken at any time
/// without locking.
pub(crate) struct MetricsCollector {
    pub connected_peers: AtomicUsize,
    pub mesh_peers_blocks: AtomicUsize,
    pub mesh_peers_transactions: AtomicUsize,
    pub mesh_peers_weather: AtomicUsize,
    pub mesh_peers_forecast: AtomicUsize,
    pub blocks_received: AtomicU64,
    pub blocks_published: AtomicU64,
    pub txs_received: AtomicU64,
    pub txs_published: AtomicU64,
    pub weather_msgs_received: AtomicU64,
    pub weather_msgs_published: AtomicU64,
    pub weather_msgs_invalid_sig: AtomicU64,
    pub forecast_tasks_received: AtomicU64,
    pub forecast_tasks_published: AtomicU64,
    pub finality_votes_received: AtomicU64,
    pub finality_votes_published: AtomicU64,
    pub finality_certificates_received: AtomicU64,
    pub finality_certificates_published: AtomicU64,
    pub bytes_inbound: AtomicU64,
    pub bytes_outbound: AtomicU64,
    // Per-topic inbound bytes
    pub bytes_inbound_blocks: AtomicU64,
    pub bytes_inbound_txs: AtomicU64,
    pub bytes_inbound_weather: AtomicU64,
    pub bytes_inbound_forecast: AtomicU64,
    // Per-topic outbound bytes
    pub bytes_outbound_blocks: AtomicU64,
    pub bytes_outbound_txs: AtomicU64,
    pub bytes_outbound_weather: AtomicU64,
    pub bytes_outbound_forecast: AtomicU64,
    pub messages_rejected: AtomicU64,
    pub pending_broadcasts: AtomicUsize,
    pub pending_broadcast_bytes: AtomicUsize,
    pub pending_broadcasts_enqueued: AtomicU64,
    pub pending_broadcasts_deduplicated: AtomicU64,
    pub pending_broadcast_retry_attempts: AtomicU64,
    pub pending_broadcasts_published: AtomicU64,
    pub pending_broadcasts_overflowed: AtomicU64,
    pub pending_broadcasts_exhausted: AtomicU64,
    pub pending_broadcasts_expired: AtomicU64,
    // Bootstrap auto-redial counters (#1668).
    pub bootstrap_redial_attempts_success: AtomicU64,
    pub bootstrap_redial_attempts_fail: AtomicU64,
    pub outgoing_dial_failures: AtomicU64,
    pub genesis_handshake_republished: AtomicU64,
    // Outbound slot separation / diversity (#2114).
    pub outbound_dials_rejected_subnet: AtomicU64,
    pub outbound_dials_rejected_source: AtomicU64,
    pub outbound_peers: AtomicUsize,
    pub outbound_distinct_subnets: AtomicUsize,
    pub outbound_low_diversity: AtomicBool,
    pub outbound_low_diversity_events: AtomicU64,
    // Per-reason connection-quota rejections (#2113). One counter per reason so
    // an operator can tell which ceiling is biting, not merely that one is.
    pub conn_rejected_per_ip: AtomicU64,
    pub conn_rejected_ipv4_subnet: AtomicU64,
    pub conn_rejected_ipv6_subnet: AtomicU64,
    /// Pre-authentication budget refusals (#2117).
    pub preauth_verify_skipped: AtomicU64,
    // Unproven-peer bounds (#2116).
    pub handshake_timeouts: AtomicU64,
    pub handshake_concurrent_per_ip_drops: AtomicU64,
    pub sentry_allowlist_refusals: AtomicU64,
    pub unproven_peers: AtomicUsize,
    pub sync_local_height: AtomicU64,
    pub sync_target_height: AtomicU64,
    pub sync_state: AtomicU64,
    pub sync_pending_ranges: AtomicUsize,
    pub sync_in_flight_ranges: AtomicUsize,
    pub sync_requests_dispatched: AtomicU64,
    pub sync_responses_received: AtomicU64,
    pub sync_transport_failures: AtomicU64,
    pub sync_ranges_requeued: AtomicU64,
    pub sync_peer_rotations: AtomicU64,
    pub sync_connection_refresh_attempts: AtomicU64,
    pub sync_cooldown_deferrals: AtomicU64,
    pub sync_last_resort_dispatches: AtomicU64,
    pub sync_reconnect_status_broadcasts: AtomicU64,
    pub sync_inbound_requests: AtomicU64,
    pub sync_responses_sent: AtomicU64,
    pub sync_inbound_failures: AtomicU64,
    pub sync_response_send_failures: AtomicU64,
    start: Instant,
}

impl MetricsCollector {
    /// Create a fresh collector (all counters zero).
    pub fn new() -> Self {
        Self {
            connected_peers: AtomicUsize::new(0),
            mesh_peers_blocks: AtomicUsize::new(0),
            mesh_peers_transactions: AtomicUsize::new(0),
            mesh_peers_weather: AtomicUsize::new(0),
            mesh_peers_forecast: AtomicUsize::new(0),
            blocks_received: AtomicU64::new(0),
            blocks_published: AtomicU64::new(0),
            txs_received: AtomicU64::new(0),
            txs_published: AtomicU64::new(0),
            weather_msgs_received: AtomicU64::new(0),
            weather_msgs_published: AtomicU64::new(0),
            weather_msgs_invalid_sig: AtomicU64::new(0),
            forecast_tasks_received: AtomicU64::new(0),
            forecast_tasks_published: AtomicU64::new(0),
            finality_votes_received: AtomicU64::new(0),
            finality_votes_published: AtomicU64::new(0),
            finality_certificates_received: AtomicU64::new(0),
            finality_certificates_published: AtomicU64::new(0),
            bytes_inbound: AtomicU64::new(0),
            bytes_outbound: AtomicU64::new(0),
            bytes_inbound_blocks: AtomicU64::new(0),
            bytes_inbound_txs: AtomicU64::new(0),
            bytes_inbound_weather: AtomicU64::new(0),
            bytes_inbound_forecast: AtomicU64::new(0),
            bytes_outbound_blocks: AtomicU64::new(0),
            bytes_outbound_txs: AtomicU64::new(0),
            bytes_outbound_weather: AtomicU64::new(0),
            bytes_outbound_forecast: AtomicU64::new(0),
            messages_rejected: AtomicU64::new(0),
            pending_broadcasts: AtomicUsize::new(0),
            pending_broadcast_bytes: AtomicUsize::new(0),
            pending_broadcasts_enqueued: AtomicU64::new(0),
            pending_broadcasts_deduplicated: AtomicU64::new(0),
            pending_broadcast_retry_attempts: AtomicU64::new(0),
            pending_broadcasts_published: AtomicU64::new(0),
            pending_broadcasts_overflowed: AtomicU64::new(0),
            pending_broadcasts_exhausted: AtomicU64::new(0),
            pending_broadcasts_expired: AtomicU64::new(0),
            bootstrap_redial_attempts_success: AtomicU64::new(0),
            bootstrap_redial_attempts_fail: AtomicU64::new(0),
            outgoing_dial_failures: AtomicU64::new(0),
            genesis_handshake_republished: AtomicU64::new(0),
            outbound_dials_rejected_subnet: AtomicU64::new(0),
            outbound_dials_rejected_source: AtomicU64::new(0),
            outbound_peers: AtomicUsize::new(0),
            outbound_distinct_subnets: AtomicUsize::new(0),
            outbound_low_diversity: AtomicBool::new(false),
            outbound_low_diversity_events: AtomicU64::new(0),
            conn_rejected_per_ip: AtomicU64::new(0),
            conn_rejected_ipv4_subnet: AtomicU64::new(0),
            conn_rejected_ipv6_subnet: AtomicU64::new(0),
            preauth_verify_skipped: AtomicU64::new(0),
            handshake_timeouts: AtomicU64::new(0),
            handshake_concurrent_per_ip_drops: AtomicU64::new(0),
            sentry_allowlist_refusals: AtomicU64::new(0),
            unproven_peers: AtomicUsize::new(0),
            sync_local_height: AtomicU64::new(0),
            sync_target_height: AtomicU64::new(0),
            sync_state: AtomicU64::new(sync_state_code("idle")),
            sync_pending_ranges: AtomicUsize::new(0),
            sync_in_flight_ranges: AtomicUsize::new(0),
            sync_requests_dispatched: AtomicU64::new(0),
            sync_responses_received: AtomicU64::new(0),
            sync_transport_failures: AtomicU64::new(0),
            sync_ranges_requeued: AtomicU64::new(0),
            sync_peer_rotations: AtomicU64::new(0),
            sync_connection_refresh_attempts: AtomicU64::new(0),
            sync_cooldown_deferrals: AtomicU64::new(0),
            sync_last_resort_dispatches: AtomicU64::new(0),
            sync_reconnect_status_broadcasts: AtomicU64::new(0),
            sync_inbound_requests: AtomicU64::new(0),
            sync_responses_sent: AtomicU64::new(0),
            sync_inbound_failures: AtomicU64::new(0),
            sync_response_send_failures: AtomicU64::new(0),
            start: Instant::now(),
        }
    }

    /// Rehydrate a collector from a snapshot, used by tests that need a
    /// lightweight `P2pClient` without running the swarm.
    pub fn from_snapshot(snapshot: &NetworkStats) -> Self {
        let start = Instant::now()
            .checked_sub(Duration::from_secs(snapshot.uptime_secs))
            .unwrap_or_else(Instant::now);
        Self {
            connected_peers: AtomicUsize::new(snapshot.connected_peers),
            mesh_peers_blocks: AtomicUsize::new(
                *snapshot.mesh_peers_by_topic.get("blocks").unwrap_or(&0),
            ),
            mesh_peers_transactions: AtomicUsize::new(
                *snapshot.mesh_peers_by_topic.get("transactions").unwrap_or(&0),
            ),
            mesh_peers_weather: AtomicUsize::new(
                *snapshot.mesh_peers_by_topic.get("weather").unwrap_or(&0),
            ),
            mesh_peers_forecast: AtomicUsize::new(
                *snapshot.mesh_peers_by_topic.get("forecast").unwrap_or(&0),
            ),
            blocks_received: AtomicU64::new(snapshot.blocks_received),
            blocks_published: AtomicU64::new(snapshot.blocks_published),
            txs_received: AtomicU64::new(snapshot.txs_received),
            txs_published: AtomicU64::new(snapshot.txs_published),
            weather_msgs_received: AtomicU64::new(snapshot.weather_msgs_received),
            weather_msgs_published: AtomicU64::new(snapshot.weather_msgs_published),
            weather_msgs_invalid_sig: AtomicU64::new(snapshot.weather_msgs_invalid_sig),
            forecast_tasks_received: AtomicU64::new(snapshot.forecast_tasks_received),
            forecast_tasks_published: AtomicU64::new(snapshot.forecast_tasks_published),
            finality_votes_received: AtomicU64::new(snapshot.finality_votes_received),
            finality_votes_published: AtomicU64::new(snapshot.finality_votes_published),
            finality_certificates_received: AtomicU64::new(snapshot.finality_certificates_received),
            finality_certificates_published: AtomicU64::new(snapshot.finality_certificates_published),
            bytes_inbound: AtomicU64::new(snapshot.bytes_inbound),
            bytes_outbound: AtomicU64::new(snapshot.bytes_outbound),
            bytes_inbound_blocks: AtomicU64::new(snapshot.inbound_blocks.bytes),
            bytes_inbound_txs: AtomicU64::new(snapshot.inbound_txs.bytes),
            bytes_inbound_weather: AtomicU64::new(snapshot.inbound_weather.bytes),
            bytes_inbound_forecast: AtomicU64::new(snapshot.inbound_forecast.bytes),
            bytes_outbound_blocks: AtomicU64::new(snapshot.outbound_blocks.bytes),
            bytes_outbound_txs: AtomicU64::new(snapshot.outbound_txs.bytes),
            bytes_outbound_weather: AtomicU64::new(snapshot.outbound_weather.bytes),
            bytes_outbound_forecast: AtomicU64::new(snapshot.outbound_forecast.bytes),
            messages_rejected: AtomicU64::new(snapshot.messages_rejected),
            pending_broadcasts: AtomicUsize::new(snapshot.pending_broadcasts),
            pending_broadcast_bytes: AtomicUsize::new(snapshot.pending_broadcast_bytes),
            pending_broadcasts_enqueued: AtomicU64::new(snapshot.pending_broadcasts_enqueued),
            pending_broadcasts_deduplicated: AtomicU64::new(
                snapshot.pending_broadcasts_deduplicated,
            ),
            pending_broadcast_retry_attempts: AtomicU64::new(
                snapshot.pending_broadcast_retry_attempts,
            ),
            pending_broadcasts_published: AtomicU64::new(
                snapshot.pending_broadcasts_published,
            ),
            pending_broadcasts_overflowed: AtomicU64::new(
                snapshot.pending_broadcasts_overflowed,
            ),
            pending_broadcasts_exhausted: AtomicU64::new(
                snapshot.pending_broadcasts_exhausted,
            ),
            pending_broadcasts_expired: AtomicU64::new(snapshot.pending_broadcasts_expired),
            genesis_handshake_republished: AtomicU64::new(snapshot.genesis_handshake_republished),
            outbound_dials_rejected_subnet: AtomicU64::new(snapshot.outbound_dials_rejected_subnet),
            outbound_dials_rejected_source: AtomicU64::new(snapshot.outbound_dials_rejected_source),
            outbound_peers: AtomicUsize::new(snapshot.outbound_peers),
            outbound_distinct_subnets: AtomicUsize::new(snapshot.outbound_distinct_subnets),
            outbound_low_diversity: AtomicBool::new(snapshot.outbound_low_diversity),
            outbound_low_diversity_events: AtomicU64::new(snapshot.outbound_low_diversity_events),
            conn_rejected_per_ip: AtomicU64::new(snapshot.conn_rejected_per_ip),
            conn_rejected_ipv4_subnet: AtomicU64::new(snapshot.conn_rejected_ipv4_subnet),
            conn_rejected_ipv6_subnet: AtomicU64::new(snapshot.conn_rejected_ipv6_subnet),
            preauth_verify_skipped: AtomicU64::new(snapshot.preauth_verify_skipped),
            handshake_timeouts: AtomicU64::new(snapshot.handshake_timeouts),
            handshake_concurrent_per_ip_drops: AtomicU64::new(
                snapshot.handshake_concurrent_per_ip_drops,
            ),
            sentry_allowlist_refusals: AtomicU64::new(snapshot.sentry_allowlist_refusals),
            unproven_peers: AtomicUsize::new(snapshot.unproven_peers),
            sync_local_height: AtomicU64::new(snapshot.sync_local_height),
            sync_target_height: AtomicU64::new(snapshot.sync_target_height),
            sync_state: AtomicU64::new(sync_state_code(&snapshot.sync_state)),
            sync_pending_ranges: AtomicUsize::new(snapshot.sync_pending_ranges),
            sync_in_flight_ranges: AtomicUsize::new(snapshot.sync_in_flight_ranges),
            sync_requests_dispatched: AtomicU64::new(snapshot.sync_requests_dispatched),
            sync_responses_received: AtomicU64::new(snapshot.sync_responses_received),
            sync_transport_failures: AtomicU64::new(snapshot.sync_transport_failures),
            sync_ranges_requeued: AtomicU64::new(snapshot.sync_ranges_requeued),
            sync_peer_rotations: AtomicU64::new(snapshot.sync_peer_rotations),
            sync_connection_refresh_attempts: AtomicU64::new(
                snapshot.sync_connection_refresh_attempts,
            ),
            sync_cooldown_deferrals: AtomicU64::new(snapshot.sync_cooldown_deferrals),
            sync_last_resort_dispatches: AtomicU64::new(snapshot.sync_last_resort_dispatches),
            sync_reconnect_status_broadcasts: AtomicU64::new(
                snapshot.sync_reconnect_status_broadcasts,
            ),
            sync_inbound_requests: AtomicU64::new(snapshot.sync_inbound_requests),
            sync_responses_sent: AtomicU64::new(snapshot.sync_responses_sent),
            sync_inbound_failures: AtomicU64::new(snapshot.sync_inbound_failures),
            sync_response_send_failures: AtomicU64::new(snapshot.sync_response_send_failures),
            bootstrap_redial_attempts_success: AtomicU64::new(
                snapshot.bootstrap_redial_attempts_success,
            ),
            bootstrap_redial_attempts_fail: AtomicU64::new(
                snapshot.bootstrap_redial_attempts_fail,
            ),
            outgoing_dial_failures: AtomicU64::new(snapshot.outgoing_dial_failures),
            start,
        }
    }

    /// Take a point-in-time snapshot.
    pub fn snapshot(&self, connected_peers: usize) -> NetworkStats {
        self.connected_peers
            .store(connected_peers, Ordering::Relaxed);
        let mut mesh_peers_by_topic = BTreeMap::new();
        mesh_peers_by_topic.insert(
            "blocks".to_string(),
            self.mesh_peers_blocks.load(Ordering::Relaxed),
        );
        mesh_peers_by_topic.insert(
            "transactions".to_string(),
            self.mesh_peers_transactions.load(Ordering::Relaxed),
        );
        mesh_peers_by_topic.insert(
            "weather".to_string(),
            self.mesh_peers_weather.load(Ordering::Relaxed),
        );
        mesh_peers_by_topic.insert(
            "forecast".to_string(),
            self.mesh_peers_forecast.load(Ordering::Relaxed),
        );
        NetworkStats {
            connected_peers,
            mesh_peers_by_topic,
            blocks_received: self.blocks_received.load(Ordering::Relaxed),
            blocks_published: self.blocks_published.load(Ordering::Relaxed),
            txs_received: self.txs_received.load(Ordering::Relaxed),
            txs_published: self.txs_published.load(Ordering::Relaxed),
            weather_msgs_received: self.weather_msgs_received.load(Ordering::Relaxed),
            weather_msgs_published: self.weather_msgs_published.load(Ordering::Relaxed),
            weather_msgs_invalid_sig: self.weather_msgs_invalid_sig.load(Ordering::Relaxed),
            forecast_tasks_received: self.forecast_tasks_received.load(Ordering::Relaxed),
            forecast_tasks_published: self.forecast_tasks_published.load(Ordering::Relaxed),
            finality_votes_received: self.finality_votes_received.load(Ordering::Relaxed),
            finality_votes_published: self.finality_votes_published.load(Ordering::Relaxed),
            finality_certificates_received: self.finality_certificates_received.load(Ordering::Relaxed),
            finality_certificates_published: self.finality_certificates_published.load(Ordering::Relaxed),
            bytes_inbound: self.bytes_inbound.load(Ordering::Relaxed),
            bytes_outbound: self.bytes_outbound.load(Ordering::Relaxed),
            inbound_blocks: TopicStats {
                bytes: self.bytes_inbound_blocks.load(Ordering::Relaxed),
                messages: self.blocks_received.load(Ordering::Relaxed),
            },
            inbound_txs: TopicStats {
                bytes: self.bytes_inbound_txs.load(Ordering::Relaxed),
                messages: self.txs_received.load(Ordering::Relaxed),
            },
            inbound_weather: TopicStats {
                bytes: self.bytes_inbound_weather.load(Ordering::Relaxed),
                messages: self.weather_msgs_received.load(Ordering::Relaxed),
            },
            inbound_forecast: TopicStats {
                bytes: self.bytes_inbound_forecast.load(Ordering::Relaxed),
                messages: self.forecast_tasks_received.load(Ordering::Relaxed),
            },
            outbound_blocks: TopicStats {
                bytes: self.bytes_outbound_blocks.load(Ordering::Relaxed),
                messages: self.blocks_published.load(Ordering::Relaxed),
            },
            outbound_txs: TopicStats {
                bytes: self.bytes_outbound_txs.load(Ordering::Relaxed),
                messages: self.txs_published.load(Ordering::Relaxed),
            },
            outbound_weather: TopicStats {
                bytes: self.bytes_outbound_weather.load(Ordering::Relaxed),
                messages: self.weather_msgs_published.load(Ordering::Relaxed),
            },
            outbound_forecast: TopicStats {
                bytes: self.bytes_outbound_forecast.load(Ordering::Relaxed),
                messages: self.forecast_tasks_published.load(Ordering::Relaxed),
            },
            messages_rejected: self.messages_rejected.load(Ordering::Relaxed),
            pending_broadcasts: self.pending_broadcasts.load(Ordering::Relaxed),
            pending_broadcast_bytes: self.pending_broadcast_bytes.load(Ordering::Relaxed),
            pending_broadcasts_enqueued: self
                .pending_broadcasts_enqueued
                .load(Ordering::Relaxed),
            pending_broadcasts_deduplicated: self
                .pending_broadcasts_deduplicated
                .load(Ordering::Relaxed),
            pending_broadcast_retry_attempts: self
                .pending_broadcast_retry_attempts
                .load(Ordering::Relaxed),
            pending_broadcasts_published: self
                .pending_broadcasts_published
                .load(Ordering::Relaxed),
            pending_broadcasts_overflowed: self
                .pending_broadcasts_overflowed
                .load(Ordering::Relaxed),
            pending_broadcasts_exhausted: self
                .pending_broadcasts_exhausted
                .load(Ordering::Relaxed),
            pending_broadcasts_expired: self
                .pending_broadcasts_expired
                .load(Ordering::Relaxed),
            bootstrap_redial_attempts_success: self
                .bootstrap_redial_attempts_success
                .load(Ordering::Relaxed),
            bootstrap_redial_attempts_fail: self
                .bootstrap_redial_attempts_fail
                .load(Ordering::Relaxed),
            outgoing_dial_failures: self.outgoing_dial_failures.load(Ordering::Relaxed),
            outbound_dials_rejected_subnet: self
                .outbound_dials_rejected_subnet
                .load(Ordering::Relaxed),
            outbound_dials_rejected_source: self
                .outbound_dials_rejected_source
                .load(Ordering::Relaxed),
            outbound_peers: self.outbound_peers.load(Ordering::Relaxed),
            outbound_distinct_subnets: self.outbound_distinct_subnets.load(Ordering::Relaxed),
            outbound_low_diversity: self.outbound_low_diversity.load(Ordering::Relaxed),
            outbound_low_diversity_events: self
                .outbound_low_diversity_events
                .load(Ordering::Relaxed),
            conn_rejected_per_ip: self.conn_rejected_per_ip.load(Ordering::Relaxed),
            conn_rejected_ipv4_subnet: self.conn_rejected_ipv4_subnet.load(Ordering::Relaxed),
            conn_rejected_ipv6_subnet: self.conn_rejected_ipv6_subnet.load(Ordering::Relaxed),
            preauth_verify_skipped: self.preauth_verify_skipped.load(Ordering::Relaxed),
            handshake_timeouts: self.handshake_timeouts.load(Ordering::Relaxed),
            handshake_concurrent_per_ip_drops: self
                .handshake_concurrent_per_ip_drops
                .load(Ordering::Relaxed),
            sentry_allowlist_refusals: self.sentry_allowlist_refusals.load(Ordering::Relaxed),
            unproven_peers: self.unproven_peers.load(Ordering::Relaxed),
            genesis_handshake_republished: self
                .genesis_handshake_republished
                .load(Ordering::Relaxed),
            uptime_secs: self.start.elapsed().as_secs(),
            sync_local_height: self.sync_local_height.load(Ordering::Relaxed),
            sync_target_height: self.sync_target_height.load(Ordering::Relaxed),
            sync_state: sync_state_label(self.sync_state.load(Ordering::Relaxed)).to_string(),
            sync_pending_ranges: self.sync_pending_ranges.load(Ordering::Relaxed),
            sync_in_flight_ranges: self.sync_in_flight_ranges.load(Ordering::Relaxed),
            sync_outstanding_ranges: self
                .sync_pending_ranges
                .load(Ordering::Relaxed)
                .saturating_add(self.sync_in_flight_ranges.load(Ordering::Relaxed)),
            sync_requests_dispatched: self.sync_requests_dispatched.load(Ordering::Relaxed),
            sync_responses_received: self.sync_responses_received.load(Ordering::Relaxed),
            sync_transport_failures: self.sync_transport_failures.load(Ordering::Relaxed),
            sync_ranges_requeued: self.sync_ranges_requeued.load(Ordering::Relaxed),
            sync_peer_rotations: self.sync_peer_rotations.load(Ordering::Relaxed),
            sync_connection_refresh_attempts: self
                .sync_connection_refresh_attempts
                .load(Ordering::Relaxed),
            sync_cooldown_deferrals: self.sync_cooldown_deferrals.load(Ordering::Relaxed),
            sync_last_resort_dispatches: self
                .sync_last_resort_dispatches
                .load(Ordering::Relaxed),
            sync_reconnect_status_broadcasts: self
                .sync_reconnect_status_broadcasts
                .load(Ordering::Relaxed),
            sync_inbound_requests: self.sync_inbound_requests.load(Ordering::Relaxed),
            sync_responses_sent: self.sync_responses_sent.load(Ordering::Relaxed),
            sync_inbound_failures: self.sync_inbound_failures.load(Ordering::Relaxed),
            sync_response_send_failures: self
                .sync_response_send_failures
                .load(Ordering::Relaxed),
        }
    }

    /// Count an outbound dial candidate refused by the diversity policy (#2114).
    ///
    /// Exhaustive match rather than a lookup: a new rejection variant then
    /// fails to compile here instead of going silently uncounted.
    pub fn record_diversity_rejection(&self, rejection: crate::slots::DiversityRejection) {
        let counter = match rejection {
            crate::slots::DiversityRejection::Ipv4Subnet => &self.outbound_dials_rejected_subnet,
            crate::slots::DiversityRejection::DiscoverySource => {
                &self.outbound_dials_rejected_source
            },
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the counter for a connection-quota rejection reason (#2113).
    ///
    /// Matching exhaustively rather than via a lookup table is deliberate: a
    /// new `RejectReason` variant then fails to compile here instead of
    /// silently going uncounted.
    pub fn record_quota_rejection(&self, reason: crate::quota::RejectReason) {
        let counter = match reason {
            crate::quota::RejectReason::PerIp => &self.conn_rejected_per_ip,
            crate::quota::RejectReason::Ipv4Subnet => &self.conn_rejected_ipv4_subnet,
            crate::quota::RejectReason::Ipv6Subnet => &self.conn_rejected_ipv6_subnet,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    /// Count an unproven peer dropped or refused (#2116).
    ///
    /// Exhaustive match: a new reason fails to compile here rather than going
    /// silently uncounted.
    pub fn record_handshake_drop(&self, reason: crate::handshake_guard::DropReason) {
        let counter = match reason {
            crate::handshake_guard::DropReason::Timeout => &self.handshake_timeouts,
            crate::handshake_guard::DropReason::ConcurrentPerIp => {
                &self.handshake_concurrent_per_ip_drops
            },
            crate::handshake_guard::DropReason::NotAllowlistedSentry => &self.sentry_allowlist_refusals,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    /// Publish the current outbound-diversity state (#2114).
    ///
    /// The event counter increments only on the *transition* into low
    /// diversity, so a node sitting eclipsed does not inflate it once per
    /// sample — the boolean already says it is still happening.
    pub fn set_outbound_diversity(
        &self,
        outbound_peers: usize,
        distinct_subnets: usize,
        low_diversity: bool,
    ) {
        self.outbound_peers.store(outbound_peers, Ordering::Relaxed);
        self.outbound_distinct_subnets
            .store(distinct_subnets, Ordering::Relaxed);
        let was_low = self
            .outbound_low_diversity
            .swap(low_diversity, Ordering::Relaxed);
        if low_diversity && !was_low {
            self.outbound_low_diversity_events
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Publish the current count of not-yet-proven peers (#2116).
    pub fn set_unproven_peers(&self, count: usize) {
        self.unproven_peers.store(count, Ordering::Relaxed);
    }

    /// Update the cached connected-peer count without taking a full snapshot.
    pub fn set_connected_peers(&self, connected_peers: usize) {
        self.connected_peers
            .store(connected_peers, Ordering::Relaxed);
    }

    /// Publish one coherent-enough downloader observation for operator reads.
    ///
    /// The fields are independent atomics because metrics are advisory; a
    /// scrape racing the 250 ms sync tick may span adjacent states but can
    /// never block request dispatch (#2796).
    pub fn set_sync_observation(
        &self,
        local_height: u64,
        target_height: u64,
        state: &str,
        pending_ranges: usize,
        in_flight_ranges: usize,
    ) {
        self.sync_local_height.store(local_height, Ordering::Relaxed);
        self.sync_target_height.store(target_height, Ordering::Relaxed);
        self.sync_state
            .store(sync_state_code(state), Ordering::Relaxed);
        self.sync_pending_ranges
            .store(pending_ranges, Ordering::Relaxed);
        self.sync_in_flight_ranges
            .store(in_flight_ranges, Ordering::Relaxed);
    }

    pub fn set_sync_liveness(&self, metrics: crate::sync::SyncLivenessMetrics) {
        self.sync_requests_dispatched
            .store(metrics.requests_dispatched, Ordering::Relaxed);
        self.sync_responses_received
            .store(metrics.responses_received, Ordering::Relaxed);
        self.sync_transport_failures
            .store(metrics.transport_failures, Ordering::Relaxed);
        self.sync_ranges_requeued
            .store(metrics.ranges_requeued, Ordering::Relaxed);
        self.sync_peer_rotations
            .store(metrics.peer_rotations, Ordering::Relaxed);
        self.sync_cooldown_deferrals
            .store(metrics.cooldown_deferrals, Ordering::Relaxed);
        self.sync_last_resort_dispatches
            .store(metrics.last_resort_dispatches, Ordering::Relaxed);
    }

    pub fn record_sync_reconnect_status_broadcast(&self) {
        self.sync_reconnect_status_broadcasts
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_sync_connection_refresh_attempt(&self) {
        self.sync_connection_refresh_attempts
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_sync_inbound_request(&self) {
        self.sync_inbound_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_sync_response_sent(&self) {
        self.sync_responses_sent.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_sync_inbound_failure(&self) {
        self.sync_inbound_failures.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_sync_response_send_failure(&self) {
        self.sync_response_send_failures
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Update the cached gossipsub mesh size for a topic label.
    pub fn set_mesh_peers(&self, topic: &str, mesh_peers: usize) {
        match topic {
            "blocks" => self.mesh_peers_blocks.store(mesh_peers, Ordering::Relaxed),
            "transactions" => self
                .mesh_peers_transactions
                .store(mesh_peers, Ordering::Relaxed),
            "weather" => self.mesh_peers_weather.store(mesh_peers, Ordering::Relaxed),
            "forecast" => self.mesh_peers_forecast.store(mesh_peers, Ordering::Relaxed),
            _ => {},
        }
    }

    /// Take a snapshot using the most recently observed peer count.
    pub fn snapshot_cached(&self) -> NetworkStats {
        self.snapshot(self.connected_peers.load(Ordering::Relaxed))
    }

    /// Record inbound bytes tagged by topic label.
    ///
    /// Topic labels: `"blocks"`, `"transactions"`, `"weather"`, `"forecast"`.
    pub fn record_inbound_topic(&self, topic: &str, bytes: u64) {
        self.bytes_inbound.fetch_add(bytes, Ordering::Relaxed);
        match topic {
            "blocks" => { self.bytes_inbound_blocks.fetch_add(bytes, Ordering::Relaxed); }
            "transactions" => { self.bytes_inbound_txs.fetch_add(bytes, Ordering::Relaxed); }
            "weather" => { self.bytes_inbound_weather.fetch_add(bytes, Ordering::Relaxed); }
            "forecast" => { self.bytes_inbound_forecast.fetch_add(bytes, Ordering::Relaxed); }
            _ => {}
        }
    }

    /// Record outbound bytes tagged by topic label.
    pub fn record_outbound_topic(&self, topic: &str, bytes: u64) {
        self.bytes_outbound.fetch_add(bytes, Ordering::Relaxed);
        match topic {
            "blocks" => { self.bytes_outbound_blocks.fetch_add(bytes, Ordering::Relaxed); }
            "transactions" => { self.bytes_outbound_txs.fetch_add(bytes, Ordering::Relaxed); }
            "weather" => { self.bytes_outbound_weather.fetch_add(bytes, Ordering::Relaxed); }
            "forecast" => { self.bytes_outbound_forecast.fetch_add(bytes, Ordering::Relaxed); }
            _ => {}
        }
    }
}

fn sync_state_code(state: &str) -> u64 {
    match state {
        "idle" => 0,
        "discovering" => 1,
        "downloading" => 2,
        "stalled" => 3,
        "complete" => 4,
        "validation_halted" => 5,
        _ => 0,
    }
}

fn sync_state_label(code: u64) -> &'static str {
    match code {
        1 => "discovering",
        2 => "downloading",
        3 => "stalled",
        4 => "complete",
        5 => "validation_halted",
        _ => "idle",
    }
}

/// Map a topic name from [`TopicManager::topic_name()`] to a short label.
pub(crate) fn topic_label(topic_name: Option<&str>) -> &'static str {
    match topic_name {
        Some(t) if t.contains("blocks") => "blocks",
        Some(t) if t.contains("transactions") => "transactions",
        Some(t) if t.contains("weather") => "weather",
        Some(t) if t.contains("forecast") => "forecast",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_inbound_topic_increments_aggregate_and_per_topic() {
        let mc = MetricsCollector::new();
        mc.record_inbound_topic("blocks", 100);
        mc.record_inbound_topic("blocks", 50);
        mc.record_inbound_topic("transactions", 200);
        mc.record_inbound_topic("weather", 300);
        mc.record_inbound_topic("forecast", 75);

        let snap = mc.snapshot(0);
        assert_eq!(snap.bytes_inbound, 725);
        assert_eq!(snap.inbound_blocks.bytes, 150);
        assert_eq!(snap.inbound_txs.bytes, 200);
        assert_eq!(snap.inbound_weather.bytes, 300);
        assert_eq!(snap.inbound_forecast.bytes, 75);
    }

    #[test]
    fn record_outbound_topic_increments_aggregate_and_per_topic() {
        let mc = MetricsCollector::new();
        mc.record_outbound_topic("blocks", 400);
        mc.record_outbound_topic("transactions", 100);
        mc.record_outbound_topic("weather", 50);
        mc.record_outbound_topic("forecast", 25);

        let snap = mc.snapshot(0);
        assert_eq!(snap.bytes_outbound, 575);
        assert_eq!(snap.outbound_blocks.bytes, 400);
        assert_eq!(snap.outbound_txs.bytes, 100);
        assert_eq!(snap.outbound_weather.bytes, 50);
        assert_eq!(snap.outbound_forecast.bytes, 25);
    }

    #[test]
    fn unknown_topic_only_increments_aggregate() {
        let mc = MetricsCollector::new();
        mc.record_inbound_topic("unknown", 500);

        let snap = mc.snapshot(0);
        assert_eq!(snap.bytes_inbound, 500);
        assert_eq!(snap.inbound_blocks.bytes, 0);
        assert_eq!(snap.inbound_txs.bytes, 0);
        assert_eq!(snap.inbound_weather.bytes, 0);
        assert_eq!(snap.inbound_forecast.bytes, 0);
    }

    #[test]
    fn topic_label_maps_correctly() {
        assert_eq!(topic_label(Some("wrfcoin/blocks/1")), "blocks");
        assert_eq!(topic_label(Some("wrfcoin/transactions/1")), "transactions");
        assert_eq!(topic_label(Some("wrfcoin/weather-data/1")), "weather");
        assert_eq!(topic_label(Some("wrfcoin/forecast-tasks/1")), "forecast");
        assert_eq!(topic_label(None), "unknown");
    }

    #[test]
    fn snapshot_message_counts_mirror_topic_stats() {
        let mc = MetricsCollector::new();
        mc.blocks_received.fetch_add(3, Ordering::Relaxed);
        mc.forecast_tasks_received.fetch_add(2, Ordering::Relaxed);

        let snap = mc.snapshot(5);
        assert_eq!(snap.connected_peers, 5);
        assert_eq!(snap.inbound_blocks.messages, 3);
        assert_eq!(snap.inbound_forecast.messages, 2);
        assert_eq!(snap.forecast_tasks_received, 2);
    }

    #[test]
    fn snapshot_includes_mesh_peer_counts_by_topic() {
        let mc = MetricsCollector::new();
        mc.set_mesh_peers("blocks", 2);
        mc.set_mesh_peers("transactions", 1);
        mc.set_mesh_peers("weather", 3);
        mc.set_mesh_peers("forecast", 4);

        let snap = mc.snapshot(0);
        assert_eq!(snap.mesh_peers_by_topic.get("blocks"), Some(&2));
        assert_eq!(snap.mesh_peers_by_topic.get("transactions"), Some(&1));
        assert_eq!(snap.mesh_peers_by_topic.get("weather"), Some(&3));
        assert_eq!(snap.mesh_peers_by_topic.get("forecast"), Some(&4));
    }

    #[test]
    fn sync_observation_exposes_heights_state_and_range_counts_2796() {
        let mc = MetricsCollector::new();
        mc.set_sync_observation(11_016, 12_801, "downloading", 2, 3);

        let snap = mc.snapshot(1);
        assert_eq!(snap.sync_local_height, 11_016);
        assert_eq!(snap.sync_target_height, 12_801);
        assert_eq!(snap.sync_state, "downloading");
        assert_eq!(snap.sync_pending_ranges, 2);
        assert_eq!(snap.sync_in_flight_ranges, 3);
        assert_eq!(snap.sync_outstanding_ranges, 5);
    }

    #[test]
    fn sync_liveness_snapshot_exposes_requester_responder_and_discovery_paths_2816() {
        let mc = MetricsCollector::new();
        mc.set_sync_liveness(crate::sync::SyncLivenessMetrics {
            requests_dispatched: 7,
            responses_received: 5,
            transport_failures: 2,
            ranges_requeued: 3,
            peer_rotations: 1,
            cooldown_deferrals: 4,
            last_resort_dispatches: 6,
        });
        mc.record_sync_reconnect_status_broadcast();
        mc.record_sync_connection_refresh_attempt();
        mc.record_sync_inbound_request();
        mc.record_sync_response_sent();
        mc.record_sync_inbound_failure();
        mc.record_sync_response_send_failure();

        let snap = mc.snapshot(2);
        assert_eq!(snap.sync_requests_dispatched, 7);
        assert_eq!(snap.sync_responses_received, 5);
        assert_eq!(snap.sync_transport_failures, 2);
        assert_eq!(snap.sync_ranges_requeued, 3);
        assert_eq!(snap.sync_peer_rotations, 1);
        assert_eq!(snap.sync_connection_refresh_attempts, 1);
        assert_eq!(snap.sync_cooldown_deferrals, 4);
        assert_eq!(snap.sync_last_resort_dispatches, 6);
        assert_eq!(snap.sync_reconnect_status_broadcasts, 1);
        assert_eq!(snap.sync_inbound_requests, 1);
        assert_eq!(snap.sync_responses_sent, 1);
        assert_eq!(snap.sync_inbound_failures, 1);
        assert_eq!(snap.sync_response_send_failures, 1);
    }

    /// #2819: the general dial-failure counter must survive the
    /// snapshot -> restore -> snapshot round trip like every other counter,
    /// or restarts silently zero the only visibility into junk-address churn.
    #[test]
    fn outgoing_dial_failures_counter_round_trips_2819() {
        let mc = MetricsCollector::new();
        mc.outgoing_dial_failures
            .fetch_add(44, std::sync::atomic::Ordering::Relaxed);

        let snap = mc.snapshot(1);
        assert_eq!(snap.outgoing_dial_failures, 44);

        let restored = MetricsCollector::from_snapshot(&snap);
        assert_eq!(restored.snapshot(1).outgoing_dial_failures, 44);
    }
}

#[cfg(test)]
mod sentry_refusal_metric_tests_2120 {
    use super::*;

    #[test]
    fn sentry_allowlist_refusals_are_counted_in_their_own_metric() {
        let metrics = MetricsCollector::new();
        let before = metrics.snapshot(0);
        assert_eq!(before.sentry_allowlist_refusals, 0);
        metrics.record_handshake_drop(crate::handshake_guard::DropReason::NotAllowlistedSentry);
        metrics.record_handshake_drop(crate::handshake_guard::DropReason::NotAllowlistedSentry);
        let after = metrics.snapshot(0);
        assert_eq!(after.sentry_allowlist_refusals, 2);
        assert_eq!(after.handshake_concurrent_per_ip_drops, before.handshake_concurrent_per_ip_drops, "not folded into another counter");
        assert_eq!(after.handshake_timeouts, before.handshake_timeouts);
        // Survives the snapshot round-trip the collector supports.
        let restored = MetricsCollector::from_snapshot(&after);
        assert_eq!(restored.snapshot(0).sentry_allowlist_refusals, 2);
        assert_eq!(crate::handshake_guard::DropReason::NotAllowlistedSentry.as_str(), "sentry_allowlist_refusal");
    }
}
