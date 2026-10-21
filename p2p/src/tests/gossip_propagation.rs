//! Integration tests: Gossipsub delivery correctness and propagation latency.
//!
//! Verified:
//! 1. A -> B -> C (Linear chain)
//! 2. Node A broadcasts a 1KB transaction.
//! 3. Node C receives it in the always-run correctness suite.
//! 4. The deliberately-run performance check requires delivery in < 500ms.

use std::time::{Duration, Instant};
use tokio::time::timeout;
use super::helpers::allocate_port;
use crate::{P2pCommand, P2pConfig, P2pEvent, P2pNode};

const TEST_TIMEOUT: Duration = Duration::from_secs(30);
const PROPAGATION_THRESHOLD: Duration = Duration::from_millis(500);

async fn exercise_three_node_gossip_delivery() -> Duration {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    // 1. Start Node B (The Middleman)
    let port_b = allocate_port().await;
    let addr_b_base: libp2p::Multiaddr = format!("/ip4/127.0.0.1/tcp/{port_b}").parse().unwrap();
    let cfg_b = P2pConfig {
        listen_addresses: vec![addr_b_base.clone()],
        max_peers_per_subnet: 0, // disable — all test nodes share 127.0.0.1
        ..super::helpers::peering_config()
    };
    let handle_b = P2pNode::start(cfg_b).await.expect("node B should start");
    let addr_b = addr_b_base.with(libp2p::multiaddr::Protocol::P2p(handle_b.local_peer_id));

    // 2. Start Node A (The Sender) - Connects to B
    let cfg_a = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()],
        bootstrap_peers: vec![addr_b.clone()],
        max_peers_per_subnet: 0,
        ..super::helpers::peering_config()
    };
    let handle_a = P2pNode::start(cfg_a).await.expect("node A should start");

    // 3. Start Node C (The Receiver) - Connects to B
    let cfg_c = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()],
        bootstrap_peers: vec![addr_b.clone()],
        max_peers_per_subnet: 0,
        ..super::helpers::peering_config()
    };
    let mut handle_c = P2pNode::start(cfg_c).await.expect("node C should start");

    // Wait for mesh formation via gossipsub subscriptions.
    // Each node should have subscribed to at least one topic before gossip is ready.
    super::helpers::wait_for_gossipsub_subscriptions(&mut handle_c, 1, Duration::from_secs(30)).await;

    // 4. Node A broadcasts a 1KB transaction
    let tx_payload = vec![vec![0u8; 1024]]; // 1KB
    let start_time = Instant::now();
    handle_a.command_tx.send(P2pCommand::BroadcastTransactions(tx_payload.clone())).await.unwrap();

    // 5. Node C receives it
    let end_time = timeout(TEST_TIMEOUT, async {
        loop {
            if let Some(P2pEvent::NewTransactions(txs)) = handle_c.event_rx.recv().await {
                if txs == tx_payload {
                    return Instant::now();
                }
            }
        }
    })
    .await
    .expect("Node C failed to receive transaction");
    let latency = end_time.duration_since(start_time);

    // Shut down
    let _ = handle_a.command_tx.send(P2pCommand::Shutdown).await;
    let _ = handle_b.command_tx.send(P2pCommand::Shutdown).await;
    let _ = handle_c.command_tx.send(P2pCommand::Shutdown).await;

    latency
}

#[tokio::test]
async fn gossip_reaches_non_bootstrapped_peer() {
    let _latency = exercise_three_node_gossip_delivery().await;
}

#[tokio::test]
#[ignore = "performance contract; run deliberately on an uncontended host"]
async fn gossip_propagation_meets_latency_budget() {
    let latency = exercise_three_node_gossip_delivery().await;
    println!("Gossip Latency (A -> B -> C): {latency:?}");
    assert!(
        latency < PROPAGATION_THRESHOLD,
        "Latency {latency:?} exceeded threshold {PROPAGATION_THRESHOLD:?}"
    );
}
