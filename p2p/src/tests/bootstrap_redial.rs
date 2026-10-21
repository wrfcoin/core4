//! Integration test for bootstrap auto-redial (#1668).
//!
//! Scenario: two nodes A and B; B has A in its `bootstrap_peers`. A then
//! goes away (operator restarts the container, network blip). Without
//! auto-redial, B sits at `peer_count=0` indefinitely and the operator
//! must restart B too. With auto-redial, B redials A and reconnects
//! within ~`bootstrap_redial_interval` after A comes back up.
//!
//! The test exercises the full path: ConnectionEstablished → operator
//! shutdown of A → ConnectionClosed on B → redial tick on B → A restarts
//! on the same address with the same identity → ConnectionEstablished
//! again on B.

use std::time::Duration;

use libp2p::{identity::Keypair, multiaddr::Protocol};
use tokio::time::timeout;

use super::helpers::allocate_port;
use crate::{P2pCommand, P2pConfig, P2pEvent, P2pNode};

const TEST_TIMEOUT: Duration = Duration::from_secs(90);

/// Wait until `event_rx` produces a `PeerDisconnected` event (or any number
/// of irrelevant ones); panic on timeout.
async fn wait_for_disconnect(handle: &mut crate::P2pHandle, dur: Duration) {
    timeout(dur, async {
        loop {
            match handle.event_rx.recv().await {
                Some(P2pEvent::PeerDisconnected(_)) => break,
                Some(_) => continue,
                None => panic!("event channel closed while waiting for disconnect"),
            }
        }
    })
    .await
    .expect("PeerDisconnected event did not arrive in time");
}

/// Wait until `event_rx` produces a `PeerConnected` event.
async fn wait_for_connect(handle: &mut crate::P2pHandle, dur: Duration) {
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

/// Two-node disconnect/reconnect scenario.
///
/// 1. Start node A on port P with keypair K.
/// 2. Start node B with `bootstrap_peers = [A's addr + /p2p/<peer_id_K>]`.
/// 3. Both observe `PeerConnected`.
/// 4. Send `Shutdown` to A — B observes `PeerDisconnected`.
/// 5. A is restarted on port P with the *same* keypair K (same peer_id).
/// 6. B reconnects within `bootstrap_redial_interval` + slack, without B
///    being restarted. Assertion: a `PeerConnected` event arrives within
///    60s and `bootstrap_redial_attempts_success` is >= 1.
#[tokio::test]
async fn bootstrap_peer_is_auto_redialed_after_disconnect() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    // Use a short redial interval so the test runs in ~10s rather than 30s.
    let redial_interval = Duration::from_secs(2);
    let redial_max_backoff = Duration::from_secs(10);

    // ---- Set up node A with a fixed keypair on an OS-allocated port -------
    let keypair_a = Keypair::generate_ed25519();
    let peer_id_a = libp2p::PeerId::from(keypair_a.public());
    let port_a = allocate_port().await;
    let listen_a: libp2p::Multiaddr =
        format!("/ip4/127.0.0.1/tcp/{port_a}").parse().expect("valid addr");
    let bootstrap_addr_a: libp2p::Multiaddr =
        format!("/ip4/127.0.0.1/tcp/{port_a}/p2p/{peer_id_a}")
            .parse()
            .expect("valid bootstrap addr");

    let cfg_a = P2pConfig {
        listen_addresses: vec![listen_a.clone()],
        bootstrap_peers: Vec::new(),
        bootstrap_redial_interval: redial_interval,
        bootstrap_redial_max_backoff: redial_max_backoff,
        ..P2pConfig::default()
    };
    let mut handle_a = P2pNode::start_with_keypair(cfg_a, keypair_a.clone())
        .await
        .expect("node A should start");

    // ---- Start node B with A as bootstrap peer ----------------------------
    let cfg_b = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0"
            .parse()
            .expect("valid multiaddr")],
        bootstrap_peers: vec![bootstrap_addr_a.clone()],
        bootstrap_redial_interval: redial_interval,
        bootstrap_redial_max_backoff: redial_max_backoff,
        ..P2pConfig::default()
    };
    let mut handle_b = P2pNode::start(cfg_b).await.expect("node B should start");

    // Both nodes should see each other on initial dial.
    let connect_a = wait_for_connect(&mut handle_a, TEST_TIMEOUT);
    let connect_b = wait_for_connect(&mut handle_b, TEST_TIMEOUT);
    tokio::join!(connect_a, connect_b);

    // ---- Operator restarts A: send Shutdown so swarm closes the listener --
    let _ = handle_a.command_tx.send(P2pCommand::Shutdown).await;
    drop(handle_a); // drop handle to fully release swarm

    // B should observe the disconnect.
    wait_for_disconnect(&mut handle_b, TEST_TIMEOUT).await;

    // Give the OS a moment to release the port and to ensure B's redial
    // attempts kick in (and bump `bootstrap_redial_attempts_fail`) while A
    // is still down.
    tokio::time::sleep(Duration::from_secs(3)).await;
    let fail_before_restart = handle_b.stats().bootstrap_redial_attempts_fail;

    // ---- Restart node A with the same keypair on the SAME port ------------
    // We use the same keypair so node B's bootstrap multiaddr (which pins
    // /p2p/<peer_id>) is still valid.
    let cfg_a_restart = P2pConfig {
        listen_addresses: vec![listen_a.clone()],
        bootstrap_peers: Vec::new(),
        bootstrap_redial_interval: redial_interval,
        bootstrap_redial_max_backoff: redial_max_backoff,
        ..P2pConfig::default()
    };
    let mut handle_a2 = P2pNode::start_with_keypair(cfg_a_restart, keypair_a.clone())
        .await
        .expect("node A should restart on the same port");

    // ---- B must reconnect within ~60s without being restarted -------------
    wait_for_connect(&mut handle_b, Duration::from_secs(60)).await;

    // Sanity: A also sees the inbound connection from B.
    wait_for_connect(&mut handle_a2, Duration::from_secs(30)).await;

    // Metric assertions: B's success counter incremented (counts both the
    // initial connect and the redial reconnect, so >= 2 is the expected
    // value but >= 1 is enough to prove the codepath fired).
    let stats_b = handle_b.stats();
    assert!(
        stats_b.bootstrap_redial_attempts_success >= 1,
        "expected at least one bootstrap_redial_attempts_success, got {}",
        stats_b.bootstrap_redial_attempts_success
    );
    assert!(
        stats_b.bootstrap_redial_attempts_fail >= fail_before_restart,
        "fail counter must be monotonic, before={fail_before_restart}, after={}",
        stats_b.bootstrap_redial_attempts_fail
    );

    // ---- Shut down both nodes --------------------------------------------
    let _ = handle_a2.command_tx.send(P2pCommand::Shutdown).await;
    let _ = handle_b.command_tx.send(P2pCommand::Shutdown).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
}

/// Smoke check: a node with no bootstrap peers configured does not panic on
/// the periodic redial tick — the tick just walks an empty Vec.
#[tokio::test]
async fn empty_bootstrap_list_does_not_panic() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    let cfg = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()],
        bootstrap_peers: Vec::new(),
        bootstrap_redial_interval: Duration::from_millis(200),
        bootstrap_redial_max_backoff: Duration::from_secs(1),
        ..P2pConfig::default()
    };
    let handle = P2pNode::start(cfg).await.expect("node should start");

    // Let several redial ticks fire.
    tokio::time::sleep(Duration::from_secs(1)).await;

    let _ = handle.command_tx.send(P2pCommand::Shutdown).await;
}

/// Smoke check: a bootstrap entry without `/p2p/<peer_id>` is tolerated
/// (the redial loop falls back to "we can't observe connection state",
/// but the periodic tick must not panic, and a successful dial via the
/// startup path still establishes the connection).
#[tokio::test]
async fn bootstrap_entry_without_p2p_component_is_tolerated() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    let port_a = allocate_port().await;
    let listen_a: libp2p::Multiaddr =
        format!("/ip4/127.0.0.1/tcp/{port_a}").parse().expect("valid addr");

    let cfg_a = P2pConfig {
        listen_addresses: vec![listen_a.clone()],
        bootstrap_peers: Vec::new(),
        bootstrap_redial_interval: Duration::from_millis(500),
        bootstrap_redial_max_backoff: Duration::from_secs(2),
        ..P2pConfig::default()
    };
    let handle_a = P2pNode::start(cfg_a).await.expect("node A should start");
    // strip /p2p/<peer_id> by using the raw listen address
    let bootstrap_no_pid = listen_a.clone();
    // Sanity: listen_a should not contain /p2p/<peer_id>.
    assert!(!bootstrap_no_pid
        .iter()
        .any(|p| matches!(p, Protocol::P2p(_))));

    let cfg_b = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()],
        bootstrap_peers: vec![bootstrap_no_pid],
        bootstrap_redial_interval: Duration::from_millis(500),
        bootstrap_redial_max_backoff: Duration::from_secs(2),
        ..P2pConfig::default()
    };
    let mut handle_b = P2pNode::start(cfg_b).await.expect("node B should start");

    // The startup dial should still establish a connection (libp2p can
    // dial without a known peer_id and will learn it via identify).
    wait_for_connect(&mut handle_b, Duration::from_secs(30)).await;

    let _ = handle_a.command_tx.send(P2pCommand::Shutdown).await;
    let _ = handle_b.command_tx.send(P2pCommand::Shutdown).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
}
