//! Connected-peer tracking with subnet diversity enforcement.
//!
//! IPv4 peers are grouped by **/16**, IPv6 peers by **/64** (#2113).
//!
//! # Why IPv6 needed its own key
//!
//! `subnet_prefix` returned `Option<[u8; 2]>` extracted from the `/ip4/`
//! component, and `subnet_allows` mapped `None` to *allowed*. Every IPv6 peer
//! therefore bypassed subnet diversity **entirely** — and an IPv6 host
//! effectively owns a whole /64, i.e. unlimited addresses, which is exactly the
//! unlimited-identity case the quota exists to bound. It was latent rather than
//! live only because the node listens on `/ip4/` addresses today; adding one
//! IPv6 listen address would have switched the quota off for those peers with no
//! error, no metric, and a comment that read as deliberate.
//!
//! # Why IPv4 stays at /16
//!
//! #2113 asks for /24. Narrowing /16 → /24 creates 256× more buckets, so it
//! ALLOWS more peers per /16 — a loosening, and one whose effect depends on how
//! the real testnet's addresses are distributed. Two nodes behind one hosting
//! provider's /16 are common. That decision needs measurement, so it is
//! deliberately not made here; this change closes the silent bypass and nothing
//! else.
//!
//! `None` (a DNS or otherwise non-IP multiaddr) is still allowed: there is no
//! address to attribute, and refusing would reject legitimate `/dns/` peers.

use libp2p::{Multiaddr, PeerId, multiaddr::Protocol, swarm::ConnectionId};
use rustc_hash::FxHashMap;

/// The subnet a peer's address belongs to, for diversity accounting.
///
/// IPv4 and IPv6 keys are distinct variants rather than a flattened byte array
/// so a v4 prefix can never collide with the first bytes of a v6 prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SubnetKey {
    /// First two octets of an IPv4 address — a /16.
    V4([u8; 2]),
    /// First eight octets of an IPv6 address — a /64, the standard single-host
    /// allocation and therefore the smallest unit an attacker cannot trivially
    /// multiply.
    V6([u8; 8]),
}

/// Extract the subnet key from a multiaddr.
///
/// Returns `Some(V4)` for `/ip4/…`, `Some(V6)` for `/ip6/…`, and `None` for an
/// address with no IP component (e.g. `/dns/…`), which cannot be attributed to a
/// subnet and is therefore not counted.
pub fn subnet_prefix(addr: &Multiaddr) -> Option<SubnetKey> {
    for proto in addr.iter() {
        match proto {
            Protocol::Ip4(ip) => {
                let o = ip.octets();
                return Some(SubnetKey::V4([o[0], o[1]]));
            },
            Protocol::Ip6(ip) => {
                let o = ip.octets();
                return Some(SubnetKey::V6([
                    o[0], o[1], o[2], o[3], o[4], o[5], o[6], o[7],
                ]));
            },
            _ => {},
        }
    }
    None
}

/// Extract the remote IP address from a multiaddr, v4 or v6 (#2118).
///
/// Distinct from [`subnet_prefix`], which is IPv4-only and /16: ban-score aggregation needs
/// the full address so it can bucket at /24 and /64. Returns `None` for addresses with no
/// IP component (memory transports, DNS multiaddrs before resolution) — and `None` must
/// mean "no subnet suspicion recorded", never "allow", because an address we cannot parse
/// tells us nothing about the peer.
pub fn remote_ip(addr: &Multiaddr) -> Option<std::net::IpAddr> {
    for proto in addr.iter() {
        match proto {
            Protocol::Ip4(ip) => return Some(std::net::IpAddr::V4(ip)),
            Protocol::Ip6(ip) => return Some(std::net::IpAddr::V6(ip)),
            _ => {},
        }
    }
    None
}

/// Returns `true` when the multiaddr uses QUIC transport.
pub fn multiaddr_uses_quic(addr: &Multiaddr) -> bool {
    addr.iter().any(|proto| matches!(proto, Protocol::QuicV1))
}

/// Information about a single established connection to a peer.
#[derive(Debug, Clone)]
pub struct PeerConnection {
    pub id: ConnectionId,
    pub is_quic: bool,
}

/// Information about a connected peer.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// Known listen addresses for this peer.
    pub addresses: Vec<Multiaddr>,
    /// The remote end of the connection (from `ConnectedPoint`).
    /// Used to decrement the /16 subnet counter when the final connection
    /// to this peer closes.
    pub subnet_remote_addr: Option<Multiaddr>,
    /// Active connections to the peer.
    pub connections: Vec<PeerConnection>,
    /// The peer's self-reported chain height (from `ChainStatus` messages).
    pub chain_height: Option<u64>,
    /// Hash of the peer's tip block.
    pub chain_tip: Option<[u8; 32]>,
    /// Merkle root of the peer's state trie (from `ChainStatus` messages).
    pub state_root: Option<[u8; 32]>,
    /// When this peer entry was created (first connection established) —
    /// drives the connection-age surface (#2820).
    pub connected_at: std::time::Instant,
}

impl PeerInfo {
    /// Create a new `PeerInfo` entry for a freshly connected peer.
    pub fn new(_peer_id: PeerId) -> Self {
        Self {
            addresses: Vec::new(),
            subnet_remote_addr: None,
            connections: Vec::new(),
            chain_height: None,
            chain_tip: None,
            state_root: None,
            connected_at: std::time::Instant::now(),
        }
    }

    /// Update chain status from a `ChainStatus` gossipsub message.
    pub fn update_chain_status(&mut self, height: u64, tip_hash: [u8; 32], state_root: [u8; 32]) {
        self.chain_height = Some(height);
        self.chain_tip = Some(tip_hash);
        self.state_root = Some(state_root);
    }

    /// Add a discovered address for this peer (deduplicating).
    pub fn add_address(&mut self, addr: Multiaddr) {
        if !self.addresses.contains(&addr) {
            self.addresses.push(addr);
        }
    }

    /// Register a new active connection for this peer.
    ///
    /// Returns `true` when this is the first active connection to the peer.
    pub fn add_connection(&mut self, connection_id: ConnectionId, remote_addr: Multiaddr) -> bool {
        if self
            .connections
            .iter()
            .any(|connection| connection.id == connection_id)
        {
            return self.connections.len() == 1;
        }

        let is_first_connection = self.connections.is_empty();
        if is_first_connection {
            self.subnet_remote_addr = Some(remote_addr.clone());
        }

        self.connections.push(PeerConnection {
            id: connection_id,
            is_quic: multiaddr_uses_quic(&remote_addr),
        });

        is_first_connection
    }

    /// Remove a single connection from the peer.
    pub fn remove_connection(&mut self, connection_id: ConnectionId) -> Option<PeerConnection> {
        self.connections
            .iter()
            .position(|connection| connection.id == connection_id)
            .map(|position| self.connections.remove(position))
    }

    /// Returns `true` when any active connection to this peer uses QUIC.
    pub fn has_quic_connection(&self) -> bool {
        self.connections.iter().any(|connection| connection.is_quic)
    }

    /// Returns `true` when any active connection to this peer does not use QUIC.
    #[cfg(test)]
    pub fn has_non_quic_connection(&self) -> bool {
        self.connections
            .iter()
            .any(|connection| !connection.is_quic)
    }

    /// Connection IDs that should be pruned once a QUIC connection exists.
    pub fn redundant_non_quic_connection_ids(&self) -> Vec<ConnectionId> {
        if !self.has_quic_connection() {
            return Vec::new();
        }

        self.connections
            .iter()
            .filter(|connection| !connection.is_quic)
            .map(|connection| connection.id)
            .collect()
    }

    /// Only retire TCP after Identify was received on a still-active QUIC
    /// connection. Transport establishment alone can precede the remote swarm
    /// admitting that connection; closing TCP then can reset its gossip state.
    pub fn redundant_non_quic_after_identify(
        &self,
        identified: ConnectionId,
    ) -> Vec<ConnectionId> {
        if !self
            .connections
            .iter()
            .any(|connection| connection.id == identified && connection.is_quic)
        {
            return Vec::new();
        }
        self.redundant_non_quic_connection_ids()
    }

    /// Known QUIC listen addresses advertised by this peer.
    pub fn known_quic_addresses(&self) -> Vec<Multiaddr> {
        self.addresses
            .iter()
            .filter(|addr| multiaddr_uses_quic(addr))
            .cloned()
            .collect()
    }
}

/// Tracks the set of currently connected peers and per-subnet connection counts.
pub struct PeerManager {
    peers: FxHashMap<PeerId, PeerInfo>,
    /// Maps /16 subnet prefix (first 2 octets of IPv4 address) to the count
    /// of currently connected peers from that subnet.
    subnet_counts: FxHashMap<SubnetKey, usize>,
}

impl Default for PeerManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PeerManager {
    /// Create an empty peer manager.
    pub fn new() -> Self {
        Self {
            peers: FxHashMap::default(),
            subnet_counts: FxHashMap::default(),
        }
    }

    /// Returns `true` when the peer is already tracked.
    pub fn contains(&self, peer_id: &PeerId) -> bool {
        self.peers.contains_key(peer_id)
    }

    /// Record a new connection.  If the peer was already tracked the existing
    /// entry is updated instead.
    ///
    /// Returns `true` when this is the first active connection to the peer.
    pub fn peer_connected(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        remote_addr: Multiaddr,
    ) -> bool {
        let is_new_peer = !self.peers.contains_key(&peer_id);
        let info = self
            .peers
            .entry(peer_id)
            .or_insert_with(|| PeerInfo::new(peer_id));
        let is_first_connection = info.add_connection(connection_id, remote_addr.clone());

        if is_new_peer && is_first_connection {
            self.record_subnet(&remote_addr);
        }

        is_first_connection
    }

    /// Remove a connection for a peer.
    ///
    /// Returns `true` when the peer no longer has any active connections.
    pub fn peer_connection_closed(
        &mut self,
        peer_id: &PeerId,
        connection_id: ConnectionId,
    ) -> bool {
        let Some(info) = self.peers.get_mut(peer_id) else {
            return false;
        };

        if info.remove_connection(connection_id).is_none() {
            return false;
        }

        if !info.connections.is_empty() {
            return false;
        }

        let subnet_addr = info.subnet_remote_addr.clone();
        self.peers.remove(peer_id);
        if let Some(addr) = subnet_addr.as_ref() {
            self.release_subnet(addr);
        }

        true
    }

    /// Mutable look up.
    /// Immutable peer lookup (read-only surfaces, #2820).
    pub fn get(&self, peer_id: &PeerId) -> Option<&PeerInfo> {
        self.peers.get(peer_id)
    }

    pub fn get_mut(&mut self, peer_id: &PeerId) -> Option<&mut PeerInfo> {
        self.peers.get_mut(peer_id)
    }

    /// Total number of connected peers.
    pub fn connected_count(&self) -> usize {
        self.peers.len()
    }

    // -- Subnet diversity ---------------------------------------------------

    /// Check whether adding a peer from the given remote address would exceed
    /// the per-subnet limit. Returns `true` if the connection should be
    /// allowed.
    ///
    /// If `max_peers_per_subnet` is 0, enforcement is disabled and this always
    /// returns `true`.
    pub fn subnet_allows(&self, remote_addr: &Multiaddr, max_peers_per_subnet: usize) -> bool {
        if max_peers_per_subnet == 0 {
            return true; // enforcement disabled
        }
        match subnet_prefix(remote_addr) {
            Some(prefix) => {
                let current = self.subnet_counts.get(&prefix).copied().unwrap_or(0);
                current < max_peers_per_subnet
            },
            // No IP component (e.g. `/dns/…`): nothing to attribute.
            None => true,
        }
    }

    /// Record that a peer from the given address has connected.
    /// Increments the subnet counter for its /16 (v4) or /64 (v6).
    pub fn record_subnet(&mut self, remote_addr: &Multiaddr) {
        if let Some(prefix) = subnet_prefix(remote_addr) {
            *self.subnet_counts.entry(prefix).or_insert(0) += 1;
        }
    }

    /// Remove a peer's subnet contribution when it disconnects.
    /// Decrements the subnet counter, removing the entry if it reaches 0.
    pub fn release_subnet(&mut self, remote_addr: &Multiaddr) {
        if let Some(prefix) = subnet_prefix(remote_addr) {
            if let Some(count) = self.subnet_counts.get_mut(&prefix) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    self.subnet_counts.remove(&prefix);
                }
            }
        }
    }

    /// Current count of connected peers from a given subnet.
    #[cfg(test)]
    pub fn subnet_count(&self, prefix: &SubnetKey) -> usize {
        self.subnet_counts.get(prefix).copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(ip: &str, port: u16) -> Multiaddr {
        format!("/ip4/{ip}/tcp/{port}").parse().unwrap()
    }

    fn quic_addr(ip: &str, port: u16) -> Multiaddr {
        format!("/ip4/{ip}/udp/{port}/quic-v1").parse().unwrap()
    }

    #[test]
    fn subnet_prefix_extraction() {
        let a = addr("10.0.1.5", 8555);
        assert_eq!(subnet_prefix(&a), Some(SubnetKey::V4([10, 0])));

        let b = addr("192.168.3.7", 9000);
        assert_eq!(subnet_prefix(&b), Some(SubnetKey::V4([192, 168])));
    }

    #[test]
    fn remote_ip_extracts_v4_v6_and_none() {
        use std::net::{Ipv4Addr, Ipv6Addr};
        assert_eq!(
            remote_ip(&addr("10.0.1.5", 8555)),
            Some(std::net::IpAddr::V4(Ipv4Addr::new(10, 0, 1, 5)))
        );
        let v6: Multiaddr = "/ip6/2001:db8::1/tcp/8555".parse().expect("valid v6 multiaddr");
        assert_eq!(
            remote_ip(&v6),
            Some(std::net::IpAddr::V6("2001:db8::1".parse::<Ipv6Addr>().expect("v6")))
        );
        let mem: Multiaddr = "/memory/1".parse().expect("valid memory multiaddr");
        assert_eq!(remote_ip(&mem), None, "no IP component must be None, not a guess");
    }

    /// THE #2113 regression: an IPv6 peer must be attributed to a subnet, not
    /// waved through. Before this, `subnet_prefix` returned `None` for every
    /// `/ip6/` address and `subnet_allows` mapped `None` to allowed, so subnet
    /// diversity was off for IPv6 entirely.
    #[test]
    fn ipv6_addresses_are_grouped_by_slash_64() {
        let a: Multiaddr = "/ip6/2001:db8:1:2::1/tcp/8555".parse().unwrap();
        let b: Multiaddr = "/ip6/2001:db8:1:2::ffff/tcp/8555".parse().unwrap();
        assert_eq!(
            subnet_prefix(&a),
            subnet_prefix(&b),
            "two addresses in one /64 must share a subnet key — an IPv6 host owns \
             its whole /64, so per-address counting bounds nothing"
        );
        assert!(matches!(subnet_prefix(&a), Some(SubnetKey::V6(_))));
    }

    #[test]
    fn distinct_ipv6_slash_64s_are_distinct_subnets() {
        let a: Multiaddr = "/ip6/2001:db8:1:2::1/tcp/8555".parse().unwrap();
        let c: Multiaddr = "/ip6/2001:db8:1:3::1/tcp/8555".parse().unwrap();
        assert_ne!(subnet_prefix(&a), subnet_prefix(&c));
    }

    /// A v4 key must never collide with a v6 key, which a flattened byte array
    /// would have permitted.
    #[test]
    fn ipv4_and_ipv6_keys_cannot_collide() {
        let v4: Multiaddr = "/ip4/32.1.0.0/tcp/8555".parse().unwrap();
        let v6: Multiaddr = "/ip6/2001::/tcp/8555".parse().unwrap();
        // Both must RESOLVE — asserting only inequality would pass vacuously if
        // the v6 side went back to returning None, which is the bug this file
        // exists to close.
        assert!(matches!(subnet_prefix(&v4), Some(SubnetKey::V4(_))));
        assert!(matches!(subnet_prefix(&v6), Some(SubnetKey::V6(_))));
        assert_ne!(subnet_prefix(&v4), subnet_prefix(&v6));
    }

    /// The behavioural test: the quota actually binds for IPv6.
    #[test]
    fn ipv6_peers_in_one_slash_64_are_capped() {
        let mut reg = PeerManager::new();
        let a: Multiaddr = "/ip6/2001:db8:1:2::1/tcp/8555".parse().unwrap();
        let b: Multiaddr = "/ip6/2001:db8:1:2::2/tcp/8555".parse().unwrap();
        let c: Multiaddr = "/ip6/2001:db8:1:2::3/tcp/8555".parse().unwrap();

        assert!(reg.subnet_allows(&a, 2));
        reg.record_subnet(&a);
        assert!(reg.subnet_allows(&b, 2));
        reg.record_subnet(&b);
        assert!(
            !reg.subnet_allows(&c, 2),
            "the third peer from one /64 must be refused at a limit of 2"
        );

        // ...and releasing one frees the slot again.
        reg.release_subnet(&b);
        assert!(reg.subnet_allows(&c, 2));
    }

    /// A different /64 must be unaffected by another's exhaustion.
    #[test]
    fn ipv6_quota_is_per_slash_64_not_global() {
        let mut reg = PeerManager::new();
        for host in ["/ip6/2001:db8:1:2::1/tcp/8555", "/ip6/2001:db8:1:2::2/tcp/8555"] {
            let addr: Multiaddr = host.parse().unwrap();
            reg.record_subnet(&addr);
        }
        // The same /64 must be exhausted... (without this the test passes even
        // when IPv6 is not tracked at all, which is exactly the bypass).
        let same: Multiaddr = "/ip6/2001:db8:1:2::9/tcp/8555".parse().unwrap();
        assert!(
            !reg.subnet_allows(&same, 2),
            "the source /64 must be exhausted after two peers"
        );
        // ...while a different /64 is unaffected.
        let other: Multiaddr = "/ip6/2001:db8:1:9::1/tcp/8555".parse().unwrap();
        assert!(
            reg.subnet_allows(&other, 2),
            "a peer in a different /64 must not be blocked by another subnet"
        );
    }

    /// Unchanged: an address with no IP component cannot be attributed, so it is
    /// still allowed. Refusing would reject legitimate `/dns/` peers.
    #[test]
    fn subnet_prefix_non_ipv4_returns_none() {
        let dns: Multiaddr = "/dns4/example.com/tcp/8555".parse().unwrap();
        assert_eq!(subnet_prefix(&dns), None);
    }

    #[test]
    fn detects_quic_multiaddrs() {
        assert!(multiaddr_uses_quic(&quic_addr("127.0.0.1", 8555)));
        assert!(!multiaddr_uses_quic(&addr("127.0.0.1", 8555)));
    }

    #[test]
    fn subnet_allows_within_limit() {
        let mut mgr = PeerManager::new();
        let a = addr("10.0.1.5", 8555);
        mgr.record_subnet(&a);
        // 1 peer from 10.0.x.x, limit 2 → allowed
        assert!(mgr.subnet_allows(&addr("10.0.2.3", 8556), 2));
    }

    #[test]
    fn subnet_allows_at_limit() {
        let mut mgr = PeerManager::new();
        mgr.record_subnet(&addr("10.0.1.5", 8555));
        mgr.record_subnet(&addr("10.0.2.3", 8556));
        // 2 peers from 10.0.x.x, limit 2 → rejected
        assert!(!mgr.subnet_allows(&addr("10.0.99.1", 8557), 2));
        // different /16 → allowed
        assert!(mgr.subnet_allows(&addr("192.168.1.1", 8558), 2));
    }

    #[test]
    fn subnet_allows_disabled_when_zero() {
        let mut mgr = PeerManager::new();
        for i in 0..100 {
            mgr.record_subnet(&addr(&format!("10.0.0.{i}"), 8555 + i));
        }
        // limit 0 → always allowed regardless of count
        assert!(mgr.subnet_allows(&addr("10.0.0.200", 9999), 0));
    }

    #[test]
    fn release_subnet_decrements() {
        let mut mgr = PeerManager::new();
        let a = addr("10.0.1.5", 8555);
        let b = addr("10.0.2.3", 8556);
        mgr.record_subnet(&a);
        mgr.record_subnet(&b);
        assert_eq!(mgr.subnet_count(&SubnetKey::V4([10, 0])), 2);

        mgr.release_subnet(&a);
        assert_eq!(mgr.subnet_count(&SubnetKey::V4([10, 0])), 1);
        // Now back under limit of 2
        assert!(mgr.subnet_allows(&addr("10.0.3.1", 8557), 2));
    }

    #[test]
    fn non_ipv4_always_allowed() {
        let mgr = PeerManager::new();
        let dns: Multiaddr = "/dns4/example.com/tcp/8555".parse().unwrap();
        assert!(mgr.subnet_allows(&dns, 1));
    }

    #[test]
    fn second_connection_to_same_peer_does_not_double_count_subnet() {
        let mut mgr = PeerManager::new();
        let peer = PeerId::random();
        let tcp = addr("10.0.1.5", 8555);
        let quic = quic_addr("10.0.1.5", 8555);

        assert!(mgr.peer_connected(peer, ConnectionId::new_unchecked(1), tcp));
        assert_eq!(mgr.subnet_count(&SubnetKey::V4([10, 0])), 1);

        assert!(!mgr.peer_connected(peer, ConnectionId::new_unchecked(2), quic));
        assert_eq!(mgr.subnet_count(&SubnetKey::V4([10, 0])), 1);

        assert!(!mgr.peer_connection_closed(&peer, ConnectionId::new_unchecked(1)));
        assert_eq!(mgr.subnet_count(&SubnetKey::V4([10, 0])), 1);

        assert!(mgr.peer_connection_closed(&peer, ConnectionId::new_unchecked(2)));
        assert_eq!(mgr.subnet_count(&SubnetKey::V4([10, 0])), 0);
    }

    #[test]
    fn redundant_non_quic_connections_are_reported_once_quic_exists() {
        let mut info = PeerInfo::new(PeerId::random());
        info.add_connection(ConnectionId::new_unchecked(1), addr("127.0.0.1", 8555));
        info.add_connection(ConnectionId::new_unchecked(2), quic_addr("127.0.0.1", 9555));

        assert!(info.has_quic_connection());
        assert!(info.has_non_quic_connection());
        assert_eq!(
            info.redundant_non_quic_connection_ids(),
            vec![ConnectionId::new_unchecked(1)]
        );
    }

    #[test]
    fn known_quic_addresses_filter_to_quic_only() {
        let mut info = PeerInfo::new(PeerId::random());
        info.add_address(addr("127.0.0.1", 8555));
        info.add_address(quic_addr("127.0.0.1", 9555));

        assert_eq!(
            info.known_quic_addresses(),
            vec![quic_addr("127.0.0.1", 9555)]
        );
    }

    #[test]
    fn tcp_pruning_requires_identify_on_a_current_quic_connection_3275() {
        let mut info = PeerInfo::new(PeerId::random());
        let tcp = ConnectionId::new_unchecked(1);
        let quic = ConnectionId::new_unchecked(2);
        info.add_connection(tcp, addr("127.0.0.1", 8555));
        info.add_connection(quic, quic_addr("127.0.0.1", 9555));
        // A pending/new QUIC transport, a TCP Identify, or a stale connection
        // ID is insufficient. The same peer's old TCP stream stays usable.
        assert!(info.redundant_non_quic_after_identify(tcp).is_empty());
        assert!(info
            .redundant_non_quic_after_identify(ConnectionId::new_unchecked(3))
            .is_empty());
        assert_eq!(info.redundant_non_quic_after_identify(quic), vec![tcp]);
        info.remove_connection(quic);
        assert!(info.redundant_non_quic_after_identify(quic).is_empty());
    }
}
