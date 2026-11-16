//! `forecast_late_score` (skill pay, 6 h and 12 h leads) is schedulable but
//! ships unscheduled. A schedule extension naming it is admitted, signed by
//! the library signer and accepted in a testnet genesis only at least
//! `FORECAST_LATE_SCORE_KEY_BINDING_LEAD_BLOCKS` after
//! `forecaster_key_binding`; it never asks this binary for an upgrade.
//!
//! Own test binary: nothing here installs a schedule, so the node's effective
//! schedule names neither feature.

use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::consensus::forecast_late_score::{LEAD_HOURS, TRACK_RECORD_MIN_SUM};
use wrfcoin_shared_protocol::crypto::chain_binding::*;
use wrfcoin_shared_protocol::crypto::hash::Hash;
use wrfcoin_shared_protocol::crypto::protocol_support::{
    KNOWN_FEATURES, upgrade_required_in_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::signature::PrivateKey;
use wrfcoin_shared_protocol::governance::schedule_extension::{
    FeatureScheduleExtension, ScheduleEntry, ScheduleExtensionError, check_admission_policy,
    sign_feature_schedule_extension_with_key,
};

const LEAD: u64 = FORECAST_LATE_SCORE_KEY_BINDING_LEAD_BLOCKS;

fn extension(entries: &[(&str, u64)]) -> FeatureScheduleExtension {
    let mut entries: Vec<ScheduleEntry> =
        entries.iter().map(|(f, h)| ScheduleEntry::new(*f, *h)).collect();
    entries.sort_by(|a, b| (a.activation_height, &a.feature).cmp(&(b.activation_height, &b.feature)));
    FeatureScheduleExtension {
        chain_id: "888".into(),
        network_id: "testnet".into(),
        genesis_hash: "ab".repeat(32),
        sequence: 0,
        entries,
        version_steps: Vec::new(),
        min_protocol_version: 1,
        release_commit: "cd".repeat(20),
    }
}

#[test]
fn schedulable_on_every_chain_but_unscheduled() {
    assert_eq!(LEAD_HOURS, [6, 12], "only the 6 h and 12 h leads are paid");
    assert_eq!(TRACK_RECORD_MIN_SUM, 0, "the margin is set later");
    let f = ConsensusFeature::ForecastLateScore;
    for node in [None, Some("ab".repeat(32)), Some("cd".repeat(32))] {
        assert!(f.schedulable(node.as_deref()), "{node:?}");
    }
    assert!(f.has_behaviour());
    assert!(KNOWN_FEATURES.contains(&f.name()));
    assert!(!FeatureSchedule::default().active(f, u64::MAX), "unscheduled by default");
    for height in [0, 1, LEAD, LEAD + 1, u64::MAX] {
        assert!(!forecast_late_score_active(height));
    }
    // Late-data pay stays held.
    assert!(!ConsensusFeature::LateDataPay.schedulable(None));
}

#[test]
fn admission_takes_it_only_after_the_key_binding_lead() {
    let node = Some("ab".repeat(32));
    // With the key binding in the same extension, LEAD blocks earlier: admitted.
    check_admission_policy(
        &extension(&[("forecaster_key_binding", 30_000), ("forecast_late_score", 30_000 + LEAD)]),
        node.as_deref(),
    )
    .expect("admitted at the lead");
    // One block short: refused for the lead, never as "not schedulable".
    match check_admission_policy(
        &extension(&[("forecaster_key_binding", 30_001), ("forecast_late_score", 30_000 + LEAD)]),
        node.as_deref(),
    ) {
        Err(ScheduleExtensionError::ForecastLateScoreTooEarly { activation_height, key_binding_activation }) => {
            assert_eq!((activation_height, key_binding_activation), (30_000 + LEAD, Some(30_001)));
        },
        other => panic!("expected ForecastLateScoreTooEarly, got {other:?}"),
    }
    // Alone, the key binding never scheduled on this node: refused.
    assert!(matches!(
        check_admission_policy(&extension(&[("forecast_late_score", 40_000)]), node.as_deref()),
        Err(ScheduleExtensionError::ForecastLateScoreTooEarly { key_binding_activation: None, .. })
    ));
}

#[test]
fn the_library_signer_signs_it() {
    let key = PrivateKey::from_bytes(&[0x5a; 32]).unwrap();
    let ext = extension(&[("forecaster_key_binding", 30_000), ("forecast_late_score", 30_000 + LEAD)]);
    sign_feature_schedule_extension_with_key(&ext, &key).expect("not held");
}

#[test]
fn a_node_scheduling_it_needs_no_upgrade() {
    let schedule = FeatureSchedule::from_entries(vec![
        ("forecaster_key_binding".to_string(), 2),
        ("forecast_late_score".to_string(), 2 + LEAD),
    ])
    .expect("resolves");
    assert_eq!(upgrade_required_in_feature_schedule(&schedule), None);
    assert!(schedule.active(ConsensusFeature::ForecastLateScore, 2 + LEAD));
}

const SCHEDULE: &str = "0:v1,2:schedule_extension,3:reward_gross_outlier_guard,4:weather_skew_block_time_allowance,5:station_active_reward_gate,6:station_owner_actions,7:station_owner_consent,8:station_registration_nonce,9:node_share_settlement";

fn genesis(extra: &str) -> Block {
    let mut b = Block::new(3, Hash::default(), vec![], 1, 1);
    b.timestamp = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
    let schedule = format!("{SCHEDULE}{extra}");
    for (k, v) in [
        (GENESIS_CHAIN_ID_KEY, "acr888"), (GENESIS_NETWORK_ID_KEY, "testnet"),
        (GENESIS_PROTOCOL_VERSION_KEY, "v1"), (GENESIS_ACTIVATION_SCHEDULE_KEY, schedule.as_str()),
        (GENESIS_FAUCET_ADDRESS_KEY, "0x2323232323232323232323232323232323232323"),
        (GENESIS_FLAT_REWARD_KEY, "600"), (GENESIS_FLAT_REWARD_UNITS_KEY, "2222222222"), (GENESIS_FAUCET_ALLOCATION_KEY, "1000000"),
        (GENESIS_GOVERNANCE_KEYS_KEY, "e7bec166ad446314d26fb7526bfa8cec1b363b438440262fb001f7b04b20bf00"), // pragma: allowlist secret -- existing public governance key
        (GENESIS_NODE_EPOCH_KEY, "512"), (GENESIS_NODE_CHALLENGE_KEY, "512"),
    ] { b.metadata.insert(k.into(), v.into()); }
    b
}

#[test]
fn a_testnet_genesis_takes_it_only_after_the_key_binding_lead() {
    validate_testnet_genesis(&genesis("")).expect("the base genesis is valid");
    // At the lead (the activation-crossing run's schedule): valid.
    validate_testnet_genesis(&genesis(&format!(",10:forecaster_key_binding,{}:forecast_late_score", 10 + LEAD)))
        .expect("at the lead");
    // One block short, or with no key binding: refused, naming the rule.
    for extra in [
        format!(",10:forecaster_key_binding,{}:forecast_late_score", 9 + LEAD),
        format!(",{}:forecast_late_score", 10 + LEAD),
    ] {
        let refused = validate_testnet_genesis(&genesis(&extra)).unwrap_err();
        assert!(refused.contains("forecast_late_score must activate"), "{extra}: {refused}");
    }
}
