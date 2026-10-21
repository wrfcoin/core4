//! Peer churn integration tests (closes #1594, supports #1668).
//!
//! Codifies the R100 lesson: IBD stalls and reconnect failures that cost ~6h of
//! debugging at 2 AM should blow up in CI at PR time, not in production.
//!
//! Two in-process libp2p nodes proxy for "N5" (always-on blockchain node) and
//! "desktop" (intermittently connected peer). No Docker or external services.
//!
//! # Run
//! ```sh
//! cargo test --test peer_churn_e2e -p wrfcoin-native-chain --features p2p
//! ```

// An empty test binary reports "0 passed; 0 failed; 0 ignored" — identical
// to a passing target and invisible behind sibling suites (the vacuity shape
// scripts/cargo_test_nonvacuous.py rejects as EMPTY_TEST_BINARY). Without the
// feature this target now declares its absence as a countable `ignored`
// instead of compiling to silence — #2748's principle, extended to the cfg
// dimension.
#[cfg(not(feature = "p2p"))]
#[test]
#[ignore = "this target's tests require --features p2p; without it the binary would be empty and indistinguishable from a passing one. Run: cargo test -p wrfcoin-native-chain --features p2p --test peer_churn_e2e"]
fn tests_require_the_p2p_feature() {}

#[cfg(feature = "p2p")]
mod gated {
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;
    use tokio::time::timeout;
    use wrfcoin_native_chain::p2p_divergence::{
        ChainStateView, StateDivergenceRecoveryPolicy, observe_peer_chain_status,
    };
    use wrfcoin_p2p::{P2pCommand, P2pConfig, P2pEvent, P2pNode};
    use wrfcoin_shared_protocol::crypto::chain_binding::{
        ActivationTable, SigningProtocolVersion, install_activation_table,
    };
    use wrfcoin_shared_protocol::monitoring::CORE_METRICS;

    const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
    const MESH_TIMEOUT: Duration = Duration::from_secs(20);
    const CONVERGENCE_TIMEOUT: Duration = Duration::from_secs(30);
    // A shared non-placeholder identity is required for the production genesis
    // handshake to retain the P2P connection after subscription exchange.
    const TEST_GENESIS_HASH: &str =
        "a3301def68ac695636e90d7a66f83e6144a74da0ed5f234a298b73906a437658";

    // ---------------------------------------------------------------------------
    // Shared test helpers
    // ---------------------------------------------------------------------------

    /// Declare the signing protocol version for this test process (#2275, #2434).
    ///
    /// `P2pNode::start` refuses to boot when no version is installed, because the
    /// genesis handshake advertises the version the node actually signs with and
    /// must not fall back to a default — a fleet that advertised V1 while signing
    /// V2 would agree with itself all the way into the #2076 failure.
    ///
    /// In production `assert_startup_protocol_compatibility` installs it from
    /// genesis. A test harness has no genesis, so it declares one explicitly; that
    /// is exactly what the refusal message instructs. Without this, all three tests
    /// here died in `P2pNode::start` before reaching a single assertion.
    ///
    /// V2 because that is what this chain signs with today, so the handshake under
    /// test is the one that actually runs.
    ///
    /// `install_active` is first-install-wins and idempotent, so all three tests
    /// race safely to the same value. The returned value is asserted rather than
    /// discarded: if some other install won with a different version, these tests
    /// would be exercising a handshake nobody runs, and that must fail loudly
    /// rather than pass quietly.
    fn install_signing_version() {
        let installed = SigningProtocolVersion::install_active(SigningProtocolVersion::V2);
        assert_eq!(
            installed,
            SigningProtocolVersion::V2,
            "another install won the race with a different version — broken test binary"
        );

        // #1396: p2p startup refuses to advertise a default schedule, so a harness
        // that installs only the scalar version cannot start a node at all — all
        // three tests failed here with "no protocol activation schedule installed".
        //
        // The refusal message suggests `ActivationTable::default()`, but that
        // identity schedule resolves V1 from genesis while this harness declares
        // V2 as active. The current handshake carries only the scalar version, so
        // this mismatch is latent in these tests; it is still the wrong model for
        // a production-shaped harness and could become observable when a P2P
        // caller consults the height-derived schedule.
        //
        // So pin V2 at genesis, matching `install_active` above. Same
        // first-install-wins contract, so the three tests race safely; the result
        // is asserted rather than discarded for the same reason.
        let table = ActivationTable::from_entries(vec![(0, SigningProtocolVersion::V2)])
            .expect("V2-at-genesis is a valid single-entry schedule");
        let installed_table = install_activation_table(table);
        assert_eq!(
            installed_table.version_at(0),
            SigningProtocolVersion::V2,
            "another install won the race with a different schedule — broken test binary"
        );
        assert_eq!(
            installed_table.version_at(u64::MAX),
            SigningProtocolVersion::V2,
            "the installed schedule must be V2 at every height these tests reach"
        );
    }

    /// Bind to `127.0.0.1:0`, record the assigned port, drop the listener.
    async fn allocate_port() -> u16 {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("ephemeral port bind");
        let port = l.local_addr().unwrap().port();
        drop(l);
        port
    }

    /// Drain `handle.event_rx` until a `PeerConnected` event is observed.
    async fn wait_connected(handle: &mut wrfcoin_p2p::P2pHandle, label: &str, timeout_dur: Duration) {
        timeout(timeout_dur, async {
            loop {
                match handle.event_rx.recv().await {
                    Some(P2pEvent::PeerConnected(_)) => break,
                    Some(_) => {},
                    None => panic!("[{label}] event channel closed while waiting for PeerConnected"),
                }
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!("[{label}] timed out after {timeout_dur:?} waiting for PeerConnected")
        });
    }

    /// Drain `handle.event_rx` until a `PeerDisconnected` event is observed.
    async fn wait_disconnected(
        handle: &mut wrfcoin_p2p::P2pHandle,
        label: &str,
        timeout_dur: Duration,
    ) {
        timeout(timeout_dur, async {
            loop {
                match handle.event_rx.recv().await {
                    Some(P2pEvent::PeerDisconnected(_)) => break,
                    Some(_) => {},
                    None => panic!("[{label}] event channel closed while waiting for PeerDisconnected"),
                }
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!("[{label}] timed out after {timeout_dur:?} waiting for PeerDisconnected")
        });
    }

    /// Drain `handle.event_rx` until a peer-subscription event is observed.
    ///
    /// `SubscriptionStateChanged{subscribed: true}` indicates that the gossipsub
    /// mesh is ready to deliver messages on the subscribed topic. We wait for this
    /// on node A after B2 connects to ensure that `BroadcastChainStatus` from A
    /// actually reaches B2.
    async fn wait_peer_subscribed(
        handle: &mut wrfcoin_p2p::P2pHandle,
        label: &str,
        timeout_dur: Duration,
    ) {
        timeout(timeout_dur, async {
            loop {
                match handle.event_rx.recv().await {
                    Some(P2pEvent::SubscriptionStateChanged {
                        subscribed: true, ..
                    }) => break,
                    Some(_) => {},
                    None => {
                        panic!(
                            "[{label}] event channel closed while waiting for gossipsub subscription"
                        )
                    },
                }
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!("[{label}] timed out after {timeout_dur:?} waiting for gossipsub mesh subscription")
        });
    }

    /// Drain `handle.event_rx` until `PeerChainStatus` with height >= `min_height`
    /// is observed. Returns the observed chain view.
    ///
    /// Failure message includes both the expected and received heights so a
    /// divergence is immediately actionable without re-running with `--nocapture`.
    async fn wait_chain_status(
        handle: &mut wrfcoin_p2p::P2pHandle,
        label: &str,
        min_height: u64,
        timeout_dur: Duration,
    ) -> ChainStateView {
        let mut last_seen: Option<u64> = None;
        timeout(timeout_dur, async {
            loop {
                match handle.event_rx.recv().await {
                    Some(P2pEvent::PeerChainStatus {
                        height,
                        tip_hash,
                        state_root,
                        ..
                    }) => {
                        last_seen = Some(height);
                        if height >= min_height {
                            break ChainStateView {
                                height,
                                tip_hash,
                                state_root,
                            };
                        }
                    }
                    Some(_) => {}
                    None => panic!("[{label}] event channel closed while waiting for PeerChainStatus"),
                }
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "[{label}] timed out after {timeout_dur:?}: wanted PeerChainStatus height >= {min_height}, \
                 last seen = {last_seen:?}"
            )
        })
    }

    // ---------------------------------------------------------------------------
    // Test (a): N5 keeps advancing under peer disconnect
    // ---------------------------------------------------------------------------

    /// Regression for R100: the N5 node must keep advancing its local chain
    /// height and immediately reflect `peer_count == 0` after the desktop disconnects.
    ///
    /// In R100, the IBD stall obscured the fact that peer tracking was lagging; this
    /// test catches any deadlock or panic in the chain-state update path when the
    /// gossipsub mesh is empty.
    #[tokio::test]
    async fn test_n5_progresses_under_peer_disconnect() {
        let _ = env_logger::builder().is_test(true).try_init();
        install_signing_version();

        // Node A: N5 proxy on a fixed port for readability.
        let port_a = allocate_port().await;
        let addr_a = format!("/ip4/127.0.0.1/tcp/{port_a}");
        let cfg_a = P2pConfig {
            listen_addresses: vec![addr_a.parse().expect("valid addr")],
            genesis_hash: TEST_GENESIS_HASH.to_string(),
            ..P2pConfig::default()
        };
        let mut handle_a = P2pNode::start(cfg_a)
            .await
            .expect("node A (N5) should start");

        // Node B: desktop proxy, connects to A.
        let cfg_b = P2pConfig {
            listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().expect("valid addr")],
            bootstrap_peers: vec![addr_a.parse().expect("valid addr")],
            genesis_hash: TEST_GENESIS_HASH.to_string(),
            ..P2pConfig::default()
        };
        let mut handle_b = P2pNode::start(cfg_b)
            .await
            .expect("node B (desktop) should start");

        // Both sides confirm the TCP connection.
        tokio::join!(
            wait_connected(&mut handle_a, "N5", CONNECT_TIMEOUT),
            wait_connected(&mut handle_b, "desktop", CONNECT_TIMEOUT),
        );

        // Simulate A mining 3 blocks while B is still online.
        handle_a
            .command_tx
            .send(P2pCommand::UpdateLocalChainState {
                height: 3,
                tip_hash: [0xAA; 32],
                state_root: [0; 32],
            })
            .await
            .expect("UpdateLocalChainState must succeed with peer connected");

        // Force-disconnect B (desktop power-off / network drop).
        let _ = handle_b.command_tx.send(P2pCommand::Shutdown).await;

        // A must observe the disconnection promptly.
        wait_disconnected(&mut handle_a, "N5", CONNECT_TIMEOUT).await;

        let peers_after_drop = handle_a.stats().connected_peers;
        assert_eq!(
            peers_after_drop, 0,
            "peer_count must drop to 0 immediately after desktop disconnects, got {peers_after_drop}"
        );

        // Mine 5 more blocks on A with zero peers. Each UpdateLocalChainState must
        // complete without panic or deadlock even though gossipsub has nobody to
        // propagate to.
        for h in 4u64..=8 {
            handle_a
                .command_tx
                .send(P2pCommand::UpdateLocalChainState {
                    height: h,
                    tip_hash: [0xBB; 32],
                    state_root: [0; 32],
                })
                .await
                .unwrap_or_else(|_| {
                    panic!("UpdateLocalChainState at height {h} must not fail with 0 peers")
                });
        }
        let final_height = 8u64;

        // Peer count still zero after mining through the disconnection.
        let final_peers = handle_a.stats().connected_peers;
        assert_eq!(
            final_peers, 0,
            "peer_count must remain 0 after advancing to height {final_height}, got {final_peers}"
        );

        // Confirm that the BroadcastChainStatus path is also non-blocking with 0 peers.
        handle_a
            .command_tx
            .send(P2pCommand::BroadcastChainStatus {
                height: final_height,
                tip_hash: [0xBB; 32],
                state_root: [0; 32],
            })
            .await
            .expect("BroadcastChainStatus must not block or error with 0 peers");

        let _ = handle_a.command_tx.send(P2pCommand::Shutdown).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // ---------------------------------------------------------------------------
    // Test (b): Desktop peer height-matches N5 after reconnect
    // ---------------------------------------------------------------------------

    /// Regression for R100: once the desktop reconnects it must converge on N5's
    /// current chain tip within 30 seconds.
    ///
    /// R100 spent ~30 min rediscovering that the desktop had not auto-reconnected
    /// at all. This test codifies the convergence guarantee at the P2P gossip layer:
    /// as soon as a fresh TCP connection is open and the gossipsub mesh is ready,
    /// `BroadcastChainStatus` from A must reach B2 and the heights must match.
    #[tokio::test]
    async fn test_peer_height_match_after_reconnect() {
        let _ = env_logger::builder().is_test(true).try_init();
        install_signing_version();

        // Node A: N5 proxy on a fixed port so B2 can reconnect to the same address.
        let port_a = allocate_port().await;
        let addr_a = format!("/ip4/127.0.0.1/tcp/{port_a}");
        let cfg_a = P2pConfig {
            listen_addresses: vec![addr_a.parse().expect("valid addr")],
            genesis_hash: TEST_GENESIS_HASH.to_string(),
            ..P2pConfig::default()
        };
        let mut handle_a = P2pNode::start(cfg_a)
            .await
            .expect("node A (N5) should start");

        // Node B1: first desktop session.
        let cfg_b1 = P2pConfig {
            listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().expect("valid addr")],
            bootstrap_peers: vec![addr_a.parse().expect("valid addr")],
            genesis_hash: TEST_GENESIS_HASH.to_string(),
            ..P2pConfig::default()
        };
        let mut handle_b1 = P2pNode::start(cfg_b1).await.expect("node B1 should start");

        tokio::join!(
            wait_connected(&mut handle_a, "N5 (B1 session)", CONNECT_TIMEOUT),
            wait_connected(&mut handle_b1, "desktop B1", CONNECT_TIMEOUT),
        );

        // Desktop goes offline.
        let _ = handle_b1.command_tx.send(P2pCommand::Shutdown).await;
        wait_disconnected(&mut handle_a, "N5 (after B1 drop)", CONNECT_TIMEOUT).await;

        // N5 advances 10 blocks while desktop is offline — the height gap B2 must close.
        let n5_height: u64 = 10;
        let n5_tip: [u8; 32] = [0xDE; 32];
        handle_a
            .command_tx
            .send(P2pCommand::UpdateLocalChainState {
                height: n5_height,
                tip_hash: n5_tip,
                state_root: [0; 32],
            })
            .await
            .expect("UpdateLocalChainState must succeed");

        // B2: fresh desktop session, dials A (simulates manual or auto-reconnect).
        let cfg_b2 = P2pConfig {
            listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().expect("valid addr")],
            bootstrap_peers: vec![addr_a.parse().expect("valid addr")],
            genesis_hash: TEST_GENESIS_HASH.to_string(),
            ..P2pConfig::default()
        };
        let mut handle_b2 = P2pNode::start(cfg_b2).await.expect("node B2 should start");

        // Wait for A to see B2 connect, then wait for the gossipsub mesh to form.
        // Only once A has received B2's topic subscriptions is gossipsub ready to
        // deliver messages to B2.
        wait_connected(&mut handle_a, "N5 (B2 session)", CONNECT_TIMEOUT).await;
        wait_peer_subscribed(&mut handle_a, "N5 (B2 subscription)", MESH_TIMEOUT).await;

        // A announces its current tip. In production this fires every 10-300 s; we
        // trigger it immediately so the test stays well under the 60 s budget.
        handle_a
            .command_tx
            .send(P2pCommand::BroadcastChainStatus {
                height: n5_height,
                tip_hash: n5_tip,
                state_root: [0; 32],
            })
            .await
            .expect("BroadcastChainStatus must succeed after B2 reconnects");

        // B2 must receive A's chain status with the correct height within 30 s.
        let observed =
            wait_chain_status(&mut handle_b2, "desktop B2", n5_height, CONVERGENCE_TIMEOUT).await;

        assert_eq!(
            observed.height, n5_height,
            "after reconnect, desktop (B2) must converge on N5 chain height {n5_height}, got {}",
            observed.height
        );

        let _ = handle_a.command_tx.send(P2pCommand::Shutdown).await;
        let _ = handle_b2.command_tx.send(P2pCommand::Shutdown).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // ---------------------------------------------------------------------------
    // Test (c): Chain-state divergence metric fires on partition
    // ---------------------------------------------------------------------------

    /// Verifies that `wrfcoin_chain_state_diverged_total` increments when two nodes
    /// hold different tips at the same height (partition / accidental fork).
    ///
    /// The P2P node emits the peer chain-status gossip; the native-chain app bridge
    /// owns the latching Prometheus metric behavior that operators alert on.
    #[tokio::test]
    async fn test_chain_state_diverged_metric_fires_on_partition() {
        let _ = env_logger::builder().is_test(true).try_init();
        install_signing_version();

        // Nodes A and B start at the same height but with different tip hashes,
        // simulating a partition where each side mined a competing block.
        let port_a = allocate_port().await;
        let addr_a = format!("/ip4/127.0.0.1/tcp/{port_a}");
        let cfg_a = P2pConfig {
            listen_addresses: vec![addr_a.parse().expect("valid addr")],
            genesis_hash: TEST_GENESIS_HASH.to_string(),
            ..P2pConfig::default()
        };
        let mut handle_a = P2pNode::start(cfg_a).await.expect("node A should start");

        let cfg_b = P2pConfig {
            listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().expect("valid addr")],
            bootstrap_peers: vec![addr_a.parse().expect("valid addr")],
            genesis_hash: TEST_GENESIS_HASH.to_string(),
            ..P2pConfig::default()
        };
        let mut handle_b = P2pNode::start(cfg_b).await.expect("node B should start");

        tokio::join!(
            wait_connected(&mut handle_a, "A (partition)", CONNECT_TIMEOUT),
            wait_connected(&mut handle_b, "B (partition)", CONNECT_TIMEOUT),
        );
        wait_peer_subscribed(&mut handle_a, "A (subscription)", MESH_TIMEOUT).await;

        // Both nodes at height 50 but with different tips — a fork.
        let fork_height: u64 = 50;
        let tip_a: [u8; 32] = [0xAA; 32];
        let tip_b: [u8; 32] = [0xBB; 32];
        let root_a: [u8; 32] = [0; 32];
        let root_b: [u8; 32] = [1; 32];
        let counter_before = CORE_METRICS.state_diverged_total.get();

        handle_a
            .command_tx
            .send(P2pCommand::UpdateLocalChainState {
                height: fork_height,
                tip_hash: tip_a,
                state_root: root_a,
            })
            .await
            .expect("UpdateLocalChainState A must succeed");

        handle_b
            .command_tx
            .send(P2pCommand::UpdateLocalChainState {
                height: fork_height,
                tip_hash: tip_b,
                state_root: root_b,
            })
            .await
            .expect("UpdateLocalChainState B must succeed");

        // Both broadcast their chain status. Each peer will see height-matched but
        // tip-mismatched PeerChainStatus, which should trigger the divergence counter.
        handle_a
            .command_tx
            .send(P2pCommand::BroadcastChainStatus {
                height: fork_height,
                tip_hash: tip_a,
                state_root: root_a,
            })
            .await
            .expect("BroadcastChainStatus A must succeed");

        handle_b
            .command_tx
            .send(P2pCommand::BroadcastChainStatus {
                height: fork_height,
                tip_hash: tip_b,
                state_root: root_b,
            })
            .await
            .expect("BroadcastChainStatus B must succeed");

        // Verify that each node received the other's chain status (gossip path sanity).
        let observed_by_b =
            wait_chain_status(&mut handle_b, "B sees A", fork_height, CONVERGENCE_TIMEOUT).await;
        let observed_by_a =
            wait_chain_status(&mut handle_a, "A sees B", fork_height, CONVERGENCE_TIMEOUT).await;

        let node_b_diverged = AtomicBool::new(false);
        let node_a_diverged = AtomicBool::new(false);
        observe_peer_chain_status(
            "12D3KooWPeerChurnNodeA",
            observed_by_b,
            ChainStateView {
                height: fork_height,
                tip_hash: tip_b,
                state_root: root_b,
            },
            &node_b_diverged,
            StateDivergenceRecoveryPolicy::PeerAgreement,
        );
        observe_peer_chain_status(
            "12D3KooWPeerChurnNodeB",
            observed_by_a,
            ChainStateView {
                height: fork_height,
                tip_hash: tip_a,
                state_root: root_a,
            },
            &node_a_diverged,
            StateDivergenceRecoveryPolicy::PeerAgreement,
        );

        let counter_after = CORE_METRICS.state_diverged_total.get();
        assert_eq!(
            counter_after.saturating_sub(counter_before),
            2,
            "both partitioned nodes should latch exactly one divergence episode"
        );

        let _ = handle_a.command_tx.send(P2pCommand::Shutdown).await;
        let _ = handle_b.command_tx.send(P2pCommand::Shutdown).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
