//! Wiring tests for the address-derived connection quotas (#2113).
//!
//! The unit tests in `crate::quota` cover the counting rules in isolation. These
//! cover the thing those cannot: that the quota is actually *reachable* in the
//! composed [`WrfcoinBehaviour`], and that a rejection lands on the right
//! counter.
//!
//! That distinction is the whole point. A quota module with perfect unit tests
//! and no field in the behaviour struct passes its own suite and defends
//! nothing. These tests fail if the wiring is removed, which is what makes them
//! worth having.
//!
//! No swarm is needed: `handle_pending_inbound_connection` is a trait method,
//! and the `NetworkBehaviour` derive generates a composed implementation that
//! fans out to every field. Calling it on the built behaviour therefore
//! exercises the real dispatch path.
//!
//! They are `#[tokio::test]` only because the composed behaviour includes a
//! upnp behaviour whose constructor requires a reactor. Nothing here awaits,
//! and nothing binds a socket — so these run where socket binds are denied
//! (#1821).

use std::sync::Arc;

use libp2p::identity::Keypair;
use libp2p::swarm::{ConnectionId, ListenFailure, NetworkBehaviour};
use libp2p::{Multiaddr, relay};

use crate::behaviour::build_behaviour;
use crate::config::P2pConfig;
use crate::metrics::MetricsCollector;
use crate::quota::ConnectionLimits;

fn tcp(addr: &str) -> Multiaddr {
    format!("/ip4/{addr}/tcp/9000").parse().unwrap()
}

/// Build the real composed behaviour with the given quota ceilings.
///
/// Returns the metrics collector too, so a test can assert on the counters the
/// behaviour bumps.
fn composed(limits: ConnectionLimits) -> (impl NetworkBehaviour, Arc<MetricsCollector>) {
    let keypair = Keypair::generate_ed25519();
    let peer_id = keypair.public().to_peer_id();
    // The relay client behaviour is the one piece production gets from
    // `SwarmBuilder::with_relay_client`; `relay::client::new` yields it
    // directly.
    let (_relay_transport, relay_behaviour) = relay::client::new(peer_id);

    let mut config = P2pConfig::default();
    config.connection_quota = limits;

    let metrics = Arc::new(MetricsCollector::new());
    let behaviour = build_behaviour(&keypair, &config, relay_behaviour, Arc::clone(&metrics))
        .expect("build_behaviour");
    (behaviour, metrics)
}

fn pending_inbound(
    behaviour: &mut impl NetworkBehaviour,
    id: usize,
    remote: &Multiaddr,
) -> Result<(), String> {
    behaviour
        .handle_pending_inbound_connection(ConnectionId::new_unchecked(id), &tcp("10.0.0.1"), remote)
        .map(|_| ())
        .map_err(|denied| {
            // `ConnectionDenied` displays only as "connection denied"; the
            // reason is the wrapped cause. Walking the chain asserts that the
            // reason actually propagates to the swarm, not merely that
            // *something* refused.
            use std::error::Error;
            let mut chain = denied.to_string();
            let mut source: Option<&(dyn Error + 'static)> = denied.source();
            while let Some(cause) = source {
                chain.push_str(&format!(": {cause}"));
                source = cause.source();
            }
            chain
        })
}

/// The wiring test. If `connection_quota` is dropped from `WrfcoinBehaviour`,
/// or constructed but never inserted, every admission succeeds and this fails.
#[tokio::test]
async fn composed_behaviour_refuses_pending_inbound_past_the_per_ip_ceiling() {
    let (mut behaviour, _metrics) = composed(ConnectionLimits {
        max_per_ip: 2,
        max_per_ipv4_subnet: 0,
        max_per_ipv6_subnet: 0,
    });

    assert!(pending_inbound(&mut behaviour, 0, &tcp("203.0.113.7")).is_ok());
    assert!(pending_inbound(&mut behaviour, 1, &tcp("203.0.113.7")).is_ok());

    let denied = pending_inbound(&mut behaviour, 2, &tcp("203.0.113.7"))
        .expect_err("third connection from one IP must be refused before upgrade");
    assert!(
        denied.contains("per_ip"),
        "denial must name the ceiling that bit, got: {denied}"
    );
}

#[tokio::test]
async fn composed_behaviour_refuses_pending_inbound_past_the_ipv4_subnet_ceiling() {
    let (mut behaviour, _metrics) = composed(ConnectionLimits {
        max_per_ip: 0,
        max_per_ipv4_subnet: 2,
        max_per_ipv6_subnet: 0,
    });

    // Three distinct addresses inside one /24.
    assert!(pending_inbound(&mut behaviour, 0, &tcp("203.0.113.1")).is_ok());
    assert!(pending_inbound(&mut behaviour, 1, &tcp("203.0.113.2")).is_ok());
    let denied = pending_inbound(&mut behaviour, 2, &tcp("203.0.113.3"))
        .expect_err("a third distinct address in one /24 must be refused");
    assert!(denied.contains("ipv4_subnet"), "got: {denied}");

    // A different /24 is unaffected — the ceiling is per block, not global.
    assert!(pending_inbound(&mut behaviour, 3, &tcp("203.0.114.1")).is_ok());
}

#[tokio::test]
async fn a_rejection_increments_exactly_one_reason_counter() {
    let (mut behaviour, metrics) = composed(ConnectionLimits {
        max_per_ip: 1,
        max_per_ipv4_subnet: 0,
        max_per_ipv6_subnet: 0,
    });

    let before = metrics.snapshot(0);
    assert_eq!(before.conn_rejected_per_ip, 0);

    assert!(pending_inbound(&mut behaviour, 0, &tcp("203.0.113.7")).is_ok());
    assert!(pending_inbound(&mut behaviour, 1, &tcp("203.0.113.7")).is_err());

    let after = metrics.snapshot(0);
    assert_eq!(
        after.conn_rejected_per_ip, 1,
        "the per-IP rejection counter must move"
    );
    assert_eq!(
        (after.conn_rejected_ipv4_subnet, after.conn_rejected_ipv6_subnet),
        (0, 0),
        "no other reason counter may move — otherwise an operator cannot tell \
         which ceiling is biting, which is the entire reason these are split"
    );
}

/// A connection admitted at the pending stage that never establishes must give
/// its slot back. Without this the quota only ever counts up, and the node
/// eventually refuses everyone — a self-inflicted outage wearing the costume of
/// a working defence.
#[tokio::test]
async fn a_pending_connection_that_never_establishes_releases_its_slot() {
    let (mut behaviour, _metrics) = composed(ConnectionLimits {
        max_per_ip: 1,
        max_per_ipv4_subnet: 0,
        max_per_ipv6_subnet: 0,
    });

    let addr = tcp("203.0.113.7");
    assert!(pending_inbound(&mut behaviour, 0, &addr).is_ok());
    assert!(
        pending_inbound(&mut behaviour, 1, &addr).is_err(),
        "the single slot is taken"
    );

    let local = tcp("10.0.0.1");
    behaviour.on_swarm_event(libp2p::swarm::FromSwarm::ListenFailure(ListenFailure {
        local_addr: &local,
        send_back_addr: &addr,
        connection_id: ConnectionId::new_unchecked(0),
        error: &libp2p::swarm::ListenError::Aborted,
        peer_id: None,
    }));

    assert!(
        pending_inbound(&mut behaviour, 2, &addr).is_ok(),
        "the abandoned pending connection must have returned its slot"
    );
}

/// Loopback must stay exempt through the composed behaviour, not merely in the
/// quota unit. Local multi-node development depends on it.
#[tokio::test]
async fn composed_behaviour_exempts_loopback() {
    let (mut behaviour, _metrics) = composed(ConnectionLimits {
        max_per_ip: 1,
        max_per_ipv4_subnet: 1,
        max_per_ipv6_subnet: 1,
    });

    for i in 0..20 {
        assert!(
            pending_inbound(&mut behaviour, i, &tcp("127.0.0.1")).is_ok(),
            "loopback connection {i} must be admitted"
        );
    }
}
