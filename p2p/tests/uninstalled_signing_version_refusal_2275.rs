//! #2275 — p2p startup refuses to run before a signing protocol version is
//! installed.
//!
//! This test binary is its own process and deliberately NEVER calls
//! `SigningProtocolVersion::install_active` (nor the startup gate that wraps
//! it). It must stay a single-test file: adding a second test that installs a
//! version would race this one through the process-wide `OnceLock` and turn
//! the refusal it exists to pin into a flake.
//!
//! This is the executable form of the launch ordering contract in
//! `launch_api/main.rs`: `assert_startup_protocol_compatibility` (installs the
//! version from genesis) must run before `P2pNode::start_with_keypair` (builds
//! the genesis handshake that advertises it). Before #2275 that ordering was
//! two line numbers with nothing enforcing them, and a mis-ordered node
//! silently advertised the V1 default while signing whatever the gate later
//! installed — the #2076 shape. Swapping the ordering back now fails startup
//! with the error this test asserts, instead of telling peers a lie.
//!
//! The refusal fires before any socket is bound, so this test is safe in
//! sandboxes that deny binds (#1821).

use wrfcoin_p2p::{P2pConfig, P2pError, P2pNode};

#[tokio::test]
async fn p2p_startup_refuses_when_no_signing_version_is_installed() {
    let cfg = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().expect("valid multiaddr")],
        bootstrap_peers: Vec::new(),
        ..P2pConfig::default()
    };

    let err = match P2pNode::start(cfg).await {
        Ok(_) => panic!("startup must refuse: nothing installed a signing protocol version"),
        Err(err) => err,
    };

    match err {
        P2pError::Config(msg) => {
            assert!(
                msg.contains("no signing protocol version installed"),
                "refusal must name the actual fault, got: {msg}"
            );
            assert!(
                msg.contains("assert_startup_protocol_compatibility"),
                "refusal must point the operator at the boot-ordering fix, got: {msg}"
            );
        },
        other => panic!("expected P2pError::Config refusal, got: {other:?}"),
    }
}
