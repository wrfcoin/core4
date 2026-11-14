//! `station_registrar_key` on a chain with no pinned key: it cannot be
//! scheduled, a genesis naming it is refused, a binary without the key stops
//! cleanly at the activation, and if a block schedules it anyway it accepts
//! nothing.
//!
//! The feature is active from `K` here, as it would be if a governance-signed
//! extension reached a block despite every admission and signer refusal. No
//! registrar pin is installed in this process.
//!
//! Run from core4:
//!   cargo test -p wrfcoin-shared-protocol --test station_registrar_key_unpinned

use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::state::StateTransition;
use wrfcoin_shared_protocol::blockchain::transaction::{
    StationRegistrationPayload, Transaction, TransactionInput, TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::consensus::station_registrar::{
    MOBILE_STATION_SLOT, PHONE_HARDWARE_MODEL, mobile_station_id, registrar_keys_at,
    station_registrar_key_active,
};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, FeatureSchedule, GENESIS_ACTIVATION_SCHEDULE_KEY, GENESIS_CHAIN_ID_KEY,
    GENESIS_FAUCET_ADDRESS_KEY, GENESIS_NETWORK_ID_KEY, install_feature_schedule,
    validate_testnet_genesis,
};
use wrfcoin_shared_protocol::crypto::hash::{Hash, hash_string};
use wrfcoin_shared_protocol::crypto::protocol_support::upgrade_required_in_feature_schedule;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::governance::schedule_extension::{
    FeatureScheduleExtension, MIN_RUNUP_BLOCKS, STATION_REGISTRAR_KEY_PINS, ScheduleEntry,
    ScheduleExtensionError, check_admission_policy, install_schedule_extension_authority_for_tests,
    sign_feature_schedule_extension_with_key, station_registrar_keys,
};
use wrfcoin_shared_protocol::station_registry::{ContributionProvenanceClass, DataQualityTier};

const GOVERNANCE_ENV: &str = "WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS";
const K: u64 = 5;
const GOVERNANCE: u8 = 0x60;
const WOULD_BE_REGISTRAR: u8 = 0x51;
/// Chain 888's public block-1 hash, from `packaging/windows/chain-888.conf`.
const CHAIN_888: &str = "553a3758386d9631037d7469686acb4a121ced81dcf5cde256e2d05667c5f332"; // pragma: allowlist secret -- public genesis hash
/// A chain instance with no registrar pin (a made-up block-1 hash).
const OTHER_CHAIN: &str = "1111111111111111111111111111111111111111111111111111111111111111"; // pragma: allowlist secret -- not a real chain

fn install() {
    std::env::set_var(GOVERNANCE_ENV, pk_hex(GOVERNANCE));
    install_feature_schedule(
        FeatureSchedule::from_entries(vec![
            ("station_registration_nonce".into(), 1),
            ("station_registrar_key".into(), K),
        ])
        .unwrap(),
    );
    install_schedule_extension_authority_for_tests(
        "777",
        "testnet",
        &genesis_hex(),
        vec![pk_hex(GOVERNANCE)],
    );
}

fn key(seed: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[seed; 32]).expect("32-byte ed25519 seed")
}
fn pk_hex(seed: u8) -> String {
    PublicKey::from_private_key(&key(seed)).to_hex()
}
fn addr(k: &PrivateKey) -> String {
    PublicKey::from_private_key(k).to_address()
}
fn genesis_hex() -> String {
    hex::encode([9u8; 32])
}

fn tx_of(tag: &str, payload: &StationRegistrationPayload) -> Transaction {
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(&format!("registrar-input-{tag}")),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            amount: 0,
            address: payload.owner_address.clone(),
            script: String::new(),
            nft_data: None,
        }],
    );
    tx.tx_type = TransactionType::StationRegistration;
    tx.fee = 0;
    tx.data = Some(serde_json::to_string(payload).unwrap());
    tx.set_id();
    tx
}

/// An unsigned phone payload for `owner`, inside every registrar limit.
fn phone(owner: &PrivateKey) -> StationRegistrationPayload {
    let owner_address = addr(owner);
    let station_id = mobile_station_id(&owner_address, MOBILE_STATION_SLOT).unwrap();
    StationRegistrationPayload::new(
        station_id,
        owner_address,
        46.8,
        -92.1,
        PHONE_HARDWARE_MODEL,
        DataQualityTier::Unverified,
        ContributionProvenanceClass::Real,
    )
    .with_expected_nonce(0)
}

/// Owner proof, then the authorizing signature (governance or registrar).
fn signed(mut p: StationRegistrationPayload, owner: &PrivateKey, signer: u8) -> Transaction {
    p.sign_ownership(owner);
    p.sign(&key(signer));
    let tag = format!(
        "{}-{signer}-{}",
        p.station_id,
        serde_json::to_string(&p).unwrap().len()
    );
    tx_of(&tag, &p)
}

#[test]
fn this_binary_pins_registrar_keys_on_chain_888_only() {
    assert_eq!(STATION_REGISTRAR_KEY_PINS.len(), 1);
    assert_eq!(station_registrar_keys(CHAIN_888).len(), 2);
    assert!(station_registrar_keys(OTHER_CHAIN).is_empty());
    assert!(station_registrar_keys(&genesis_hex()).is_empty());
}

#[test]
fn active_with_no_pin_only_governance_registers() {
    install();
    assert!(station_registrar_key_active(K));
    assert!(registrar_keys_at(K).is_empty());
    let owner = key(0xF1);
    let would_be = signed(phone(&owner), &owner, WOULD_BE_REGISTRAR);
    for h in [K, K + 1, u64::MAX] {
        assert!(
            would_be.station_registration_step(h).is_none(),
            "height {h}"
        );
    }
    assert!(matches!(
        signed(phone(&owner), &owner, GOVERNANCE).station_registration_step(K),
        Some(StateTransition::ApplyStationRegistrationV2 { .. })
    ));
}

#[test]
fn a_binary_without_the_key_stops_cleanly_at_the_activation() {
    install();
    let schedule =
        FeatureSchedule::from_entries(vec![("station_registrar_key".into(), K)]).unwrap();
    let required = upgrade_required_in_feature_schedule(&schedule).unwrap();
    assert_eq!(required.feature, "station_registrar_key");
    assert_eq!(required.height, K);
    let other = FeatureSchedule::from_entries(vec![("passkey_bond_payout".into(), K)]).unwrap();
    assert_eq!(upgrade_required_in_feature_schedule(&other), None);
}

#[test]
fn it_is_schedulable_on_chain_888_only_once_pinned() {
    let f = ConsensusFeature::StationRegistrarKey;
    assert!(f.schedulable(Some(CHAIN_888)));
    assert!(!f.schedulable(Some(OTHER_CHAIN)));
    assert!(!f.schedulable(None));
}

#[test]
fn it_cannot_be_scheduled_on_an_unpinned_chain() {
    let f = ConsensusFeature::StationRegistrarKey;
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
    let err = validate_testnet_genesis(&genesis("0:v1,5:station_registrar_key")).unwrap_err();
    assert!(err.contains("station_registrar_key"), "{err}");
}
