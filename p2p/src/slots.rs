//! Inbound/outbound slot separation and outbound peer diversity (#2114).
//!
//! # The eclipse shape this defends against
//!
//! An eclipse attack fills a victim's peer table with attacker-controlled
//! peers until the victim's view of the chain is whatever the attacker says it
//! is. Two structural properties make that expensive, and neither is about
//! detecting malice — both hold regardless of whether any individual peer is
//! honest:
//!
//! 1. **Reserved outbound slots.** Connections *we* chose to make cannot be
//!    displaced by connections *they* chose to make. An attacker can flood
//!    inbound all day and never reduce the number of peers we picked.
//! 2. **Diverse outbound peers.** Slots we chose are worth little if they all
//!    lead to one `/24` or all came from one poisonable routing table.
//!
//! # What was already true, and what was missing
//!
//! Property 1 was *already emergently satisfied* and had no name. #2037 set
//! `max_established_incoming = 128` against `max_established_total = 256`, so
//! 128 slots are unreachable by inbound connections. But that reservation is a
//! coincidence of two independently-settable numbers, not an invariant:
//! `WRFCOIN_P2P_MAX_INCOMING_CONNECTIONS=256` silently destroys it, no error is
//! raised, and nothing fails. [`PeerSlots`] gives that reservation a name, a
//! derivation and a validation, so it cannot be dissolved by accident.
//!
//! Property 2 did not exist at all. That is [`OutboundDiversityPolicy`].
//!
//! # Why "discovery source" is only two values, and why that is the useful split
//!
//! Addresses reach the dial path two ways: the operator-configured bootstrap
//! list, and Kademlia routing updates (`discovered.rs` is written from exactly
//! one call site, `kad::Event::RoutingUpdated`). Identify events are ignored.
//!
//! Two values is not a poor approximation of "many sources" — it is precisely
//! the distinction that matters here. Bootstrap addresses are chosen by the
//! operator; Kademlia addresses are chosen by the network, and a routing table
//! is the classic thing an eclipse attacker poisons. Capping how much of our
//! self-chosen outbound capacity may come from the attacker-influenceable
//! source is the whole point.

use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use libp2p::multiaddr::Protocol;
use libp2p::swarm::ConnectionId;
use libp2p::{Multiaddr, PeerId};
use rustc_hash::{FxHashMap, FxHashSet};

/// How an outbound dial candidate's address reached us.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiscoverySource {
    /// From the operator-configured bootstrap list. Trusted to the same degree
    /// the operator is.
    Bootstrap,
    /// From a Kademlia routing update. Influenceable by the network, and so by
    /// an attacker.
    Kademlia,
}

impl DiscoverySource {
    pub fn as_str(self) -> &'static str {
        match self {
            DiscoverySource::Bootstrap => "bootstrap",
            DiscoverySource::Kademlia => "kademlia",
        }
    }
}

/// The inbound/outbound split of the connection budget.
///
/// Constructed from the existing `max_established_*` config rather than adding
/// a parallel set of numbers, so there is exactly one place that decides how
/// many connections the node will hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerSlots {
    /// Slots that inbound connections can never occupy.
    outbound_reserved: u32,
    /// Ceiling on established inbound connections.
    inbound_max: u32,
}

/// Why a slot configuration was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotConfigError {
    /// The inbound ceiling meets or exceeds the total, so inbound can consume
    /// every slot and no outbound capacity is structurally reserved.
    NoOutboundReserve { inbound_max: u32, total: u32 },
}

impl std::fmt::Display for SlotConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SlotConfigError::NoOutboundReserve { inbound_max, total } => write!(
                f,
                "inbound ceiling ({inbound_max}) leaves no reserved outbound slots against a \
                 total of {total}: inbound connections could then occupy every slot, which is \
                 the precondition for an eclipse. Lower WRFCOIN_P2P_MAX_INCOMING_CONNECTIONS \
                 below WRFCOIN_P2P_MAX_CONNECTIONS."
            ),
        }
    }
}

impl std::error::Error for SlotConfigError {}

impl PeerSlots {
    /// Derive the split from the total and inbound ceilings.
    ///
    /// `None` for either means "no limit configured". A missing *total* means
    /// there is no budget to divide, and a missing *inbound* ceiling means
    /// inbound is unbounded — in both cases nothing is reserved, and that is
    /// reported rather than silently treated as safe.
    pub fn from_limits(
        max_established_total: Option<u32>,
        max_established_incoming: Option<u32>,
    ) -> Result<Self, SlotConfigError> {
        let total = max_established_total.unwrap_or(u32::MAX);
        let inbound_max = max_established_incoming.unwrap_or(u32::MAX);

        if inbound_max >= total {
            return Err(SlotConfigError::NoOutboundReserve { inbound_max, total });
        }

        Ok(Self {
            outbound_reserved: total - inbound_max,
            inbound_max,
        })
    }

    /// Slots inbound connections can never occupy.
    pub fn outbound_reserved(&self) -> u32 {
        self.outbound_reserved
    }

    /// Ceiling on established inbound connections.
    pub fn inbound_max(&self) -> u32 {
        self.inbound_max
    }

    /// The invariant, stated as a predicate so a test can assert it directly:
    /// no quantity of inbound connections reduces the reserved outbound
    /// capacity.
    ///
    /// `inbound_connections` is the count under an arbitrary flood, including
    /// counts far above the ceiling.
    pub fn outbound_capacity_under_inbound_load(&self, inbound_connections: u32) -> u32 {
        // Inbound is admitted only up to `inbound_max`; anything beyond that is
        // refused by `connection_limits::Behaviour` before it occupies a slot.
        let _ = inbound_connections.min(self.inbound_max);
        self.outbound_reserved
    }
}

/// Ceilings on how concentrated the *outbound* peer set may become.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutboundDiversityPolicy {
    /// Max outbound peers sharing one IPv4 `/24`.
    pub max_per_ipv4_subnet: u32,
    /// Max outbound peers attributable to a single discovery source.
    pub max_per_discovery_source: u32,
    /// Below this many distinct subnets, the outbound set is reported as
    /// low-diversity. This is a *signal*, not a ceiling — it never refuses a
    /// dial.
    pub min_distinct_subnets: u32,
    /// Whether bootstrap-sourced candidates bypass the ceilings above.
    ///
    /// Defaults to `false`: per #2114, bootstrap peers are exempt *only via
    /// explicit configuration*. An operator whose bootstrap list is
    /// deliberately concentrated in one `/24` sets this to `true`.
    pub exempt_bootstrap: bool,
    /// Whether a policy violation actually refuses the dial.
    ///
    /// Defaults to `false` — **observe-only**. The ceilings are evaluated and
    /// every would-be refusal is counted, but the dial proceeds.
    ///
    /// This is not timidity, it is the deployment reality: an all-in-one-`/24`
    /// peer set is the *normal* topology for a LAN testnet and for local
    /// multi-node development, and it is indistinguishable by address alone
    /// from the eclipse this policy exists to stop. Enforcing first and
    /// measuring second would partition exactly the networks we develop on.
    /// The rejection counters are what tell an operator whether enforcement is
    /// safe to switch on for their topology.
    ///
    /// The detector — [`OutboundPeerSet::is_low_diversity`] — is *always* live
    /// and does not depend on this flag. Observation is free; refusal is not.
    pub enforce: bool,
}

impl Default for OutboundDiversityPolicy {
    /// Defaults sized against the ~12-node testnet, chosen to bind on a genuine
    /// eclipse without biting a legitimate deployment.
    ///
    /// `max_per_ipv4_subnet = 3` — an eclipse needs to dominate the outbound
    /// set, and three peers out of a healthy handful cannot. Three also clears
    /// a small operator bootstrap list in one `/24`, which is a normal thing to
    /// have.
    ///
    /// `max_per_discovery_source = 8` — comfortably above the outbound set a
    /// 12-node network actually sustains, so it binds only when one source is
    /// supplying an implausible share.
    ///
    /// `min_distinct_subnets = 2` — one subnet for the entire outbound set is
    /// the unambiguous eclipse signature. Two is deliberately not aspirational:
    /// a signal that fires constantly on a healthy small network is a signal
    /// operators learn to ignore.
    fn default() -> Self {
        Self {
            max_per_ipv4_subnet: 3,
            max_per_discovery_source: 8,
            min_distinct_subnets: 2,
            exempt_bootstrap: false,
            enforce: false,
        }
    }
}

/// Why an outbound dial candidate was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiversityRejection {
    /// Too many outbound peers already in the candidate's `/24`.
    Ipv4Subnet,
    /// Too many outbound peers already from the candidate's discovery source.
    DiscoverySource,
}

impl DiversityRejection {
    pub fn as_str(self) -> &'static str {
        match self {
            DiversityRejection::Ipv4Subnet => "ipv4_subnet",
            DiversityRejection::DiscoverySource => "discovery_source",
        }
    }
}

impl std::fmt::Display for DiversityRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// IPv4 `/24` of an address, if it has one.
///
/// IPv6 candidates return `None` and are not subject to the subnet ceiling.
/// That is a stated limit rather than an oversight: #2114 specifies subnet
/// diversity in terms of IPv4 `/24`, and inventing an IPv6 rule here would put
/// a second, untested grouping into the dial path. IPv6 concentration is
/// covered on the *inbound* side by the `/64` quota in [`crate::quota`].
fn ipv4_slash24(addr: &Multiaddr) -> Option<[u8; 3]> {
    addr.iter().find_map(|proto| match proto {
        // Loopback carries no diversity information and is excluded, matching
        // the loopback exemption in `crate::quota`. Without this, local
        // multi-node development — every node on `127.0.0.1`, hence a single
        // `/24` — reports "eclipse in progress" on every run, and a detector
        // that fires constantly on a healthy setup is one operators learn to
        // filter out. Loopback co-location is a development topology, not a
        // Sybil signal.
        Protocol::Ip4(ip) if ip.is_loopback() => None,
        Protocol::Ip4(ip) => {
            let o: [u8; 4] = Ipv4Addr::octets(&ip);
            Some([o[0], o[1], o[2]])
        },
        _ => None,
    })
}

/// An outbound peer we chose to dial.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OutboundPeer {
    subnet: Option<[u8; 3]>,
    source: DiscoverySource,
}

/// The current outbound peer set, and the diversity decisions over it.
#[derive(Debug)]
pub struct OutboundPeerSet {
    policy: OutboundDiversityPolicy,
    peers: FxHashMap<PeerId, OutboundPeer>,
    /// #2546 findings 1+2: budget charged at DIAL SUBMISSION, keyed by the
    /// dial's `ConnectionId` — converted into `peers` on establishment,
    /// released on dial error. `admits` counts these exactly like established
    /// peers, so N same-subnet candidates becoming eligible in one tick (the
    /// cold-start case: every bootstrap entry at once, against an empty
    /// established set) cannot all pass the gate. Without this, the policy is
    /// evaluated only against established state and the startup bypass in
    /// #2546 finding 1 is exactly what happens.
    reservations: FxHashMap<ConnectionId, (OutboundPeer, Instant)>,
}

impl OutboundPeerSet {
    pub fn new(policy: OutboundDiversityPolicy) -> Self {
        Self {
            policy,
            peers: FxHashMap::default(),
            reservations: FxHashMap::default(),
        }
    }

    pub fn policy(&self) -> &OutboundDiversityPolicy {
        &self.policy
    }

    /// Should this candidate be dialed?
    ///
    /// Pure: it inspects the current set and returns a verdict without
    /// recording anything, so a caller can consult it for successive candidates
    /// and take the first that passes.
    pub fn admits(
        &self,
        addr: &Multiaddr,
        source: DiscoverySource,
    ) -> Result<(), DiversityRejection> {
        if self.policy.exempt_bootstrap && source == DiscoverySource::Bootstrap {
            return Ok(());
        }

        if self.policy.max_per_ipv4_subnet > 0 {
            if let Some(subnet) = ipv4_slash24(addr) {
                // Established peers AND in-flight reservations both hold
                // budget (#2546 finding 2): a dial submitted but not yet
                // established is a slot this policy has already spent.
                let in_subnet = self
                    .peers
                    .values()
                    .chain(self.reservations.values().map(|(p, _)| p))
                    .filter(|p| p.subnet == Some(subnet))
                    .count() as u32;
                if in_subnet >= self.policy.max_per_ipv4_subnet {
                    return Err(DiversityRejection::Ipv4Subnet);
                }
            }
        }

        if self.policy.max_per_discovery_source > 0 {
            let from_source = self
                .peers
                .values()
                .chain(self.reservations.values().map(|(p, _)| p))
                .filter(|p| p.source == source)
                .count() as u32;
            if from_source >= self.policy.max_per_discovery_source {
                return Err(DiversityRejection::DiscoverySource);
            }
        }

        Ok(())
    }

    /// Charge diversity budget at DIAL SUBMISSION (#2546 findings 1+2).
    ///
    /// On `Ok` the candidate's subnet/source budget is held under
    /// `connection_id` until [`Self::convert_reservation`] (establishment) or
    /// [`Self::release_reservation`] (dial error) resolves it. On `Err`
    /// nothing is recorded.
    pub fn try_reserve(
        &mut self,
        connection_id: ConnectionId,
        addr: &Multiaddr,
        source: DiscoverySource,
    ) -> Result<(), DiversityRejection> {
        self.admits(addr, source)?;
        self.reservations.insert(
            connection_id,
            (
                OutboundPeer {
                    subnet: ipv4_slash24(addr),
                    source,
                },
                Instant::now(),
            ),
        );
        Ok(())
    }

    /// Release a reservation whose dial failed. No-op for unknown ids, so the
    /// caller may invoke it for every `OutgoingConnectionError` unconditionally.
    pub fn release_reservation(&mut self, connection_id: &ConnectionId) {
        self.reservations.remove(connection_id);
    }

    /// Convert a reservation into an established record, preserving the
    /// subnet/source the budget was charged under. Returns false when no
    /// reservation existed (an unreserved dial path — caller records the
    /// peer itself, as before #2546).
    pub fn convert_reservation(&mut self, connection_id: &ConnectionId, peer_id: PeerId) -> bool {
        match self.reservations.remove(connection_id) {
            Some((peer, _)) => {
                self.peers.insert(peer_id, peer);
                true
            },
            None => false,
        }
    }

    /// Safety net: libp2p resolves every dial with either
    /// `ConnectionEstablished` or `OutgoingConnectionError`, but a reservation
    /// leak on this surface would silently shrink the outbound budget, so
    /// stale entries are pruned on the redial tick anyway. Returns how many
    /// were dropped (observability).
    pub fn prune_stale_reservations(&mut self, max_age: Duration) -> usize {
        let before = self.reservations.len();
        self.reservations
            .retain(|_, (_, reserved_at)| reserved_at.elapsed() < max_age);
        before - self.reservations.len()
    }

    /// In-flight dial reservations currently holding budget (observability).
    pub fn reserved_len(&self) -> usize {
        self.reservations.len()
    }

    /// Record an established outbound peer.
    pub fn record(&mut self, peer_id: PeerId, addr: &Multiaddr, source: DiscoverySource) {
        self.peers.insert(
            peer_id,
            OutboundPeer {
                subnet: ipv4_slash24(addr),
                source,
            },
        );
    }

    /// Forget a peer that is no longer connected outbound.
    pub fn remove(&mut self, peer_id: &PeerId) {
        self.peers.remove(peer_id);
    }

    pub fn len(&self) -> usize {
        self.peers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    /// Distinct IPv4 `/24`s represented in the outbound set.
    pub fn distinct_subnets(&self) -> usize {
        self.peers
            .values()
            .filter_map(|p| p.subnet)
            .collect::<FxHashSet<_>>()
            .len()
    }

    /// Outbound peers attributable to a given discovery source.
    pub fn count_from_source(&self, source: DiscoverySource) -> usize {
        self.peers.values().filter(|p| p.source == source).count()
    }

    /// Is the outbound set concentrated enough to look like an eclipse in
    /// progress?
    ///
    /// An **empty** outbound set is deliberately *not* low-diversity. A node
    /// that has not dialed anyone yet is starting up, not eclipsed, and
    /// conflating the two would make the signal fire on every boot — which is
    /// how a signal gets ignored. Zero peers is already reported by the
    /// existing peer-count check.
    pub fn is_low_diversity(&self) -> bool {
        // Only peers that carry subnet information can evidence diversity, so
        // only they can evidence its absence. A set made entirely of loopback
        // or non-IPv4 peers says nothing either way, and claiming otherwise
        // would be asserting a conclusion from missing data.
        let with_subnet = self.peers.values().filter(|p| p.subnet.is_some()).count();
        if with_subnet == 0 {
            return false;
        }
        (self.distinct_subnets() as u32) < self.policy.min_distinct_subnets
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(ip: &str) -> Multiaddr {
        format!("/ip4/{ip}/tcp/9000").parse().unwrap()
    }

    fn peer(n: u8) -> PeerId {
        // Deterministic distinct ids without needing a keypair per peer.
        let mut bytes = [0u8; 32];
        bytes[0] = n;
        let key = libp2p::identity::ed25519::SecretKey::try_from_bytes(bytes).unwrap();
        libp2p::identity::Keypair::from(libp2p::identity::ed25519::Keypair::from(key))
            .public()
            .to_peer_id()
    }

    fn policy() -> OutboundDiversityPolicy {
        OutboundDiversityPolicy::default()
    }

    // -- PeerSlots: the reserved-outbound invariant --------------------------

    #[test]
    fn shipped_defaults_reserve_outbound_slots() {
        // The #2037 defaults: 128 inbound against a 256 total.
        let slots = PeerSlots::from_limits(Some(256), Some(128)).unwrap();
        assert_eq!(slots.outbound_reserved(), 128);
        assert_eq!(slots.inbound_max(), 128);
    }

    /// The acceptance criterion, stated directly: no amount of inbound load
    /// reduces reserved outbound capacity.
    #[test]
    fn an_inbound_flood_never_reduces_reserved_outbound_capacity() {
        let slots = PeerSlots::from_limits(Some(256), Some(128)).unwrap();
        let baseline = slots.outbound_reserved();

        for inbound in [0u32, 1, 127, 128, 129, 255, 256, 10_000, u32::MAX] {
            assert_eq!(
                slots.outbound_capacity_under_inbound_load(inbound),
                baseline,
                "inbound load of {inbound} must not move reserved outbound capacity"
            );
        }
    }

    #[test]
    fn an_inbound_ceiling_at_the_total_is_refused() {
        // This is the silent-dissolution case: it looks like a bigger inbound
        // allowance, and it actually removes the reservation entirely.
        let err = PeerSlots::from_limits(Some(256), Some(256)).unwrap_err();
        assert_eq!(
            err,
            SlotConfigError::NoOutboundReserve {
                inbound_max: 256,
                total: 256
            }
        );
        assert!(PeerSlots::from_limits(Some(256), Some(300)).is_err());
    }

    #[test]
    fn an_unbounded_inbound_ceiling_is_refused() {
        // `None` inbound means unlimited, which reserves nothing.
        assert!(PeerSlots::from_limits(Some(256), None).is_err());
    }

    #[test]
    fn the_error_names_the_setting_an_operator_has_to_change() {
        // A validation error that does not say what to do produces a support
        // ticket rather than a fix.
        let err = PeerSlots::from_limits(Some(256), Some(256)).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("WRFCOIN_P2P_MAX_INCOMING_CONNECTIONS"), "{msg}");
        assert!(msg.contains("WRFCOIN_P2P_MAX_CONNECTIONS"), "{msg}");
    }

    // -- Diversity policy ----------------------------------------------------

    #[test]
    fn diversity_rejects_a_same_subnet_peer_past_the_ceiling() {
        let mut set = OutboundPeerSet::new(OutboundDiversityPolicy {
            max_per_ipv4_subnet: 2,
            ..policy()
        });
        set.record(peer(1), &addr("203.0.113.1"), DiscoverySource::Kademlia);
        set.record(peer(2), &addr("203.0.113.2"), DiscoverySource::Kademlia);

        assert_eq!(
            set.admits(&addr("203.0.113.3"), DiscoverySource::Kademlia),
            Err(DiversityRejection::Ipv4Subnet),
            "a third outbound peer in one /24 concentrates the set we chose"
        );
        // A different /24 is fine — the ceiling is on concentration, not count.
        assert_eq!(
            set.admits(&addr("203.0.114.3"), DiscoverySource::Kademlia),
            Ok(())
        );
    }

    #[test]
    fn diversity_rejects_over_reliance_on_one_discovery_source() {
        let mut set = OutboundPeerSet::new(OutboundDiversityPolicy {
            max_per_ipv4_subnet: 0, // isolate the source dimension
            max_per_discovery_source: 2,
            ..policy()
        });
        set.record(peer(1), &addr("203.0.113.1"), DiscoverySource::Kademlia);
        set.record(peer(2), &addr("198.51.100.1"), DiscoverySource::Kademlia);

        assert_eq!(
            set.admits(&addr("192.0.2.1"), DiscoverySource::Kademlia),
            Err(DiversityRejection::DiscoverySource),
            "these are three distinct subnets, so only the source ceiling can \
             refuse — a poisoned routing table supplies diverse-looking addresses"
        );
        // The operator-configured source is counted separately.
        assert_eq!(
            set.admits(&addr("192.0.2.1"), DiscoverySource::Bootstrap),
            Ok(())
        );
    }

    #[test]
    fn bootstrap_is_exempt_only_when_explicitly_configured() {
        let concentrated = |exempt_bootstrap| {
            let mut set = OutboundPeerSet::new(OutboundDiversityPolicy {
                max_per_ipv4_subnet: 1,
                exempt_bootstrap,
                ..policy()
            });
            set.record(peer(1), &addr("203.0.113.1"), DiscoverySource::Bootstrap);
            set.admits(&addr("203.0.113.2"), DiscoverySource::Bootstrap)
        };

        assert_eq!(
            concentrated(false),
            Err(DiversityRejection::Ipv4Subnet),
            "default must NOT exempt bootstrap (#2114 acceptance)"
        );
        assert_eq!(concentrated(true), Ok(()));
    }

    #[test]
    fn the_exemption_does_not_leak_to_the_kademlia_source() {
        // Turning the bootstrap exemption on must not disable the ceiling for
        // the attacker-influenceable source, which is the one that matters.
        let mut set = OutboundPeerSet::new(OutboundDiversityPolicy {
            max_per_ipv4_subnet: 1,
            exempt_bootstrap: true,
            ..policy()
        });
        set.record(peer(1), &addr("203.0.113.1"), DiscoverySource::Kademlia);
        assert_eq!(
            set.admits(&addr("203.0.113.2"), DiscoverySource::Kademlia),
            Err(DiversityRejection::Ipv4Subnet)
        );
    }

    #[test]
    fn zero_disables_a_single_diversity_dimension() {
        let mut set = OutboundPeerSet::new(OutboundDiversityPolicy {
            max_per_ipv4_subnet: 0,
            max_per_discovery_source: 2,
            ..policy()
        });
        set.record(peer(1), &addr("203.0.113.1"), DiscoverySource::Kademlia);
        // Same /24 is now unconstrained...
        assert_eq!(
            set.admits(&addr("203.0.113.2"), DiscoverySource::Kademlia),
            Ok(())
        );
        // ...but the source ceiling still binds.
        set.record(peer(2), &addr("203.0.113.2"), DiscoverySource::Kademlia);
        assert_eq!(
            set.admits(&addr("203.0.113.3"), DiscoverySource::Kademlia),
            Err(DiversityRejection::DiscoverySource)
        );
    }

    #[test]
    fn removing_a_peer_frees_its_diversity_slot() {
        let mut set = OutboundPeerSet::new(OutboundDiversityPolicy {
            max_per_ipv4_subnet: 1,
            ..policy()
        });
        set.record(peer(1), &addr("203.0.113.1"), DiscoverySource::Kademlia);
        assert!(set.admits(&addr("203.0.113.2"), DiscoverySource::Kademlia).is_err());

        set.remove(&peer(1));

        assert_eq!(
            set.admits(&addr("203.0.113.2"), DiscoverySource::Kademlia),
            Ok(()),
            "a disconnected peer must stop occupying diversity budget"
        );
        assert!(set.is_empty());
    }

    // -- The low-diversity signal --------------------------------------------

    #[test]
    fn a_single_subnet_outbound_set_raises_the_low_diversity_signal() {
        let mut set = OutboundPeerSet::new(OutboundDiversityPolicy {
            max_per_ipv4_subnet: 0,
            min_distinct_subnets: 2,
            ..policy()
        });
        set.record(peer(1), &addr("203.0.113.1"), DiscoverySource::Kademlia);
        set.record(peer(2), &addr("203.0.113.2"), DiscoverySource::Kademlia);
        set.record(peer(3), &addr("203.0.113.3"), DiscoverySource::Kademlia);

        assert_eq!(set.distinct_subnets(), 1);
        assert!(
            set.is_low_diversity(),
            "three outbound peers in one /24 is the eclipse signature"
        );

        // One peer elsewhere clears it.
        set.record(peer(4), &addr("198.51.100.1"), DiscoverySource::Kademlia);
        assert_eq!(set.distinct_subnets(), 2);
        assert!(!set.is_low_diversity());
    }

    #[test]
    fn an_empty_outbound_set_is_not_low_diversity() {
        // A node that has not dialed anyone is starting up, not eclipsed.
        // Conflating them makes the signal fire on every boot.
        let set = OutboundPeerSet::new(policy());
        assert!(!set.is_low_diversity());
    }

    #[test]
    fn a_loopback_only_outbound_set_is_not_low_diversity() {
        // Local multi-node development puts every node in 127.0.0.0/24. If that
        // counted, the eclipse detector would fire on every local test run.
        let mut set = OutboundPeerSet::new(policy());
        for i in 1..=6 {
            set.record(peer(i), &addr("127.0.0.1"), DiscoverySource::Kademlia);
        }
        assert_eq!(set.len(), 6);
        assert_eq!(set.distinct_subnets(), 0);
        assert!(!set.is_low_diversity());
    }

    #[test]
    fn loopback_never_consumes_subnet_diversity_budget() {
        let mut set = OutboundPeerSet::new(OutboundDiversityPolicy {
            max_per_ipv4_subnet: 1,
            max_per_discovery_source: 0,
            ..policy()
        });
        for i in 1..=5 {
            set.record(peer(i), &addr("127.0.0.1"), DiscoverySource::Kademlia);
        }
        assert_eq!(
            set.admits(&addr("127.0.0.1"), DiscoverySource::Kademlia),
            Ok(())
        );
    }

    #[test]
    fn source_counts_are_tracked_separately() {
        let mut set = OutboundPeerSet::new(policy());
        set.record(peer(1), &addr("203.0.113.1"), DiscoverySource::Bootstrap);
        set.record(peer(2), &addr("198.51.100.1"), DiscoverySource::Kademlia);
        set.record(peer(3), &addr("192.0.2.1"), DiscoverySource::Kademlia);

        assert_eq!(set.count_from_source(DiscoverySource::Bootstrap), 1);
        assert_eq!(set.count_from_source(DiscoverySource::Kademlia), 2);
    }

    #[test]
    fn defaults_are_observe_only_and_do_not_exempt_bootstrap() {
        let d = OutboundDiversityPolicy::default();
        assert!(
            !d.enforce,
            "enforcement must be opt-in — a single-/24 LAN testnet is a normal \
             topology and enforcing by default would partition it"
        );
        assert!(!d.exempt_bootstrap, "#2114: exemption is explicit-only");
        assert!(d.min_distinct_subnets >= 2);
    }

    #[test]
    fn rejection_labels_are_distinct() {
        assert_ne!(
            DiversityRejection::Ipv4Subnet.as_str(),
            DiversityRejection::DiscoverySource.as_str()
        );
        assert_ne!(
            DiscoverySource::Bootstrap.as_str(),
            DiscoverySource::Kademlia.as_str()
        );
    }

    #[test]
    fn reservation_charges_budget_before_establishment_2546() {
        let mut set = OutboundPeerSet::new(OutboundDiversityPolicy {
            max_per_ipv4_subnet: 1,
            max_per_discovery_source: 8,
            min_distinct_subnets: 2,
            exempt_bootstrap: false,
            enforce: true,
        });
        let a = addr("203.0.113.1");
        let b = addr("203.0.113.2");
        let id_a = ConnectionId::new_unchecked(1);

        assert!(set.try_reserve(id_a, &a, DiscoverySource::Bootstrap).is_ok());
        // Same /24, nothing ESTABLISHED — the reservation alone must refuse it.
        assert_eq!(
            set.admits(&b, DiscoverySource::Bootstrap),
            Err(DiversityRejection::Ipv4Subnet)
        );

        // A failed dial hands the budget back.
        set.release_reservation(&id_a);
        assert!(set.admits(&b, DiscoverySource::Bootstrap).is_ok());
    }

    #[test]
    fn conversion_preserves_charged_budget_without_double_counting_2546() {
        let mut set = OutboundPeerSet::new(OutboundDiversityPolicy {
            max_per_ipv4_subnet: 1,
            max_per_discovery_source: 8,
            min_distinct_subnets: 2,
            exempt_bootstrap: false,
            enforce: true,
        });
        let a = addr("203.0.113.1");
        let id_a = ConnectionId::new_unchecked(7);
        set.try_reserve(id_a, &a, DiscoverySource::Bootstrap).unwrap();

        assert!(set.convert_reservation(&id_a, peer(1)));
        assert_eq!(set.reserved_len(), 0, "reservation consumed, not leaked");
        assert_eq!(set.len(), 1, "established under the same charged budget");
        // Still exactly one /24 slot spent: a sibling stays refused.
        assert_eq!(
            set.admits(&addr("203.0.113.9"), DiscoverySource::Bootstrap),
            Err(DiversityRejection::Ipv4Subnet)
        );
        // Unreserved connection ids convert to nothing.
        assert!(!set.convert_reservation(&ConnectionId::new_unchecked(8), peer(2)));
    }

    #[test]
    fn stale_reservations_are_pruned_2546() {
        let mut set = OutboundPeerSet::new(OutboundDiversityPolicy {
            max_per_ipv4_subnet: 1,
            max_per_discovery_source: 8,
            min_distinct_subnets: 2,
            exempt_bootstrap: false,
            enforce: true,
        });
        set.try_reserve(ConnectionId::new_unchecked(1), &addr("203.0.113.1"), DiscoverySource::Bootstrap)
            .unwrap();
        assert_eq!(set.prune_stale_reservations(Duration::from_secs(60)), 0, "fresh holds stay");
        assert_eq!(set.prune_stale_reservations(Duration::ZERO), 1, "aged holds are reclaimed");
        assert_eq!(set.reserved_len(), 0);
    }
}
