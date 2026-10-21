//! Bounding the cost of a peer that has not proven chain identity (#2116).
//!
//! # What #2116 assumed, and what the code actually does
//!
//! The issue's implementation plan describes a byte-level connection handshake:
//! a deadline on reading a handshake *frame*, a cap on *in-progress* handshakes,
//! a dribbled-bytes slowloris check, and cheap identity comparisons ordered
//! before *signature verification*.
//!
//! None of those describe this codebase, and building them would produce guards
//! that can never fire:
//!
//! - The genesis handshake is a **gossipsub message**
//!   (`WrfcoinMessage::GenesisHandshake`), published after the connection is
//!   already established. There is no handshake stream protocol — the only
//!   `request_response` protocol in `behaviour.rs` is block sync.
//! - There is therefore no per-connection "handshake in progress" state to cap,
//!   and no read loop we own in which bytes could be dribbled. Gossipsub
//!   delivers whole, already-framed messages.
//! - **There is no signature verification in the handshake path at all.**
//!   `GenesisHandshakeMessage::verify_match` is string comparison of
//!   `network_id`, `chain_id`, `genesis_hash` and `protocol_version`. Nothing
//!   in `genesis_handshake.rs` touches a key or a signature.
//!
//! That last point matters most, because the acceptance criterion "test asserts
//! signature verification is not reached on a genesis-hash mismatch" cannot be
//! satisfied honestly: a test asserting we do not reach code that does not exist
//! passes no matter what anyone later writes. It is the exact vacuous-guard
//! shape this epic keeps producing.
//!
//! # The real exposure, and what this module does about it
//!
//! The issue's *scope* — "bound the cost of an unauthenticated peer before it
//! completes the genesis handshake" — is entirely real here; only the mechanism
//! was mis-specified. In this architecture the window is:
//!
//! 1. Connection establishes (libp2p's Noise handshake, already bounded).
//! 2. We publish our handshake and wait for theirs.
//! 3. On mismatch we disconnect the author.
//!
//! Nothing bounds step 2. **A peer that simply never publishes a handshake
//! holds its connection slot forever**, having proven nothing, while remaining
//! eligible to send other gossip. That is this architecture's slowloris: not
//! dribbled bytes, but an indefinitely unproven peer. Against a slot budget
//! that #2114 shows is finite, squatting is the attack.
//!
//! So this module tracks peers that have not yet proven chain identity, expires
//! them on a deadline, and caps how many an address may hold concurrently — the
//! same two defences the issue asked for, applied to the state that exists.

use std::net::IpAddr;
use std::time::{Duration, Instant};

use libp2p::multiaddr::Protocol;
use libp2p::{Multiaddr, PeerId};
use rustc_hash::FxHashMap;

/// Ceilings on peers that have not yet proven chain identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandshakeLimits {
    /// How long a peer may hold a connection without producing a matching
    /// genesis handshake. `Duration::ZERO` disables the deadline.
    pub handshake_timeout: Duration,
    /// Max concurrently-unproven peers from one source IP. `0` disables.
    pub max_concurrent_per_ip: u32,
    /// Whether a breach actually disconnects or refuses, as opposed to being
    /// counted.
    ///
    /// Defaults to `false` — **observe-only** — because handshake delivery is
    /// not guaranteed, so "unproven" does not reliably mean "hostile".
    ///
    /// The genesis handshake is published exactly once, at connection
    /// establishment (`node.rs`, the sole production publish site), and is
    /// never retried. Gossipsub subscription propagation takes at least a
    /// heartbeat, so a handshake published into a mesh that cannot yet deliver
    /// it is simply lost — and nothing republishes it. A peer can therefore
    /// remain unproven forever while being entirely healthy.
    ///
    /// Enforcing against that would disconnect good peers on a timer. Worse,
    /// the same gap means chain-identity verification silently never runs for
    /// such a peer, so the wrong-chain disconnect this module builds on has a
    /// hole of its own. Both are worth fixing; neither is fixed by refusing
    /// connections. The counters measure how often peers go unproven, which is
    /// the number that decides whether enforcement is safe to enable.
    pub enforce: bool,
}

impl Default for HandshakeLimits {
    /// `handshake_timeout = 30s`, deliberately **not** the 10s the issue
    /// suggests.
    ///
    /// 10s is the right order for a direct connection handshake, which
    /// completes in a round trip. Here the handshake is gossipsub-mediated, so
    /// it waits on mesh or fanout delivery and heartbeat timing rather than on
    /// one round trip. 30s keeps a wide margin over that while still being
    /// far below the horizon on which a squatting peer is worth anything.
    ///
    /// The failure mode if this is still too tight is recoverable rather than
    /// terminal — a dropped peer is redialed and republishes its handshake —
    /// and it is visible, because every drop increments a counter. Setting
    /// `WRFCOIN_P2P_HANDSHAKE_TIMEOUT_SECS=0` disables the deadline outright.
    ///
    /// `max_concurrent_per_ip = 4` matches `max_established_per_peer`, so one
    /// legitimate peer working through its connections is never refused for
    /// being itself.
    fn default() -> Self {
        Self {
            handshake_timeout: Duration::from_secs(30),
            max_concurrent_per_ip: 4,
            enforce: false,
        }
    }
}

/// Why an unproven peer was dropped or refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropReason {
    /// Held a connection past the deadline without proving chain identity.
    Timeout,
    /// Too many concurrently-unproven peers already from this address.
    ConcurrentPerIp,
    /// #2120: an inbound peer that is not on this validator's sentry allowlist.
    NotAllowlistedSentry,
}

impl DropReason {
    pub fn as_str(self) -> &'static str {
        match self {
            DropReason::Timeout => "handshake_timeout",
            DropReason::ConcurrentPerIp => "concurrent_per_ip",
            DropReason::NotAllowlistedSentry => "sentry_allowlist_refusal",
        }
    }
}

impl std::fmt::Display for DropReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Source IP of a multiaddr, if it has one.
fn source_ip(addr: &Multiaddr) -> Option<IpAddr> {
    addr.iter().find_map(|proto| match proto {
        Protocol::Ip4(ip) => Some(IpAddr::V4(ip)),
        Protocol::Ip6(ip) => Some(IpAddr::V6(ip)),
        _ => None,
    })
}

#[derive(Debug, Clone, Copy)]
struct Unproven {
    since: Instant,
    /// `None` for loopback and non-IP transports, which are not counted
    /// per-address (see [`UnprovenPeers::admit`]).
    counted_ip: Option<IpAddr>,
    /// The timeout counter is an edge, not a one-second occupancy gauge.
    ///
    /// Keep the peer in `peers` until proof or disconnect so enforcement can
    /// retry a failed close, but report the deadline transition only once.
    timeout_observed: bool,
}

/// Peers connected but not yet proven to be on our chain.
///
/// Time is passed in rather than read from the clock, so the deadline can be
/// tested by advancing a value instead of sleeping. Tests that sleep are how
/// this crate acquired the contention flakes in #2534.
#[derive(Debug)]
pub struct UnprovenPeers {
    limits: HandshakeLimits,
    peers: FxHashMap<PeerId, Unproven>,
    per_ip: FxHashMap<IpAddr, u32>,
}

impl UnprovenPeers {
    pub fn new(limits: HandshakeLimits) -> Self {
        Self {
            limits,
            peers: FxHashMap::default(),
            per_ip: FxHashMap::default(),
        }
    }

    pub fn limits(&self) -> &HandshakeLimits {
        &self.limits
    }

    /// Register a newly connected, not-yet-proven peer.
    ///
    /// `Err` means the address already holds its allowance of unproven peers
    /// and this connection should be closed immediately.
    ///
    /// Loopback is not counted per-address, matching [`crate::quota`]: local
    /// multi-node development shares one source IP as a matter of course, and
    /// an attacker with loopback access is past what this bounds.
    pub fn admit(
        &mut self,
        peer_id: PeerId,
        addr: &Multiaddr,
        now: Instant,
    ) -> Result<(), DropReason> {
        // Re-registering a peer we already track must not double-count its
        // address; a peer opening a second connection is one unproven peer.
        if let Some(existing) = self.peers.get(&peer_id) {
            let _ = existing;
            return Ok(());
        }

        let counted_ip = source_ip(addr).filter(|ip| !ip.is_loopback());

        if let Some(ip) = counted_ip {
            if self.limits.max_concurrent_per_ip > 0 {
                let current = self.per_ip.get(&ip).copied().unwrap_or(0);
                if current >= self.limits.max_concurrent_per_ip {
                    return Err(DropReason::ConcurrentPerIp);
                }
            }
            *self.per_ip.entry(ip).or_insert(0) += 1;
        }

        self.peers.insert(
            peer_id,
            Unproven {
                since: now,
                counted_ip,
                timeout_observed: false,
            },
        );
        Ok(())
    }

    /// The peer produced a matching genesis handshake. It stops being unproven
    /// and stops consuming its address's allowance.
    pub fn mark_proven(&mut self, peer_id: &PeerId) {
        self.release(peer_id);
    }

    /// The peer disconnected.
    pub fn remove(&mut self, peer_id: &PeerId) {
        self.release(peer_id);
    }

    fn release(&mut self, peer_id: &PeerId) {
        let Some(entry) = self.peers.remove(peer_id) else {
            return;
        };
        if let Some(ip) = entry.counted_ip {
            if let Some(count) = self.per_ip.get_mut(&ip) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    self.per_ip.remove(&ip);
                }
            }
        }
    }

    /// Peers that have held a connection past the deadline without proving
    /// chain identity.
    ///
    /// Returns them without removing them: the caller closes the connection,
    /// and the resulting disconnect event drives [`Self::remove`]. Removing
    /// here would drop the record before the close, and a peer whose close
    /// failed would silently stop being tracked.
    pub fn expired(&self, now: Instant) -> Vec<PeerId> {
        if self.limits.handshake_timeout.is_zero() {
            return Vec::new(); // deadline disabled
        }
        self.peers
            .iter()
            .filter(|(_, u)| now.duration_since(u.since) >= self.limits.handshake_timeout)
            .map(|(peer_id, _)| *peer_id)
            .collect()
    }

    /// Peers crossing the deadline for the first time in this admission.
    ///
    /// This is the metric edge. [`Self::expired`] intentionally continues to
    /// return the full expired set so enforcement can retry a close until the
    /// disconnect event removes the peer.
    pub fn newly_expired(&mut self, now: Instant) -> Vec<PeerId> {
        if self.limits.handshake_timeout.is_zero() {
            return Vec::new();
        }
        self.peers
            .iter_mut()
            .filter_map(|(peer_id, unproven)| {
                let crossed = now.duration_since(unproven.since)
                    >= self.limits.handshake_timeout;
                if crossed && !unproven.timeout_observed {
                    unproven.timeout_observed = true;
                    Some(*peer_id)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Is this peer still unproven?
    pub fn contains(&self, peer_id: &PeerId) -> bool {
        self.peers.contains_key(peer_id)
    }

    /// Number of currently-unproven peers.
    pub fn len(&self) -> usize {
        self.peers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    /// Concurrently-unproven peers charged to one address.
    pub fn count_for_ip(&self, ip: &IpAddr) -> u32 {
        self.per_ip.get(ip).copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(ip: &str) -> Multiaddr {
        format!("/ip4/{ip}/tcp/9000").parse().unwrap()
    }

    fn peer(n: u8) -> PeerId {
        let mut bytes = [0u8; 32];
        bytes[0] = n;
        let key = libp2p::identity::ed25519::SecretKey::try_from_bytes(bytes).unwrap();
        libp2p::identity::Keypair::from(libp2p::identity::ed25519::Keypair::from(key))
            .public()
            .to_peer_id()
    }

    /// A fixed origin plus explicit offsets. No sleeping — see `UnprovenPeers`.
    fn t0() -> Instant {
        Instant::now()
    }

    // -- The deadline --------------------------------------------------------

    #[test]
    fn a_peer_that_never_proves_chain_identity_expires() {
        let limits = HandshakeLimits {
            handshake_timeout: Duration::from_secs(30),
            ..Default::default()
        };
        let mut guard = UnprovenPeers::new(limits);
        let start = t0();

        guard.admit(peer(1), &addr("203.0.113.1"), start).unwrap();

        assert!(
            guard.expired(start + Duration::from_secs(29)).is_empty(),
            "must not expire before the deadline"
        );
        assert_eq!(
            guard.expired(start + Duration::from_secs(30)),
            vec![peer(1)],
            "at the deadline the peer has held a slot without proving anything"
        );
    }

    #[test]
    fn proving_chain_identity_stops_the_clock() {
        let mut guard = UnprovenPeers::new(HandshakeLimits::default());
        let start = t0();
        guard.admit(peer(1), &addr("203.0.113.1"), start).unwrap();

        guard.mark_proven(&peer(1));

        assert!(
            guard.expired(start + Duration::from_secs(3600)).is_empty(),
            "a proven peer must never be expired by the handshake deadline"
        );
        assert!(!guard.contains(&peer(1)));
    }

    #[test]
    fn expired_does_not_remove_so_a_failed_close_is_not_forgotten() {
        // `expired` reports; the disconnect event removes. If it removed here,
        // a peer whose close failed would stop being tracked while still
        // connected.
        let mut guard = UnprovenPeers::new(HandshakeLimits {
            handshake_timeout: Duration::from_secs(10),
            ..Default::default()
        });
        let start = t0();
        guard.admit(peer(1), &addr("203.0.113.1"), start).unwrap();

        let late = start + Duration::from_secs(11);
        assert_eq!(guard.expired(late).len(), 1);
        assert_eq!(guard.expired(late).len(), 1, "still reported on re-poll");
        assert!(guard.contains(&peer(1)));

        guard.remove(&peer(1));
        assert!(guard.expired(late).is_empty());
    }

    #[test]
    fn timeout_observation_is_edge_triggered_while_expired_stays_retryable() {
        let mut guard = UnprovenPeers::new(HandshakeLimits {
            handshake_timeout: Duration::from_secs(10),
            ..Default::default()
        });
        let start = t0();
        guard.admit(peer(1), &addr("203.0.113.1"), start).unwrap();
        let late = start + Duration::from_secs(11);

        assert_eq!(guard.newly_expired(late), vec![peer(1)]);
        assert!(
            guard.newly_expired(late + Duration::from_secs(1)).is_empty(),
            "one admission must produce one timeout metric edge"
        );
        assert_eq!(
            guard.expired(late + Duration::from_secs(1)),
            vec![peer(1)],
            "the full expired set remains available for disconnect retries"
        );
    }

    #[test]
    fn a_zero_timeout_disables_the_deadline() {
        let mut guard = UnprovenPeers::new(HandshakeLimits {
            handshake_timeout: Duration::ZERO,
            ..Default::default()
        });
        let start = t0();
        guard.admit(peer(1), &addr("203.0.113.1"), start).unwrap();
        assert!(
            guard.expired(start + Duration::from_secs(86_400)).is_empty(),
            "the documented escape hatch must actually disable the deadline"
        );
    }

    // -- Concurrent unproven peers per address -------------------------------

    #[test]
    fn concurrent_unproven_peers_are_capped_per_address() {
        let mut guard = UnprovenPeers::new(HandshakeLimits {
            max_concurrent_per_ip: 2,
            ..Default::default()
        });
        let start = t0();
        let a = addr("203.0.113.1");

        assert_eq!(guard.admit(peer(1), &a, start), Ok(()));
        assert_eq!(guard.admit(peer(2), &a, start), Ok(()));
        assert_eq!(
            guard.admit(peer(3), &a, start),
            Err(DropReason::ConcurrentPerIp)
        );

        // A different address is unaffected.
        assert_eq!(guard.admit(peer(4), &addr("203.0.113.2"), start), Ok(()));
    }

    #[test]
    fn proving_identity_frees_the_addresss_allowance() {
        // This is what makes the cap a bound on *unproven* peers rather than a
        // bound on peers: honest peers pass through it continuously.
        let mut guard = UnprovenPeers::new(HandshakeLimits {
            max_concurrent_per_ip: 1,
            ..Default::default()
        });
        let start = t0();
        let a = addr("203.0.113.1");

        assert_eq!(guard.admit(peer(1), &a, start), Ok(()));
        assert_eq!(guard.admit(peer(2), &a, start), Err(DropReason::ConcurrentPerIp));

        guard.mark_proven(&peer(1));

        assert_eq!(
            guard.admit(peer(2), &a, start),
            Ok(()),
            "the slot must be reusable once the first peer proved itself"
        );
    }

    #[test]
    fn disconnecting_frees_the_addresss_allowance() {
        let mut guard = UnprovenPeers::new(HandshakeLimits {
            max_concurrent_per_ip: 1,
            ..Default::default()
        });
        let start = t0();
        let a = addr("203.0.113.1");
        guard.admit(peer(1), &a, start).unwrap();
        guard.remove(&peer(1));
        assert_eq!(guard.admit(peer(2), &a, start), Ok(()));
        assert_eq!(guard.count_for_ip(&"203.0.113.1".parse().unwrap()), 1);
    }

    #[test]
    fn a_refused_peer_does_not_consume_the_allowance_it_was_refused_for() {
        // Otherwise a rejected attempt would permanently burn a slot, and an
        // attacker could lock an address out by being refused repeatedly.
        let mut guard = UnprovenPeers::new(HandshakeLimits {
            max_concurrent_per_ip: 1,
            ..Default::default()
        });
        let start = t0();
        let a = addr("203.0.113.1");
        guard.admit(peer(1), &a, start).unwrap();

        for i in 2..=10 {
            assert_eq!(guard.admit(peer(i), &a, start), Err(DropReason::ConcurrentPerIp));
        }

        guard.remove(&peer(1));
        assert_eq!(
            guard.admit(peer(11), &a, start),
            Ok(()),
            "nine refusals must not have consumed anything"
        );
    }

    #[test]
    fn readmitting_a_tracked_peer_does_not_double_count_it() {
        // A peer opening a second connection is still one unproven peer.
        let mut guard = UnprovenPeers::new(HandshakeLimits {
            max_concurrent_per_ip: 2,
            ..Default::default()
        });
        let start = t0();
        let a = addr("203.0.113.1");
        guard.admit(peer(1), &a, start).unwrap();
        guard.admit(peer(1), &a, start).unwrap();
        assert_eq!(guard.count_for_ip(&"203.0.113.1".parse().unwrap()), 1);
        assert_eq!(guard.len(), 1);
    }

    #[test]
    fn readmitting_does_not_reset_the_deadline() {
        // Otherwise a squatter reconnects on a second connection and buys
        // itself another full timeout, indefinitely.
        let mut guard = UnprovenPeers::new(HandshakeLimits {
            handshake_timeout: Duration::from_secs(10),
            ..Default::default()
        });
        let start = t0();
        let a = addr("203.0.113.1");
        guard.admit(peer(1), &a, start).unwrap();
        guard.admit(peer(1), &a, start + Duration::from_secs(9)).unwrap();

        assert_eq!(
            guard.expired(start + Duration::from_secs(10)),
            vec![peer(1)],
            "the clock must still run from first admission"
        );
    }

    #[test]
    fn zero_disables_the_concurrency_cap() {
        let mut guard = UnprovenPeers::new(HandshakeLimits {
            max_concurrent_per_ip: 0,
            ..Default::default()
        });
        let start = t0();
        let a = addr("203.0.113.1");
        for i in 1..=50 {
            assert_eq!(guard.admit(peer(i), &a, start), Ok(()));
        }
    }

    #[test]
    fn loopback_is_not_counted_per_address() {
        // Local multi-node development shares one source IP as a matter of
        // course; capping it would cap the development topology.
        let mut guard = UnprovenPeers::new(HandshakeLimits {
            max_concurrent_per_ip: 1,
            ..Default::default()
        });
        let start = t0();
        for i in 1..=20 {
            assert_eq!(guard.admit(peer(i), &addr("127.0.0.1"), start), Ok(()));
        }
    }

    #[test]
    fn loopback_is_still_subject_to_the_deadline() {
        // Exempt from the address cap is not exempt from proving itself: a
        // local peer that never handshakes is still holding a slot for nothing.
        let mut guard = UnprovenPeers::new(HandshakeLimits {
            handshake_timeout: Duration::from_secs(10),
            max_concurrent_per_ip: 1,
            ..Default::default()
        });
        let start = t0();
        guard.admit(peer(1), &addr("127.0.0.1"), start).unwrap();
        assert_eq!(
            guard.expired(start + Duration::from_secs(10)),
            vec![peer(1)]
        );
    }

    // -- Defaults and labels -------------------------------------------------

    #[test]
    fn the_default_deadline_clears_gossipsub_mesh_formation() {
        // The handshake here is gossipsub-mediated, so it waits on mesh or
        // fanout delivery rather than one round trip. If anyone tightens this
        // to the direct-handshake order of magnitude the issue suggested, this
        // fails rather than producing a peer-reconnect loop on the testnet.
        let d = HandshakeLimits::default();
        assert!(
            d.handshake_timeout >= Duration::from_secs(30),
            "deadline {:?} is too tight for a gossipsub-mediated handshake",
            d.handshake_timeout
        );
        assert!(d.max_concurrent_per_ip >= 4);
    }

    /// Enforcing a disconnect on a handshake that is published once and never
    /// retried would drop healthy peers on a timer, because a lost handshake is
    /// indistinguishable from a peer that never sent one.
    #[test]
    fn limits_are_observe_only_by_default() {
        assert!(
            !HandshakeLimits::default().enforce,
            "enforcement must stay off until the genesis handshake is delivered \
             reliably: it is published once at connection establishment, never \
             retried, and gossipsub subscription propagation can lose it"
        );
    }

    /// Observe-only must still *detect*. A flag that silenced the measurement
    /// as well as the action would leave nothing to decide on later.
    #[test]
    fn observe_only_still_reports_breaches() {
        let mut guard = UnprovenPeers::new(HandshakeLimits {
            handshake_timeout: Duration::from_secs(10),
            max_concurrent_per_ip: 1,
            enforce: false,
        });
        let start = t0();
        let a = addr("203.0.113.1");

        guard.admit(peer(1), &a, start).unwrap();
        assert_eq!(
            guard.admit(peer(2), &a, start),
            Err(DropReason::ConcurrentPerIp),
            "the verdict is still computed; only the caller's action is gated"
        );
        assert_eq!(
            guard.expired(start + Duration::from_secs(10)),
            vec![peer(1)],
            "the deadline is still evaluated in observe-only mode"
        );
    }

    #[test]
    fn drop_reason_labels_are_distinct() {
        assert_ne!(
            DropReason::Timeout.as_str(),
            DropReason::ConcurrentPerIp.as_str()
        );
    }
}
