//! `bond_admission_key` on a chain with no pinned key: it cannot be scheduled,
//! and if a block schedules it anyway it accepts nothing. This binary pins keys
//! for chain 888 only; the chain used here (777, block-1 hash `[7; 32]`) has none.
//!
//! The feature is active from `K` here, as it would be if a governance-signed
//! extension reached a block despite every admission and signer refusal: a block
//! carrying such an extension stays valid on every node (admission is policy,
//! never validity), so the rule itself has to be inert. No pin is installed in
//! this process. The chain with pinned keys is `bond_admission_key.rs`.

#[path = "common/node_bond_fixture.rs"]
#[allow(dead_code)]
mod fixture;

use fixture::{A, address, advance, apply, key, pk};
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::common::types::Balance;
use wrfcoin_shared_protocol::consensus::node_bond_registry::{
    NODE_BOND_UNITS, NodeBondLifecycle, bond_admission_key_active,
};
use wrfcoin_shared_protocol::consensus::node_share_contracts::NodeBondAction;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ChainIdentity, ConsensusFeature, FeatureSchedule, GENESIS_ACTIVATION_SCHEDULE_KEY,
    GENESIS_CHAIN_ID_KEY, GENESIS_FAUCET_ADDRESS_KEY, GENESIS_NETWORK_ID_KEY,
    install_feature_schedule, validate_testnet_genesis,
};
use wrfcoin_shared_protocol::crypto::hash::Hash;
use wrfcoin_shared_protocol::crypto::protocol_support::upgrade_required_in_feature_schedule;
use wrfcoin_shared_protocol::governance::schedule_extension::{
    AppliedExtension, BOND_ADMISSION_KEY_PINS, FeatureScheduleExtension, MIN_RUNUP_BLOCKS,
    ScheduleEntry, ScheduleExtensionError, bond_admission_keys, check_admission_policy,
    install_schedule_extension_authority_for_tests, sign_feature_schedule_extension_with_key,
};

const K: u64 = A + 1;
const GOVERNANCE: u8 = 99;
const WOULD_BE_ADMISSION: u8 = 60;
/// Chain 888's public block-1 hash, from `packaging/windows/chain-888.conf`.
const CHAIN_888: &str = "553a3758386d9631037d7469686acb4a121ced81dcf5cde256e2d05667c5f332"; // pragma: allowlist secret -- public genesis hash
/// A chain instance with no admission pin (a made-up block-1 hash).
const OTHER_CHAIN: &str = "1111111111111111111111111111111111111111111111111111111111111111"; // pragma: allowlist secret -- not a real chain

fn state() -> State {
    install_feature_schedule(
        FeatureSchedule::from_entries(vec![
            ("node_share_settlement".into(), A),
            ("account_authorization_dispatch".into(), A),
            ("bond_admission_key".into(), K),
        ])
        .unwrap(),
    );
    install_schedule_extension_authority_for_tests(
        "777",
        "testnet",
        &hex::encode([7u8; 32]),
        vec![hex::encode(pk(GOVERNANCE))],
    );
    let mut s = State::new();
    s.genesis_emission_config = Some(
        wrfcoin_shared_protocol::crypto::chain_binding::GenesisEmissionConfig {
            chain_id: "777".into(),
            faucet_address: None,
            ..Default::default()
        },
    );
    s.feature_schedule_extensions.push(AppliedExtension {
        sequence: 0,
        carried_at_height: 0,
        entries: vec![ScheduleEntry::new("node_share_settlement", A)],
        version_steps: vec![],
        min_protocol_version: 2,
        release_commit: "test-only-explicit-schedule".into(),
        signers: vec![hex::encode(pk(GOVERNANCE))],
    });
    s.create_account(address(10), Balance::new(u128::from(NODE_BOND_UNITS) * 3));
    s.apply_transition(StateTransition::BeginNodeServiceBlock {
        block_height: A,
        parent_hash: [2; 32],
    })
    .unwrap();
    s.apply_block_finalization(A);
    s
}

fn would_be_admitted() -> NodeBondLifecycle {
    let mut p = fixture::auth(10, 1, 0, NodeBondAction::Deposit, 1);
    p.governance_key = Some(pk(WOULD_BE_ADMISSION));
    p.governance_signature = fixture::signature(&p.signed.authorization, WOULD_BE_ADMISSION);
    p
}

#[test]
fn this_binary_pins_admission_keys_on_chain_888_only() {
    assert_eq!(BOND_ADMISSION_KEY_PINS.len(), 1);
    assert_eq!(bond_admission_keys(CHAIN_888).len(), 2);
    assert!(bond_admission_keys(OTHER_CHAIN).is_empty());
    assert!(bond_admission_keys(&hex::encode([7u8; 32])).is_empty());
}

#[test]
fn active_with_no_pin_every_co_sign_but_governance_is_refused() {
    let mut s = state();
    assert!(bond_admission_key_active(K));
    advance(&mut s, K);
    let identity = ChainIdentity::new("777", "testnet");
    let governance = [hex::encode(pk(GOVERNANCE))];

    let p = would_be_admitted();
    assert!(p.verify(&identity, &[7; 32], &governance, K).is_err());
    assert!(apply(&mut s, p, K).is_err());

    let plain = fixture::auth(10, 1, 0, NodeBondAction::Deposit, 1);
    plain.verify(&identity, &[7; 32], &governance, K).unwrap();
    apply(&mut s, plain, K).unwrap();
}

#[test]
fn a_binary_without_the_key_stops_cleanly_at_the_activation() {
    let _ = state();
    let schedule = FeatureSchedule::from_entries(vec![("bond_admission_key".into(), K)]).unwrap();
    let required = upgrade_required_in_feature_schedule(&schedule).unwrap();
    assert_eq!(required.feature, "bond_admission_key");
    assert_eq!(required.height, K);
    // Every other known feature is still followed.
    let other = FeatureSchedule::from_entries(vec![("passkey_bond_payout".into(), K)]).unwrap();
    assert_eq!(upgrade_required_in_feature_schedule(&other), None);
}

#[test]
fn it_is_schedulable_on_chain_888_only_once_pinned() {
    let f = ConsensusFeature::BondAdmissionKey;
    assert!(f.schedulable(Some(CHAIN_888)));
    assert!(!f.schedulable(Some(OTHER_CHAIN)));
    assert!(!f.schedulable(None));
}

#[test]
fn it_cannot_be_scheduled_on_an_unpinned_chain() {
    let f = ConsensusFeature::BondAdmissionKey;
    let extension = FeatureScheduleExtension {
        chain_id: "999".into(),
        network_id: "testnet".into(),
        genesis_hash: OTHER_CHAIN.into(),
        sequence: 0,
        entries: vec![ScheduleEntry::new(f.name(), 21_200 + MIN_RUNUP_BLOCKS)],
        version_steps: Vec::new(),
        min_protocol_version: 3,
        release_commit: "ab1b1a4930000000000000000000000000000000".into(),
    };
    for node in [None, Some(OTHER_CHAIN)] {
        assert!(!f.schedulable(node), "{node:?}");
        assert_eq!(
            check_admission_policy(&extension, node),
            Err(ScheduleExtensionError::FeatureNotSchedulable {
                feature: f.name().into()
            }),
            "{node:?}"
        );
    }
    assert!(matches!(
        sign_feature_schedule_extension_with_key(&extension, &key(GOVERNANCE)),
        Err(ScheduleExtensionError::FeatureHeld { .. })
    ));
}

#[test]
fn a_genesis_naming_it_without_a_pin_is_refused() {
    let genesis = |schedule: &str| {
        let mut block = Block::new(3, Hash::default(), vec![], 1, 1);
        for (name, value) in [
            (GENESIS_NETWORK_ID_KEY, "testnet"),
            (GENESIS_CHAIN_ID_KEY, "999"),
            (GENESIS_ACTIVATION_SCHEDULE_KEY, schedule),
            (
                GENESIS_FAUCET_ADDRESS_KEY,
                "0x2f1a7c04d3b95e86ac41d0f6b8e7259c3a5d84b1",
            ),
        ] {
            block.metadata.insert(name.to_string(), value.to_string());
        }
        block
    };
    validate_testnet_genesis(&genesis("0:v1,5:passkey_bond_payout")).unwrap();
    let err = validate_testnet_genesis(&genesis("0:v1,5:bond_admission_key")).unwrap_err();
    assert!(err.contains("bond_admission_key"), "{err}");
}
