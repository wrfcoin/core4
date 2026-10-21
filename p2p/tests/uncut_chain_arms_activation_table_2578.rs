//! #2578 / #1396 — a node booting an UNCUT chain must arm the activation
//! schedule, not only the scalar signing version.
//!
//! # The defect this pins
//!
//! `assert_startup_protocol_compatibility` (the path for a chain that already
//! has blocks) installs BOTH the version and the activation table. The uncut
//! path added by #2556 installed only the version. A missing table is
//! represented as `None` rather than a default, so p2p startup hit the #1396
//! refusal added by #2500 — the same day, by a different PR — and **every node
//! of a freshly cut chain came up with networking silently disabled**.
//!
//! It failed OPEN, which is why nothing caught it: the operator sets
//! `WRFCOIN_P2P_ENABLED=true`, preflight reports `bootstrap_peers  1 bootstrap
//! peer(s) configured`, and the node then logs one ERROR line and serves
//! traffic with `P2P: disabled` in its own banner, exit code 0. A single-node
//! smoke test passes perfectly. No multi-node testnet could be brought up at
//! all, and void CI hid it.
//!
//! # Why this is its own test binary
//!
//! `install_active` / `install_activation_table` are process-global `OnceLock`s.
//! This file therefore runs in a process that arms exactly once, the way a node
//! does, and asserts the post-conditions. A second test that armed differently
//! would race through the same `OnceLock` and turn this into a flake — the same
//! reason `uninstalled_signing_version_refusal_2275.rs` is a single-test file.
//!
//! # What this does NOT look at
//!
//! It does not boot `launch_api`, so it cannot prove that binary calls the
//! resolver. That is covered structurally instead: `resolve_uncut_chain_protocol`
//! returns the version and the schedule **as a pair**, so the uncut branch
//! cannot obtain one without the other. This test pins the pair's contract and
//! that p2p accepts what it produces. End-to-end proof that two freshly cut
//! nodes actually gossip is `scripts/demo-two-node-convergence.sh`.

use wrfcoin_shared_protocol::crypto::chain_binding::{
    ChainIdentity, SigningProtocolVersion, install_activation_table, resolve_uncut_chain_protocol,
    try_activation_table,
};

#[test]
fn arming_an_uncut_chain_installs_the_activation_schedule() {
    // Nothing is armed yet — the state a node is in before its startup gate.
    assert!(
        try_activation_table().is_none(),
        "precondition: this process must start with no schedule installed, \
         otherwise the assertion below proves nothing"
    );

    let identity = ChainIdentity::new("777", "testnet");

    // The uncut path: the operator is MAKING the chain-wide decision, not
    // overriding one, so configuration is authoritative.
    let (resolved, schedule) = resolve_uncut_chain_protocol(Some(SigningProtocolVersion::V2), &identity)
        .expect("an uncut chain with an explicit identity must resolve");

    assert_eq!(
        resolved,
        SigningProtocolVersion::V2,
        "the configured version is authoritative on an uncut chain"
    );
    // The identity schedule: `resolved` at every height, byte-for-byte what a
    // chain with no schedule does.
    assert_eq!(
        schedule.version_at(0),
        SigningProtocolVersion::V2,
        "the schedule must answer the resolved version at genesis"
    );
    assert_eq!(
        schedule.version_at(1_000_000),
        SigningProtocolVersion::V2,
        "with no genesis schedule the resolved version applies at every height"
    );

    // Arm it the way the node does.
    SigningProtocolVersion::install_active(resolved);
    install_activation_table(schedule);

    // THE ASSERTION. Before #2578 this was `None` on every freshly cut chain,
    // and p2p refused to start because of it.
    let installed = try_activation_table()
        .expect("after the uncut startup gate runs, the activation schedule MUST be installed — \
                 `None` here is the #2578 defect: p2p refuses and the node runs with networking \
                 silently disabled");
    assert_eq!(
        installed.version_at(0),
        SigningProtocolVersion::V2,
        "the installed schedule must agree with the installed scalar version; \
         disagreement is the #2076 shape (a v1 signer against a v2 verifier)"
    );
}
