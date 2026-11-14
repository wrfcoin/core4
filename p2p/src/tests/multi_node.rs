//! Integration tests: multi-node P2P topology (issue #885).
//!
//! Tests beyond the two-node baseline:
//! - `three_node_mesh_formation`: hub topology — each node must see 2 PeerConnected events
//! - `weather_gossip_reaches_non_bootstrapped_node`: gossip relays through hub
//! - `partition_and_recovery`: node disconnect detected + fresh re-join confirmed

use std::collections::HashSet;
use std::time::Duration;
use tokio::time::timeout;

use rand::SeedableRng;

use super::helpers::allocate_port;
use crate::{P2pCommand, P2pConfig, P2pEvent, P2pNode};
use wrfcoin_shared_protocol::crypto::signature::PrivateKey;

/// Maximum time we wait for any single assertion before declaring failure.
const TEST_TIMEOUT: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

/// Start a hub-and-spoke mesh: node2 binds on a fixed port; node1 and node3
/// both bootstrap to node2.  Returns `(handle1, handle2, handle3, addr2)` where
/// `addr2` is node2's full multiaddr (with `/p2p/<peer_id>`) needed to dial it
/// again later.
async fn three_node_hub() -> (
    crate::node::P2pHandle,
    crate::node::P2pHandle,
    crate::node::P2pHandle,
    libp2p::Multiaddr,
) {
    let port2 = allocate_port().await;
    let addr2_base: libp2p::Multiaddr = format!("/ip4/127.0.0.1/tcp/{port2}")
        .parse()
        .expect("valid addr");

    let cfg2 = P2pConfig {
        listen_addresses: vec![addr2_base.clone()],
        bootstrap_peers: Vec::new(),
        max_peers_per_subnet: 0,
        ..super::helpers::peering_config()
    };
    let handle2 = P2pNode::start(cfg2).await.expect("node2 (hub) should start");

    // Full multiaddr with /p2p/<peer_id> for Kademlia bootstrap.
    let addr2 = addr2_base.with(libp2p::multiaddr::Protocol::P2p(handle2.local_peer_id));

    let cfg1 = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().expect("valid multiaddr")],
        bootstrap_peers: vec![addr2.clone()],
        max_peers_per_subnet: 0,
        ..super::helpers::peering_config()
    };
    let handle1 = P2pNode::start(cfg1).await.expect("node1 should start");

    let cfg3 = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().expect("valid multiaddr")],
        bootstrap_peers: vec![addr2.clone()],
        max_peers_per_subnet: 0,
        ..super::helpers::peering_config()
    };
    let handle3 = P2pNode::start(cfg3).await.expect("node3 should start");

    (handle1, handle2, handle3, addr2)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Three nodes form a hub-and-spoke mesh; each node must observe connections
/// from both other nodes.
#[tokio::test]
async fn three_node_mesh_formation() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    let (mut handle1, mut handle2, mut handle3, _) = three_node_hub().await;

    let peer_id1 = handle1.local_peer_id;
    let peer_id2 = handle2.local_peer_id;
    let peer_id3 = handle3.local_peer_id;

    // Each node waits until it has recorded PeerConnected from both other peers.
    let wait1 = timeout(TEST_TIMEOUT, async {
        let mut seen: HashSet<libp2p::PeerId> = HashSet::new();
        while seen.len() < 2 {
            match handle1.event_rx.recv().await {
                Some(P2pEvent::PeerConnected(pid)) => {
                    seen.insert(pid);
                }
                Some(_) => {}
                None => panic!("node1 event channel closed"),
            }
        }
        seen
    });

    let wait2 = timeout(TEST_TIMEOUT, async {
        let mut seen: HashSet<libp2p::PeerId> = HashSet::new();
        while seen.len() < 2 {
            match handle2.event_rx.recv().await {
                Some(P2pEvent::PeerConnected(pid)) => {
                    seen.insert(pid);
                }
                Some(_) => {}
                None => panic!("node2 event channel closed"),
            }
        }
        seen
    });

    let wait3 = timeout(TEST_TIMEOUT, async {
        let mut seen: HashSet<libp2p::PeerId> = HashSet::new();
        while seen.len() < 2 {
            match handle3.event_rx.recv().await {
                Some(P2pEvent::PeerConnected(pid)) => {
                    seen.insert(pid);
                }
                Some(_) => {}
                None => panic!("node3 event channel closed"),
            }
        }
        seen
    });

    let (peers1, peers2, peers3) =
        tokio::try_join!(wait1, wait2, wait3).expect("all three nodes should connect within timeout");

    assert!(peers1.contains(&peer_id2), "node1 should see node2");
    assert!(peers1.contains(&peer_id3), "node1 should see node3");
    assert!(peers2.contains(&peer_id1), "node2 should see node1");
    assert!(peers2.contains(&peer_id3), "node2 should see node3");
    assert!(peers3.contains(&peer_id1), "node3 should see node1");
    assert!(peers3.contains(&peer_id2), "node3 should see node2");

    let _ = handle1.command_tx.send(P2pCommand::Shutdown).await;
    let _ = handle2.command_tx.send(P2pCommand::Shutdown).await;
    let _ = handle3.command_tx.send(P2pCommand::Shutdown).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
}

/// Weather data published by node1 must reach node3 even though they are not
/// directly connected (both spokes relay through hub node2).
#[tokio::test]
async fn weather_gossip_reaches_non_bootstrapped_node() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    let (mut handle1, mut handle2, mut handle3, _) = three_node_hub().await;

    // Wait for the full mesh to form before sending gossip.
    let wait1 = timeout(TEST_TIMEOUT, async {
        let mut seen: HashSet<libp2p::PeerId> = HashSet::new();
        while seen.len() < 2 {
            match handle1.event_rx.recv().await {
                Some(P2pEvent::PeerConnected(pid)) => {
                    seen.insert(pid);
                }
                Some(_) => {}
                None => panic!("node1 event channel closed"),
            }
        }
    });
    let wait2 = timeout(TEST_TIMEOUT, async {
        let mut seen: HashSet<libp2p::PeerId> = HashSet::new();
        while seen.len() < 2 {
            match handle2.event_rx.recv().await {
                Some(P2pEvent::PeerConnected(pid)) => {
                    seen.insert(pid);
                }
                Some(_) => {}
                None => panic!("node2 event channel closed"),
            }
        }
    });
    let wait3 = timeout(TEST_TIMEOUT, async {
        let mut seen: HashSet<libp2p::PeerId> = HashSet::new();
        while seen.len() < 2 {
            match handle3.event_rx.recv().await {
                Some(P2pEvent::PeerConnected(pid)) => {
                    seen.insert(pid);
                }
                Some(_) => {}
                None => panic!("node3 event channel closed"),
            }
        }
    });
    tokio::try_join!(wait1, wait2, wait3).expect("mesh must form before gossip test");

    // Wait for gossipsub mesh formation via subscription confirmations.
    // Each node subscribes to gossipsub topics; wait for all 3 to confirm.
    let (_, _, _) = tokio::join!(
        super::helpers::wait_for_gossipsub_subscriptions(&mut handle1, 1, Duration::from_secs(30)),
        super::helpers::wait_for_gossipsub_subscriptions(&mut handle2, 1, Duration::from_secs(30)),
        super::helpers::wait_for_gossipsub_subscriptions(&mut handle3, 1, Duration::from_secs(30))
    );

    let mut rng = rand::rngs::StdRng::seed_from_u64(885);
    let station_key = PrivateKey::generate(&mut rng);

    let station_id = "stn-885".to_string();
    let data = b"T=22.5C".to_vec();

    handle1
        .command_tx
        .send(P2pCommand::BroadcastWeatherData {
            station_id: station_id.clone(),
            data: data.clone(),
            station_signing_key: station_key,
        })
        .await
        .expect("send BroadcastWeatherData");

    // node3 receives the message relayed through node2.
    let received = timeout(TEST_TIMEOUT, async {
        loop {
            match handle3.event_rx.recv().await {
                Some(P2pEvent::WeatherData {
                    station_id: sid,
                    data: d,
                    ..
                }) if sid == "stn-885" && d == b"T=22.5C".to_vec() => {
                    return (sid, d);
                }
                Some(_) => continue,
                None => panic!("node3 event channel closed"),
            }
        }
    })
    .await
    .expect("node3 should receive weather data within TEST_TIMEOUT");

    assert_eq!(received.0, station_id, "station_id must match");
    assert_eq!(received.1, data, "data payload must match");

    let _ = handle1.command_tx.send(P2pCommand::Shutdown).await;
    let _ = handle2.command_tx.send(P2pCommand::Shutdown).await;
    let _ = handle3.command_tx.send(P2pCommand::Shutdown).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
}

/// Simulate a node partition and verify:
/// 1. The hub (node2) emits `PeerDisconnected` for the dropped peer.
/// 2. A fresh replacement node can re-join the mesh (recovery).
#[tokio::test]
async fn partition_and_recovery() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    let (mut handle1, mut handle2, handle3, addr2) = three_node_hub().await;

    let node3_peer_id = handle3.local_peer_id;

    // Wait for the full mesh to form.
    let wait1 = timeout(TEST_TIMEOUT, async {
        let mut seen: HashSet<libp2p::PeerId> = HashSet::new();
        while seen.len() < 2 {
            match handle1.event_rx.recv().await {
                Some(P2pEvent::PeerConnected(pid)) => {
                    seen.insert(pid);
                }
                Some(_) => {}
                None => panic!("node1 event channel closed"),
            }
        }
    });
    let wait2 = timeout(TEST_TIMEOUT, async {
        let mut seen: HashSet<libp2p::PeerId> = HashSet::new();
        while seen.len() < 2 {
            match handle2.event_rx.recv().await {
                Some(P2pEvent::PeerConnected(pid)) => {
                    seen.insert(pid);
                }
                Some(_) => {}
                None => panic!("node2 event channel closed"),
            }
        }
    });
    tokio::try_join!(wait1, wait2).expect("mesh must form before partition test");

    // Wait for gossipsub mesh formation via subscription confirmations.
    let (_, _) = tokio::join!(
        super::helpers::wait_for_gossipsub_subscriptions(&mut handle1, 1, Duration::from_secs(30)),
        super::helpers::wait_for_gossipsub_subscriptions(&mut handle2, 1, Duration::from_secs(30))
    );

    // ---- Partition: send Shutdown then drop node3's handle -------------------
    let _ = handle3.command_tx.send(P2pCommand::Shutdown).await;
    drop(handle3);

    // ---- Detect disconnection on node2 --------------------------------------
    timeout(TEST_TIMEOUT, async {
        loop {
            match handle2.event_rx.recv().await {
                Some(P2pEvent::PeerDisconnected(pid)) if pid == node3_peer_id => break,
                Some(_) => continue,
                None => panic!("node2 event channel closed"),
            }
        }
    })
    .await
    .expect("node2 should observe PeerDisconnected for node3 within TEST_TIMEOUT");

    // Brief grace period for TCP teardown.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // ---- Recovery: start a fresh node3' and re-mesh to node2 ----------------
    let cfg3_prime = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().expect("valid multiaddr")],
        bootstrap_peers: vec![addr2],
        max_peers_per_subnet: 0,
        ..super::helpers::peering_config()
    };
    let mut handle3_prime = P2pNode::start(cfg3_prime).await.expect("node3' should start");

    // node2 must see a new PeerConnected for the replacement node.
    timeout(TEST_TIMEOUT, async {
        loop {
            match handle2.event_rx.recv().await {
                Some(P2pEvent::PeerConnected(_)) => break,
                Some(_) => continue,
                None => panic!("node2 event channel closed during recovery"),
            }
        }
    })
    .await
    .expect("node2 should see PeerConnected for node3' within TEST_TIMEOUT");

    // Confirm node3' itself sees its connection to node2.
    timeout(TEST_TIMEOUT, async {
        loop {
            match handle3_prime.event_rx.recv().await {
                Some(P2pEvent::PeerConnected(_)) => break,
                Some(_) => continue,
                None => panic!("node3' event channel closed"),
            }
        }
    })
    .await
    .expect("node3' should connect to the mesh within TEST_TIMEOUT");

    let _ = handle1.command_tx.send(P2pCommand::Shutdown).await;
    let _ = handle2.command_tx.send(P2pCommand::Shutdown).await;
    let _ = handle3_prime.command_tx.send(P2pCommand::Shutdown).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
}

/// A DA challenge response that reaches the challenger through a relay must
/// carry the keeper as its signed author, not the relay.
///
/// Line topology K — R — C: keeper and challenger each allow one outbound
/// connection and no inbound ones, so both stay attached only to the relay
/// hub and every message from K to C is forwarded by R. The
/// event at C must name R as the propagation source and K as the author;
/// the native-chain DA runtime attributes the response to the author.
#[tokio::test]
async fn da_challenge_response_relayed_keeps_signed_author() {
    use crate::da_challenge::{DataAvailabilityChallengeResponse, compute_proof};

    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    let port_r = allocate_port().await;
    let addr_r_base: libp2p::Multiaddr = format!("/ip4/127.0.0.1/tcp/{port_r}")
        .parse()
        .expect("valid addr");
    let mut relay = P2pNode::start(P2pConfig {
        listen_addresses: vec![addr_r_base.clone()],
        bootstrap_peers: Vec::new(),
        max_peers_per_subnet: 0,
        ..super::helpers::peering_config()
    })
    .await
    .expect("relay should start");
    let addr_r = addr_r_base.with(libp2p::multiaddr::Protocol::P2p(relay.local_peer_id));

    let spoke_config = || P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().expect("valid multiaddr")],
        bootstrap_peers: vec![addr_r.clone()],
        max_peers_per_subnet: 0,
        // One outbound connection (to the relay) and no inbound: K and C can
        // neither dial nor accept each other, so they never peer directly.
        max_established_total: Some(1),
        max_established_incoming: Some(0),
        ..super::helpers::peering_config()
    };
    let mut keeper = P2pNode::start(spoke_config()).await.expect("keeper should start");
    let mut challenger = P2pNode::start(spoke_config())
        .await
        .expect("challenger should start");

    let relay_id = relay.local_peer_id;
    let keeper_id = keeper.local_peer_id;

    async fn wait_connected(
        handle: &mut crate::node::P2pHandle,
        want: usize,
    ) -> HashSet<libp2p::PeerId> {
        let mut seen: HashSet<libp2p::PeerId> = HashSet::new();
        while seen.len() < want {
            match handle.event_rx.recv().await {
                Some(P2pEvent::PeerConnected(pid)) => {
                    seen.insert(pid);
                },
                Some(_) => {},
                None => panic!("event channel closed"),
            }
        }
        seen
    }
    let (keeper_peers, relay_peers, challenger_peers) = tokio::try_join!(
        timeout(TEST_TIMEOUT, wait_connected(&mut keeper, 1)),
        timeout(TEST_TIMEOUT, wait_connected(&mut relay, 2)),
        timeout(TEST_TIMEOUT, wait_connected(&mut challenger, 1)),
    )
    .expect("line topology must form");
    assert_eq!(keeper_peers, HashSet::from([relay_id]));
    assert_eq!(challenger_peers, HashSet::from([relay_id]));
    assert!(relay_peers.contains(&keeper_id));

    let (_, _, _) = tokio::join!(
        super::helpers::wait_for_gossipsub_subscriptions(&mut keeper, 1, Duration::from_secs(30)),
        super::helpers::wait_for_gossipsub_subscriptions(&mut relay, 1, Duration::from_secs(30)),
        super::helpers::wait_for_gossipsub_subscriptions(&mut challenger, 1, Duration::from_secs(30))
    );

    let nonce = [11u8; 32];
    let fragment = b"relayed-fragment".to_vec();
    let make_response = |attempt: i64| DataAvailabilityChallengeResponse {
        challenge_id: "relay-author-1".to_string(),
        data_fragment: fragment.clone(),
        proof: compute_proof(&nonce, &fragment),
        keeper_id: keeper_id.to_string(),
        // Distinct per attempt so a re-publish is not a gossip duplicate.
        responded_at: chrono::Utc::now().timestamp() + attempt,
    };

    // Re-publish until the relay has grafted the topic mesh and forwarded it.
    let keeper_tx = keeper.command_tx.clone();
    let received = timeout(TEST_TIMEOUT, async {
        let mut attempt = 0i64;
        loop {
            keeper_tx
                .send(P2pCommand::BroadcastDaChallengeResponse {
                    response: make_response(attempt),
                })
                .await
                .expect("send BroadcastDaChallengeResponse");
            attempt += 1;
            let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
            while let Ok(event) = tokio::time::timeout_at(deadline, challenger.event_rx.recv()).await {
                match event {
                    Some(P2pEvent::DaChallengeResponse {
                        peer_id,
                        author,
                        response,
                    }) if response.challenge_id == "relay-author-1" => {
                        return (peer_id, author, response);
                    },
                    Some(P2pEvent::PeerConnected(pid)) => {
                        assert_ne!(pid, keeper_id, "challenger must not peer with keeper directly");
                    },
                    Some(_) => {},
                    None => panic!("challenger event channel closed"),
                }
            }
        }
    })
    .await
    .expect("challenger should receive the relayed DA response");

    let (propagation_source, author, response) = received;
    assert_eq!(
        propagation_source, relay_id,
        "the response must have been forwarded by the relay"
    );
    assert_eq!(
        author,
        Some(keeper_id),
        "the event must carry the signed author (keeper), not the relay"
    );
    assert_eq!(response.keeper_id, keeper_id.to_string());

    let _ = keeper.command_tx.send(P2pCommand::Shutdown).await;
    let _ = relay.command_tx.send(P2pCommand::Shutdown).await;
    let _ = challenger.command_tx.send(P2pCommand::Shutdown).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
}
