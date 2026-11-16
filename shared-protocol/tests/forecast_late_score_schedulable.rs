//! Skill pay (6 h and 12 h leads) is schedulable under the name
//! `forecast_late_score_v2` (ordinal 41) and ships unscheduled. The ordinal-34
//! name `forecast_late_score` is retired: never schedulable, read by nothing,
//! refused in a genesis.
//!
//! Why a new name: every binary released before it lists `forecast_late_score`
//! as known, each with its own lane rule, so it would run that rule at an
//! activation of the old name and diverge. It does not know
//! `forecast_late_score_v2`, so it halts cleanly at that activation instead.
//!
//! A schedule naming v2 is admitted (mempool, producer, the signing CLI) and
//! accepted in a testnet genesis only at least
//! `FORECAST_LATE_SCORE_KEY_BINDING_LEAD_BLOCKS` after
//! `forecaster_key_binding`.
//!
//! Own test binary: nothing here installs a schedule, so the node's effective
//! schedule names none of these features.

use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::consensus::forecast_late_score::{LEAD_HOURS, TRACK_RECORD_MIN_SUM};
use wrfcoin_shared_protocol::crypto::chain_binding::*;
use wrfcoin_shared_protocol::crypto::hash::Hash;
use wrfcoin_shared_protocol::crypto::protocol_support::{
    KNOWN_FEATURES, upgrade_required_in_feature_schedule, upgrade_required_in_feature_schedule_with,
};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::governance::schedule_extension::{
    FeatureScheduleExtension, ScheduleEntry, ScheduleExtensionError, SignedFeatureScheduleExtension,
    check_admission_policy, sign_feature_schedule_extension_with_key, verify_schedule_extension_signatures,
};

const LEAD: u64 = FORECAST_LATE_SCORE_KEY_BINDING_LEAD_BLOCKS;
const V2: &str = "forecast_late_score_v2";
const RETIRED: &str = "forecast_late_score";

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
fn v2_is_schedulable_on_every_chain_but_unscheduled() {
    assert_eq!(LEAD_HOURS, [6, 12], "only the 6 h and 12 h leads are paid");
    assert_eq!(TRACK_RECORD_MIN_SUM, 0, "the margin is set later");
    let v2 = ConsensusFeature::ForecastLateScoreV2;
    assert_eq!(v2.name(), V2);
    assert_eq!(ConsensusFeature::parse(V2), Some(v2));
    assert_eq!(bincode::serialize(&v2).unwrap(), 41u32.to_le_bytes(), "appended at ordinal 41");
    assert_eq!(*ConsensusFeature::ALL.last().unwrap(), v2);
    for node in [None, Some("ab".repeat(32)), Some("cd".repeat(32))] {
        assert!(v2.schedulable(node.as_deref()), "{node:?}");
    }
    assert!(v2.has_behaviour());
    assert!(KNOWN_FEATURES.contains(&V2));
    assert!(!FeatureSchedule::default().active(v2, u64::MAX), "unscheduled by default");
    for height in [0, 1, LEAD, LEAD + 1, u64::MAX] {
        assert!(!forecast_late_score_active(height));
    }
    // Late-data pay stays held.
    assert!(!ConsensusFeature::LateDataPay.schedulable(None));
}

#[test]
fn the_retired_name_is_refused_everywhere_and_reads_nothing() {
    let old = ConsensusFeature::ForecastLateScore;
    assert_eq!(old.name(), RETIRED);
    assert_eq!(bincode::serialize(&old).unwrap(), 34u32.to_le_bytes(), "ordinal 34 stays fixed");
    for node in [None, Some("ab".repeat(32))] {
        assert!(!old.schedulable(node.as_deref()), "{node:?}");
    }
    assert!(!old.has_behaviour());
    // Still known (so it is not an "upgrade required" name), never admitted:
    // mempool, producer and the signing CLI all run this check.
    assert!(KNOWN_FEATURES.contains(&RETIRED));
    for entries in [
        vec![(RETIRED, 40_000)],
        vec![("forecaster_key_binding", 30_000), (RETIRED, 30_000 + LEAD)],
    ] {
        match check_admission_policy(&extension(&entries), Some(&"ab".repeat(32))) {
            Err(ScheduleExtensionError::FeatureNotSchedulable { feature }) => assert_eq!(feature, RETIRED),
            other => panic!("expected FeatureNotSchedulable, got {other:?}"),
        }
    }
    // Even a schedule that names it (past admission) turns nothing on: the
    // lane reads only v2.
    let only_old = |feature: ConsensusFeature, _at: u64| {
        matches!(feature, ConsensusFeature::ForecastLateScore | ConsensusFeature::ForecasterKeyBinding)
    };
    for height in [LEAD, LEAD * 10, u64::MAX] {
        assert!(!forecast_late_score_active_with(only_old, height));
    }
    let both = |feature: ConsensusFeature, _at: u64| {
        matches!(feature, ConsensusFeature::ForecastLateScoreV2 | ConsensusFeature::ForecasterKeyBinding)
    };
    assert!(forecast_late_score_active_with(both, LEAD));
    // A genesis naming it does not boot.
    let schedule = format!("0:v1,2:forecaster_key_binding,{}:{RETIRED}", 2 + LEAD);
    let metadata = [(GENESIS_ACTIVATION_SCHEDULE_KEY, schedule.as_str())];
    let refused = FeatureSchedule::from_genesis_metadata(metadata.iter().copied()).unwrap_err();
    assert!(refused.to_string().contains(RETIRED), "{refused}");
    let schedule = format!("0:v1,2:forecaster_key_binding,{}:{V2}", 2 + LEAD);
    let metadata = [(GENESIS_ACTIVATION_SCHEDULE_KEY, schedule.as_str())];
    FeatureSchedule::from_genesis_metadata(metadata.iter().copied()).expect("v2 boots");
}

#[test]
fn admission_takes_v2_only_after_the_key_binding_lead() {
    let node = Some("ab".repeat(32));
    // With the key binding in the same extension, LEAD blocks earlier: admitted.
    check_admission_policy(
        &extension(&[("forecaster_key_binding", 30_000), (V2, 30_000 + LEAD)]),
        node.as_deref(),
    )
    .expect("admitted at the lead");
    // One block short: refused for the lead, never as "not schedulable".
    match check_admission_policy(
        &extension(&[("forecaster_key_binding", 30_001), (V2, 30_000 + LEAD)]),
        node.as_deref(),
    ) {
        Err(ScheduleExtensionError::ForecastLateScoreTooEarly { activation_height, key_binding_activation }) => {
            assert_eq!((activation_height, key_binding_activation), (30_000 + LEAD, Some(30_001)));
        },
        other => panic!("expected ForecastLateScoreTooEarly, got {other:?}"),
    }
    // Alone, the key binding never scheduled on this node: refused.
    assert!(matches!(
        check_admission_policy(&extension(&[(V2, 40_000)]), node.as_deref()),
        Err(ScheduleExtensionError::ForecastLateScoreTooEarly { key_binding_activation: None, .. })
    ));
}

#[test]
fn the_library_signer_signs_v2_and_the_signature_verifies() {
    let key = PrivateKey::from_bytes(&[0x5a; 32]).unwrap();
    let signer = PublicKey::from_private_key(&key).to_hex().to_ascii_lowercase();
    let ext = extension(&[("forecaster_key_binding", 30_000), (V2, 30_000 + LEAD)]);
    let signature = sign_feature_schedule_extension_with_key(&ext, &key).expect("v2 is not held");
    assert_eq!(signature.signer_pubkey, signer);
    let signed = SignedFeatureScheduleExtension { extension: ext.clone(), signatures: vec![signature.clone()] };
    verify_schedule_extension_signatures(&signed, std::slice::from_ref(&signer)).expect("signature verifies");
    // The signature covers the entries: moving v2 by one block breaks it.
    let moved = SignedFeatureScheduleExtension {
        extension: extension(&[("forecaster_key_binding", 30_000), (V2, 30_001 + LEAD)]),
        signatures: vec![signature],
    };
    assert!(verify_schedule_extension_signatures(&moved, &[signer]).is_err());
}

#[test]
fn a_binary_without_v2_halts_at_its_activation_and_this_one_does_not() {
    let at = 2 + LEAD;
    let schedule = FeatureSchedule::from_entries(vec![
        ("forecaster_key_binding".to_string(), 2),
        (V2.to_string(), at),
    ])
    .expect("resolves");
    // This binary implements v2: no upgrade required.
    assert_eq!(upgrade_required_in_feature_schedule(&schedule), None);
    assert!(schedule.active(ConsensusFeature::ForecastLateScoreV2, at));
    // Every earlier binary's known list has the retired name but not v2: it
    // stops at the activation height instead of running its own lane rule.
    let earlier: Vec<&str> = KNOWN_FEATURES.iter().copied().filter(|name| *name != V2).collect();
    assert!(earlier.contains(&RETIRED));
    let halt = upgrade_required_in_feature_schedule_with(&schedule, &earlier).expect("halts");
    assert_eq!((halt.feature.as_str(), halt.height), (V2, at));
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
fn a_testnet_genesis_takes_v2_only_after_the_key_binding_lead() {
    validate_testnet_genesis(&genesis("")).expect("the base genesis is valid");
    validate_testnet_genesis(&genesis(&format!(",10:forecaster_key_binding,{}:{V2}", 10 + LEAD)))
        .expect("at the lead");
    // One block short, or with no key binding: refused, naming the rule.
    for extra in [
        format!(",10:forecaster_key_binding,{}:{V2}", 9 + LEAD),
        format!(",{}:{V2}", 10 + LEAD),
    ] {
        let refused = validate_testnet_genesis(&genesis(&extra)).unwrap_err();
        assert!(refused.contains("forecast_late_score_v2 must activate"), "{extra}: {refused}");
    }
}
