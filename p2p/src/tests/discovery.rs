//! Integration test: three-node Kademlia discovery.
//! 
//! Verified:
//! 1. Node 1 starts (Seed)
//! 2. Node 2 starts, bootstraps to Node 1
//! 3. Node 3 starts, bootstraps to Node 1
//! 4. Node 2 and Node 3 should eventually discover each other via Node 1.

use std::time::Duration;
use tokio::time::timeout;
use super::helpers::allocate_port;
use crate::{P2pConfig, P2pEvent, P2pNode};

const TEST_TIMEOUT: Duration = Duration::from_secs(60);

#[tokio::test]
async fn three_node_discovery() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    // 1. Start Node 1 (Seed)
    let port1 = allocate_port().await;
    let addr1_base: libp2p::Multiaddr = format!("/ip4/127.0.0.1/tcp/{port1}").parse().unwrap();

    let cfg1 = P2pConfig {
        listen_addresses: vec![addr1_base.clone()],
        bootstrap_peers: Vec::new(),
        max_peers_per_subnet: 0, // disable — all test nodes share 127.0.0.1
        ..super::helpers::peering_config()
    };
    let handle1 = P2pNode::start(cfg1).await.expect("node1 should start");

    // Full address with PeerId for Kademlia
    let addr1 = addr1_base.with(libp2p::multiaddr::Protocol::P2p(handle1.local_peer_id));

    // 2. Start Node 2 (Connects to 1)
    let cfg2 = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()],
        bootstrap_peers: vec![addr1.clone()],
        max_peers_per_subnet: 0,
        ..super::helpers::peering_config()
    };
    let mut handle2 = P2pNode::start(cfg2).await.expect("node2 should start");

    // 3. Start Node 3 (Connects ONLY to 1)
    let cfg3 = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()],
        bootstrap_peers: vec![addr1.clone()],
        max_peers_per_subnet: 0,
        ..super::helpers::peering_config()
    };
    let handle3 = P2pNode::start(cfg3).await.expect("node3 should start");

    // 4. Verify Node 2 eventually connects to Node 3 (Indirectly discovered via Node 1)
    let discovery_success = timeout(TEST_TIMEOUT, async {
        let mut connected_to = std::collections::HashSet::new();
        while !connected_to.contains(&handle3.local_peer_id) {
            if let Some(P2pEvent::PeerConnected(peer_id)) = handle2.event_rx.recv().await {
                connected_to.insert(peer_id);
                println!("Node 2 connected to: {}", peer_id);
            }
        }
        connected_to
    }).await;

    assert!(discovery_success.is_ok(), "Node 2 failed to discover Node 3 within timeout");
    println!("Node 2 successfully discovered Node 3!");
    
    // Shut down
    let _ = handle1.command_tx.send(crate::P2pCommand::Shutdown).await;
    let _ = handle2.command_tx.send(crate::P2pCommand::Shutdown).await;
    let _ = handle3.command_tx.send(crate::P2pCommand::Shutdown).await;
}
