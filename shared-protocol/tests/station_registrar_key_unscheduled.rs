//! `station_registrar_key` UNSCHEDULED, with registrar keys pinned for this
//! chain: behaviour and state roots are what they were without the feature.
//!
//! Only `station_registration_nonce` is scheduled (at 1). A registrar-signed
//! registration is unauthorized exactly as before, at every height; the
//! governance path is untouched; and a block that carries registrar-signed
//! registrations ends with the same state root as one without them.
//!
//! Run from core4:
//!   cargo test -p wrfcoin-shared-protocol --test station_registrar_key_unscheduled

use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{
    StationRegistrationAuthority, StationRegistrationPayload, Transaction, TransactionInput,
    TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::consensus::station_registrar::{
    MOBILE_STATION_SLOT, PHONE_HARDWARE_MODEL, RegistrarBlockSlot, mobile_station_id,
    registrar_keys_at, station_registrar_key_active,
};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, FeatureSchedule, feature_scheduled, install_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::governance::schedule_extension::{
    install_schedule_extension_authority_for_tests, install_station_registrar_pin_for_tests,
};
use wrfcoin_shared_protocol::station_registry::{ContributionProvenanceClass, DataQualityTier};

const GOVERNANCE_ENV: &str = "WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS";
const NONCE_AT: u64 = 1;
const GOVERNANCE: u8 = 0x60;
const REGISTRAR: u8 = 0x51;

fn install() {
    std::env::set_var(GOVERNANCE_ENV, pk_hex(GOVERNANCE));
    install_feature_schedule(
        FeatureSchedule::from_entries(vec![("station_registration_nonce".into(), NONCE_AT)])
            .unwrap(),
    );
    install_schedule_extension_authority_for_tests(
        "777",
        "testnet",
        &genesis_hex(),
        vec![pk_hex(GOVERNANCE)],
    );
    install_station_registrar_pin_for_tests(&genesis_hex(), vec![pk_hex(REGISTRAR)]);
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

/// Every extracted transition of `txs` at `height`, through the per-block slot
/// both extractors use, applied in order; then the root.
fn run_block(txs: &[Transaction], height: u64) -> (State, Vec<StateTransition>) {
    let mut state = State::new();
    let mut slot = RegistrarBlockSlot::new();
    let mut out = Vec::new();
    for tx in txs {
        if let Some(t) = slot.admit(tx.station_registration_step(height)) {
            state
                .apply_transition(t.clone())
                .expect("extracted steps never fail the block");
            out.push(t);
        }
    }
    state.recalculate_state_root();
    (state, out)
}

#[test]
fn unscheduled_registrar_key_changes_nothing() {
    install();
    assert!(!feature_scheduled(ConsensusFeature::StationRegistrarKey));
    let owner = key(0xE1);
    let registrar_tx = signed(phone(&owner), &owner, REGISTRAR);
    for h in [0, NONCE_AT, 5, 1_000_000, u64::MAX] {
        assert!(!station_registrar_key_active(h));
        assert!(
            registrar_keys_at(h).is_empty(),
            "no registrar key in force at {h}"
        );
        assert_eq!(
            registrar_tx.station_registration_authority_at_height(h),
            None
        );
        assert!(!registrar_tx.verify_station_registration_authorization_at_height(h));
        assert!(
            registrar_tx.station_registration_step(h).is_none(),
            "height {h}"
        );
    }

    // Governance registrations extract exactly as before: v2 from the nonce
    // activation, the same variant, the same station.
    let gov_owner = key(0xE2);
    let gov_tx = signed(phone(&gov_owner), &gov_owner, GOVERNANCE);
    assert_eq!(
        gov_tx.station_registration_authority_at_height(NONCE_AT),
        Some(StationRegistrationAuthority::Governance)
    );
    assert!(matches!(
        gov_tx.station_registration_step(NONCE_AT),
        Some(StateTransition::ApplyStationRegistrationV2 {
            expected_nonce: 0,
            ..
        })
    ));

    // A block carrying registrar-signed registrations ends with the same
    // state root as the block without them, and the same transitions.
    let other = key(0xE3);
    let gov_other = signed(phone(&other), &other, GOVERNANCE);
    let second_registrar = {
        let k = key(0xE4);
        signed(phone(&k), &k, REGISTRAR)
    };
    let with = [
        registrar_tx.clone(),
        gov_tx.clone(),
        second_registrar,
        gov_other.clone(),
    ];
    let without = [gov_tx, gov_other];
    let (with_state, with_transitions) = run_block(&with, NONCE_AT + 1);
    let (without_state, without_transitions) = run_block(&without, NONCE_AT + 1);
    assert_eq!(with_transitions.len(), 2);
    assert_eq!(
        serde_json::to_string(&with_transitions).unwrap(),
        serde_json::to_string(&without_transitions).unwrap()
    );
    assert_eq!(with_state.state_root, without_state.state_root);
    assert_eq!(
        bincode::serialize(&with_state).unwrap(),
        bincode::serialize(&without_state).unwrap(),
        "byte-identical state"
    );
    // And the slot never limits governance: two governance phones, one block.
    assert_eq!(with_state.station_registry.len(), 2);
}
