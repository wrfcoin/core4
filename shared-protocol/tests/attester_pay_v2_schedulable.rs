//! `attester_pay_v2` (ordinal 42) is registered, schedulable on every chain
//! and ships unscheduled.
//!
//! No earlier binary knows the name, so one that has not upgraded halts
//! cleanly at the activation height instead of applying the older attester
//! pay rule to blocks the upgraded nodes settle differently.
//!
//! Own test binary: nothing here installs a schedule, so the node's effective
//! schedule names no feature.

use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::crypto::chain_binding::*;
use wrfcoin_shared_protocol::crypto::hash::Hash;
use wrfcoin_shared_protocol::crypto::protocol_support::{
    KNOWN_FEATURES, upgrade_required_in_feature_schedule, upgrade_required_in_feature_schedule_with,
    upgrade_required_in_schedule, upgrade_required_in_schedule_with,
};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::governance::schedule_extension::{
    FeatureScheduleExtension, ScheduleEntry, SignedFeatureScheduleExtension, check_admission_policy,
    sign_feature_schedule_extension_with_key, verify_schedule_extension_signatures,
};
use wrfcoin_shared_protocol::oracle::attestation::reject_threshold;

const NAME: &str = "attester_pay_v2";

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
fn the_feature_is_registered_at_ordinal_42_schedulable_everywhere_and_unscheduled() {
    let feature = ConsensusFeature::AttesterPayV2;
    assert_eq!(feature.name(), NAME);
    assert_eq!(ConsensusFeature::parse(NAME), Some(feature));
    assert_eq!(bincode::serialize(&feature).unwrap(), 42u32.to_le_bytes(), "appended at ordinal 42");
    assert_eq!(ConsensusFeature::ALL.get(42), Some(&feature));
    assert_eq!(*ConsensusFeature::ALL.last().unwrap(), feature);
    assert_eq!(ConsensusFeature::ALL.len(), 43);
    // Every earlier ordinal is where it was.
    for (ordinal, earlier) in ConsensusFeature::ALL.iter().enumerate().take(42) {
        assert_eq!(bincode::serialize(earlier).unwrap(), (ordinal as u32).to_le_bytes(), "{earlier}");
    }
    for node in [None, Some("ab".repeat(32)), Some("cd".repeat(32))] {
        assert!(feature.schedulable(node.as_deref()), "{node:?}");
    }
    assert!(feature.has_behaviour());
    assert_eq!(KNOWN_FEATURES.last(), Some(&NAME));
    assert_eq!(KNOWN_FEATURES.len(), ConsensusFeature::ALL.len());
    assert_eq!(KNOWN_FEATURES.iter().filter(|name| **name == NAME).count(), 1);
    // Unscheduled: in no default schedule, and inactive at every height here.
    assert!(!FeatureSchedule::default().active(feature, u64::MAX), "unscheduled by default");
    assert_eq!(FeatureSchedule::default().activation_height(NAME), None);
    for height in [0, 1, 27_008, 1_000_000, u64::MAX] {
        assert!(!feature_active(feature, height), "inactive at {height}");
    }
}

#[test]
fn a_binary_without_the_name_halts_at_its_activation_and_this_one_does_not() {
    let at = 40_000;
    let schedule = FeatureSchedule::from_entries(vec![(NAME.to_string(), at)]).expect("resolves");
    assert!(schedule.active(ConsensusFeature::AttesterPayV2, at));
    assert!(!schedule.active(ConsensusFeature::AttesterPayV2, at - 1));
    // This binary implements it: no upgrade required.
    assert_eq!(upgrade_required_in_feature_schedule(&schedule), None);
    assert_eq!(upgrade_required_in_schedule(&format!("0:v1,{at}:{NAME}")), None);
    // Every earlier binary's known list is this one without the name: it
    // stops at the activation height.
    let earlier: Vec<&str> = KNOWN_FEATURES.iter().copied().filter(|name| *name != NAME).collect();
    assert_eq!(earlier.len(), 42);
    assert_eq!(earlier.last(), Some(&"forecast_late_score_v2"));
    let halt = upgrade_required_in_feature_schedule_with(&schedule, &earlier).expect("halts");
    assert_eq!((halt.feature.as_str(), halt.height), (NAME, at));
    let halt = upgrade_required_in_schedule_with(&format!("0:v1,{at}:{NAME}"), &earlier).expect("halts");
    assert_eq!((halt.feature.as_str(), halt.height), (NAME, at));
    // The halt is at the activation, not before: an earlier binary follows
    // the chain up to the block below it.
    use wrfcoin_shared_protocol::crypto::protocol_support::boot_refused;
    assert!(!boot_refused(&halt, at - 1));
    assert!(boot_refused(&halt, at));
}

#[test]
fn admission_and_the_library_signer_take_a_schedule_naming_it() {
    for node in [None, Some("ab".repeat(32))] {
        check_admission_policy(&extension(&[(NAME, 40_000)]), node.as_deref()).expect("admitted alone");
    }
    let key = PrivateKey::from_bytes(&[0x5a; 32]).unwrap();
    let signer = PublicKey::from_private_key(&key).to_hex().to_ascii_lowercase();
    let ext = extension(&[(NAME, 40_000)]);
    let signature = sign_feature_schedule_extension_with_key(&ext, &key).expect("the name is not held");
    let signed = SignedFeatureScheduleExtension { extension: ext, signatures: vec![signature.clone()] };
    verify_schedule_extension_signatures(&signed, std::slice::from_ref(&signer)).expect("signature verifies");
    // The signature covers the entries: moving the activation breaks it.
    let moved = SignedFeatureScheduleExtension {
        extension: extension(&[(NAME, 40_001)]),
        signatures: vec![signature],
    };
    assert!(verify_schedule_extension_signatures(&moved, &[signer]).is_err());
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
fn a_genesis_may_name_it_but_no_shipped_schedule_does() {
    validate_testnet_genesis(&genesis("")).expect("the base genesis is valid");
    validate_testnet_genesis(&genesis(&format!(",10:{NAME}"))).expect("a genesis naming it is valid");
    assert!(!SCHEDULE.contains(NAME));
}

#[test]
fn the_reject_threshold_is_the_documented_rule() {
    for (n, quorum, expected) in [(4usize, 2usize, 3usize), (3, 2, 2), (5, 3, 3), (7, 3, 5), (4, 1, 4), (1, 1, 1)] {
        assert_eq!(reject_threshold(n, quorum), expected, "n={n} quorum={quorum}");
        assert_eq!(reject_threshold(n, quorum), quorum.max(n - quorum + 1));
    }
}
