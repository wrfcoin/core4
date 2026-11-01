//! `passkey_bond_payout` parity: scheduled at H = 5 it flips on the
//! process-wide view at exactly 5 and moves NO state root across the
//! committed determinism fixture — before, at, or after activation.
//!
//! The fixture carries no node-bond lifecycle with a `payout_passkey` member,
//! so activation is a pure no-op for every recorded block: `expected_roots.json`
//! must hold unchanged with the feature ACTIVE from height 5 (a node that
//! activates the rule on a chain with no passkey payout traffic is
//! byte-identical to one that never heard of it). The applier, the restarted
//! applier and the committed expectation must agree.
//!
//! ONE test: the schedule is installed process-wide.

use test_server::determinism::{
    assert_root_parity, fixture_dir, replay_fixture, replay_fixture_with_restart,
};
use wrfcoin_shared_protocol::consensus::node_bond_registry::passkey_bond_payout_active;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, GENESIS_ACTIVATION_SCHEDULE_KEY, SigningProtocolVersion, feature_active,
    feature_scheduled, install_activation_table, install_feature_schedule,
    resolve_activation_schedule, resolve_feature_schedule,
};

const ACTIVATION_HEIGHT: u64 = 5;

#[test]
fn passkey_bond_payout_scheduled_at_five_flips_at_five_and_moves_no_root() {
    assert!(!feature_scheduled(ConsensusFeature::PasskeyBondPayout));
    let raw = format!("0:v1,{ACTIVATION_HEIGHT}:passkey_bond_payout");
    let metadata = [(GENESIS_ACTIVATION_SCHEDULE_KEY, raw.as_str())];
    let table = resolve_activation_schedule(metadata, SigningProtocolVersion::V1).unwrap();
    let features = resolve_feature_schedule(metadata, &table).unwrap();
    SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
    install_activation_table(table);
    install_feature_schedule(features);
    assert!(feature_scheduled(ConsensusFeature::PasskeyBondPayout));

    for h in 0..ACTIVATION_HEIGHT {
        assert!(!feature_active(ConsensusFeature::PasskeyBondPayout, h), "height {h}");
        assert!(!passkey_bond_payout_active(h), "height {h}");
    }
    for h in ACTIVATION_HEIGHT..ACTIVATION_HEIGHT + 5 {
        assert!(feature_active(ConsensusFeature::PasskeyBondPayout, h), "height {h}");
        assert!(passkey_bond_payout_active(h), "height {h}");
    }

    let dir = fixture_dir();
    let straight = replay_fixture(&dir);
    let restarted = replay_fixture_with_restart(&dir, ACTIVATION_HEIGHT as usize - 1);
    assert_eq!(
        straight, restarted,
        "a restart at the activation boundary must not move a root"
    );
    for h in [ACTIVATION_HEIGHT - 1, ACTIVATION_HEIGHT, ACTIVATION_HEIGHT + 1] {
        let a = straight.iter().find(|(height, _)| *height == h).map(|(_, r)| r);
        let b = restarted.iter().find(|(height, _)| *height == h).map(|(_, r)| r);
        assert!(a.is_some(), "fixture must cover height {h}");
        assert_eq!(a, b, "height {h}");
    }
    assert_root_parity(&straight, &dir);
}
