//! Integration test: two WRFCoin P2P nodes exchanging messages on localhost.
//!
//! Each node listens on a random OS-assigned port (`/ip4/127.0.0.1/tcp/0`)
//! so tests can run in parallel without port conflicts.

use crate::da_challenge::{
    DataAvailabilityChallengeResponse, compute_proof, generate_challenge,
    verify_challenge_response,
};
use libp2p::multiaddr::Protocol;
use std::time::Duration;
use tokio::time::timeout;

use super::helpers::allocate_port;
use crate::{InitialChainState, P2pCommand, P2pConfig, P2pEvent, P2pNode};

/// Maximum time we wait for any single assertion before declaring failure.
const TEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Start two P2P nodes, connect them, exchange a block and a transaction,
/// then shut down cleanly.
#[tokio::test]
async fn two_nodes_exchange_messages() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    // ---- Start node 2 on an OS-assigned ephemeral port -----------------------
    let port2 = allocate_port().await;
    let addr2_str = format!("/ip4/127.0.0.1/tcp/{port2}");
    let addr2: libp2p::Multiaddr = addr2_str.parse().expect("valid addr");

    let cfg2 = P2pConfig {
        listen_addresses: vec![addr2.clone()],
        bootstrap_peers: Vec::new(),
        ..super::helpers::peering_config()
    };
    let mut handle2 = P2pNode::start(cfg2).await.expect("node2 should start");

    // ---- Start node 1 bootstrapping to node 2 --------------------------------
    let cfg1 = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().expect("valid multiaddr")],
        bootstrap_peers: vec![addr2.clone()],
        ..super::helpers::peering_config()
    };
    let mut handle1 = P2pNode::start(cfg1).await.expect("node1 should start");

    // ---- Wait for PeerConnected on both sides --------------------------------
    let connected1 = timeout(TEST_TIMEOUT, async {
        loop {
            match handle1.event_rx.recv().await {
                Some(P2pEvent::PeerConnected(_)) => break,
                Some(_) => continue,
                None => panic!("node1 event channel closed unexpectedly"),
            }
        }
    });

    let connected2 = timeout(TEST_TIMEOUT, async {
        loop {
            match handle2.event_rx.recv().await {
                Some(P2pEvent::PeerConnected(_)) => break,
                Some(_) => continue,
                None => panic!("node2 event channel closed unexpectedly"),
            }
        }
    });

    // Both sides should see the connection.
    tokio::try_join!(connected1, connected2).expect("both nodes should connect");

    // Wait for gossipsub mesh formation via subscription confirmations.
    let (_, _) = tokio::join!(
        super::helpers::wait_for_gossipsub_subscriptions(&mut handle1, 1, Duration::from_secs(30)),
        super::helpers::wait_for_gossipsub_subscriptions(&mut handle2, 1, Duration::from_secs(30))
    );

    // ---- Node 1 broadcasts a block, node 2 receives it ----------------------
    let block_payload = vec![0xCA, 0xFE, 0xBA, 0xBE];
    handle1
        .command_tx
        .send(P2pCommand::BroadcastBlock(block_payload.clone()))
        .await
        .expect("send BroadcastBlock");

    let received_block = timeout(TEST_TIMEOUT, async {
        loop {
            match handle2.event_rx.recv().await {
                Some(P2pEvent::NewBlock(data)) => break data,
                Some(_) => continue,
                None => panic!("node2 event channel closed"),
            }
        }
    })
    .await
    .expect("node2 should receive the block");

    assert_eq!(received_block, block_payload, "block payload must match");

    // ---- Node 2 broadcasts transactions, node 1 receives them ---------------
    let tx_payload = vec![vec![1, 2, 3], vec![4, 5, 6]];
    handle2
        .command_tx
        .send(P2pCommand::BroadcastTransactions(tx_payload.clone()))
        .await
        .expect("send BroadcastTransactions");

    let received_txs = timeout(TEST_TIMEOUT, async {
        loop {
            match handle1.event_rx.recv().await {
                Some(P2pEvent::NewTransactions(txs)) => break txs,
                Some(_) => continue,
                None => panic!("node1 event channel closed"),
            }
        }
    })
    .await
    .expect("node1 should receive the transactions");

    assert_eq!(received_txs, tx_payload, "transaction payload must match");

    // ---- Shut down both nodes -----------------------------------------------
    let _ = handle1.command_tx.send(P2pCommand::Shutdown).await;
    let _ = handle2.command_tx.send(P2pCommand::Shutdown).await;

    // Give tasks a moment to exit.
    tokio::time::sleep(Duration::from_millis(200)).await;
}

/// Start two QUIC-only P2P nodes, connect them over `/quic-v1`, exchange a
/// block, and shut down cleanly.
#[tokio::test]
async fn two_nodes_exchange_messages_over_quic() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    let cfg2 = P2pConfig {
        listen_addresses: Vec::new(),
        quic_listen_addresses: vec![
            "/ip4/127.0.0.1/udp/0/quic-v1"
                .parse()
                .expect("valid multiaddr"),
        ],
        bootstrap_peers: Vec::new(),
        ..super::helpers::peering_config()
    };
    let mut handle2 = P2pNode::start(cfg2).await.expect("node2 should start");

    let addr2 = timeout(TEST_TIMEOUT, async {
        loop {
            match handle2.event_rx.recv().await {
                Some(P2pEvent::ListeningAddress(address))
                    if address
                        .iter()
                        .any(|protocol| matches!(protocol, Protocol::QuicV1)) =>
                {
                    break address;
                },
                Some(_) => continue,
                None => panic!("node2 event channel closed unexpectedly"),
            }
        }
    })
    .await
    .expect("node2 should emit a QUIC listening address")
    .with(Protocol::P2p(handle2.local_peer_id));

    let cfg1 = P2pConfig {
        listen_addresses: Vec::new(),
        quic_listen_addresses: vec![
            "/ip4/127.0.0.1/udp/0/quic-v1"
                .parse()
                .expect("valid multiaddr"),
        ],
        bootstrap_peers: vec![addr2],
        ..super::helpers::peering_config()
    };
    let mut handle1 = P2pNode::start(cfg1).await.expect("node1 should start");

    let connected1 = timeout(TEST_TIMEOUT, async {
        loop {
            match handle1.event_rx.recv().await {
                Some(P2pEvent::PeerConnected(_)) => break,
                Some(_) => continue,
                None => panic!("node1 event channel closed unexpectedly"),
            }
        }
    });

    let connected2 = timeout(TEST_TIMEOUT, async {
        loop {
            match handle2.event_rx.recv().await {
                Some(P2pEvent::PeerConnected(_)) => break,
                Some(_) => continue,
                None => panic!("node2 event channel closed unexpectedly"),
            }
        }
    });

    tokio::try_join!(connected1, connected2).expect("both nodes should connect over QUIC");
    // Wait for gossipsub mesh formation via subscription confirmations.
    let (_, _) = tokio::join!(
        super::helpers::wait_for_gossipsub_subscriptions(&mut handle1, 1, Duration::from_secs(30)),
        super::helpers::wait_for_gossipsub_subscriptions(&mut handle2, 1, Duration::from_secs(30))
    );

    let block_payload = vec![0x51, 0x55, 0x49, 0x43];
    handle1
        .command_tx
        .send(P2pCommand::BroadcastBlock(block_payload.clone()))
        .await
        .expect("send BroadcastBlock");

    let received_block = timeout(TEST_TIMEOUT, async {
        loop {
            match handle2.event_rx.recv().await {
                Some(P2pEvent::NewBlock(data)) => break data,
                Some(_) => continue,
                None => panic!("node2 event channel closed"),
            }
        }
    })
    .await
    .expect("node2 should receive the block over QUIC");

    assert_eq!(received_block, block_payload, "block payload must match");

    let _ = handle1.command_tx.send(P2pCommand::Shutdown).await;
    let _ = handle2.command_tx.send(P2pCommand::Shutdown).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
}

/// Start two mixed-mode nodes that bootstrap over TCP while QUIC is also
/// enabled, then verify application traffic still flows.
#[tokio::test]
async fn two_nodes_exchange_messages_with_tcp_bootstrap_and_quic_enabled() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    let port2 = allocate_port().await;
    let addr2_str = format!("/ip4/127.0.0.1/tcp/{port2}");
    let addr2: libp2p::Multiaddr = addr2_str.parse().expect("valid addr");

    let cfg2 = P2pConfig {
        listen_addresses: vec![addr2.clone()],
        quic_listen_addresses: vec![
            "/ip4/127.0.0.1/udp/0/quic-v1"
                .parse()
                .expect("valid multiaddr"),
        ],
        bootstrap_peers: Vec::new(),
        ..super::helpers::peering_config()
    };
    let mut handle2 = P2pNode::start(cfg2).await.expect("node2 should start");

    let cfg1 = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().expect("valid multiaddr")],
        quic_listen_addresses: vec![
            "/ip4/127.0.0.1/udp/0/quic-v1"
                .parse()
                .expect("valid multiaddr"),
        ],
        bootstrap_peers: vec![addr2],
        ..super::helpers::peering_config()
    };
    let mut handle1 = P2pNode::start(cfg1).await.expect("node1 should start");

    let connected1 = timeout(TEST_TIMEOUT, async {
        loop {
            match handle1.event_rx.recv().await {
                Some(P2pEvent::PeerConnected(_)) => break,
                Some(_) => continue,
                None => panic!("node1 event channel closed unexpectedly"),
            }
        }
    });

    let connected2 = timeout(TEST_TIMEOUT, async {
        loop {
            match handle2.event_rx.recv().await {
                Some(P2pEvent::PeerConnected(_)) => break,
                Some(_) => continue,
                None => panic!("node2 event channel closed unexpectedly"),
            }
        }
    });

    tokio::try_join!(connected1, connected2)
        .expect("both nodes should connect with mixed TCP and QUIC enabled");
    // A peer-level blocks mesh can still be using the TCP stream that the
    // QUIC upgrade is about to close. Under contention a successful publish
    // immediately before that close can lose the one-shot payload (#3525).
    // Query each event loop for the expected peer's current QUIC-only
    // connections and blocks mesh before broadcasting. This also proves the
    // bootstrap upgraded, rather than merely exchanging a block over TCP.
    timeout(TEST_TIMEOUT, async {
        loop {
            let (ready1, ready2) = tokio::join!(
                handle1.quic_blocks_ready_for_test(handle2.local_peer_id),
                handle2.quic_blocks_ready_for_test(handle1.local_peer_id),
            );
            if ready1 && ready2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("both nodes should finish the QUIC upgrade with a publishable blocks mesh");

    let block_payload = vec![0x54, 0x43, 0x50, 0x51];
    handle1
        .command_tx
        .send(P2pCommand::BroadcastBlock(block_payload.clone()))
        .await
        .expect("send BroadcastBlock");

    let received_block = timeout(TEST_TIMEOUT, async {
        loop {
            match handle2.event_rx.recv().await {
                Some(P2pEvent::NewBlock(data)) => break data,
                Some(_) => continue,
                None => panic!("node2 event channel closed"),
            }
        }
    })
    .await
    .expect("node2 should receive the block after TCP bootstrap with QUIC enabled");

    assert_eq!(received_block, block_payload, "block payload must match");

    let _ = handle1.command_tx.send(P2pCommand::Shutdown).await;
    let _ = handle2.command_tx.send(P2pCommand::Shutdown).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
}

/// Full event-loop binding for #2790: the command is accepted before any peer
/// exists, then the bounded readiness tick must publish it exactly once after a
/// peer joins the real blocks mesh.
#[tokio::test]
async fn broadcast_block_waits_for_publishable_mesh_and_delivers_once_2790() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    let port1 = allocate_port().await;
    let addr1: libp2p::Multiaddr = format!("/ip4/127.0.0.1/tcp/{port1}")
        .parse()
        .expect("valid node1 address");
    let cfg1 = P2pConfig {
        listen_addresses: vec![addr1.clone()],
        bootstrap_peers: Vec::new(),
        ..super::helpers::peering_config()
    };
    let handle1 = P2pNode::start(cfg1).await.expect("node1 should start");
    let payload = b"event-loop-pending-block-2790".to_vec();

    handle1
        .command_tx
        .send(P2pCommand::BroadcastBlock(payload.clone()))
        .await
        .expect("event loop must accept BroadcastBlock");
    timeout(TEST_TIMEOUT, async {
        loop {
            if handle1.stats().pending_broadcasts == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the no-peer publish must be retained, not discarded");
    assert_eq!(handle1.stats().blocks_published, 0);

    let cfg2 = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().expect("valid multiaddr")],
        bootstrap_peers: vec![addr1],
        ..super::helpers::peering_config()
    };
    let mut handle2 = P2pNode::start(cfg2).await.expect("node2 should start");

    let received = timeout(TEST_TIMEOUT, async {
        loop {
            match handle2.event_rx.recv().await {
                Some(P2pEvent::NewBlock(bytes)) => break bytes,
                Some(_) => {},
                None => panic!("node2 event channel closed"),
            }
        }
    })
    .await
    .expect("pending block must publish after the real mesh becomes ready");
    assert_eq!(received, payload);

    timeout(TEST_TIMEOUT, async {
        loop {
            let stats = handle1.stats();
            if stats.pending_broadcasts == 0 && stats.pending_broadcasts_published >= 1 {
                assert_eq!(stats.blocks_published, 1);
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("successful retry must leave no retained block intent");

    let duplicate = timeout(Duration::from_millis(500), async {
        loop {
            match handle2.event_rx.recv().await {
                Some(P2pEvent::NewBlock(bytes)) if bytes == payload => break,
                Some(_) => {},
                None => break,
            }
        }
    })
    .await;
    assert!(duplicate.is_err(), "the retained block must not be sent twice");

    let _ = handle1.command_tx.send(P2pCommand::Shutdown).await;
    let _ = handle2.command_tx.send(P2pCommand::Shutdown).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
}

/// Nodes with an already-known local tip should be able to broadcast it to
/// connected peers after updating local chain state.
#[tokio::test]
async fn connected_peer_receives_broadcast_chain_status() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    let port1 = allocate_port().await;
    let addr1: libp2p::Multiaddr = format!("/ip4/127.0.0.1/tcp/{port1}")
        .parse()
        .expect("valid addr");

    let cfg1 = P2pConfig {
        listen_addresses: vec![addr1.clone()],
        bootstrap_peers: Vec::new(),
        ..super::helpers::peering_config()
    };
    let handle1 = P2pNode::start(cfg1).await.expect("node1 should start");

    handle1
        .command_tx
        .send(P2pCommand::UpdateLocalChainState {
            height: 7,
            tip_hash: [0xAB; 32],
            state_root: [0; 32],
        })
        .await
        .expect("local chain state update should be accepted");

    let cfg2 = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().expect("valid multiaddr")],
        bootstrap_peers: vec![addr1],
        ..super::helpers::peering_config()
    };
    let mut handle2 = P2pNode::start(cfg2).await.expect("node2 should start");

    // Wait for peer connection and gossipsub mesh formation.
    // Node2 should see a peer connection to node1 and have gossipsub subscriptions ready.
    super::helpers::wait_for_gossip_mesh(&mut handle2, 1, Duration::from_secs(30)).await;

    handle1
        .command_tx
        .send(P2pCommand::BroadcastChainStatus {
            height: 7,
            tip_hash: [0xAB; 32],
            state_root: [0; 32],
        })
        .await
        .expect("chain status broadcast should be accepted");

    let observed_status = timeout(TEST_TIMEOUT, async {
        loop {
            match handle2.event_rx.recv().await {
                Some(P2pEvent::PeerChainStatus {
                    height, tip_hash, ..
                }) => break (height, tip_hash),
                Some(_) => continue,
                None => panic!("node2 event channel closed"),
            }
        }
    })
    .await
    .expect("node2 should receive the broadcast chain status");

    assert_eq!(observed_status.0, 7);
    assert_eq!(observed_status.1, [0xAB; 32]);

    let _ = handle1.command_tx.send(P2pCommand::Shutdown).await;
    let _ = handle2.command_tx.send(P2pCommand::Shutdown).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
}

/// A verified connection must advertise its current tip promptly, without
/// waiting for the 10-second minimum periodic ChainStatus tick (#2816).
#[tokio::test]
async fn genesis_verified_connection_eagerly_discovers_ahead_target_2816() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    let producer_port = allocate_port().await;
    let producer_addr: libp2p::Multiaddr =
        format!("/ip4/127.0.0.1/tcp/{producer_port}").parse().expect("valid addr");
    let producer_cfg = P2pConfig {
        listen_addresses: vec![producer_addr.clone()],
        bootstrap_peers: Vec::new(),
        ..super::helpers::peering_config()
    };
    let producer = P2pNode::start_with_initial_chain_state(
        producer_cfg,
        InitialChainState {
            height: 100,
            tip_hash: [0xa1; 32],
            state_root: [0xb2; 32],
        },
    )
    .await
    .expect("producer should start");

    let follower_cfg = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().expect("valid multiaddr")],
        bootstrap_peers: vec![producer_addr.with(Protocol::P2p(producer.local_peer_id))],
        ..super::helpers::peering_config()
    };
    let mut follower = P2pNode::start_with_initial_chain_state(
        follower_cfg,
        InitialChainState {
            height: 90,
            tip_hash: [0xc3; 32],
            state_root: [0xd4; 32],
        },
    )
    .await
    .expect("follower should start");

    // 8s, deliberately BELOW the 10s periodic ChainStatus tick this test
    // exists to beat, but above the old 5s bound that flaked ~3% on scheduler
    // hiccups (measured 1/30 at clean main, #2622 catalogue 2026-08-28) and
    // worse under full-suite parallel load. The detector stays meaningful:
    // a lazy implementation that waits for the periodic tick still fails.
    let discovered = timeout(Duration::from_secs(8), async {
        loop {
            match follower.event_rx.recv().await {
                Some(P2pEvent::PeerChainStatus { height, .. }) if height == 100 => break,
                Some(_) => continue,
                None => panic!("follower event channel closed"),
            }
        }
    })
    .await;

    assert!(
        discovered.is_ok(),
        "genesis-verified connection must discover the ahead target before the 10s periodic tick"
    );

    let _ = producer.command_tx.send(P2pCommand::Shutdown).await;
    let _ = follower.command_tx.send(P2pCommand::Shutdown).await;
}

/// A single-peer follower restored above genesis must request the immediate
/// range above its durable tip after the peer advertises a higher height.
#[tokio::test]
async fn restored_tip_dispatches_catch_up_request_to_single_peer_2796() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    let producer_port = allocate_port().await;
    let producer_addr: libp2p::Multiaddr =
        format!("/ip4/127.0.0.1/tcp/{producer_port}").parse().expect("valid addr");
    let producer_cfg = P2pConfig {
        listen_addresses: vec![producer_addr.clone()],
        bootstrap_peers: Vec::new(),
        ..super::helpers::peering_config()
    };
    let mut producer = P2pNode::start(producer_cfg).await.expect("producer should start");
    let producer_dial_addr = producer_addr.with(Protocol::P2p(producer.local_peer_id));

    let follower_cfg = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().expect("valid multiaddr")],
        bootstrap_peers: vec![producer_dial_addr],
        ..super::helpers::peering_config()
    };
    let mut follower = P2pNode::start_with_initial_chain_state(
        follower_cfg,
        InitialChainState {
            height: 11_016,
            tip_hash: [0xa1; 32],
            state_root: [0xb2; 32],
        },
    )
    .await
    .expect("restored follower should start");

    let (_, _) = tokio::join!(
        super::helpers::wait_for_gossip_mesh(&mut producer, 1, TEST_TIMEOUT),
        super::helpers::wait_for_gossip_mesh(&mut follower, 1, TEST_TIMEOUT),
    );

    // The producer's own connection-epoch status (its real tip, height 0) is retained until the
    // gossip mesh is publishable, so it can reach the follower AFTER the synthetic 12_801
    // broadcast below. The sync manager keeps only the latest status per peer, so a late height-0
    // status makes the follower see no peer ahead of it and fall back to `idle` (#3367). Wait
    // until the follower has observed that initial status, so the 12_801 broadcast is the latest.
    timeout(TEST_TIMEOUT, async {
        loop {
            match follower.event_rx.recv().await {
                Some(P2pEvent::PeerChainStatus { height: 0, .. }) => break,
                Some(_) => continue,
                None => panic!("follower event channel closed"),
            }
        }
    })
    .await
    .expect("follower should observe the producer's initial chain status");

    producer
        .command_tx
        .send(P2pCommand::BroadcastChainStatus {
            height: 12_801,
            tip_hash: [0xc3; 32],
            state_root: [0xd4; 32],
        })
        .await
        .expect("producer status should enqueue");

    let request = timeout(TEST_TIMEOUT, async {
        loop {
            match producer.event_rx.recv().await {
                Some(P2pEvent::InboundBlockSyncRequest { request, .. }) => break request,
                Some(_) => continue,
                None => panic!("producer event channel closed"),
            }
        }
    })
    .await
    .expect("restored follower should dispatch a block request");

    assert_eq!(request.start_height, 11_017);
    assert!(request.count > 0);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let stats = follower.stats();
    assert_eq!(stats.sync_local_height, 11_016);
    assert_eq!(stats.sync_target_height, 12_801);
    assert_eq!(stats.sync_state, "downloading");
    assert!(stats.sync_outstanding_ranges > 0);

    let _ = producer.command_tx.send(P2pCommand::Shutdown).await;
    let _ = follower.command_tx.send(P2pCommand::Shutdown).await;
}

/// Start two nodes, publish a DA challenge from node 1, and verify node 2
/// receives the same challenge over gossipsub.
#[tokio::test]
async fn two_nodes_propagate_da_challenge() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    let port2 = allocate_port().await;
    let addr2_base: libp2p::Multiaddr = format!("/ip4/127.0.0.1/tcp/{port2}")
        .parse()
        .expect("valid addr");
    let cfg2 = P2pConfig {
        listen_addresses: vec![addr2_base.clone()],
        max_peers_per_subnet: 0,
        ..super::helpers::peering_config()
    };
    let mut handle2 = P2pNode::start(cfg2).await.expect("node2 should start");
    let addr2 = addr2_base.with(Protocol::P2p(handle2.local_peer_id));

    let cfg1 = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().expect("valid multiaddr")],
        bootstrap_peers: vec![addr2],
        max_peers_per_subnet: 0,
        ..super::helpers::peering_config()
    };
    let mut handle1 = P2pNode::start(cfg1).await.expect("node1 should start");

    let connected1 = timeout(TEST_TIMEOUT, async {
        loop {
            match handle1.event_rx.recv().await {
                Some(P2pEvent::PeerConnected(_)) => break,
                Some(_) => continue,
                None => panic!("node1 event channel closed unexpectedly"),
            }
        }
    });

    let connected2 = timeout(TEST_TIMEOUT, async {
        loop {
            match handle2.event_rx.recv().await {
                Some(P2pEvent::PeerConnected(_)) => break,
                Some(_) => continue,
                None => panic!("node2 event channel closed unexpectedly"),
            }
        }
    });

    tokio::try_join!(connected1, connected2).expect("both nodes should connect");

    let (_, _) = tokio::join!(
        super::helpers::wait_for_gossipsub_subscriptions(&mut handle1, 1, Duration::from_secs(30)),
        super::helpers::wait_for_gossipsub_subscriptions(&mut handle2, 1, Duration::from_secs(30))
    );

    let challenge = generate_challenge(
        "ipfs:bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi",
        8_192,
        512,
    )
    .expect("challenge should be valid");

    handle1
        .command_tx
        .send(P2pCommand::BroadcastDaChallenge {
            challenge: challenge.clone(),
        })
        .await
        .expect("send BroadcastDaChallenge");

    let (peer_id, received_challenge) = timeout(TEST_TIMEOUT, async {
        loop {
            match handle2.event_rx.recv().await {
                Some(P2pEvent::DaChallenge { peer_id, challenge }) => break (peer_id, challenge),
                Some(_) => continue,
                None => panic!("node2 event channel closed"),
            }
        }
    })
    .await
    .expect("node2 should receive the DA challenge");

    assert_eq!(peer_id, handle1.local_peer_id, "challenge should come from node1");
    assert_eq!(received_challenge, challenge, "challenge payload must match");

    let _ = handle1.command_tx.send(P2pCommand::Shutdown).await;
    let _ = handle2.command_tx.send(P2pCommand::Shutdown).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
}

/// Start two nodes, publish a DA challenge from node 1, reply from node 2,
/// and verify node 1 accepts the returned proof.
#[tokio::test]
async fn two_nodes_round_trip_da_challenge_response() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    let port2 = allocate_port().await;
    let addr2_base: libp2p::Multiaddr = format!("/ip4/127.0.0.1/tcp/{port2}")
        .parse()
        .expect("valid addr");
    let cfg2 = P2pConfig {
        listen_addresses: vec![addr2_base.clone()],
        max_peers_per_subnet: 0,
        ..super::helpers::peering_config()
    };
    let mut handle2 = P2pNode::start(cfg2).await.expect("node2 should start");
    let addr2 = addr2_base.with(Protocol::P2p(handle2.local_peer_id));

    let cfg1 = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().expect("valid multiaddr")],
        bootstrap_peers: vec![addr2],
        max_peers_per_subnet: 0,
        ..super::helpers::peering_config()
    };
    let mut handle1 = P2pNode::start(cfg1).await.expect("node1 should start");

    let connected1 = timeout(TEST_TIMEOUT, async {
        loop {
            match handle1.event_rx.recv().await {
                Some(P2pEvent::PeerConnected(_)) => break,
                Some(_) => continue,
                None => panic!("node1 event channel closed unexpectedly"),
            }
        }
    });

    let connected2 = timeout(TEST_TIMEOUT, async {
        loop {
            match handle2.event_rx.recv().await {
                Some(P2pEvent::PeerConnected(_)) => break,
                Some(_) => continue,
                None => panic!("node2 event channel closed unexpectedly"),
            }
        }
    });

    tokio::try_join!(connected1, connected2).expect("both nodes should connect");

    let (_, _) = tokio::join!(
        super::helpers::wait_for_gossipsub_subscriptions(&mut handle1, 1, Duration::from_secs(30)),
        super::helpers::wait_for_gossipsub_subscriptions(&mut handle2, 1, Duration::from_secs(30))
    );

    let challenge = generate_challenge(
        "ipfs:bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi",
        4,
        8,
    )
    .expect("challenge should be valid");

    handle1
        .command_tx
        .send(P2pCommand::BroadcastDaChallenge {
            challenge: challenge.clone(),
        })
        .await
        .expect("send BroadcastDaChallenge");

    let received_challenge = timeout(TEST_TIMEOUT, async {
        loop {
            match handle2.event_rx.recv().await {
                Some(P2pEvent::DaChallenge { challenge, .. }) => break challenge,
                Some(_) => continue,
                None => panic!("node2 event channel closed"),
            }
        }
    })
    .await
    .expect("node2 should receive the DA challenge");

    let backing_blob = b"0123456789abcdef";
    let fragment = backing_blob[received_challenge.offset as usize
        ..received_challenge.offset as usize + received_challenge.length as usize]
        .to_vec();
    let response = DataAvailabilityChallengeResponse {
        challenge_id: received_challenge.challenge_id.clone(),
        data_fragment: fragment.clone(),
        proof: compute_proof(&received_challenge.nonce, &fragment),
        keeper_id: handle2.local_peer_id.to_string(),
        responded_at: chrono::Utc::now().timestamp(),
    };

    handle2
        .command_tx
        .send(P2pCommand::BroadcastDaChallengeResponse {
            response: response.clone(),
        })
        .await
        .expect("send BroadcastDaChallengeResponse");

    let (peer_id, received_response) = timeout(TEST_TIMEOUT, async {
        loop {
            match handle1.event_rx.recv().await {
                Some(P2pEvent::DaChallengeResponse { peer_id, response }) => break (peer_id, response),
                Some(_) => continue,
                None => panic!("node1 event channel closed"),
            }
        }
    })
    .await
    .expect("node1 should receive the DA challenge response");

    assert_eq!(peer_id, handle2.local_peer_id, "response should come from node2");
    verify_challenge_response(&challenge, &received_response, &fragment)
        .expect("node1 should accept node2's DA response");

    let _ = handle1.command_tx.send(P2pCommand::Shutdown).await;
    let _ = handle2.command_tx.send(P2pCommand::Shutdown).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
}
