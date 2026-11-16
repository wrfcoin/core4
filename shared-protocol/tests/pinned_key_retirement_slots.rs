//! Retirement slots follow the pin rows: on a chain whose row has ONE key,
//! `…_retire_0` is schedulable and `…_retire_1` is refused (and a binary
//! that sees `…_retire_1` scheduled there halts cleanly at it); on chain 888,
//! whose compiled rows hold two keys per role, both slots are schedulable.
//!
//! Own test binary: it installs a one-key pin per role for a synthetic
//! genesis and never schedules anything.

use wrfcoin_shared_protocol::crypto::chain_binding::{
    BOND_ADMISSION_KEY_RETIREMENTS, ConsensusFeature, FeatureSchedule,
    STATION_REGISTRAR_KEY_RETIREMENTS,
};
use wrfcoin_shared_protocol::crypto::protocol_support::upgrade_required_in_feature_schedule;
use wrfcoin_shared_protocol::governance::schedule_extension::{
    BOND_ADMISSION_KEY_PINS, FeatureScheduleExtension, MIN_RUNUP_BLOCKS, STATION_REGISTRAR_KEY_PINS,
    ScheduleEntry, ScheduleExtensionError, check_admission_policy,
    install_bond_admission_pin_for_tests, install_schedule_extension_authority_for_tests,
    install_station_registrar_pin_for_tests,
};

const CHAIN_888: &str = "553a3758386d9631037d7469686acb4a121ced81dcf5cde256e2d05667c5f332"; // pragma: allowlist secret -- public genesis hash

fn one_slot_genesis() -> String {
    hex::encode([5u8; 32])
}

fn install() {
    install_schedule_extension_authority_for_tests(
        "777",
        "testnet",
        &one_slot_genesis(),
        vec!["ab".repeat(32)],
    );
    install_bond_admission_pin_for_tests(&one_slot_genesis(), vec!["11".repeat(32)]);
    install_station_registrar_pin_for_tests(&one_slot_genesis(), vec!["22".repeat(32)]);
}

fn extension(genesis: &str, feature: &str) -> FeatureScheduleExtension {
    FeatureScheduleExtension {
        chain_id: "777".into(),
        network_id: "testnet".into(),
        genesis_hash: genesis.into(),
        sequence: 0,
        entries: vec![ScheduleEntry::new(feature, 21_200 + MIN_RUNUP_BLOCKS)],
        version_steps: Vec::new(),
        min_protocol_version: 3,
        release_commit: "ab1b1a4930000000000000000000000000000000".into(),
    }
}

#[test]
fn a_one_key_row_has_slot_0_but_no_slot_1() {
    install();
    let g = one_slot_genesis();
    for f in [ConsensusFeature::BondAdmissionKeyRetire0, ConsensusFeature::StationRegistrarKeyRetire0] {
        assert!(f.schedulable(Some(&g)), "{}", f.name());
        assert_eq!(check_admission_policy(&extension(&g, f.name()), Some(&g)), Ok(()));
        let schedule = FeatureSchedule::from_entries(vec![(f.name().into(), 100)]).unwrap();
        assert_eq!(upgrade_required_in_feature_schedule(&schedule), None);
    }
    for f in [ConsensusFeature::BondAdmissionKeyRetire1, ConsensusFeature::StationRegistrarKeyRetire1] {
        assert!(!f.schedulable(Some(&g)), "{}", f.name());
        assert_eq!(
            check_admission_policy(&extension(&g, f.name()), Some(&g)),
            Err(ScheduleExtensionError::FeatureNotSchedulable { feature: f.name().into() })
        );
        // Scheduled anyway (a block may carry what admission refuses): this
        // binary has no such slot, so it halts cleanly at the height.
        let schedule = FeatureSchedule::from_entries(vec![(f.name().into(), 100)]).unwrap();
        let halt = upgrade_required_in_feature_schedule(&schedule).expect("halts");
        assert_eq!((halt.feature.as_str(), halt.height), (f.name(), 100));
    }
}

#[test]
fn chain_888_has_both_slots_for_both_roles() {
    install();
    for f in BOND_ADMISSION_KEY_RETIREMENTS.iter().chain(STATION_REGISTRAR_KEY_RETIREMENTS) {
        assert!(f.schedulable(Some(CHAIN_888)), "{}", f.name());
        assert_eq!(check_admission_policy(&extension(CHAIN_888, f.name()), Some(CHAIN_888)), Ok(()));
    }
}

/// A pin row may never hold more keys than its role has retirement
/// features: appending a third key must first add `…_retire_2`, or that key
/// could never be retired.
#[test]
fn every_pinned_slot_has_a_retirement_feature() {
    for pin in BOND_ADMISSION_KEY_PINS {
        assert!(pin.keys.len() <= BOND_ADMISSION_KEY_RETIREMENTS.len(), "{}", pin.genesis_hash);
    }
    for pin in STATION_REGISTRAR_KEY_PINS {
        assert!(pin.keys.len() <= STATION_REGISTRAR_KEY_RETIREMENTS.len(), "{}", pin.genesis_hash);
    }
}
