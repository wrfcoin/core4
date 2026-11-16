//! With no `…_key_retire_N` feature scheduled, pinned-key retirement changes
//! nothing, byte for byte.
//!
//! `bond_admission_key` and `station_registrar_key` are active from `K`, and
//! both pinned keys of each role are used across a long run of heights
//! through the real `State` apply path: node-bond deposits co-signed by the
//! online admission key, the spare and governance, a refused co-sign by an
//! unknown key, and phone-station registrations by both registrar keys. The
//! state-root material and the snapshot export must be byte-identical to the
//! ones `origin/main` e3bc8ab (before retirement existed) produces for the
//! same steps; the pinned digests below were taken there.
//!
//! Own test binary: the feature schedule is process-wide, and this file must
//! never schedule a retirement.

#[path = "common/node_bond_fixture.rs"]
#[allow(dead_code)]
mod fixture;

use fixture::{A, address, advance, apply, key, pk};
use sha3::{Digest, Sha3_256};
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::state_snapshot::export_state_entries;
use wrfcoin_shared_protocol::blockchain::transaction::{
    StationRegistrationPayload, Transaction, TransactionInput, TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::common::types::Balance;
use wrfcoin_shared_protocol::consensus::node_bond_registry::{NODE_BOND_UNITS, NodeBondLifecycle};
use wrfcoin_shared_protocol::consensus::node_share_contracts::NodeBondAction;
use wrfcoin_shared_protocol::consensus::station_registrar::{
    MOBILE_STATION_SLOT, PHONE_HARDWARE_MODEL, mobile_station_id,
};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, FeatureSchedule, feature_active, install_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::signature::PublicKey;
use wrfcoin_shared_protocol::governance::schedule_extension::{
    AppliedExtension, ScheduleEntry, install_bond_admission_pin_for_tests,
    install_schedule_extension_authority_for_tests, install_station_registrar_pin_for_tests,
};
use wrfcoin_shared_protocol::station_registry::{ContributionProvenanceClass, DataQualityTier};

/// SHA3-256 of `State::state_root_material()` after the steps below, on
/// origin/main e3bc8ab.
const ROOT_MATERIAL_SHA3: &str = "5be4a43e072cc4743c083c7d02a73884bcf86acea8ae9a411742c6a5cc574c3e"; // pragma: allowlist secret -- state digest, not a secret
/// SHA3-256 over the snapshot export (each key and value, length-prefixed), on
/// origin/main e3bc8ab.
const SNAPSHOT_SHA3: &str = "91ecf88314bc2a83fee08cf5ea9cdf604651ace54f68ed45ad0e6d029fe0b8e3"; // pragma: allowlist secret -- state digest, not a secret

const K: u64 = A + 2;
const LAST: u64 = 40;
const GOVERNANCE: u8 = 99;
const ADMISSION: u8 = 60;
const ADMISSION_SPARE: u8 = 61;
const REGISTRAR: u8 = 51;
const REGISTRAR_SPARE: u8 = 52;
const GOVERNANCE_ENV: &str = "WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS";

fn genesis_hex() -> String {
    hex::encode([7u8; 32])
}

fn governance_keys() -> Vec<String> {
    vec![hex::encode(pk(GOVERNANCE))]
}

fn install() {
    std::env::set_var(GOVERNANCE_ENV, hex::encode(pk(GOVERNANCE)));
    install_feature_schedule(
        FeatureSchedule::from_entries(vec![
            ("node_share_settlement".into(), A),
            ("account_authorization_dispatch".into(), A),
            ("station_registration_nonce".into(), 1),
            ("station_owner_actions".into(), 1),
            ("bond_admission_key".into(), K),
            ("station_registrar_key".into(), K),
        ])
        .unwrap(),
    );
    install_schedule_extension_authority_for_tests(
        "777",
        "testnet",
        &genesis_hex(),
        governance_keys(),
    );
    install_bond_admission_pin_for_tests(
        &genesis_hex(),
        vec![hex::encode(pk(ADMISSION)), hex::encode(pk(ADMISSION_SPARE))],
    );
    install_station_registrar_pin_for_tests(
        &genesis_hex(),
        vec![hex::encode(pk(REGISTRAR)), hex::encode(pk(REGISTRAR_SPARE))],
    );
}

fn cosigned(owner: u8, id: u8, cosigner: u8) -> NodeBondLifecycle {
    let mut p = fixture::auth(owner, id, 0, NodeBondAction::Deposit, 1);
    p.governance_key = Some(pk(cosigner));
    p.governance_signature = fixture::signature(&p.signed.authorization, cosigner);
    p
}

fn registration(owner: u8, signer: u8) -> Transaction {
    let owner_address = PublicKey::from_private_key(&key(owner)).to_address();
    let station_id = mobile_station_id(&owner_address, MOBILE_STATION_SLOT).unwrap();
    let mut p = StationRegistrationPayload::new(
        station_id,
        owner_address.clone(),
        46.8,
        -92.1,
        PHONE_HARDWARE_MODEL,
        DataQualityTier::Unverified,
        ContributionProvenanceClass::Real,
    )
    .with_expected_nonce(0);
    p.sign_ownership(&key(owner));
    p.sign(&key(signer));
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(&format!("inactive-input-{owner}-{signer}")),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            amount: 0,
            address: owner_address,
            script: String::new(),
            nft_data: None,
        }],
    );
    tx.tx_type = TransactionType::StationRegistration;
    tx.fee = 0;
    tx.data = Some(serde_json::to_string(&p).unwrap());
    tx.set_id();
    tx
}

fn run() -> State {
    install();
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
        signers: governance_keys(),
    });
    for owner in [10, 20, 30, 40, 50] {
        s.create_account(address(owner), Balance::new(u128::from(NODE_BOND_UNITS) * 3));
    }
    s.apply_transition(StateTransition::BeginNodeServiceBlock { block_height: A, parent_hash: [2; 32] })
        .unwrap();
    s.apply_block_finalization(A);

    // Bond deposits: the online admission key early and late, the spare, the
    // governance key, and an unknown key (refused).
    let bonds: [(u64, u8, u8, u8, bool); 5] = [
        (K, 10, 1, ADMISSION, true),
        (12, 20, 2, ADMISSION_SPARE, true),
        (20, 30, 3, GOVERNANCE, true),
        (30, 40, 4, 62, false),
        (LAST, 50, 5, ADMISSION, true),
    ];
    // Registrations: both registrar keys, below and well past every height a
    // retirement test uses.
    let stations: [(u64, u8, u8); 4] =
        [(K, 0xA1, REGISTRAR), (9, 0xA2, REGISTRAR_SPARE), (25, 0xA3, REGISTRAR), (LAST, 0xA4, REGISTRAR_SPARE)];
    for h in K..=LAST {
        advance(&mut s, h);
        for (at, owner, id, cosigner, ok) in bonds {
            if at == h {
                assert_eq!(apply(&mut s, cosigned(owner, id, cosigner), h).is_ok(), ok, "bond {id} at {h}");
            }
        }
        for (at, owner, signer) in stations {
            if at == h {
                let step = registration(owner, signer).station_registration_step(h).expect("registrar step");
                s.apply_transition(step).unwrap();
            }
        }
    }
    s.recalculate_state_root();
    s
}

fn snapshot_digest(state: &State) -> String {
    let mut hasher = Sha3_256::new();
    for (k, v) in export_state_entries(state).expect("export") {
        hasher.update((k.len() as u64).to_le_bytes());
        hasher.update(&k);
        hasher.update((v.len() as u64).to_le_bytes());
        hasher.update(&v);
    }
    hex::encode(hasher.finalize())
}

#[test]
fn no_retirement_scheduled_leaves_root_and_snapshot_byte_identical() {
    let state = run();
    assert_eq!(state.node_bond_registry.as_ref().unwrap().registrations.len(), 4);
    for owner in [0xA1u8, 0xA2, 0xA3, 0xA4] {
        let id = mobile_station_id(&PublicKey::from_private_key(&key(owner)).to_address(), 0).unwrap();
        assert!(state.station_registry.get(&id).is_some(), "station of {owner:#x} registered");
    }
    let root = hex::encode(Sha3_256::digest(state.state_root_material()));
    let snapshot = snapshot_digest(&state);
    println!("root_material_sha3={root} snapshot_sha3={snapshot}");
    assert_eq!(root, ROOT_MATERIAL_SHA3, "state-root material differs from main");
    assert_eq!(snapshot, SNAPSHOT_SHA3, "snapshot export differs from main");
}

#[test]
fn no_retirement_feature_is_active_in_this_binary() {
    install();
    for name in [
        "bond_admission_key_retire_0",
        "bond_admission_key_retire_1",
        "station_registrar_key_retire_0",
        "station_registrar_key_retire_1",
    ] {
        let f = ConsensusFeature::parse(name).expect("known");
        for h in [0, K, LAST, u64::MAX] {
            assert!(!feature_active(f, h), "{name} at {h}");
        }
    }
}
