//! #3092 parity: a consensus feature scheduled at H = 5 flips on every node
//! at exactly 5 and moves no state root.
//!
//! The replay harness is the applier surface (`State::apply_transition`);
//! `replay_fixture_with_restart` is the same applier across a serialise /
//! restore boundary (the IBD-reload analogue); `expected_roots.json` is the
//! committed expectation every producer-built block was recorded against.
//! With `domain_separated_preimages` scheduled at 5 and no consumer yet, all
//! three must agree at heights 4, 5 and 6 — and everywhere else — while
//! `feature_active` reports the flip. The producer and P2P surfaces consult
//! the same `feature_active` sites (`proof_of_publishing.rs`) and are covered
//! by native-chain's producer/applier/WAL parity targets.
//!
//! This file holds ONE test because the schedule is installed process-wide.

use test_server::determinism::{
    assert_root_parity, fixture_dir, replay_fixture, replay_fixture_with_restart,
};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, GENESIS_ACTIVATION_SCHEDULE_KEY, SigningProtocolVersion, feature_active,
    install_activation_table, install_feature_schedule, protocol_version_at,
    resolve_activation_schedule, resolve_feature_schedule, try_feature_schedule,
};

const ACTIVATION_HEIGHT: u64 = 5;

#[test]
fn a_feature_scheduled_at_five_flips_at_five_on_every_surface_and_moves_no_root() {
    let raw = format!("0:v1,{ACTIVATION_HEIGHT}:domain_separated_preimages");
    let metadata = [(GENESIS_ACTIVATION_SCHEDULE_KEY, raw.as_str())];
    let table = resolve_activation_schedule(metadata, SigningProtocolVersion::V1).unwrap();
    let features = resolve_feature_schedule(metadata, &table).unwrap();

    // Arm exactly as the boot gate does: version, table, features.
    SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
    install_activation_table(table);
    install_feature_schedule(features);
    assert!(try_feature_schedule().is_some());

    // The flip is at H, on the process-wide view every surface consults.
    for h in 0..ACTIVATION_HEIGHT {
        assert!(!feature_active(ConsensusFeature::DomainSeparatedPreimages, h), "height {h}");
    }
    for h in ACTIVATION_HEIGHT..ACTIVATION_HEIGHT + 5 {
        assert!(feature_active(ConsensusFeature::DomainSeparatedPreimages, h), "height {h}");
    }
    // The signing version is untouched by a named feature.
    for h in 0..10 {
        assert!(!feature_active(ConsensusFeature::SigningV2, h));
        assert_eq!(protocol_version_at(h), SigningProtocolVersion::V1);
    }

    // Applier, restarted applier (restart at the activation boundary), and
    // the committed expectation: identical at 4, 5, 6 and every other height.
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
