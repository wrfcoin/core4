//! Integration test for peer-table recovery after a *full churn* (#1608).
//!
//! "Full churn" means every known peer disconnects at the same time: on the
//! two-node testnet this is a simultaneous restart of both nodes, leaving each
//! with an empty peer table. There is no mDNS in the libp2p feature set (see
//! `docs/operations/p2p-networking.md`), so a freshly-restarted two-node network
//! self-heals *only* via the static bootstrap list (`WRFCOIN_P2P_BOOTSTRAP`)
//! plus the #1668 auto-redial loop. These tests pin that behaviour.
//!
//! This complements two existing pieces of coverage:
//!   * `src/tests/bootstrap_redial.rs` restarts node A while node B stays alive,
//!     so B's in-memory redial state survives. Here *both* nodes are dropped and
//!     re-created, so the recovering node starts from a cold, empty peer table.
//!   * `scripts/drills/peer-discovery-full-churn.sh` exercises the same
//!     acceptance criteria against a live testnet over HTTP; this test makes the
//!     core assertion (`peer_count >= 1` within 60 s) runnable in CI.
//!
//! Acceptance criteria mirrored from the issue:
//!   * Both nodes stopped simultaneously, then restarted in either order.
//!   * The node rediscovers its peer within 60 s of the second node coming
//!     online, with no manual peer injection.
//!   * `connected_peers >= 1` (the in-process analogue of the
//!     `GET /api/v1/network/info` `peer_count` field) within that window.

use std::time::Duration;

use libp2p::{identity::Keypair, Multiaddr, PeerId};
use tokio::time::timeout;

use super::helpers::allocate_port;
use crate::{P2pCommand, P2pConfig, P2pEvent, P2pHandle, P2pNode};

/// Overall per-test budget; individual rediscovery assertions use the 60 s
/// window from the acceptance criteria.
const TEST_TIMEOUT: Duration = Duration::from_secs(90);
const REDISCOVERY_WINDOW: Duration = Duration::from_secs(60);

/// Wait until `event_rx` produces a `PeerConnected` event; panic on timeout.
async fn wait_for_connect(handle: &mut P2pHandle, dur: Duration) {
    timeout(dur, async {
        loop {
            match handle.event_rx.recv().await {
                Some(P2pEvent::PeerConnected(_)) => break,
                Some(_) => continue,
                None => panic!("event channel closed while waiting for connect"),
            }
        }
    })
    .await
    .expect("PeerConnected event did not arrive in time");
}

/// Drop a handle after asking its swarm to shut down and release its listener.
async fn shutdown(handle: P2pHandle) {
    let _ = handle.command_tx.send(P2pCommand::Shutdown).await;
    drop(handle);
}

/// Build node B's config: cold node whose only recovery lever is the static
/// bootstrap entry pointing at node A (no mDNS, no other seeds).
fn node_b_config(bootstrap_addr_a: &Multiaddr, redial: Duration, max_backoff: Duration) -> P2pConfig {
    P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().expect("valid multiaddr")],
        bootstrap_peers: vec![bootstrap_addr_a.clone()],
        max_peers_per_subnet: 0, // all test nodes share 127.0.0.1
        bootstrap_redial_interval: redial,
        bootstrap_redial_max_backoff: max_backoff,
        ..P2pConfig::default()
    }
}

/// Build node A's config: the always-on seed, no bootstrap of its own.
fn node_a_config(listen_a: &Multiaddr, redial: Duration, max_backoff: Duration) -> P2pConfig {
    P2pConfig {
        listen_addresses: vec![listen_a.clone()],
        bootstrap_peers: Vec::new(),
        max_peers_per_subnet: 0,
        bootstrap_redial_interval: redial,
        bootstrap_redial_max_backoff: max_backoff,
        ..P2pConfig::default()
    }
}

/// Full churn, A-then-B restart order: both nodes are dropped, then A is
/// restarted on its original address/identity, then a *fresh, cold* B is
/// started with only the static bootstrap entry. B must rebuild its peer
/// table from empty within the 60 s rediscovery window.
#[tokio::test]
async fn full_churn_cold_node_rebuilds_peer_table() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    let redial = Duration::from_secs(2);
    let max_backoff = Duration::from_secs(10);

    // Node A: fixed identity on a fixed port so its bootstrap multiaddr stays
    // valid across the restart.
    let keypair_a = Keypair::generate_ed25519();
    let peer_id_a = PeerId::from(keypair_a.public());
    let port_a = allocate_port().await;
    let listen_a: Multiaddr =
        format!("/ip4/127.0.0.1/tcp/{port_a}").parse().expect("valid addr");
    let bootstrap_addr_a: Multiaddr = format!("/ip4/127.0.0.1/tcp/{port_a}/p2p/{peer_id_a}")
        .parse()
        .expect("valid bootstrap addr");

    let mut handle_a =
        P2pNode::start_with_keypair(node_a_config(&listen_a, redial, max_backoff), keypair_a.clone())
            .await
            .expect("node A should start");
    let mut handle_b = P2pNode::start(node_b_config(&bootstrap_addr_a, redial, max_backoff))
        .await
        .expect("node B should start");

    // Establish the baseline two-node mesh.
    tokio::join!(
        wait_for_connect(&mut handle_a, TEST_TIMEOUT),
        wait_for_connect(&mut handle_b, TEST_TIMEOUT),
    );

    // ---- Full churn: stop BOTH nodes simultaneously ----------------------
    tokio::join!(shutdown(handle_a), shutdown(handle_b));

    // Let the OS release node A's port before we re-bind it.
    tokio::time::sleep(Duration::from_secs(2)).await;

    // ---- Restart in A-then-B order ---------------------------------------
    let mut handle_a2 = P2pNode::start_with_keypair(
        node_a_config(&listen_a, redial, max_backoff),
        keypair_a.clone(),
    )
    .await
    .expect("node A should restart on the same port/identity");

    // B is brand new: empty peer table, only the static bootstrap entry.
    let mut handle_b2 = P2pNode::start(node_b_config(&bootstrap_addr_a, redial, max_backoff))
        .await
        .expect("node B should restart");

    // B must rebuild its peer table within the 60 s rediscovery window.
    wait_for_connect(&mut handle_b2, REDISCOVERY_WINDOW).await;
    wait_for_connect(&mut handle_a2, REDISCOVERY_WINDOW).await;

    assert!(
        handle_b2.stats().connected_peers >= 1,
        "B should report peer_count >= 1 after rebuilding from full churn, got {}",
        handle_b2.stats().connected_peers
    );

    tokio::join!(shutdown(handle_a2), shutdown(handle_b2));
    tokio::time::sleep(Duration::from_millis(200)).await;
}

/// Full churn, B-then-A restart order (the "restarted in either order" branch):
/// the bootstrapping node B comes back *before* its only peer A. B's startup
/// dial fails against a still-down A, the auto-redial loop keeps trying from a
/// cold start, and B reconnects within the 60 s window once A returns — no
/// manual peer injection and no restart of B.
#[tokio::test]
async fn full_churn_redials_until_peer_returns() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    let redial = Duration::from_secs(2);
    let max_backoff = Duration::from_secs(8);

    let keypair_a = Keypair::generate_ed25519();
    let peer_id_a = PeerId::from(keypair_a.public());
    let port_a = allocate_port().await;
    let listen_a: Multiaddr =
        format!("/ip4/127.0.0.1/tcp/{port_a}").parse().expect("valid addr");
    let bootstrap_addr_a: Multiaddr = format!("/ip4/127.0.0.1/tcp/{port_a}/p2p/{peer_id_a}")
        .parse()
        .expect("valid bootstrap addr");

    let mut handle_a =
        P2pNode::start_with_keypair(node_a_config(&listen_a, redial, max_backoff), keypair_a.clone())
            .await
            .expect("node A should start");
    let mut handle_b = P2pNode::start(node_b_config(&bootstrap_addr_a, redial, max_backoff))
        .await
        .expect("node B should start");

    tokio::join!(
        wait_for_connect(&mut handle_a, TEST_TIMEOUT),
        wait_for_connect(&mut handle_b, TEST_TIMEOUT),
    );

    // ---- Full churn: stop BOTH nodes -------------------------------------
    tokio::join!(shutdown(handle_a), shutdown(handle_b));
    tokio::time::sleep(Duration::from_secs(2)).await;

    // ---- Restart B FIRST, while A is still down --------------------------
    let mut handle_b2 = P2pNode::start(node_b_config(&bootstrap_addr_a, redial, max_backoff))
        .await
        .expect("node B should restart cold");

    // Give the cold redial loop time to fire against the absent peer.
    tokio::time::sleep(Duration::from_secs(5)).await;
    let fail_before = handle_b2.stats().bootstrap_redial_attempts_fail;
    assert!(
        fail_before >= 1,
        "cold-started B should have attempted (and failed) at least one redial while A was down, got {fail_before}"
    );
    assert_eq!(
        handle_b2.stats().connected_peers,
        0,
        "B must have an empty peer table while its only bootstrap peer is down"
    );

    // ---- A returns on its original address/identity ----------------------
    let mut handle_a2 = P2pNode::start_with_keypair(
        node_a_config(&listen_a, redial, max_backoff),
        keypair_a.clone(),
    )
    .await
    .expect("node A should restart on the same port/identity");

    // B reconnects within the rediscovery window without being restarted.
    wait_for_connect(&mut handle_b2, REDISCOVERY_WINDOW).await;
    wait_for_connect(&mut handle_a2, REDISCOVERY_WINDOW).await;

    let stats_b = handle_b2.stats();
    assert!(
        stats_b.connected_peers >= 1,
        "B should report peer_count >= 1 after redial recovery, got {}",
        stats_b.connected_peers
    );
    assert!(
        stats_b.bootstrap_redial_attempts_success >= 1,
        "the redial codepath should have driven the recovery, success={}",
        stats_b.bootstrap_redial_attempts_success
    );

    tokio::join!(shutdown(handle_a2), shutdown(handle_b2));
    tokio::time::sleep(Duration::from_millis(200)).await;
}

/// Guard against silent regression of the recovery contract: a node whose only
/// peer is gone and that has *no* bootstrap entry (and no mDNS) cannot rebuild
/// its peer table on its own. This documents the gap from the issue's AC #5 as
/// an executable assertion — if dynamic discovery (e.g. mDNS) is ever added,
/// this test will start failing and should be updated.
#[tokio::test]
async fn no_bootstrap_means_no_self_healing() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    let cfg = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().expect("valid multiaddr")],
        bootstrap_peers: Vec::new(),
        max_peers_per_subnet: 0,
        bootstrap_redial_interval: Duration::from_millis(200),
        bootstrap_redial_max_backoff: Duration::from_secs(1),
        ..P2pConfig::default()
    };
    let handle = P2pNode::start(cfg).await.expect("node should start");

    // No peers configured and no mDNS: after several redial ticks the node is
    // still alone. This is the documented gap, not a bug.
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(
        handle.stats().connected_peers,
        0,
        "without a bootstrap entry there is no discovery mechanism to rebuild the peer table"
    );

    shutdown(handle).await;
}
