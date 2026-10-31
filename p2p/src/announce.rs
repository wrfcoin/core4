//! Which addresses this node tells other peers about.
//!
//! A node learns addresses for itself from three places: its own listeners
//! (a `0.0.0.0` listener expands to every interface, including a Tailscale
//! `100.x` or a home-LAN `192.168.x` one), external-address confirmation
//! (AutoNAT / UPnP), and the operator (`WRFCOIN_P2P_EXTERNAL_ADDR` /
//! `WRFCOIN_P2P_QUIC_EXTERNAL_ADDR`). It also learns addresses for OTHER peers
//! from their identify messages and re-serves those through Kademlia.
//!
//! Before this module every one of those was passed on to every peer, so a
//! public seed that also sat on a tailnet told the whole internet its private
//! `100.x` addresses (and its tailnet neighbours'). By default a node now only
//! announces addresses that are globally routable, plus whatever the operator
//! configured explicitly — they chose it, even if it is private.
//!
//! Set [`ANNOUNCE_NON_GLOBAL_ADDRS_ENV`] (`WRFCOIN_P2P_ANNOUNCE_PRIVATE_ADDRS=true`)
//! to keep the old behaviour, e.g. for a LAN-, docker- or tailnet-only cluster
//! whose members find each other through identify/Kademlia on private addresses.

use libp2p::multiaddr::Protocol;
use libp2p::Multiaddr;
use std::net::{Ipv4Addr, Ipv6Addr};

/// Opt-in that keeps the pre-filter behaviour: announce private, CGNAT,
/// link-local and loopback addresses to peers. Default: `false`.
pub const ANNOUNCE_NON_GLOBAL_ADDRS_ENV: &str = "WRFCOIN_P2P_ANNOUNCE_PRIVATE_ADDRS";

/// `true` for an IPv4 address that must not be announced to the internet:
/// RFC1918, CGNAT `100.64.0.0/10` (Tailscale), link-local, loopback,
/// unspecified.
fn is_non_global_ipv4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    let cgnat = o[0] == 100 && (o[1] & 0b1100_0000) == 0b0100_0000;
    ip.is_private() || cgnat || ip.is_link_local() || ip.is_loopback() || ip.is_unspecified()
}

/// `true` for an IPv6 address that must not be announced to the internet:
/// loopback, unspecified, ULA `fc00::/7`, link-local `fe80::/10`, and an
/// IPv4-mapped address whose IPv4 part is itself non-global.
fn is_non_global_ipv6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    let ula = (first & 0xfe00) == 0xfc00;
    let link_local = (first & 0xffc0) == 0xfe80;
    let mapped_private = ip.to_ipv4_mapped().is_some_and(is_non_global_ipv4);
    ip.is_loopback() || ip.is_unspecified() || ula || link_local || mapped_private
}

/// `true` when the multiaddr's IP component (the first one, which is the one a
/// dialer connects to) is not globally routable. Addresses without an IP
/// component — `/dns4`, `/dns6`, `/dns`, `/memory` — are not judged here and
/// return `false`: a DNS name is the operator's public face by construction.
pub fn is_non_global_addr(addr: &Multiaddr) -> bool {
    for proto in addr.iter() {
        match proto {
            Protocol::Ip4(ip) => return is_non_global_ipv4(ip),
            Protocol::Ip6(ip) => return is_non_global_ipv6(ip),
            Protocol::Dns(_) | Protocol::Dns4(_) | Protocol::Dns6(_) | Protocol::Dnsaddr(_) => {
                return false
            },
            _ => {},
        }
    }
    false
}

fn is_loopback_addr(addr: &Multiaddr) -> bool {
    match addr.iter().next() {
        Some(Protocol::Ip4(ip)) => ip.is_loopback(),
        Some(Protocol::Ip6(ip)) => ip.is_loopback(),
        _ => false,
    }
}

/// The address-announcement decision for one running node.
#[derive(Debug, Clone)]
pub struct AnnouncePolicy {
    /// [`ANNOUNCE_NON_GLOBAL_ADDRS_ENV`]: announce everything, as before.
    announce_all_scopes: bool,
    /// Every listener is loopback: a single-host rehearsal or an in-process
    /// test cluster. Loopback addresses are the only ones such a node has, and
    /// they reveal nothing, so they stay announceable.
    loopback_rehearsal: bool,
    /// Operator-configured external addresses: always announced.
    operator: Vec<Multiaddr>,
}

impl AnnouncePolicy {
    /// Build the policy for a node started with `config`.
    pub fn from_config(config: &crate::config::P2pConfig) -> Self {
        Self {
            announce_all_scopes: config.announce_non_global_addrs,
            loopback_rehearsal: crate::behaviour::loopback_listeners_only(config),
            operator: config
                .external_address
                .iter()
                .chain(config.quic_external_address.iter())
                .cloned()
                .collect(),
        }
    }

    /// Default production policy: filter, with these operator addresses kept.
    pub fn filtering(operator: Vec<Multiaddr>) -> Self {
        Self {
            announce_all_scopes: false,
            loopback_rehearsal: false,
            operator,
        }
    }

    /// The opt-in policy: announce everything (the pre-filter behaviour).
    pub fn announce_all() -> Self {
        Self {
            announce_all_scopes: true,
            loopback_rehearsal: false,
            operator: Vec::new(),
        }
    }

    /// `true` when filtering is in effect (the default).
    pub fn is_filtering(&self) -> bool {
        !self.announce_all_scopes
    }

    /// Is `addr` one of the operator-configured external addresses?
    pub fn is_operator_addr(&self, addr: &Multiaddr) -> bool {
        self.operator.contains(addr)
    }

    /// May this node announce `addr` as one of ITS OWN addresses?
    pub fn may_announce_own(&self, addr: &Multiaddr) -> bool {
        self.announce_all_scopes
            || self.operator.contains(addr)
            || !is_non_global_addr(addr)
            || (self.loopback_rehearsal && is_loopback_addr(addr))
    }

    /// May an address a PEER advertised to us be stored where Kademlia will
    /// re-serve it to other peers?
    ///
    /// Loopback stays eligible: the existing same-host rule
    /// (`should_ingest_advertised_addr`) already admits it only from a peer on
    /// this host, every localhost cluster depends on it, and a loopback
    /// address names no network.
    pub fn may_relay_peer_addr(&self, addr: &Multiaddr) -> bool {
        self.announce_all_scopes || !is_non_global_addr(addr) || is_loopback_addr(addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ma(s: &str) -> Multiaddr {
        s.parse().expect("test multiaddr")
    }

    #[test]
    fn non_global_ranges_are_detected() {
        for local in [
            "/ip4/10.1.2.3/tcp/9000",
            "/ip4/172.16.0.5/tcp/9000",
            "/ip4/192.168.0.10/tcp/9000",
            "/ip4/100.64.0.1/tcp/9000",
            "/ip4/100.127.255.254/udp/9000/quic-v1",
            "/ip4/169.254.1.1/tcp/9000",
            "/ip4/127.0.0.1/tcp/9000",
            "/ip4/0.0.0.0/tcp/9000",
            "/ip6/::1/tcp/9000",
            "/ip6/::/tcp/9000",
            "/ip6/fd7a:115c:a1e0::1/tcp/9000",
            "/ip6/fc00::1/tcp/9000",
            "/ip6/fe80::1/tcp/9000",
            "/ip6/::ffff:100.100.1.1/tcp/9000",
        ] {
            assert!(is_non_global_addr(&ma(local)), "{local} must be non-global");
        }
        for public in [
            "/ip4/8.8.8.8/tcp/9000",
            // Just outside the CGNAT /10 on both sides.
            "/ip4/100.63.255.255/tcp/9000",
            "/ip4/100.128.0.1/tcp/9000",
            "/ip6/2001:4860:4860::8888/tcp/9000",
            "/dns4/seed.example.org/tcp/9000",
            "/dns6/seed.example.org/tcp/9000",
            "/dns/seed.example.org/tcp/9000",
        ] {
            assert!(!is_non_global_addr(&ma(public)), "{public} must be announceable");
        }
    }

    #[test]
    fn default_policy_drops_cgnat_and_keeps_dns4() {
        let policy = AnnouncePolicy::filtering(Vec::new());
        assert!(policy.is_filtering());
        assert!(!policy.may_announce_own(&ma("/ip4/100.64.12.34/tcp/9000")));
        assert!(!policy.may_announce_own(&ma("/ip4/192.168.0.10/tcp/9000")));
        assert!(!policy.may_announce_own(&ma("/ip4/127.0.0.1/tcp/9000")));
        assert!(policy.may_announce_own(&ma("/dns4/seed.example.org/tcp/9000")));
        assert!(policy.may_announce_own(&ma("/ip4/8.8.8.8/tcp/9000")));
        assert!(!policy.may_relay_peer_addr(&ma("/ip4/100.64.12.34/tcp/9000")));
        assert!(policy.may_relay_peer_addr(&ma("/dns4/seed.example.org/tcp/9000")));
    }

    #[test]
    fn operator_external_address_is_announced_even_when_private() {
        let chosen = ma("/ip4/100.64.0.7/tcp/9000");
        let policy = AnnouncePolicy::filtering(vec![chosen.clone()]);
        assert!(policy.may_announce_own(&chosen));
        // Only the exact configured address, not its neighbours.
        assert!(!policy.may_announce_own(&ma("/ip4/100.64.0.8/tcp/9000")));
    }

    #[test]
    fn opt_in_keeps_todays_behaviour() {
        let policy = AnnouncePolicy::announce_all();
        assert!(!policy.is_filtering());
        assert!(policy.may_announce_own(&ma("/ip4/100.64.12.34/tcp/9000")));
        assert!(policy.may_relay_peer_addr(&ma("/ip4/10.0.0.1/tcp/9000")));
    }

    #[test]
    fn loopback_only_rehearsal_still_announces_loopback() {
        let mut config = crate::config::P2pConfig::default();
        config.announce_non_global_addrs = false;
        config.external_address = None;
        config.quic_external_address = None;
        config.listen_addresses = vec![ma("/ip4/127.0.0.1/tcp/0")];
        config.quic_listen_addresses.clear();
        let policy = AnnouncePolicy::from_config(&config);
        assert!(policy.may_announce_own(&ma("/ip4/127.0.0.1/tcp/4001")));
        assert!(!policy.may_announce_own(&ma("/ip4/100.64.0.1/tcp/4001")));

        config.listen_addresses = vec![ma("/ip4/0.0.0.0/tcp/0")];
        let policy = AnnouncePolicy::from_config(&config);
        assert!(!policy.may_announce_own(&ma("/ip4/127.0.0.1/tcp/4001")));
    }

    #[test]
    fn env_opt_in_reaches_the_config() {
        // Only this test touches the variable.
        std::env::set_var(ANNOUNCE_NON_GLOBAL_ADDRS_ENV, "true");
        let on = crate::config::P2pConfig::default().announce_non_global_addrs;
        std::env::remove_var(ANNOUNCE_NON_GLOBAL_ADDRS_ENV);
        let off = crate::config::P2pConfig::default().announce_non_global_addrs;
        assert!(on, "{ANNOUNCE_NON_GLOBAL_ADDRS_ENV}=true must opt in");
        assert!(!off, "unset must filter (the default)");
    }
}
