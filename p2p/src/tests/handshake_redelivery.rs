//! Regression: a newcomer joining a seed whose mesh is already full, with flood
//! publish off, must still receive the seed's genesis handshake and become a
//! sync source candidate for that seed.
//!
//! The control for this test is the same topology with the seed's mesh below
//! `mesh_n`. Without re-delivery that case takes milliseconds, because gossipsub
//! tops up to `mesh_n` with extras, and the full-mesh case never completes.

use std::time::Duration;

use libp2p::{Multiaddr, PeerId, multiaddr::Protocol};
use tokio::time::{Instant, timeout};

use super::helpers::{allocate_port, peering_config};
use crate::{InitialChainState, P2pClient, P2pConfig, P2pEvent, P2pNode};

/// Past the last redelivery attempt, plus the status/mesh round trip.
const DELIVERY_BOUND: Duration = Duration::from_secs(25);
const SETUP_BOUND: Duration = Duration::from_secs(45);
const SEED_HEIGHT: u64 = 20;

fn flood_off_config() -> P2pConfig {
    P2pConfig {
        flood_publish: false,
        // Mirrors the defaults, spelled out because the failure depends on them.
        mesh_n: 4,
        mesh_n_low: 3,
        mesh_n_high: 8,
        mesh_outbound_min: 2,
        max_peers_per_subnet: 0,
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().expect("valid multiaddr")],
        ..peering_config()
    }
}

fn state(height: u64) -> InitialChainState {
    InitialChainState {
        height,
        tip_hash: [height as u8; 32],
        state_root: [height as u8; 32],
    }
}

/// Keep a node's event channel drained; a full channel would stall its loop.
fn run_detached(handle: crate::node::P2pHandle) -> P2pClient {
    let (client, mut events) = handle.into_parts();
    tokio::spawn(async move { while events.recv().await.is_some() {} });
    client
}

/// A seed plus four settled peers, so the seed's handshake-topic mesh is full.
async fn seed_with_full_mesh() -> (P2pClient, Multiaddr, Vec<P2pClient>) {
    let port = allocate_port().await;
    let base: Multiaddr = format!("/ip4/127.0.0.1/tcp/{port}")
        .parse()
        .expect("valid multiaddr");
    let seed_cfg = P2pConfig {
        listen_addresses: vec![base.clone()],
        ..flood_off_config()
    };
    let seed = P2pNode::start_with_initial_chain_state(seed_cfg, state(SEED_HEIGHT))
        .await
        .expect("seed starts");
    let seed_addr = base.with(Protocol::P2p(seed.local_peer_id));
    let seed = run_detached(seed);

    let mut fillers = Vec::new();
    for _ in 0..4 {
        let cfg = P2pConfig {
            bootstrap_peers: vec![seed_addr.clone()],
            ..flood_off_config()
        };
        let node = P2pNode::start_with_initial_chain_state(cfg, state(SEED_HEIGHT))
            .await
            .expect("filler starts");
        fillers.push(run_detached(node));
    }

    let settled = timeout(SETUP_BOUND, async {
        loop {
            let mesh = seed
                .stats()
                .mesh_peers_by_topic
                .get("blocks")
                .copied()
                .unwrap_or(0);
            if mesh >= 4 {
                break mesh;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await;
    assert!(
        settled.is_ok(),
        "precondition: the seed's mesh must fill to mesh_n"
    );
    // Let every filler's own handshake traffic go quiet, so nothing but the
    // newcomer's arrival can make the seed publish a handshake.
    tokio::time::sleep(Duration::from_secs(3)).await;
    (seed, seed_addr, fillers)
}

/// Wait for the newcomer to hold the seed as a proven, advertised sync peer.
/// Returns how long it took.
async fn newcomer_reaches_seed(
    newcomer: &mut crate::node::P2pHandle,
    hub: PeerId,
    bound: Duration,
) -> Option<Duration> {
    let started = Instant::now();
    timeout(bound, async {
        loop {
            match newcomer.event_rx.recv().await {
                Some(P2pEvent::PeerChainStatus {
                    peer_id, height, ..
                }) if peer_id == hub && height == SEED_HEIGHT => {
                    return started.elapsed();
                },
                Some(P2pEvent::PeerConnected(peer_id)) => assert_eq!(
                    peer_id, hub,
                    "precondition: the newcomer must be connected to the seed alone"
                ),
                Some(_) => {},
                None => panic!("newcomer event channel closed"),
            }
        }
    })
    .await
    .ok()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn newcomer_to_a_full_mesh_seed_receives_handshake_and_reaches_peer_chains() {
    let _guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    let (hub, seed_addr, fillers) = seed_with_full_mesh().await;
    let seed_peer = hub.local_peer_id;

    // A newcomer behind NAT reaches the seed and nobody else. Without this the
    // fillers discover it over Kademlia on loopback and hand it the seed's
    // handshake through their own IHAVE gossip, which hides the failure: a real
    // fresh node has only the seed. Refusing inbound means nothing can dial in; a
    // private Kademlia protocol means it finds nobody to dial out to.
    let cfg = P2pConfig {
        bootstrap_peers: vec![seed_addr],
        max_established_incoming: Some(0),
        kademlia_protocol: "/wrfcoin-test/isolated-newcomer/kad/1.0.0".to_string(),
        ..flood_off_config()
    };
    let mut newcomer = P2pNode::start_with_initial_chain_state(cfg, state(0))
        .await
        .expect("newcomer starts");

    let reached = newcomer_reaches_seed(&mut newcomer, seed_peer, DELIVERY_BOUND).await;

    // `unproven_peers` clears only on a matching handshake from that peer, and the
    // seed is the newcomer's only peer: this is the handshake itself, separate from
    // the ChainStatus that follows it.
    let stats = newcomer.stats();
    assert_eq!(
        stats.unproven_peers,
        0,
        "the newcomer never received the seed's genesis handshake within \
         {DELIVERY_BOUND:?}, while connected to it (connected: {}, seed mesh: {:?})",
        stats.connected_peers,
        hub.stats().mesh_peers_by_topic.get("blocks"),
    );
    assert!(
        reached.is_some(),
        "the newcomer holds the seed's handshake but never accepted its ChainStatus"
    );
    // `PeerChainStatus` is emitted right after `record_peer_status`, so the seed
    // is in `peer_chains`. The surface is refreshed on the 250 ms sync tick.
    timeout(Duration::from_secs(3), async {
        loop {
            if newcomer
                .sync_peers
                .snapshot()
                .iter()
                .any(|entry| entry.peer_id == seed_peer.to_string())
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the seed must appear as a sync peer once its status is accepted");

    drop(fillers);
}
