//! Shared test helpers for P2P integration tests.
//!
//! - `allocate_port()` — OS-assigned ephemeral port (no collisions in parallel tests).
//! - `wait_for_gossip_mesh()` — condition-polling loop for gossipsub mesh formation.

use std::{
    sync::{Mutex, MutexGuard},
    time::Duration,
};

use crate::node::P2pHandle;
use crate::P2pEvent;

/// Serialize tests that start real P2P nodes.
///
/// These tests otherwise compete for libp2p runtimes and sockets inside one
/// libtest process. External process contention is still possible, so this is
/// not a substitute for separating correctness from latency assertions.
static MULTI_NODE_TEST_LOCK: Mutex<()> = Mutex::new(());

pub fn serialize_multi_node_test() -> MutexGuard<'static, ()> {
    MULTI_NODE_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A [`crate::P2pConfig`] whose chain identity is real rather than the
/// all-zero placeholder, for tests whose nodes are supposed to peer.
///
/// `P2pConfig::default()` resolves `genesis_hash` through
/// `config::resolve_genesis_hash`, which falls back to `ZERO_GENESIS_HASH`
/// when nothing is pinned. `GenesisHandshakeMessage::has_placeholder_identity`
/// treats that as launch-unsafe and `verify_match` refuses it — including
/// against another node with the *same* placeholder. That is deliberate: a node
/// with no configured genesis has no chain identity to agree about, so peering
/// it proves nothing.
///
/// So a multi-node test built on `P2pConfig::default()` is a topology
/// production would tear down on the first handshake. Until #2544 that never
/// showed, because no handshake was ever delivered: every node published
/// byte-identical handshake bytes, a content-derived `MessageId` made them one
/// message, and each node's own publish poisoned its duplicate cache against
/// every peer's copy. Once handshakes actually arrive, the fixtures have to
/// name a chain — as `genesis_handshake_disconnect` already did.
pub fn peering_config() -> crate::P2pConfig {
    crate::P2pConfig {
        network_id: "testnet".to_string(),
        chain_id: "wrfcoin-test".to_string(),
        genesis_hash: "aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000"
            .to_string(),
        ..crate::P2pConfig::default()
    }
}

/// Allocate an ephemeral TCP port by binding to `:0` and immediately releasing.
/// The OS guarantees the port is free at the time of allocation.
pub async fn allocate_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("should bind to ephemeral port");
    let port = listener.local_addr().unwrap().port();
    drop(listener); // Release before libp2p binds
    port
}

/// Poll `handle.event_rx` until at least `expected_peers` `PeerConnected` events
/// have been observed, then wait for gossipsub subscription confirmations to indicate
/// mesh formation. Bounded by `timeout_dur`.
///
/// This replaces the old fixed `sleep(2100ms)` pattern which was flaky on node host
/// under mining load, where gossipsub heartbeat jitter could exceed 2s.
pub async fn wait_for_gossip_mesh(
    handle: &mut P2pHandle,
    expected_peers: usize,
    timeout_dur: Duration,
) {
    use std::collections::HashSet;

    // Phase 1: wait for peer connections
    let connected = tokio::time::timeout(timeout_dur, async {
        let mut count = 0usize;
        loop {
            match handle.event_rx.try_recv() {
                Ok(P2pEvent::PeerConnected(_)) => {
                    count += 1;
                    if count >= expected_peers {
                        break;
                    }
                }
                Ok(_) => {}
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                    panic!("event channel closed while waiting for peers");
                }
            }
        }
    })
    .await;

    if connected.is_err() {
        panic!(
            "timed out waiting for {expected_peers} peer connections within {timeout_dur:?}"
        );
    }

    // Phase 2: wait for gossipsub mesh formation via subscription events.
    // Once all expected peers have subscribed to at least one topic, the mesh is ready.
    // This is more reliable than a fixed sleep, especially under load.
    let mesh_ready = tokio::time::timeout(timeout_dur, async {
        let mut subscribed_peers: HashSet<libp2p::PeerId> = HashSet::new();
        loop {
            match handle.event_rx.try_recv() {
                Ok(P2pEvent::SubscriptionStateChanged {
                    peer_id,
                    subscribed: true,
                    ..
                }) => {
                    subscribed_peers.insert(peer_id);
                    if subscribed_peers.len() >= expected_peers {
                        break;
                    }
                }
                Ok(_) => {}
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                    panic!("event channel closed while waiting for mesh formation");
                }
            }
        }
    })
    .await;

    if mesh_ready.is_err() {
        panic!(
            "timed out waiting for gossipsub mesh formation ({} peer subscriptions) within {timeout_dur:?}",
            expected_peers
        );
    }
}

/// Wait for gossipsub subscriptions to be confirmed on a single handle, indicating
/// that the mesh is ready for gossip messages. Use this after peer connections are
/// already confirmed, or in tests where you've manually verified connectivity.
///
/// This replaces the old pattern of fixed `sleep(2100ms)` waits for gossipsub mesh
/// heartbeats and is more reliable under load.
pub async fn wait_for_gossipsub_subscriptions(
    handle: &mut P2pHandle,
    expected_subscriptions: usize,
    timeout_dur: Duration,
) {
    let mesh_ready = tokio::time::timeout(timeout_dur, async {
        let mut subscription_count = 0usize;
        loop {
            match handle.event_rx.try_recv() {
                Ok(P2pEvent::SubscriptionStateChanged {
                    subscribed: true,
                    ..
                }) => {
                    subscription_count += 1;
                    if subscription_count >= expected_subscriptions {
                        break;
                    }
                }
                Ok(_) => {}
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                    panic!("event channel closed while waiting for subscriptions");
                }
            }
        }
    })
    .await;

    if mesh_ready.is_err() {
        panic!(
            "timed out waiting for gossipsub subscriptions ({} expected) within {timeout_dur:?}",
            expected_subscriptions
        );
    }
}
