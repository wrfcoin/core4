//! #1396 — p2p startup refuses to run when the protocol activation schedule is
//! not installed, **even though a signing version is**.
//!
//! This is the case #2275's harness cannot express. That file pins "nothing
//! installed at all"; this one pins the strictly harder state that #1396
//! introduces: **version present, schedule absent**. Adding
//! `try_activation_table() -> Option` makes "not established" representable,
//! but representability is worthless unless some caller refuses on it. This
//! test is the executable proof that the genesis-handshake boundary is that
//! caller.
//!
//! Found by review of PR #2500: the Option was added and documented as
//! preserving refuse-on-None, but no production caller consulted it, so a node
//! with the version installed and the schedule absent reached socket
//! construction. The counterexample was a `Transport(...)` error instead of a
//! `Config` refusal — startup had already passed the correctness gate.
//!
//! Why this must be its own test binary: `ACTIVE_VERSION` and `ACTIVE_TABLE`
//! are process-wide `OnceLock`s. This file installs a version and deliberately
//! never installs a schedule. A second test here that installed one would race
//! through the same `OnceLock` and turn the refusal into a flake — the same
//! single-test constraint #2275's harness documents.
//!
//! The refusal fires before any socket is bound, so this is safe in sandboxes
//! that deny binds (#1821).

use wrfcoin_p2p::{P2pConfig, P2pError, P2pNode};
use wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion;

#[tokio::test]
async fn p2p_startup_refuses_when_the_activation_schedule_is_absent() {
    // Install ONLY the version. This is the state a node reaches if the startup
    // gate installs the scalar but not the schedule — i.e. exactly the boot
    // ordering that #1396's gate is responsible for making impossible.
    SigningProtocolVersion::install_active(SigningProtocolVersion::V2);
    assert!(
        SigningProtocolVersion::try_active().is_some(),
        "precondition: the version must be installed, or this test would pass \
         for #2275's reason instead of this one"
    );

    let cfg = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().expect("valid multiaddr")],
        bootstrap_peers: Vec::new(),
        ..P2pConfig::default()
    };

    let err = match P2pNode::start(cfg).await {
        Ok(_) => panic!(
            "startup must refuse: a signing version is installed but no activation \
             schedule is, so `protocol_version_at` would silently answer from the \
             uninstalled fallback"
        ),
        Err(err) => err,
    };

    match err {
        P2pError::Config(msg) => {
            assert!(
                msg.contains("no protocol activation schedule installed"),
                "refusal must name the actual fault — not the #2275 version fault, \
                 which is satisfied here — got: {msg}"
            );
            assert!(
                msg.contains("assert_startup_protocol_compatibility"),
                "refusal must point the operator at the boot-ordering fix, got: {msg}"
            );
        },
        // A Transport error here is the pre-fix behaviour: startup passed the
        // correctness gate and got as far as building a socket.
        other => panic!(
            "expected P2pError::Config refusal before any socket state; got: {other:?}"
        ),
    }
}
