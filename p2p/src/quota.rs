//! Per-IP and per-subnet connection quotas enforced *before* protocol upgrade
//! (#2113).
//!
//! # What this adds, and what it deliberately does not
//!
//! Issue #2113 lists six ceilings. Three of them — total connections, per-peer
//! connections and pending inbound connections — were already implemented in
//! #2037 and are enforced by libp2p's own `connection_limits::Behaviour`, which
//! sits first in [`crate::behaviour::WrfcoinBehaviour`]. Re-implementing them
//! here would create two independent sources of truth for the same ceiling,
//! which is strictly worse than having one: the two would drift, and an
//! operator reading one would draw wrong conclusions about the other. So this
//! module implements only the three *address-derived* dimensions that had no
//! enforcement at all:
//!
//! | Dimension | Enforced by |
//! |---|---|
//! | pending inbound / total / per-peer | `connection_limits::Behaviour` (#2037) |
//! | **per source IP** | this module |
//! | **per IPv4 /24** | this module |
//! | **per IPv6 /64** | this module |
//!
//! # Why pre-upgrade, and what "pre-upgrade" can actually mean
//!
//! The pre-existing `/16` check in the event loop (`PeerManager::subnet_allows`)
//! runs on `SwarmEvent::ConnectionEstablished` — *after* the Noise handshake,
//! the Yamux upgrade and every behaviour's `handle_established_*` hook have
//! already run. It then closes the connection retroactively. An attacker
//! therefore still extracts the full cost of a handshake per rejected
//! connection, which is precisely the work a flood ceiling is supposed to
//! avoid. This module hooks
//! [`NetworkBehaviour::handle_pending_inbound_connection`] instead, which runs
//! before the upgrade begins, so a rejected connection costs a map lookup.
//!
//! One consequence is worth stating plainly rather than leaving as a surprise:
//! **a per-peer ceiling cannot be enforced pre-upgrade at all.** The remote
//! `PeerId` is a product of the Noise handshake, so it does not exist yet at
//! the pending stage — the hook is handed only the local and remote
//! `Multiaddr`. That is not an oversight in libp2p; it is why libp2p's own
//! limiter splits its checks across the pending and established hooks. Address
//! quotas are the only kind that *can* be applied pre-upgrade, and they are
//! what this module applies.
//!
//! # Loopback is exempt
//!
//! Connections from a loopback address bypass every quota here. This is not a
//! weakening: an attacker who can already open loopback sockets on the node
//! host has capabilities that a connection ceiling does not address. Enforcing
//! against loopback would instead break the thing developers actually do —
//! running a dozen nodes on `127.0.0.1`, where a shared source IP is normal
//! rather than hostile.

use std::net::IpAddr;
use std::sync::Arc;
use std::task::{Context, Poll};

use libp2p::core::transport::PortUse;
use libp2p::core::Endpoint;
use libp2p::multiaddr::Protocol;
use libp2p::swarm::{
    ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandler, THandlerInEvent,
    THandlerOutEvent, ToSwarm, dummy,
};
use libp2p::{Multiaddr, PeerId};
use rustc_hash::FxHashMap;

use crate::metrics::MetricsCollector;

/// IPv4 grouping width. `/24` is the smallest block routinely allocated to a
/// single customer, so it is the finest grouping at which "these addresses are
/// one party" is a safe assumption.
pub const IPV4_SUBNET_BITS: u8 = 24;

/// IPv6 grouping width. `/64` is the standard single-LAN allocation, and an
/// IPv6 host can trivially source from every address inside its own `/64` —
/// which is why counting IPv6 by individual address alone is close to useless.
pub const IPV6_SUBNET_BITS: u8 = 64;

/// Why a pending inbound connection was refused. Each variant maps to its own
/// counter so an operator can tell *which* ceiling is biting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    /// Too many concurrent connections from one source IP.
    PerIp,
    /// Too many concurrent connections from one IPv4 `/24`.
    Ipv4Subnet,
    /// Too many concurrent connections from one IPv6 `/64`.
    Ipv6Subnet,
}

impl RejectReason {
    /// Stable identifier used in logs and in the metrics snapshot.
    pub fn as_str(self) -> &'static str {
        match self {
            RejectReason::PerIp => "per_ip",
            RejectReason::Ipv4Subnet => "ipv4_subnet",
            RejectReason::Ipv6Subnet => "ipv6_subnet",
        }
    }
}

impl std::fmt::Display for RejectReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::error::Error for RejectReason {}

/// Address-derived connection ceilings.
///
/// Every field uses the `0 = disabled` sentinel already established by
/// `max_peers_per_subnet` elsewhere in this crate, so a single dimension can be
/// switched off in the field without rebuilding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectionLimits {
    /// Max concurrent connections from one source IP address.
    pub max_per_ip: u32,
    /// Max concurrent connections from one IPv4 `/24`.
    pub max_per_ipv4_subnet: u32,
    /// Max concurrent connections from one IPv6 `/64`.
    pub max_per_ipv6_subnet: u32,
}

impl Default for ConnectionLimits {
    /// Defaults sized with explicit headroom over the existing ~12-node
    /// testnet, deliberately loose rather than tight.
    ///
    /// The arithmetic, because a ceiling nobody can reconstruct is a ceiling
    /// nobody will dare tune:
    ///
    /// - `max_established_per_peer` already defaults to **4** (#2037), so one
    ///   peer identity can legitimately hold 4 connections.
    /// - The testnet runs ~**12** nodes. The pathological-but-legitimate case
    ///   is all 12 sharing one `/24` — which is the *normal* case on a LAN
    ///   (`192.168.0.0/24`) and entirely possible on a tailnet. That is
    ///   `12 × 4 = 48` concurrent connections from one `/24`.
    /// - `128` therefore leaves ~2.6× headroom on the worst legitimate case,
    ///   while still shedding a flood two orders of magnitude before the
    ///   256-connection global cap.
    ///
    /// `max_per_ip = 16` allows four distinct peers behind one NAT to each hold
    /// their full four connections. Loopback is exempt (see module docs), so
    /// this does not constrain local multi-node development.
    ///
    /// These are ceilings on *hostile* concentration, not sizing targets. If a
    /// deployment legitimately exceeds them the fix is to raise them, and the
    /// per-reason counters are what tells you that is happening.
    fn default() -> Self {
        Self {
            max_per_ip: 16,
            max_per_ipv4_subnet: 128,
            max_per_ipv6_subnet: 128,
        }
    }
}

/// One counted grouping. A single connection is counted against both its exact
/// IP and its enclosing subnet, so the two ceilings compose rather than one
/// masking the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum QuotaKey {
    Ip(IpAddr),
    V4Subnet([u8; 3]),
    V6Subnet([u8; 8]),
}

impl QuotaKey {
    /// The reason reported when *this* grouping is the one over its ceiling.
    fn reason(self) -> RejectReason {
        match self {
            QuotaKey::Ip(_) => RejectReason::PerIp,
            QuotaKey::V4Subnet(_) => RejectReason::Ipv4Subnet,
            QuotaKey::V6Subnet(_) => RejectReason::Ipv6Subnet,
        }
    }

    /// This grouping's ceiling, or `None` when the dimension is disabled.
    fn limit(self, limits: &ConnectionLimits) -> Option<u32> {
        let raw = match self {
            QuotaKey::Ip(_) => limits.max_per_ip,
            QuotaKey::V4Subnet(_) => limits.max_per_ipv4_subnet,
            QuotaKey::V6Subnet(_) => limits.max_per_ipv6_subnet,
        };
        (raw > 0).then_some(raw)
    }
}

/// Extract the source IP from a multiaddr, if it has one.
///
/// Returns `None` for address families with no IP component — `/memory`
/// transports used in tests, and relayed circuits whose observed address is the
/// relay's rather than the peer's. Those are not counted, which is the correct
/// behaviour: counting a relayed connection against the *relay's* IP would let
/// one busy relay exhaust its own quota and lock out every peer behind it.
fn source_ip(addr: &Multiaddr) -> Option<IpAddr> {
    addr.iter().find_map(|proto| match proto {
        Protocol::Ip4(ip) => Some(IpAddr::V4(ip)),
        Protocol::Ip6(ip) => Some(IpAddr::V6(ip)),
        _ => None,
    })
}

/// The keys a connection from `ip` is counted against: its exact address and
/// its enclosing subnet.
fn keys_for(ip: IpAddr) -> [QuotaKey; 2] {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            [QuotaKey::Ip(ip), QuotaKey::V4Subnet([o[0], o[1], o[2]])]
        },
        IpAddr::V6(v6) => {
            let o = v6.octets();
            let mut prefix = [0u8; 8];
            prefix.copy_from_slice(&o[..8]);
            [QuotaKey::Ip(ip), QuotaKey::V6Subnet(prefix)]
        },
    }
}

/// Concurrent-connection counts per address grouping.
///
/// Kept free of any libp2p behaviour machinery so the counting rules can be
/// tested directly, without a swarm.
#[derive(Debug)]
pub struct ConnectionQuota {
    limits: ConnectionLimits,
    counts: FxHashMap<QuotaKey, u32>,
    /// Keys charged to each admitted connection. Release consults this rather
    /// than re-deriving from an address, so a connection can never decrement a
    /// grouping it was not charged to — including the case where the address is
    /// no longer available at close time.
    ///
    /// `Option` slots rather than a `Vec` keep admission allocation-free, which
    /// matters on a path whose entire purpose is being cheap under flood.
    held: FxHashMap<ConnectionId, [Option<QuotaKey>; 2]>,
}

impl ConnectionQuota {
    pub fn new(limits: ConnectionLimits) -> Self {
        Self {
            limits,
            counts: FxHashMap::default(),
            held: FxHashMap::default(),
        }
    }

    /// Admit a pending inbound connection, charging it to its address
    /// groupings.
    ///
    /// Admission is all-or-nothing: if any grouping is at its ceiling, no
    /// grouping is incremented. A partially-charged connection could never be
    /// released correctly.
    pub fn admit(
        &mut self,
        connection_id: ConnectionId,
        remote: &Multiaddr,
    ) -> Result<(), RejectReason> {
        let Some(ip) = source_ip(remote) else {
            return Ok(()); // no IP to count — see `source_ip`
        };
        if ip.is_loopback() {
            return Ok(()); // see module docs
        }

        // Only groupings whose dimension is enabled are charged at all. A
        // disabled dimension is then genuinely absent from the map rather than
        // silently accumulating counts nobody reads.
        let mut charged: [Option<QuotaKey>; 2] = [None, None];
        for (slot, key) in charged.iter_mut().zip(keys_for(ip)) {
            if key.limit(&self.limits).is_some() {
                *slot = Some(key);
            }
        }

        // Check every charged grouping before mutating any of them.
        for key in charged.iter().flatten() {
            // `limit` is `Some` for every charged key by construction above.
            let limit = key.limit(&self.limits).unwrap_or(u32::MAX);
            let current = self.counts.get(key).copied().unwrap_or(0);
            if current >= limit {
                return Err(key.reason());
            }
        }

        for key in charged.iter().flatten() {
            *self.counts.entry(*key).or_insert(0) += 1;
        }
        if charged.iter().any(Option::is_some) {
            self.held.insert(connection_id, charged);
        }
        Ok(())
    }

    /// Release a connection's charge. A no-op for connections that were never
    /// admitted — a rejected connection still produces a close/failure event,
    /// and must not decrement a grouping it never incremented.
    pub fn release(&mut self, connection_id: ConnectionId) {
        let Some(keys) = self.held.remove(&connection_id) else {
            return;
        };
        for key in keys.iter().flatten() {
            if let Some(count) = self.counts.get_mut(key) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    self.counts.remove(key);
                }
            }
        }
    }

    /// Number of groupings currently tracked. Used by tests to assert that
    /// releasing drains state rather than leaking it.
    pub fn tracked_groupings(&self) -> usize {
        self.counts.len()
    }

    /// Number of connections currently holding a charge.
    pub fn tracked_connections(&self) -> usize {
        self.held.len()
    }
}

/// A [`NetworkBehaviour`] that applies [`ConnectionQuota`] at the pending
/// inbound stage.
///
/// It has no connection handler and emits no events — its entire purpose is the
/// veto in `handle_pending_inbound_connection`.
pub struct QuotaBehaviour {
    quota: ConnectionQuota,
    metrics: Arc<MetricsCollector>,
}

impl QuotaBehaviour {
    // `pub(crate)`: `MetricsCollector` is a crate-internal type, so this
    // constructor cannot be part of the public surface.
    pub(crate) fn new(limits: ConnectionLimits, metrics: Arc<MetricsCollector>) -> Self {
        Self {
            quota: ConnectionQuota::new(limits),
            metrics,
        }
    }

    /// Read-only view, for tests and diagnostics.
    pub fn quota(&self) -> &ConnectionQuota {
        &self.quota
    }
}

impl NetworkBehaviour for QuotaBehaviour {
    type ConnectionHandler = dummy::ConnectionHandler;
    type ToSwarm = std::convert::Infallible;

    fn handle_pending_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        _local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        match self.quota.admit(connection_id, remote_addr) {
            Ok(()) => Ok(()),
            Err(reason) => {
                self.metrics.record_quota_rejection(reason);
                tracing::debug!(
                    "Refusing pending inbound connection from {remote_addr}: {reason} quota reached"
                );
                Err(ConnectionDenied::new(reason))
            },
        }
    }

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _peer: PeerId,
        _local_addr: &Multiaddr,
        _remote_addr: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(dummy::ConnectionHandler)
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _peer: PeerId,
        _addr: &Multiaddr,
        _role_override: Endpoint,
        _port_use: PortUse,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(dummy::ConnectionHandler)
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        match event {
            // The connection is gone; give its groupings their capacity back.
            FromSwarm::ConnectionClosed(closed) => self.quota.release(closed.connection_id),
            // An inbound connection we admitted at the pending stage never
            // reached establishment — denied by another behaviour, or the
            // upgrade failed. Without this the grouping leaks capacity
            // permanently, and a quota that only ever counts up is a slow
            // self-inflicted outage rather than a defence.
            FromSwarm::ListenFailure(failure) => self.quota.release(failure.connection_id),
            _ => {},
        }
    }

    fn on_connection_handler_event(
        &mut self,
        _peer_id: PeerId,
        _connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        // `dummy::ConnectionHandler` never produces events.
        match event {}
    }

    fn poll(&mut self, _cx: &mut Context<'_>) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Distinct `ConnectionId`s. `ConnectionId::new_unchecked` is the only
    /// public constructor, which is fine here — these ids never reach a swarm.
    fn conn(n: usize) -> ConnectionId {
        ConnectionId::new_unchecked(n)
    }

    fn tcp(addr: &str) -> Multiaddr {
        format!("/ip4/{addr}/tcp/9000").parse().unwrap()
    }

    fn tcp6(addr: &str) -> Multiaddr {
        format!("/ip6/{addr}/tcp/9000").parse().unwrap()
    }

    /// Only the per-IP dimension active, so a rejection can only be per-IP.
    fn only_per_ip(max: u32) -> ConnectionLimits {
        ConnectionLimits {
            max_per_ip: max,
            max_per_ipv4_subnet: 0,
            max_per_ipv6_subnet: 0,
        }
    }

    /// Only the IPv4 `/24` dimension active.
    fn only_v4_subnet(max: u32) -> ConnectionLimits {
        ConnectionLimits {
            max_per_ip: 0,
            max_per_ipv4_subnet: max,
            max_per_ipv6_subnet: 0,
        }
    }

    #[test]
    fn n_plus_one_connections_from_one_ip_rejects_the_last() {
        let mut q = ConnectionQuota::new(only_per_ip(3));
        for i in 0..3 {
            assert_eq!(q.admit(conn(i), &tcp("203.0.113.7")), Ok(()), "conn {i}");
        }
        assert_eq!(
            q.admit(conn(3), &tcp("203.0.113.7")),
            Err(RejectReason::PerIp)
        );
    }

    #[test]
    fn n_plus_one_distinct_ips_in_one_slash24_rejects_the_last() {
        let mut q = ConnectionQuota::new(only_v4_subnet(3));
        // Three *different* addresses, all inside 203.0.113.0/24.
        for (i, host) in ["203.0.113.1", "203.0.113.2", "203.0.113.3"]
            .into_iter()
            .enumerate()
        {
            assert_eq!(q.admit(conn(i), &tcp(host)), Ok(()), "{host}");
        }
        assert_eq!(
            q.admit(conn(9), &tcp("203.0.113.4")),
            Err(RejectReason::Ipv4Subnet),
            "a fourth distinct address in the same /24 must still be refused — \
             per-IP counting alone would have let this through"
        );
    }

    #[test]
    fn distinct_slash24_subnets_are_all_accepted() {
        let mut q = ConnectionQuota::new(only_v4_subnet(1));
        // Differ in the third octet, so each is its own /24.
        assert_eq!(q.admit(conn(0), &tcp("203.0.113.1")), Ok(()));
        assert_eq!(q.admit(conn(1), &tcp("203.0.114.1")), Ok(()));
        assert_eq!(q.admit(conn(2), &tcp("203.0.115.1")), Ok(()));
        assert_eq!(q.tracked_groupings(), 3);
    }

    #[test]
    fn slash16_neighbours_are_not_grouped_together() {
        // Guards the /24 boundary specifically: these share a /16 but not a
        // /24, so the pre-existing /16 rule would group them and this one
        // must not.
        let mut q = ConnectionQuota::new(only_v4_subnet(1));
        assert_eq!(q.admit(conn(0), &tcp("198.51.100.1")), Ok(()));
        assert_eq!(q.admit(conn(1), &tcp("198.51.200.1")), Ok(()));
    }

    #[test]
    fn ipv6_addresses_are_grouped_by_slash64() {
        let mut q = ConnectionQuota::new(ConnectionLimits {
            max_per_ip: 0,
            max_per_ipv4_subnet: 0,
            max_per_ipv6_subnet: 2,
        });
        // Same /64, different interface identifiers.
        assert_eq!(q.admit(conn(0), &tcp6("2001:db8:1:2::1")), Ok(()));
        assert_eq!(q.admit(conn(1), &tcp6("2001:db8:1:2::2")), Ok(()));
        assert_eq!(
            q.admit(conn(2), &tcp6("2001:db8:1:2::3")),
            Err(RejectReason::Ipv6Subnet),
            "an IPv6 host owns its whole /64, so per-address counting alone \
             would be free to bypass"
        );
        // A different /64 is unaffected.
        assert_eq!(q.admit(conn(3), &tcp6("2001:db8:1:3::1")), Ok(()));
    }

    #[test]
    fn per_ip_cap_is_independent_of_the_subnet_cap() {
        // Subnet ceiling is roomy; the per-IP ceiling must still bite, and the
        // reported reason must be the per-IP one.
        let mut q = ConnectionQuota::new(ConnectionLimits {
            max_per_ip: 2,
            max_per_ipv4_subnet: 100,
            max_per_ipv6_subnet: 100,
        });
        assert_eq!(q.admit(conn(0), &tcp("203.0.113.7")), Ok(()));
        assert_eq!(q.admit(conn(1), &tcp("203.0.113.7")), Ok(()));
        assert_eq!(
            q.admit(conn(2), &tcp("203.0.113.7")),
            Err(RejectReason::PerIp)
        );
        // A neighbour in the same /24 is still admitted: the per-IP ceiling
        // constrains one address, not the block.
        assert_eq!(q.admit(conn(3), &tcp("203.0.113.8")), Ok(()));
    }

    #[test]
    fn releasing_a_connection_returns_its_capacity() {
        let mut q = ConnectionQuota::new(only_per_ip(1));
        assert_eq!(q.admit(conn(0), &tcp("203.0.113.7")), Ok(()));
        assert_eq!(
            q.admit(conn(1), &tcp("203.0.113.7")),
            Err(RejectReason::PerIp)
        );

        q.release(conn(0));

        assert_eq!(
            q.admit(conn(1), &tcp("203.0.113.7")),
            Ok(()),
            "capacity must come back on close, or the quota is a one-way \
             ratchet that eventually refuses everyone"
        );
    }

    #[test]
    fn releasing_drains_state_rather_than_leaking_it() {
        let mut q = ConnectionQuota::new(ConnectionLimits::default());
        for i in 0..5 {
            assert_eq!(q.admit(conn(i), &tcp(&format!("203.0.113.{i}"))), Ok(()));
        }
        assert_eq!(q.tracked_connections(), 5);
        for i in 0..5 {
            q.release(conn(i));
        }
        assert_eq!(q.tracked_connections(), 0);
        assert_eq!(
            q.tracked_groupings(),
            0,
            "zeroed groupings must be removed, not left as empty entries — \
             otherwise the map grows without bound over a long uptime"
        );
    }

    #[test]
    fn releasing_a_connection_that_was_never_admitted_is_a_no_op() {
        // A rejected connection still produces a close/failure event. If that
        // decremented the grouping, an attacker could mint capacity by making
        // rejected attempts.
        let mut q = ConnectionQuota::new(only_per_ip(1));
        assert_eq!(q.admit(conn(0), &tcp("203.0.113.7")), Ok(()));
        assert_eq!(
            q.admit(conn(1), &tcp("203.0.113.7")),
            Err(RejectReason::PerIp)
        );

        q.release(conn(1)); // the rejected one
        q.release(conn(42)); // never seen at all

        assert_eq!(
            q.admit(conn(2), &tcp("203.0.113.7")),
            Err(RejectReason::PerIp),
            "the surviving connection still holds the only slot"
        );
    }

    #[test]
    fn a_rejected_admission_charges_nothing() {
        // Admission is all-or-nothing. Here the /24 is full but the exact IP is
        // not; the rejection must not leave the per-IP grouping incremented.
        let mut q = ConnectionQuota::new(ConnectionLimits {
            max_per_ip: 10,
            max_per_ipv4_subnet: 1,
            max_per_ipv6_subnet: 10,
        });
        assert_eq!(q.admit(conn(0), &tcp("203.0.113.1")), Ok(()));
        let before = q.tracked_groupings();
        assert_eq!(
            q.admit(conn(1), &tcp("203.0.113.2")),
            Err(RejectReason::Ipv4Subnet)
        );
        assert_eq!(
            q.tracked_groupings(),
            before,
            "the refused connection must not have created a per-IP grouping"
        );

        // And releasing the survivor must fully clear the /24.
        q.release(conn(0));
        assert_eq!(q.tracked_groupings(), 0);
    }

    #[test]
    fn zero_disables_one_dimension_without_disabling_the_others() {
        let mut q = ConnectionQuota::new(ConnectionLimits {
            max_per_ip: 0, // disabled
            max_per_ipv4_subnet: 2,
            max_per_ipv6_subnet: 0,
        });
        // Far past any plausible per-IP ceiling, from one address.
        assert_eq!(q.admit(conn(0), &tcp("203.0.113.7")), Ok(()));
        assert_eq!(q.admit(conn(1), &tcp("203.0.113.7")), Ok(()));
        // But the /24 ceiling is still live.
        assert_eq!(
            q.admit(conn(2), &tcp("203.0.113.7")),
            Err(RejectReason::Ipv4Subnet)
        );
    }

    #[test]
    fn loopback_is_exempt_from_every_dimension() {
        let mut q = ConnectionQuota::new(ConnectionLimits {
            max_per_ip: 1,
            max_per_ipv4_subnet: 1,
            max_per_ipv6_subnet: 1,
        });
        for i in 0..50 {
            assert_eq!(
                q.admit(conn(i), &tcp("127.0.0.1")),
                Ok(()),
                "loopback connection {i} must be admitted"
            );
        }
        assert_eq!(q.admit(conn(100), &tcp6("::1")), Ok(()));
        assert_eq!(
            q.tracked_groupings(),
            0,
            "exempt connections must not be tracked at all"
        );
    }

    #[test]
    fn addresses_without_an_ip_component_are_not_counted() {
        let mut q = ConnectionQuota::new(ConnectionLimits {
            max_per_ip: 1,
            max_per_ipv4_subnet: 1,
            max_per_ipv6_subnet: 1,
        });
        let memory: Multiaddr = "/memory/12345".parse().unwrap();
        assert_eq!(q.admit(conn(0), &memory), Ok(()));
        assert_eq!(q.admit(conn(1), &memory), Ok(()));
        assert_eq!(q.tracked_groupings(), 0);
    }

    #[test]
    fn reject_reasons_have_distinct_stable_labels() {
        // These strings appear in metrics and logs; collisions would silently
        // merge two counters into one.
        let labels = [
            RejectReason::PerIp.as_str(),
            RejectReason::Ipv4Subnet.as_str(),
            RejectReason::Ipv6Subnet.as_str(),
        ];
        let unique: std::collections::HashSet<_> = labels.iter().collect();
        assert_eq!(unique.len(), labels.len(), "labels: {labels:?}");
    }

    #[test]
    fn documented_defaults_clear_the_testnet_worst_case() {
        // The default doc comment claims headroom over "12 nodes x 4
        // connections all sharing one /24". If someone tightens the defaults
        // without revisiting that reasoning, this fails rather than silently
        // making the testnet unreachable.
        let d = ConnectionLimits::default();
        let worst_case_legitimate = 12 * 4;
        assert!(
            d.max_per_ipv4_subnet > worst_case_legitimate,
            "/24 default {} does not clear the {worst_case_legitimate}-connection \
             worst case for a 12-node testnet sharing one /24",
            d.max_per_ipv4_subnet
        );
        assert!(d.max_per_ipv6_subnet > worst_case_legitimate);
        assert!(
            d.max_per_ip >= 16,
            "per-IP default must leave room for several peers behind one NAT"
        );
    }
}
