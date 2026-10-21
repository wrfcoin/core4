//! Gossipsub peer-scoring configuration for WRFCoin.
//!
//! Scoring prevents free-riders and amplification attacks by rewarding peers
//! that relay messages promptly and penalising those that inject invalid data.

use libp2p::gossipsub::{PeerScoreParams, PeerScoreThresholds, TopicScoreParams};
use std::time::Duration;

use crate::message::TopicManager;

/// Transport class hint for a sync peer, sourced from node/peer telemetry
/// (#1800). Lets the block-download scheduler prefer cheaper, higher-bandwidth
/// links (WAN/AP) over metered or bandwidth-constrained ones (LTE failover,
/// mesh) when peers are otherwise equal.
///
/// This is a purely **operational** hint: it influences peer *selection* only,
/// never block validity, consensus, or any state-root input. Unknown is the
/// neutral default used until telemetry reports a transport for the peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransportClass {
    /// Wired / high-bandwidth WAN backbone — most preferred.
    Wan,
    /// Local access-point / LAN link — preferred (cheap, fast, unmetered).
    Ap,
    /// Mesh-radio relay — usable but lower bandwidth; mildly preferred over LTE.
    Mesh,
    /// Cellular / LTE failover — metered and bandwidth-constrained; strongly
    /// de-prioritised for bulk block sync to avoid data cost and throttling.
    Lte,
    /// Transport not yet reported by telemetry; neutral weight.
    #[default]
    Unknown,
}

impl TransportClass {
    /// Whether the link is metered (billed by data volume). Metered links are
    /// de-prioritised for bulk block sync. Currently only LTE is metered.
    pub fn is_metered(self) -> bool {
        matches!(self, TransportClass::Lte)
    }

    /// Additive priority weight (higher = preferred). Bounded well below the
    /// per-incident invalid-block penalty (250) so a real misbehaviour signal
    /// always outranks a transport preference, and below the ban threshold
    /// handling in selection — this only breaks ties between healthy peers.
    pub fn priority_weight(self) -> i64 {
        match self {
            TransportClass::Wan => 100,
            TransportClass::Ap => 80,
            TransportClass::Mesh => 20,
            TransportClass::Lte => -150,
            TransportClass::Unknown => 0,
        }
    }
}

/// App-level sync scoring tracked alongside gossipsub reputation.
#[derive(Debug, Clone, Default)]
pub struct SyncPeerScore {
    response_latency_ms: f64,
    samples: u32,
    invalid_blocks: u32,
    stall_count: u32,
    invalid_weather_sigs: u32,
    transport: TransportClass,
}

impl SyncPeerScore {
    /// Record a successful block-range response latency.
    pub fn record_latency(&mut self, latency: Duration) {
        let sample = latency.as_secs_f64() * 1000.0;
        self.samples += 1;

        if self.samples == 1 {
            self.response_latency_ms = sample;
        } else {
            let total = self.response_latency_ms * f64::from(self.samples - 1);
            self.response_latency_ms = (total + sample) / f64::from(self.samples);
        }
    }

    /// Record that the peer returned invalid sync data.
    pub fn record_invalid_block(&mut self) {
        self.invalid_blocks += 1;
    }

    /// Record that the peer stalled during sync.
    pub fn record_stall(&mut self) {
        self.stall_count += 1;
    }

    /// Average response latency in milliseconds.
    pub fn response_latency_ms(&self) -> f64 {
        self.response_latency_ms
    }

    /// Number of invalid block incidents attributed to the peer.
    pub fn invalid_blocks(&self) -> u32 {
        self.invalid_blocks
    }

    /// Number of times the peer timed out during sync.
    pub fn stall_count(&self) -> u32 {
        self.stall_count
    }

    /// Record that the peer relayed a WeatherData message with an invalid or missing station signature.
    pub fn record_invalid_weather_sig(&mut self) {
        self.invalid_weather_sigs += 1;
    }

    /// Number of invalid weather-signature incidents attributed to the peer.
    pub fn invalid_weather_sigs(&self) -> u32 {
        self.invalid_weather_sigs
    }

    /// Record the peer's transport class as reported by telemetry (#1800).
    pub fn set_transport(&mut self, transport: TransportClass) {
        self.transport = transport;
    }

    /// The peer's last-reported transport class (defaults to `Unknown`).
    pub fn transport(&self) -> TransportClass {
        self.transport
    }

    /// Whether the peer's link is metered (LTE failover). Convenience boolean
    /// equivalent of the transport hint for observability (#1800).
    pub fn is_metered(&self) -> bool {
        self.transport.is_metered()
    }

    /// Higher scores are preferred when choosing peers for sync ranges.
    ///
    /// The transport-aware term (#1800) is additive and bounded so it only
    /// breaks ties between otherwise-healthy peers — the invalid-block and stall
    /// penalties still dominate, and banned peers are excluded by the caller
    /// before `priority()` is consulted.
    pub fn priority(&self) -> i64 {
        let latency_penalty = (self.response_latency_ms / 25.0).round() as i64;
        1_000 - latency_penalty - i64::from(self.invalid_blocks) * 250 - i64::from(self.stall_count) * 50
            + self.transport.priority_weight()
    }

    /// Peers are banned from sync after repeatedly serving invalid blocks or weather-signature violations.
    pub fn is_banned(&self) -> bool {
        self.invalid_blocks >= 3 || self.invalid_weather_sigs >= 3
    }
}

/// Build topic-level scoring parameters for the block-announcement topic.
fn blocks_topic_params() -> TopicScoreParams {
    TopicScoreParams {
        topic_weight: 1.0,

        // P1 -- time in mesh: reward long-lived mesh membership.
        time_in_mesh_weight: 0.5,
        time_in_mesh_quantum: Duration::from_secs(1),
        time_in_mesh_cap: 100.0,

        // P2 -- first message deliveries: reward being the first relay.
        first_message_deliveries_weight: 1.0,
        first_message_deliveries_decay: 0.97,
        first_message_deliveries_cap: 1000.0,

        // P3 -- mesh message delivery rate: penalise slow relays.
        mesh_message_deliveries_weight: -1.0,
        mesh_message_deliveries_decay: 0.97,
        mesh_message_deliveries_cap: 100.0,
        mesh_message_deliveries_threshold: 1.0,
        mesh_message_deliveries_window: Duration::from_millis(500),
        mesh_message_deliveries_activation: Duration::from_secs(60),

        // P3b -- sticky mesh failures.
        mesh_failure_penalty_weight: -1.0,
        mesh_failure_penalty_decay: 0.97,

        // P4 -- invalid messages: heavy penalty.
        invalid_message_deliveries_weight: -100.0,
        invalid_message_deliveries_decay: 0.5,
    }
}

/// Build topic-level scoring parameters for the transaction topic.
fn transactions_topic_params() -> TopicScoreParams {
    TopicScoreParams {
        topic_weight: 0.7,

        time_in_mesh_weight: 0.5,
        time_in_mesh_quantum: Duration::from_secs(1),
        time_in_mesh_cap: 100.0,

        first_message_deliveries_weight: 1.0,
        first_message_deliveries_decay: 0.97,
        first_message_deliveries_cap: 2000.0,

        mesh_message_deliveries_weight: -1.0,
        mesh_message_deliveries_decay: 0.97,
        mesh_message_deliveries_cap: 200.0,
        mesh_message_deliveries_threshold: 1.0,
        mesh_message_deliveries_window: Duration::from_millis(500),
        mesh_message_deliveries_activation: Duration::from_secs(30),

        mesh_failure_penalty_weight: -1.0,
        mesh_failure_penalty_decay: 0.97,

        invalid_message_deliveries_weight: -100.0,
        invalid_message_deliveries_decay: 0.5,
    }
}

/// Build topic-level scoring parameters for the weather-data topic.
fn weather_data_topic_params() -> TopicScoreParams {
    TopicScoreParams {
        topic_weight: 0.5,

        time_in_mesh_weight: 0.5,
        time_in_mesh_quantum: Duration::from_secs(1),
        time_in_mesh_cap: 100.0,

        first_message_deliveries_weight: 1.0,
        first_message_deliveries_decay: 0.99,
        first_message_deliveries_cap: 5000.0,

        mesh_message_deliveries_weight: -0.5,
        mesh_message_deliveries_decay: 0.99,
        mesh_message_deliveries_cap: 500.0,
        mesh_message_deliveries_threshold: 1.0,
        mesh_message_deliveries_window: Duration::from_millis(500),
        mesh_message_deliveries_activation: Duration::from_secs(120),

        mesh_failure_penalty_weight: -0.5,
        mesh_failure_penalty_decay: 0.99,

        invalid_message_deliveries_weight: -100.0,
        invalid_message_deliveries_decay: 0.5,
    }
}

/// Build topic-level scoring parameters for the forecast-tasks topic.
fn forecast_tasks_topic_params() -> TopicScoreParams {
    TopicScoreParams {
        topic_weight: 0.6,

        time_in_mesh_weight: 0.5,
        time_in_mesh_quantum: Duration::from_secs(1),
        time_in_mesh_cap: 100.0,

        first_message_deliveries_weight: 1.0,
        first_message_deliveries_decay: 0.98,
        first_message_deliveries_cap: 3000.0,

        mesh_message_deliveries_weight: -0.5,
        mesh_message_deliveries_decay: 0.98,
        mesh_message_deliveries_cap: 300.0,
        mesh_message_deliveries_threshold: 1.0,
        mesh_message_deliveries_window: Duration::from_millis(500),
        mesh_message_deliveries_activation: Duration::from_secs(60),

        mesh_failure_penalty_weight: -0.5,
        mesh_failure_penalty_decay: 0.98,

        invalid_message_deliveries_weight: -100.0,
        invalid_message_deliveries_decay: 0.5,
    }
}

/// Build the complete peer-scoring parameter set and thresholds.
///
/// The returned tuple is intended to be passed directly to
/// [`gossipsub::Behaviour::with_peer_score`].
/// Addresses exempt from the IP co-location penalty (#1975).
///
/// Loopback only. Without this, narrowing `ip_colocation_factor_threshold` to
/// 1.0 would score local multi-node development into uselessness: twelve nodes
/// on `127.0.0.1` give a surplus of 11, so `-10 * 11^2 = -1210` — past
/// `publish_threshold` (-1000), meaning the local mesh stops propagating. Co-
/// location on loopback is a development topology, not a Sybil signal.
///
/// This mirrors the loopback exemption in [`crate::quota`], deliberately: the
/// two defences should agree about what loopback means.
fn loopback_colocation_whitelist() -> std::collections::HashSet<std::net::IpAddr> {
    // `HashSet` rather than `FxHashSet` because the libp2p field is typed as
    // `std::collections::HashSet`; the crate-wide preference does not apply to
    // a type we do not own.
    #[allow(clippy::disallowed_types)]
    let mut set = std::collections::HashSet::new();
    set.insert(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
    set.insert(std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST));
    set
}

/// Note on the other half of #1975: the issue also proposes `graylist_threshold
/// = -100`. That cannot be applied as written. libp2p validates
/// `graylist_threshold <= publish_threshold` (`peer_score/params.rs`), and
/// `publish_threshold` here is -1000, so -100 fails validation outright. Making
/// it coherent means moving the whole threshold ladder, which is a live
/// scoring change with its own liveness risk and does not belong inside a
/// connection-quota story. Left to #1975; the two remedies applied here are the
/// threshold narrowing above and the per-IP dial guard in [`crate::quota`].
pub fn build_peer_scoring_params() -> (PeerScoreParams, PeerScoreThresholds) {
    let tm = TopicManager::new();

    let mut params = PeerScoreParams {
        topic_score_cap: 1000.0,
        app_specific_weight: 1.0,

        // P6 -- IP co-location: penalise many peers from the same IP.
        //
        // #1975: this was 3.0, which let three Sybil identities share one IP at
        // zero score cost. With MAX_PARALLEL_PEERS = 4 that is 75% of the sync
        // slots taken by one host — an eclipse for the price of one address.
        // At 1.0 the *second* identity from an address already pays.
        //
        // The penalty is `weight * surplus^2` where `surplus = peers - threshold`
        // (libp2p-gossipsub `peer_score.rs`), so against `gossip_threshold`
        // (-100.0) the schedule is now:
        //
        //   2 peers/IP ->  -10    4 peers/IP ->  -90
        //   3 peers/IP ->  -40    5 peers/IP -> -160  (below gossip threshold)
        //
        // Two or three co-located peers stay comfortably usable, which matters
        // because a legitimately NAT'd deployment does exactly that; five stop
        // receiving gossip. Loopback is whitelisted below.
        ip_colocation_factor_weight: -10.0,
        ip_colocation_factor_threshold: 1.0,
        ip_colocation_factor_whitelist: loopback_colocation_whitelist(),

        behaviour_penalty_weight: -1.0,
        behaviour_penalty_threshold: 6.0,
        behaviour_penalty_decay: 0.97,

        decay_interval: Duration::from_secs(1),
        decay_to_zero: 0.01,
        retain_score: Duration::from_secs(3600),

        ..Default::default()
    };

    params
        .topics
        .insert(tm.blocks_hash(), blocks_topic_params());
    params
        .topics
        .insert(tm.transactions_hash(), transactions_topic_params());
    params
        .topics
        .insert(tm.weather_data_hash(), weather_data_topic_params());
    params
        .topics
        .insert(tm.forecast_tasks_hash(), forecast_tasks_topic_params());

    let thresholds = PeerScoreThresholds {
        gossip_threshold: -100.0,
        publish_threshold: -1000.0,
        graylist_threshold: -10000.0,
        accept_px_threshold: 10.0,
        opportunistic_graft_threshold: 5.0,
    };

    (params, thresholds)
}

/// Lightweight sync-specific peer metrics used by the block-download scheduler.
#[derive(Debug, Clone, Default)]
pub struct SyncPeerMetrics {
    pub response_latency_ms: Option<f64>,
    pub successful_responses: u32,
    pub invalid_blocks: u32,
    pub stall_count: u32,
    pub banned: bool,
}

impl SyncPeerMetrics {
    pub fn record_success(&mut self, latency: Duration) {
        let latency_ms = latency.as_secs_f64() * 1000.0;
        self.response_latency_ms = Some(match self.response_latency_ms {
            Some(existing) => (existing * 0.7) + (latency_ms * 0.3),
            None => latency_ms,
        });
        self.successful_responses = self.successful_responses.saturating_add(1);
    }

    pub fn record_invalid_blocks(&mut self, count: u32) {
        self.invalid_blocks = self.invalid_blocks.saturating_add(count);
        if self.invalid_blocks >= 3 {
            self.banned = true;
        }
    }

    pub fn record_stall(&mut self) {
        self.stall_count = self.stall_count.saturating_add(1);
    }

    pub fn priority_score(&self, advertised_height: u64) -> i64 {
        let latency_penalty = self.response_latency_ms.unwrap_or(0.0) as i64;
        let reward = (advertised_height as i64) * 10 + (self.successful_responses as i64 * 15);
        let penalties = (self.invalid_blocks as i64 * 500) + (self.stall_count as i64 * 100);
        if self.banned {
            i64::MIN / 4
        } else {
            reward - penalties - latency_penalty
        }
    }
}

// --- Ban-score persistence across identity churn (#2118) --------------------------------

use rustc_hash::FxHashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Maximum tracked peer identities. Bounded because the map is keyed by attacker-chosen
/// data: `weather_peer_scores` is an unbounded `FxHashMap<PeerId, _>` today, so churning
/// identities is not only a ban reset but a memory-growth vector (#2077 class).
pub const MAX_TRACKED_PEERS: usize = 8_192;
/// Maximum tracked subnets. Smaller: the whole point is that many identities collapse into
/// few subnets.
pub const MAX_TRACKED_SUBNETS: usize = 2_048;

/// How much of a subnet's suspicion a fresh identity from that subnet inherits.
///
/// Deliberately < 1. A new peer from a subnet that has misbehaved starts suspicious, not
/// banned: NAT'd home operators share a /24 with strangers, and a public testnet wants
/// exactly those diverse operators. Inheriting a FULL ban would blacklist a neighbourhood
/// for one bad actor.
pub const SUBNET_WEIGHT: f64 = 0.5;

/// An offence count that decays over time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecayingScore {
    value: f64,
    updated_ticks: u64,
}

impl DecayingScore {
    pub fn new(now_ticks: u64) -> Self {
        Self { value: 0.0, updated_ticks: now_ticks }
    }

    /// Value after decaying to `now_ticks`. Pure: takes the clock rather than reading one,
    /// so decay is testable with a simulated clock and cannot differ between nodes.
    pub fn value_at(&self, now_ticks: u64, half_life_ticks: u64) -> f64 {
        if half_life_ticks == 0 || now_ticks <= self.updated_ticks {
            return self.value;
        }
        let elapsed = (now_ticks - self.updated_ticks) as f64;
        self.value * 0.5f64.powf(elapsed / half_life_ticks as f64)
    }

    pub fn add(&mut self, amount: f64, now_ticks: u64, half_life_ticks: u64) {
        self.value = self.value_at(now_ticks, half_life_ticks) + amount;
        self.updated_ticks = now_ticks;
    }
}

/// The /24 (v4) or /64 (v6) an address belongs to.
///
/// Aggregating at /24 and /64 is the point: a single operator churning PeerIds keeps the
/// same subnet, so suspicion follows the network rather than the identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Subnet {
    V4([u8; 3]),
    V6([u8; 8]),
}

impl Subnet {
    pub fn of(ip: IpAddr) -> Self {
        match ip {
            IpAddr::V4(a) => {
                let o = a.octets();
                Subnet::V4([o[0], o[1], o[2]])
            },
            IpAddr::V6(a) => {
                let s = a.octets();
                let mut p = [0u8; 8];
                p.copy_from_slice(&s[..8]);
                Subnet::V6(p)
            },
        }
    }

    pub fn of_v4(a: Ipv4Addr) -> Self { Subnet::of(IpAddr::V4(a)) }
    pub fn of_v6(a: Ipv6Addr) -> Self { Subnet::of(IpAddr::V6(a)) }
}

/// A map that evicts its least-recently-touched entry once `cap` is reached.
///
/// Hand-rolled rather than pulling in an LRU crate: the whole requirement is one explicit
/// bound, and an unbounded map keyed by attacker-controlled data is the defect this exists
/// to avoid. Eviction is O(n) over a capped n.
#[derive(Debug, Clone)]
struct BoundedMap<K: std::hash::Hash + Eq + Clone> {
    entries: FxHashMap<K, (DecayingScore, u64)>,
    cap: usize,
    tick: u64,
}

impl<K: std::hash::Hash + Eq + Clone> BoundedMap<K> {
    fn new(cap: usize) -> Self {
        Self { entries: FxHashMap::default(), cap: cap.max(1), tick: 0 }
    }

    fn touch(&mut self, key: K, now_ticks: u64) -> &mut DecayingScore {
        self.tick += 1;
        let seq = self.tick;
        if !self.entries.contains_key(&key) && self.entries.len() >= self.cap {
            if let Some(victim) = self
                .entries
                .iter()
                .min_by_key(|(_, (_, s))| *s)
                .map(|(k, _)| k.clone())
            {
                self.entries.remove(&victim);
            }
        }
        let e = self
            .entries
            .entry(key)
            .or_insert_with(|| (DecayingScore::new(now_ticks), seq));
        e.1 = seq;
        &mut e.0
    }

    fn get(&self, key: &K) -> Option<&DecayingScore> {
        self.entries.get(key).map(|(s, _)| s)
    }

    fn len(&self) -> usize { self.entries.len() }
}

/// Ban score that survives a peer reconnecting under a fresh identity (#2118).
#[derive(Debug, Clone)]
pub struct PersistentScore {
    by_peer: BoundedMap<libp2p::PeerId>,
    by_subnet: BoundedMap<Subnet>,
    addrs: FxHashMap<libp2p::PeerId, IpAddr>,
    exempt: Vec<Subnet>,
    half_life_ticks: u64,
    ban_threshold: f64,
}

impl PersistentScore {
    pub fn new(half_life_ticks: u64, ban_threshold: f64) -> Self {
        Self {
            by_peer: BoundedMap::new(MAX_TRACKED_PEERS),
            by_subnet: BoundedMap::new(MAX_TRACKED_SUBNETS),
            addrs: FxHashMap::default(),
            exempt: Vec::new(),
            half_life_ticks,
            ban_threshold,
        }
    }

    /// Subnets that never accrue shared suspicion — community VPNs, shared hosting an
    /// operator vouches for. Peer-level scoring still applies; only the inheritance is off.
    pub fn exempt_subnet(&mut self, subnet: Subnet) {
        if !self.exempt.contains(&subnet) {
            self.exempt.push(subnet);
        }
    }

    pub fn is_exempt(&self, subnet: Subnet) -> bool {
        self.exempt.contains(&subnet)
    }

    /// Record an offence against both the identity and its subnet.
    pub fn record_offence(&mut self, peer: libp2p::PeerId, ip: IpAddr, amount: f64, now: u64) {
        let hl = self.half_life_ticks;
        self.by_peer.touch(peer, now).add(amount, now, hl);
        let subnet = Subnet::of(ip);
        if !self.is_exempt(subnet) {
            self.by_subnet.touch(subnet, now).add(amount, now, hl);
        }
    }

    /// Effective score for a peer: `max(own, subnet * SUBNET_WEIGHT)`.
    ///
    /// The `max` is what makes a fresh identity inherit suspicion. An exempt subnet
    /// contributes nothing, so an honest shared-infrastructure operator is judged only on
    /// its own behaviour.
    pub fn effective(&self, peer: &libp2p::PeerId, ip: IpAddr, now: u64) -> f64 {
        let hl = self.half_life_ticks;
        let own = self.by_peer.get(peer).map_or(0.0, |s| s.value_at(now, hl));
        let subnet = Subnet::of(ip);
        if self.is_exempt(subnet) {
            return own;
        }
        let inherited = self
            .by_subnet
            .get(&subnet)
            .map_or(0.0, |s| s.value_at(now, hl))
            * SUBNET_WEIGHT;
        own.max(inherited)
    }

    pub fn is_banned(&self, peer: &libp2p::PeerId, ip: IpAddr, now: u64) -> bool {
        self.effective(peer, ip, now) >= self.ban_threshold
    }

    /// Remember which address a peer connected from, so an offence recorded later (where
    /// only the `PeerId` is in scope) can still be attributed to a subnet.
    ///
    /// Bounded by the same peer cap: the map is keyed by attacker-chosen `PeerId`, so it
    /// gets the same LRU treatment as the score map rather than a second unbounded map.
    pub fn note_address(&mut self, peer: libp2p::PeerId, ip: IpAddr, now: u64) {
        self.by_peer.touch(peer, now);
        if self.addrs.len() >= MAX_TRACKED_PEERS {
            if let Some(k) = self.addrs.keys().next().copied() {
                self.addrs.remove(&k);
            }
        }
        self.addrs.insert(peer, ip);
    }

    /// Address-free offence recording, for sites that only hold a `PeerId`.
    ///
    /// An unknown address records against the identity ONLY. That is deliberate: guessing
    /// a subnet would spread suspicion to a network we never observed.
    pub fn record_offence_for(&mut self, peer: libp2p::PeerId, amount: f64, now: u64) {
        match self.addrs.get(&peer).copied() {
            Some(ip) => self.record_offence(peer, ip, amount, now),
            None => {
                let hl = self.half_life_ticks;
                self.by_peer.touch(peer, now).add(amount, now, hl);
            },
        }
    }

    /// Effective score for a peer whose address we may know.
    pub fn effective_for(&self, peer: &libp2p::PeerId, now: u64) -> f64 {
        match self.addrs.get(peer).copied() {
            Some(ip) => self.effective(peer, ip, now),
            None => {
                let hl = self.half_life_ticks;
                self.by_peer.get(peer).map_or(0.0, |s| s.value_at(now, hl))
            },
        }
    }

    /// Would this peer be banned under the persistent policy?
    pub fn would_ban(&self, peer: &libp2p::PeerId, now: u64) -> bool {
        self.effective_for(peer, now) >= self.ban_threshold
    }

    pub fn tracked_peers(&self) -> usize { self.by_peer.len() }
    pub fn tracked_subnets(&self) -> usize { self.by_subnet.len() }
}

#[cfg(test)]
mod persistent_score_tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    const HALF_LIFE: u64 = 100;
    const BAN: f64 = 3.0;

    fn peer() -> libp2p::PeerId {
        libp2p::identity::Keypair::generate_ed25519().public().to_peer_id()
    }
    fn ip(d: u8) -> IpAddr { IpAddr::V4(Ipv4Addr::new(10, 1, 1, d)) }

    /// THE defect: today a fresh PeerId gets a fresh score, so a banned peer just
    /// reconnects with a new keypair. A new identity from a penalised subnet must inherit.
    #[test]
    fn fresh_identity_from_a_penalised_subnet_inherits_suspicion() {
        let mut s = PersistentScore::new(HALF_LIFE, BAN);
        let (bad, fresh) = (peer(), peer());
        assert_ne!(bad, fresh);

        for _ in 0..4 {
            s.record_offence(bad, ip(7), 1.0, 0);
        }
        assert!(s.is_banned(&bad, ip(7), 0), "the offender is banned");

        // Same subnet, brand-new identity, zero personal history.
        let inherited = s.effective(&fresh, ip(8), 0);
        assert!(inherited > 0.0, "a fresh identity must inherit subnet suspicion");
        assert!(
            inherited < s.effective(&bad, ip(7), 0),
            "but LESS than the offender's own score — weighted, not a full ban"
        );
    }

    /// The other half: inheritance must not blacklist a neighbourhood. One offence from a
    /// neighbour must not ban an innocent peer outright.
    #[test]
    fn a_single_neighbour_offence_does_not_ban_the_subnet() {
        let mut s = PersistentScore::new(HALF_LIFE, BAN);
        s.record_offence(peer(), ip(7), 1.0, 0);
        let innocent = peer();
        assert!(
            !s.is_banned(&innocent, ip(9), 0),
            "one neighbour offence must not ban a NAT'd home operator"
        );
    }

    #[test]
    fn score_decays_over_simulated_time() {
        let mut s = PersistentScore::new(HALF_LIFE, BAN);
        let p = peer();
        s.record_offence(p, ip(7), 4.0, 0);
        let at0 = s.effective(&p, ip(7), 0);
        let after_one_half_life = s.effective(&p, ip(7), HALF_LIFE);
        let after_ten = s.effective(&p, ip(7), HALF_LIFE * 10);

        assert!((after_one_half_life - at0 / 2.0).abs() < 1e-9, "one half-life halves it");
        assert!(after_ten < 0.01, "a one-off offence ages out: {after_ten}");
        assert!(s.is_banned(&p, ip(7), 0) && !s.is_banned(&p, ip(7), HALF_LIFE * 10));
    }

    /// Both maps bounded — identity churn must not grow memory without limit.
    #[test]
    fn maps_stay_bounded_under_identity_churn() {
        let mut s = PersistentScore::new(HALF_LIFE, BAN);
        for i in 0..10_000u32 {
            s.record_offence(peer(), ip((i % 200) as u8), 1.0, i as u64);
        }
        assert!(
            s.tracked_peers() <= MAX_TRACKED_PEERS,
            "peer map unbounded: {}",
            s.tracked_peers()
        );
        assert!(
            s.tracked_subnets() <= MAX_TRACKED_SUBNETS,
            "subnet map unbounded: {}",
            s.tracked_subnets()
        );
    }

    #[test]
    fn exempt_subnet_shares_no_suspicion() {
        let mut s = PersistentScore::new(HALF_LIFE, BAN);
        s.exempt_subnet(Subnet::of(ip(0)));
        let (bad, innocent) = (peer(), peer());
        for _ in 0..10 {
            s.record_offence(bad, ip(7), 1.0, 0);
        }
        assert_eq!(
            s.effective(&innocent, ip(8), 0),
            0.0,
            "an exempt subnet must not spread suspicion"
        );
        assert!(
            s.effective(&bad, ip(7), 0) > 0.0,
            "but the offender is still judged on its OWN behaviour"
        );
    }

    /// /24 and /64 aggregation: a different subnet must not inherit.
    #[test]
    fn a_different_subnet_inherits_nothing() {
        let mut s = PersistentScore::new(HALF_LIFE, BAN);
        for _ in 0..10 {
            s.record_offence(peer(), ip(7), 1.0, 0);
        }
        let elsewhere = IpAddr::V4(Ipv4Addr::new(10, 2, 2, 7));
        assert_eq!(s.effective(&peer(), elsewhere, 0), 0.0);
    }
}

#[cfg(test)]
mod colocation_tests {
    use super::{build_peer_scoring_params, loopback_colocation_whitelist};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    /// #1975: three Sybil identities per IP must no longer be free.
    #[test]
    fn ip_colocation_threshold_penalises_the_second_peer_from_an_address() {
        let (params, _) = build_peer_scoring_params();
        assert!(
            params.ip_colocation_factor_threshold <= 1.0,
            "threshold {} still grants free co-location slots (#1975)",
            params.ip_colocation_factor_threshold
        );
        assert!(params.ip_colocation_factor_weight < 0.0);
    }

    /// The penalty schedule is `weight * surplus^2`. Pin both ends: a small
    /// NAT'd deployment stays usable, a Sybil cluster does not.
    #[test]
    fn colocation_penalty_schedule_spares_small_clusters_and_bites_large_ones() {
        let (params, thresholds) = build_peer_scoring_params();
        let penalty = |peers: f64| {
            let surplus = peers - params.ip_colocation_factor_threshold;
            if surplus <= 0.0 {
                0.0
            } else {
                params.ip_colocation_factor_weight * surplus * surplus
            }
        };

        assert!(
            penalty(3.0) > thresholds.gossip_threshold,
            "three co-located peers must stay above the gossip threshold — a \
             NAT'd deployment does this legitimately (got {})",
            penalty(3.0)
        );
        assert!(
            penalty(5.0) < thresholds.gossip_threshold,
            "five peers from one address must fall below the gossip threshold \
             (got {})",
            penalty(5.0)
        );
    }

    /// The whitelist is not cosmetic. Without it the threshold narrowing scores
    /// local multi-node development below `publish_threshold`, and the local
    /// mesh silently stops propagating.
    #[test]
    fn loopback_is_whitelisted_from_colocation_penalties() {
        let (params, thresholds) = build_peer_scoring_params();
        let whitelist = &params.ip_colocation_factor_whitelist;
        assert!(whitelist.contains(&IpAddr::V4(Ipv4Addr::LOCALHOST)));
        assert!(whitelist.contains(&IpAddr::V6(Ipv6Addr::LOCALHOST)));

        // Demonstrate the harm the whitelist prevents, so this test explains
        // itself if anyone considers dropping it.
        let local_nodes = 12.0;
        let surplus = local_nodes - params.ip_colocation_factor_threshold;
        let unwhitelisted = params.ip_colocation_factor_weight * surplus * surplus;
        assert!(
            unwhitelisted < thresholds.publish_threshold,
            "expected the un-whitelisted local-dev penalty ({unwhitelisted}) to be \
             below publish_threshold ({}) — if it is not, this test no longer \
             demonstrates why the whitelist exists",
            thresholds.publish_threshold
        );
    }

    /// Guards the claim written into the #1975 note: -100 really is unusable as
    /// a graylist threshold against this publish threshold.
    #[test]
    fn thresholds_keep_the_ordering_libp2p_validates() {
        let (_, t) = build_peer_scoring_params();
        assert!(
            t.graylist_threshold <= t.publish_threshold,
            "libp2p rejects graylist_threshold > publish_threshold"
        );
        assert!(t.publish_threshold <= t.gossip_threshold);
    }

    #[test]
    fn whitelist_contains_only_loopback() {
        // A wider whitelist would quietly re-open the hole #1975 describes.
        let set = loopback_colocation_whitelist();
        assert_eq!(set.len(), 2);
        assert!(set.iter().all(|ip| ip.is_loopback()));
    }
}
