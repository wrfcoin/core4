//! Testnet genesis validation checks EVERY retirement it lists, not only the
//! first: a genesis naming `bond_admission_key_retire_0` (schedulable, the
//! row has slot 0) and `bond_admission_key_retire_1` (no slot 1 in a one-key
//! row) must be refused, and the refusal names slot 1.
//!
//! Own test binary: it pins ONE admission key for the hash of the genesis it
//! builds, and the pin is process-wide (first install wins).

use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::crypto::chain_binding::*;
use wrfcoin_shared_protocol::crypto::hash::Hash;
use wrfcoin_shared_protocol::governance::schedule_extension::install_bond_admission_pin_for_tests;

const SCHEDULE: &str = "0:v1,2:schedule_extension,3:reward_gross_outlier_guard,4:weather_skew_block_time_allowance,5:station_active_reward_gate,6:station_owner_actions,7:station_owner_consent,8:station_registration_nonce,9:node_share_settlement,10:bond_admission_key_retire_0,11:bond_admission_key_retire_1";

fn genesis() -> Block {
    let mut b = Block::new(3, Hash::default(), vec![], 1, 1);
    b.timestamp = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
    for (k, v) in [
        (GENESIS_CHAIN_ID_KEY, "888"), (GENESIS_NETWORK_ID_KEY, "testnet"),
        (GENESIS_PROTOCOL_VERSION_KEY, "v1"), (GENESIS_ACTIVATION_SCHEDULE_KEY, SCHEDULE),
        (GENESIS_FAUCET_ADDRESS_KEY, "0x2323232323232323232323232323232323232323"),
        (GENESIS_FLAT_REWARD_KEY, "600"), (GENESIS_FLAT_REWARD_UNITS_KEY, "2222222222"), (GENESIS_FAUCET_ALLOCATION_KEY, "1000000"),
        (GENESIS_GOVERNANCE_KEYS_KEY, "e7bec166ad446314d26fb7526bfa8cec1b363b438440262fb001f7b04b20bf00"), // pragma: allowlist secret -- existing public governance key
        (GENESIS_NODE_EPOCH_KEY, "512"), (GENESIS_NODE_CHALLENGE_KEY, "512"),
    ] { b.metadata.insert(k.into(), v.into()); }
    b
}

#[test]
fn a_schedulable_retire_0_does_not_hide_an_unschedulable_retire_1() {
    let b = genesis();
    let hash = b.calculate_hash().to_hex();
    install_bond_admission_pin_for_tests(&hash, vec!["11".repeat(32)]);
    assert!(ConsensusFeature::BondAdmissionKeyRetire0.schedulable(Some(&hash)));
    assert!(!ConsensusFeature::BondAdmissionKeyRetire1.schedulable(Some(&hash)));
    let refused = validate_testnet_genesis(&b).unwrap_err();
    assert!(refused.contains("bond_admission_key_retire_1"), "{refused}");

    // The same genesis without the retirements is valid, so the refusal
    // above is the retire_1 slot check and nothing else.
    let mut plain = b.clone();
    plain.metadata.insert(
        GENESIS_ACTIVATION_SCHEDULE_KEY.into(),
        SCHEDULE.replace(",10:bond_admission_key_retire_0,11:bond_admission_key_retire_1", ""),
    );
    validate_testnet_genesis(&plain).unwrap();
}
