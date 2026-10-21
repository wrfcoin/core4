//! Regression test: genesis handshake mismatch must disconnect the message
//! *author* (`message.source`) rather than the *relay* (`propagation_source`).
//!
//! In a multi-hop gossipsub mesh, `propagation_source` is the immediate peer
//! that forwarded the payload.  Before the fix, disconnecting
//! `propagation_source` would drop an innocent relay while the actual offender
//! remained connected.
//!
//! Topology used here:
//!   observer ←─── relay ←─── attacker
//!
//! The attacker has a mismatched genesis hash.  When it connects to the relay
//! it publishes its `GenesisHandshake`.  The relay (and gossipsub flood-publish)
//! may forward that message to the observer before the relay itself processes
//! the mismatch and disconnects the attacker.
//!
//! In the observer's handler:
//!   `propagation_source` = relay's PeerId
//!   `message.source`     = attacker's PeerId  (MessageAuthenticity::Signed)
//!
//! After the fix the observer disconnects the attacker, NOT the relay.
//! The key assertion is therefore: observer must never emit
//! `PeerDisconnected(relay_peer_id)`.

use std::time::Duration;
use tokio::time::timeout;

use libp2p::multiaddr::Protocol;

use super::helpers::allocate_port;
use crate::{P2pConfig, P2pEvent, P2pNode};

const TEST_TIMEOUT: Duration = Duration::from_secs(30);

#[tokio::test]
async fn handshake_mismatch_disconnects_author_not_relay() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    // -- relay (hub, matching genesis hash) ------------------------------------
    let relay_port = allocate_port().await;
    let relay_addr_base: libp2p::Multiaddr = format!("/ip4/127.0.0.1/tcp/{relay_port}")
        .parse()
        .expect("valid multiaddr");

    let relay_cfg = P2pConfig {
        listen_addresses: vec![relay_addr_base.clone()],
        genesis_hash: "aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000"
            .to_string(),
        network_id: "testnet".to_string(),
        chain_id: "wrfcoin-test".to_string(),
        max_peers_per_subnet: 0,
        ..P2pConfig::default()
    };
    let relay_handle = P2pNode::start(relay_cfg).await.expect("relay should start");
    let relay_peer_id = relay_handle.local_peer_id;
    let relay_addr = relay_addr_base.with(Protocol::P2p(relay_peer_id));

    // -- observer (spoke, same matching genesis hash) --------------------------
    let observer_cfg = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().expect("valid multiaddr")],
        bootstrap_peers: vec![relay_addr.clone()],
        genesis_hash: "aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000"
            .to_string(),
        network_id: "testnet".to_string(),
        chain_id: "wrfcoin-test".to_string(),
        max_peers_per_subnet: 0,
        ..P2pConfig::default()
    };
    let mut observer = P2pNode::start(observer_cfg)
        .await
        .expect("observer should start");

    // Wait for observer ↔ relay connection before introducing the attacker.
    timeout(TEST_TIMEOUT, async {
        loop {
            match observer.event_rx.recv().await {
                Some(P2pEvent::PeerConnected(_)) => break,
                Some(_) => continue,
                None => panic!("observer event channel closed waiting for relay connection"),
            }
        }
    })
    .await
    .expect("observer should connect to relay within timeout");

    // -- attacker (spoke, WRONG genesis hash) ----------------------------------
    let attacker_cfg = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().expect("valid multiaddr")],
        bootstrap_peers: vec![relay_addr],
        genesis_hash: "ffff0000ffff0000ffff0000ffff0000ffff0000ffff0000ffff0000ffff0000"
            .to_string(),
        network_id: "testnet".to_string(),
        chain_id: "wrfcoin-test".to_string(),
        max_peers_per_subnet: 0,
        ..P2pConfig::default()
    };
    let attacker = P2pNode::start(attacker_cfg)
        .await
        .expect("attacker should start");

    // Allow time for:
    //   1. attacker ↔ relay connection + gossipsub mesh heartbeats
    //   2. GenesisHandshake propagation from attacker → relay → observer
    //   3. observer processing the mismatch and acting on it
    tokio::time::sleep(Duration::from_millis(4_000)).await;

    // Drain all pending events from observer.
    // The critical assertion: relay must NOT appear in PeerDisconnected events.
    let mut relay_was_disconnected = false;
    while let Ok(event) = observer.event_rx.try_recv() {
        if let P2pEvent::PeerDisconnected(pid) = event {
            if pid == relay_peer_id {
                relay_was_disconnected = true;
            }
        }
    }

    assert!(
        !relay_was_disconnected,
        "observer must NOT disconnect the relay when an attacker sends a mismatched \
         genesis handshake that the relay forwarded — only the attacker (author) \
         should be disconnected"
    );

    drop(attacker);
}

/// Two-node variant: when a peer connects directly with a wrong genesis hash,
/// it must be disconnected.  In the direct case `source == propagation_source`
/// so this confirms the basic mismatch path works.
/// Establish a matching connection first, then send the wrong hash: otherwise
/// the attacker can reject our handshake and close before we receive its own.
#[tokio::test]
async fn handshake_mismatch_direct_peer_is_disconnected() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    let observer_port = allocate_port().await;
    let observer_addr: libp2p::Multiaddr = format!("/ip4/127.0.0.1/tcp/{observer_port}")
        .parse()
        .expect("valid multiaddr");

    let observer_cfg = P2pConfig {
        listen_addresses: vec![observer_addr.clone()],
        genesis_hash: "aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000"
            .to_string(),
        network_id: "testnet".to_string(),
        chain_id: "wrfcoin-test".to_string(),
        max_peers_per_subnet: 0,
        ..P2pConfig::default()
    };
    let mut observer = P2pNode::start(observer_cfg)
        .await
        .expect("observer should start");

    let attacker_cfg = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().expect("valid multiaddr")],
        bootstrap_peers: vec![observer_addr.with(Protocol::P2p(observer.local_peer_id))],
        genesis_hash: "aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000"
            .to_string(),
        network_id: "testnet".to_string(),
        chain_id: "wrfcoin-test".to_string(),
        max_peers_per_subnet: 0,
        ..P2pConfig::default()
    };
    let mut attacker = P2pNode::start(attacker_cfg)
        .await
        .expect("attacker should start");
    let attacker_peer_id = attacker.local_peer_id;

    timeout(TEST_TIMEOUT, async {
        loop {
            match attacker.event_rx.recv().await {
                Some(P2pEvent::PeerConnected(_)) => break,
                Some(_) => continue,
                None => panic!("attacker event channel closed"),
            }
        }
    })
    .await
    .expect("attacker should connect to observer within timeout");

    // Preserve handshake outcomes throughout subscription setup: the generic
    // subscription helper discards unrelated events.
    // Neither subscription readiness nor a disconnect alone proves the penalty.
    let mut penalized_author: Option<libp2p::PeerId> = None;
    let mut disconnected_peer: Option<libp2p::PeerId> = None;
    let mut collect_mismatch_event = |event| {
        match event {
            P2pEvent::PeerPenalized {
                peer_id, reason, ..
            } if reason.contains("genesis handshake mismatch") => {
                penalized_author = Some(peer_id);
            },
            P2pEvent::PeerDisconnected(peer_id) => disconnected_peer = Some(peer_id),
            _ => {},
        }
        penalized_author.is_some() && disconnected_peer.is_some()
    };
    let mut mismatch_observed = false;
    let (_, _) = tokio::join!(
        async {
            timeout(TEST_TIMEOUT, async {
                loop {
                    match observer.event_rx.recv().await {
                        Some(P2pEvent::SubscriptionStateChanged {
                            peer_id,
                            subscribed: true,
                            ..
                        }) if peer_id == attacker_peer_id => break,
                        Some(event) => mismatch_observed = collect_mismatch_event(event),
                        None => panic!("observer event channel closed waiting for subscriptions"),
                    }
                }
            })
            .await
            .expect("observer should see attacker subscriptions within timeout");
        },
        super::helpers::wait_for_gossipsub_subscriptions(&mut attacker, 1, TEST_TIMEOUT),
    );

    attacker
        .command_tx
        .send(crate::P2pCommand::BroadcastGenesisHandshake {
            genesis_hash: "ffff0000ffff0000ffff0000ffff0000ffff0000ffff0000ffff0000ffff0000"
                .to_string(),
        })
        .await
        .expect("attacker should broadcast a mismatched handshake once subscriptions are ready");

    // Collect BOTH outcomes in either order, including anything seen during
    // setup. A bare disconnect must still fail the #2116 penalty wiring assertion.
    // Bound the wait, then assert each outcome separately for useful diagnostics.
    let _ = timeout(TEST_TIMEOUT, async {
        while !mismatch_observed {
            match observer.event_rx.recv().await {
                Some(event) => mismatch_observed = collect_mismatch_event(event),
                None => panic!("observer event channel closed"),
            }
        }
    })
    .await;

    assert_eq!(
        disconnected_peer,
        Some(attacker_peer_id),
        "observer must disconnect the attacker that sent the mismatched genesis handshake"
    );

    assert_eq!(
        penalized_author,
        Some(attacker_peer_id),
        "the mismatch must also COST the attacker: before #2116 the whole action was a bare \
         disconnect, so a wrong-chain peer could reconnect and repeat indefinitely, each \
         cycle costing a full Noise handshake. This assertion is what proves the penalty is \
         actually wired into the match arm rather than merely unit-tested."
    );

    drop(attacker);
}
