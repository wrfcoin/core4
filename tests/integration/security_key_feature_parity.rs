//! `security_key_spend` determinism input: scheduled inside the committed
//! determinism fixture (with `account_authorization_dispatch`, which it rides
//! on), the feature flips at exactly its height and moves NO state root —
//! before, at, or after activation, and across a restart at the boundary.
//!
//! The fixture carries no `SecurityKeyAction` and no protected account, so
//! activation is a pure no-op for every recorded block: `expected_roots.json`
//! must hold unchanged. That is the rolling-upgrade guarantee the owner's
//! determinism differential then checks against the live chain.
//!
//! ONE test: the schedule is installed process-wide.

use test_server::determinism::{
    assert_root_parity, fixture_dir, replay_fixture, replay_fixture_with_restart,
};
use wrfcoin_shared_protocol::accounts::security_key::{security_key_active, security_key_scheduled};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, GENESIS_ACTIVATION_SCHEDULE_KEY, SigningProtocolVersion, feature_active,
    install_activation_table, install_feature_schedule, resolve_activation_schedule,
    resolve_feature_schedule,
};

const DISPATCH_HEIGHT: u64 = 3;
const ACTIVATION_HEIGHT: u64 = 5;

#[test]
fn security_key_spend_scheduled_at_five_flips_at_five_and_moves_no_root() {
    assert!(!security_key_scheduled());
    let raw = format!(
        "0:v1,{DISPATCH_HEIGHT}:account_authorization_dispatch,{ACTIVATION_HEIGHT}:security_key_spend"
    );
    let metadata = [(GENESIS_ACTIVATION_SCHEDULE_KEY, raw.as_str())];
    let table = resolve_activation_schedule(metadata, SigningProtocolVersion::V1).unwrap();
    let features = resolve_feature_schedule(metadata, &table).unwrap();
    SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
    install_activation_table(table);
    install_feature_schedule(features);
    assert!(security_key_scheduled());

    for h in 0..ACTIVATION_HEIGHT {
        assert!(!feature_active(ConsensusFeature::SecurityKeySpend, h), "height {h}");
        assert!(!security_key_active(h), "height {h}");
    }
    for h in ACTIVATION_HEIGHT..ACTIVATION_HEIGHT + 5 {
        assert!(feature_active(ConsensusFeature::SecurityKeySpend, h), "height {h}");
        assert!(security_key_active(h), "height {h}");
    }

    let dir = fixture_dir();
    let straight = replay_fixture(&dir);
    let restarted = replay_fixture_with_restart(&dir, ACTIVATION_HEIGHT as usize - 1);
    assert_eq!(straight, restarted, "a restart at the activation boundary must not move a root");
    for h in [ACTIVATION_HEIGHT - 1, ACTIVATION_HEIGHT, ACTIVATION_HEIGHT + 1] {
        let a = straight.iter().find(|(height, _)| *height == h).map(|(_, r)| r);
        let b = restarted.iter().find(|(height, _)| *height == h).map(|(_, r)| r);
        assert!(a.is_some(), "fixture must cover height {h}");
        assert_eq!(a, b, "height {h}");
    }
    assert_root_parity(&straight, &dir);
}
