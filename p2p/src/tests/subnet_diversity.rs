//! Integration tests for /16 subnet diversity enforcement (eclipse hardening).

use std::time::Duration;
use tokio::time::timeout;

use super::helpers::allocate_port;
use crate::{P2pCommand, P2pConfig, P2pEvent, P2pNode};

const TEST_TIMEOUT: Duration = Duration::from_secs(30);

/// With `max_peers_per_subnet = 1`, the hub node should accept the first
/// spoke from 127.0.0.1 but reject the second (same /16 subnet).
#[tokio::test]
async fn subnet_limit_rejects_excess_peers() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    // 1. Start hub with strict subnet limit (1 peer per /16)
    let port_hub = allocate_port().await;
    let addr_hub_base: libp2p::Multiaddr =
        format!("/ip4/127.0.0.1/tcp/{port_hub}").parse().unwrap();

    let cfg_hub = P2pConfig {
        listen_addresses: vec![addr_hub_base.clone()],
        bootstrap_peers: Vec::new(),
        max_peers_per_subnet: 1,
        ..super::helpers::peering_config()
    };
    let mut handle_hub = P2pNode::start(cfg_hub).await.expect("hub should start");

    let addr_hub = addr_hub_base.with(libp2p::multiaddr::Protocol::P2p(handle_hub.local_peer_id));

    // 2. Spoke A connects (should succeed)
    let cfg_a = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()],
        bootstrap_peers: vec![addr_hub.clone()],
        max_peers_per_subnet: 0,
        ..super::helpers::peering_config()
    };
    let _handle_a = P2pNode::start(cfg_a).await.expect("spoke A should start");

    // Wait for hub to emit PeerConnected for spoke A
    let first_connected = timeout(TEST_TIMEOUT, async {
        while let Some(event) = handle_hub.event_rx.recv().await {
            if matches!(event, P2pEvent::PeerConnected(_)) {
                return true;
            }
        }
        false
    })
    .await;
    assert!(
        first_connected.is_ok() && first_connected.unwrap(),
        "Hub should accept spoke A"
    );

    // 3. Spoke B connects (should be rejected — same /16 subnet)
    let cfg_b = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()],
        bootstrap_peers: vec![addr_hub.clone()],
        max_peers_per_subnet: 0,
        ..super::helpers::peering_config()
    };
    let _handle_b = P2pNode::start(cfg_b).await.expect("spoke B should start");

    // Hub should NOT emit another PeerConnected within a reasonable window.
    // Instead it will disconnect spoke B immediately.
    let second_connected = timeout(Duration::from_secs(5), async {
        while let Some(event) = handle_hub.event_rx.recv().await {
            if matches!(event, P2pEvent::PeerConnected(_)) {
                return true;
            }
        }
        false
    })
    .await;

    // Either timeout (Ok but the future returned false) or the timeout itself
    // fires (Err) -- both mean spoke B was not accepted.
    let was_accepted = second_connected.unwrap_or(false);
    assert!(
        !was_accepted,
        "Hub should reject spoke B (same /16 subnet, limit 1)"
    );

    // Cleanup
    let _ = handle_hub
        .command_tx
        .send(P2pCommand::Shutdown)
        .await;
}

/// With `max_peers_per_subnet = 0` (disabled), all peers should connect
/// regardless of subnet.
#[tokio::test]
async fn subnet_limit_disabled_allows_all() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    let port_hub = allocate_port().await;
    let addr_hub_base: libp2p::Multiaddr =
        format!("/ip4/127.0.0.1/tcp/{port_hub}").parse().unwrap();

    let cfg_hub = P2pConfig {
        listen_addresses: vec![addr_hub_base.clone()],
        bootstrap_peers: Vec::new(),
        max_peers_per_subnet: 0, // disabled
        ..super::helpers::peering_config()
    };
    let mut handle_hub = P2pNode::start(cfg_hub).await.expect("hub should start");

    let addr_hub = addr_hub_base.with(libp2p::multiaddr::Protocol::P2p(handle_hub.local_peer_id));

    // Start two spokes on same subnet
    let cfg_a = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()],
        bootstrap_peers: vec![addr_hub.clone()],
        max_peers_per_subnet: 0,
        ..super::helpers::peering_config()
    };
    let _handle_a = P2pNode::start(cfg_a).await.expect("spoke A should start");

    let cfg_b = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()],
        bootstrap_peers: vec![addr_hub.clone()],
        max_peers_per_subnet: 0,
        ..super::helpers::peering_config()
    };
    let _handle_b = P2pNode::start(cfg_b).await.expect("spoke B should start");

    // Both should connect
    let both_connected = timeout(TEST_TIMEOUT, async {
        let mut count = 0;
        while let Some(event) = handle_hub.event_rx.recv().await {
            if matches!(event, P2pEvent::PeerConnected(_)) {
                count += 1;
                if count >= 2 {
                    return true;
                }
            }
        }
        false
    })
    .await;

    assert!(
        both_connected.is_ok() && both_connected.unwrap(),
        "Hub should accept both spokes when subnet limit is disabled"
    );

    let _ = handle_hub
        .command_tx
        .send(P2pCommand::Shutdown)
        .await;
}
