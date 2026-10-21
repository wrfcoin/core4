//! #3277 S5, T5: pre-`H_ENABLE` (and zero-extension) state-root parity.
//!
//! B3 installs its base feature schedule from block 1 WITH the per-chain
//! bootstrap (`schedule_extension` at `H_ENABLE`). Until an extension is
//! actually applied, that must not move one byte of any state root: the
//! extension section is folded into `state_root_material` only once the list
//! is non-empty (S2). So the committed fixture — every root recorded by the
//! pre-S2 applier, identical since the deployed `9ef92a235` — must replay
//! byte-identically on B3, at every height before AND after the gate opens.
//!
//! The committed bootstrap row's height is the placeholder until merge, so
//! this test merges a row for the live chain's block-1 hash at `GATE = 3`,
//! inside the fixture, through the same `with_bootstrap_rows` production
//! uses. RED control (in the PR): folding the section unconditionally.
//!
//! Own process: the schedule is installed process-wide.

use test_server::determinism::{
    assert_root_parity, fixture_dir, replay_fixture, replay_fixture_with_restart,
};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, FeatureSchedule, LEGACY_CHAIN_BOOTSTRAP, LegacyChainBootstrap,
    SigningProtocolVersion, feature_active, install_activation_table, install_feature_schedule,
    resolve_activation_schedule, with_bootstrap_rows,
};

const GATE: u64 = 3;

#[test]
fn the_bootstrap_gate_moves_no_state_root_before_or_after_it_opens() {
    let live = LEGACY_CHAIN_BOOTSTRAP[0];
    let rows = [LegacyChainBootstrap { height: GATE, ..live }];
    let base = with_bootstrap_rows(FeatureSchedule::default(), live.genesis_hash, &rows)
        .expect("merge");
    let metadata: [(&str, &str); 0] = [];
    let table = resolve_activation_schedule(metadata, SigningProtocolVersion::V1).unwrap();
    SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
    install_activation_table(table);
    install_feature_schedule(base);

    assert!(!feature_active(ConsensusFeature::ScheduleExtension, GATE - 1));
    assert!(feature_active(ConsensusFeature::ScheduleExtension, GATE));

    let dir = fixture_dir();
    let straight = replay_fixture(&dir);
    assert!(
        straight.iter().any(|(h, _)| *h > GATE),
        "the fixture must run past the gate"
    );
    let restarted = replay_fixture_with_restart(&dir, GATE as usize);
    assert_eq!(straight, restarted, "a restart across the gate moves no root");
    assert_root_parity(&straight, &dir);
}
