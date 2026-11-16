//! Height-aware retirement of pinned keys: `bond_admission_key_retire_N` and
//! `station_registrar_key_retire_N` take slot N of that role's pin row out of
//! the key set from their activation height. Below it the key works exactly
//! as before (replay of old blocks is unchanged); the other slot keeps working.
//!
//! This binary installs its own process-wide schedule: both roles active at
//! `K` = 5, slot 0 of both retired at `R0` = 8, slot 1 of both retired at
//! `R1` = 11. One synthetic genesis (`[7; 32]`) carries two synthetic keys
//! per role (slot 0 "online", slot 1 "spare").
//!
//! Run from core4:
//!   cargo test -p wrfcoin-shared-protocol --test pinned_key_retirement

#[path = "common/node_bond_fixture.rs"]
#[allow(dead_code)]
mod fixture;

use fixture::{A, address, advance, apply, key, pk};
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{
    StationRegistrationAuthority, StationRegistrationPayload, Transaction, TransactionInput,
    TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::common::types::Balance;
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::node_bond_registry::{
    NODE_BOND_UNITS, NodeBondLifecycle, admission_keys_at,
};
use wrfcoin_shared_protocol::consensus::node_share_contracts::NodeBondAction;
use wrfcoin_shared_protocol::consensus::station_registrar::{
    MOBILE_STATION_SLOT, PHONE_HARDWARE_MODEL, mobile_station_id, registrar_keys_at,
};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    BOND_ADMISSION_KEY_RETIREMENTS, ChainIdentity, ConsensusFeature, FeatureSchedule,
    STATION_REGISTRAR_KEY_RETIREMENTS, feature_active, install_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::protocol_support::{
    KNOWN_FEATURES, upgrade_required_in_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::signature::PublicKey;
use wrfcoin_shared_protocol::governance::schedule_extension::{
    AppliedExtension, FeatureScheduleExtension, MIN_RUNUP_BLOCKS, ScheduleEntry,
    ScheduleExtensionError, check_admission_policy, install_bond_admission_pin_for_tests,
    install_schedule_extension_authority_for_tests, install_station_registrar_pin_for_tests,
    sign_feature_schedule_extension_with_key,
};
use wrfcoin_shared_protocol::mempool::pool::Mempool;
use wrfcoin_shared_protocol::station_registry::{ContributionProvenanceClass, DataQualityTier};

const K: u64 = A + 2;
const R0: u64 = 8;
const R1: u64 = 11;
const GOVERNANCE: u8 = 99;
const ADMISSION: u8 = 60;
const ADMISSION_SPARE: u8 = 61;
const REGISTRAR: u8 = 51;
const REGISTRAR_SPARE: u8 = 52;
const GOVERNANCE_ENV: &str = "WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS";

fn genesis_hex() -> String {
    hex::encode([7u8; 32])
}

fn identity() -> ChainIdentity {
    ChainIdentity::new("777", "testnet")
}

fn governance_keys() -> Vec<String> {
    vec![hex::encode(pk(GOVERNANCE))]
}

fn pk_hex(seed: u8) -> String {
    hex::encode(pk(seed))
}

fn install() {
    // Every test in this process sets the same value.
    std::env::set_var(GOVERNANCE_ENV, pk_hex(GOVERNANCE));
    install_feature_schedule(
        FeatureSchedule::from_entries(vec![
            ("node_share_settlement".into(), A),
            ("account_authorization_dispatch".into(), A),
            ("station_registration_nonce".into(), 1),
            ("station_owner_actions".into(), 1),
            ("bond_admission_key".into(), K),
            ("station_registrar_key".into(), K),
            ("bond_admission_key_retire_0".into(), R0),
            ("station_registrar_key_retire_0".into(), R0),
            ("bond_admission_key_retire_1".into(), R1),
            ("station_registrar_key_retire_1".into(), R1),
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
        vec![pk_hex(ADMISSION), pk_hex(ADMISSION_SPARE)],
    );
    install_station_registrar_pin_for_tests(
        &genesis_hex(),
        vec![pk_hex(REGISTRAR), pk_hex(REGISTRAR_SPARE)],
    );
}

// ---------------------------------------------------------------------------
// Bond admission
// ---------------------------------------------------------------------------

fn bond_state() -> State {
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
    for owner in [10, 20, 30, 40] {
        s.create_account(
            address(owner),
            Balance::new(u128::from(NODE_BOND_UNITS) * 3),
        );
    }
    s.apply_transition(StateTransition::BeginNodeServiceBlock {
        block_height: A,
        parent_hash: [2; 32],
    })
    .unwrap();
    s.apply_block_finalization(A);
    s
}

fn cosigned(owner: u8, id: u8, cosigner: u8) -> NodeBondLifecycle {
    let mut p = fixture::auth(owner, id, 0, NodeBondAction::Deposit, 1);
    p.governance_key = Some(pk(cosigner));
    p.governance_signature = fixture::signature(&p.signed.authorization, cosigner);
    p
}

fn bond_ok(p: &NodeBondLifecycle, height: u64) -> bool {
    install();
    p.verify(&identity(), &[7; 32], &governance_keys(), height)
        .is_ok()
}

#[test]
fn retirement_features_are_appended_known_and_unscheduled_by_default() {
    install();
    let names = [
        (ConsensusFeature::BondAdmissionKeyRetire0, "bond_admission_key_retire_0", 37u32),
        (ConsensusFeature::BondAdmissionKeyRetire1, "bond_admission_key_retire_1", 38),
        (ConsensusFeature::StationRegistrarKeyRetire0, "station_registrar_key_retire_0", 39),
        (ConsensusFeature::StationRegistrarKeyRetire1, "station_registrar_key_retire_1", 40),
    ];
    for (f, name, ordinal) in names {
        assert_eq!(f.name(), name);
        assert_eq!(ConsensusFeature::parse(name), Some(f));
        assert_eq!(ConsensusFeature::ALL[ordinal as usize], f);
        assert_eq!(bincode::serialize(&f).unwrap(), ordinal.to_le_bytes());
        assert!(f.has_behaviour());
        assert!(KNOWN_FEATURES.contains(&name), "{name}");
        assert!(!FeatureSchedule::default().active(f, u64::MAX), "{name} unscheduled by default");
    }
    // `forecast_late_score_v2` is appended after them at ordinal 41.
    assert_eq!(ConsensusFeature::ALL.len(), 42);
    assert_eq!(
        BOND_ADMISSION_KEY_RETIREMENTS,
        &[ConsensusFeature::BondAdmissionKeyRetire0, ConsensusFeature::BondAdmissionKeyRetire1]
    );
    assert_eq!(
        STATION_REGISTRAR_KEY_RETIREMENTS,
        &[
            ConsensusFeature::StationRegistrarKeyRetire0,
            ConsensusFeature::StationRegistrarKeyRetire1
        ]
    );
}

#[test]
fn admission_key_set_by_height() {
    install();
    let g = [7u8; 32];
    assert!(admission_keys_at(&g, K - 1).is_empty(), "role inactive");
    for h in [K, R0 - 1] {
        assert_eq!(admission_keys_at(&g, h), vec![pk_hex(ADMISSION), pk_hex(ADMISSION_SPARE)], "{h}");
    }
    for h in [R0, R1 - 1] {
        assert_eq!(admission_keys_at(&g, h), vec![pk_hex(ADMISSION_SPARE)], "{h}");
    }
    for h in [R1, R1 + 1, u64::MAX] {
        assert!(admission_keys_at(&g, h).is_empty(), "{h}");
    }
}

#[test]
fn retired_admission_key_works_below_retirement_and_is_refused_from_it() {
    let online = cosigned(10, 1, ADMISSION);
    let spare = cosigned(20, 2, ADMISSION_SPARE);
    let unknown = cosigned(10, 1, 62);
    for h in [K, K + 1, R0 - 1] {
        assert!(bond_ok(&online, h), "online key, historical height {h}");
        assert!(bond_ok(&spare, h), "spare, height {h}");
    }
    for h in [R0, R0 + 1, R1 - 1] {
        assert!(!bond_ok(&online, h), "online key retired at {h}");
        assert_eq!(
            format!("{:?}", online.verify(&identity(), &[7; 32], &governance_keys(), h)),
            format!("{:?}", unknown.verify(&identity(), &[7; 32], &governance_keys(), h)),
            "a retired key is refused exactly like an unknown key at {h}"
        );
        assert!(bond_ok(&spare, h), "spare after the transition, height {h}");
    }

    // Through the real apply path: the old key's deposit applied below R0
    // stays applied (replay), the same co-sign is refused from R0, the spare
    // is accepted after it.
    let mut s = bond_state();
    advance(&mut s, R0 - 1);
    apply(&mut s, online.clone(), R0 - 1).expect("historical height accepts the online key");
    advance(&mut s, R0);
    let mut s2 = bond_state();
    advance(&mut s2, R0);
    assert!(apply(&mut s2, online.clone(), R0).is_err(), "retired at R0");
    apply(&mut s, cosigned(30, 3, ADMISSION_SPARE), R0).expect("spare after transition");
    assert_eq!(s.node_bond_registry.as_ref().unwrap().registrations.len(), 2);
}

#[test]
fn retiring_both_admission_slots_leaves_only_the_governance_keys() {
    let online = cosigned(10, 1, ADMISSION);
    let spare = cosigned(20, 2, ADMISSION_SPARE);
    for h in [R1, R1 + 1, u64::MAX - 1] {
        assert!(!bond_ok(&online, h), "{h}");
        assert!(!bond_ok(&spare, h), "{h}");
        assert!(
            bond_ok(&fixture::auth(10, 1, 0, NodeBondAction::Deposit, 1), h),
            "governance co-sign still works at {h}"
        );
    }
    let mut s = bond_state();
    advance(&mut s, R1);
    assert!(apply(&mut s, spare, R1).is_err());
    apply(&mut s, fixture::auth(40, 4, 0, NodeBondAction::Deposit, 1), R1).unwrap();
}

#[test]
fn a_retired_admission_key_still_cannot_hold_a_bond_role() {
    // The fixture's server key is owner + 1: owner 59 makes ADMISSION (60)
    // the server key. Retired at R0, it is still a pinned key and still
    // forbidden as a node role key.
    let as_server = fixture::auth(59, 1, 0, NodeBondAction::Deposit, 1);
    assert_eq!(as_server.signed.authorization.server_key, pk(ADMISSION));
    let mut s = bond_state();
    s.create_account(address(59), Balance::new(u128::from(NODE_BOND_UNITS) * 3));
    advance(&mut s, R1);
    assert!(apply(&mut s, as_server, R1).is_err());
}

#[test]
fn mempool_admission_follows_the_retirement_height() {
    let _ = bond_state();
    let tx = fixture::transaction(&cosigned(10, 1, ADMISSION), 1_000_000);
    // Tip R0 - 2 carries at R0 - 1: still accepted. Tip R0 - 1 carries at R0: refused.
    let mut pool = Mempool::new(&Config::default());
    pool.add_transaction_with_signing_keys_at_height(tx.clone(), None, Some(R0 - 2))
        .unwrap();
    let mut pool = Mempool::new(&Config::default());
    let refused = pool
        .add_transaction_with_signing_keys_at_height(tx, None, Some(R0 - 1))
        .unwrap_err();
    assert!(refused.to_string().contains("SP_NODE_BOND_AUTHORIZATION"), "{refused}");
}

// ---------------------------------------------------------------------------
// Station registrar
// ---------------------------------------------------------------------------

fn addr(seed: u8) -> String {
    PublicKey::from_private_key(&key(seed)).to_address()
}

fn registration(owner: u8, signer: u8) -> Transaction {
    let owner_address = addr(owner);
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
            txid: hash_string(&format!("retire-input-{owner}-{signer}")),
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

fn authority(tx: &Transaction, h: u64) -> Option<StationRegistrationAuthority> {
    install();
    tx.station_registration_authority_at_height(h)
}

#[test]
fn registrar_key_set_by_height() {
    install();
    assert!(registrar_keys_at(K - 1).is_empty());
    for h in [K, R0 - 1] {
        assert_eq!(registrar_keys_at(h), vec![pk_hex(REGISTRAR), pk_hex(REGISTRAR_SPARE)], "{h}");
    }
    for h in [R0, R1 - 1] {
        assert_eq!(registrar_keys_at(h), vec![pk_hex(REGISTRAR_SPARE)], "{h}");
    }
    for h in [R1, u64::MAX] {
        assert!(registrar_keys_at(h).is_empty(), "{h}");
    }
}

#[test]
fn retired_registrar_key_works_below_retirement_and_is_refused_from_it() {
    let online = registration(0xA1, REGISTRAR);
    let spare = registration(0xA2, REGISTRAR_SPARE);
    for h in [K, R0 - 1] {
        assert_eq!(authority(&online, h), Some(StationRegistrationAuthority::Registrar), "{h}");
        assert_eq!(authority(&spare, h), Some(StationRegistrationAuthority::Registrar), "{h}");
        assert!(online.station_registration_step(h).is_some(), "{h}");
    }
    for h in [R0, R1 - 1] {
        assert_eq!(authority(&online, h), None, "online key retired at {h}");
        assert!(online.station_registration_step(h).is_none(), "{h}");
        assert!(!online.verify_station_registration_authorization_at_height(h), "{h}");
        assert_eq!(authority(&spare, h), Some(StationRegistrationAuthority::Registrar), "{h}");
        assert!(spare.station_registration_step(h).is_some(), "{h}");
    }
    // Real apply path below the retirement.
    let mut s = State::new();
    let step = online.station_registration_step(R0 - 1).expect("historical height");
    s.apply_transition(step).unwrap();
    let id = mobile_station_id(&addr(0xA1), 0).unwrap();
    assert!(s.station_registry.get(&id).is_some());
}

#[test]
fn retiring_both_registrar_slots_leaves_only_governance() {
    let online = registration(0xA3, REGISTRAR);
    let spare = registration(0xA4, REGISTRAR_SPARE);
    let governed = registration(0xA5, GOVERNANCE);
    for h in [R1, R1 + 1] {
        assert_eq!(authority(&online, h), None, "{h}");
        assert_eq!(authority(&spare, h), None, "{h}");
        assert_eq!(
            authority(&governed, h),
            Some(StationRegistrationAuthority::Governance),
            "governance still registers at {h}"
        );
    }
}

// ---------------------------------------------------------------------------
// Scheduling guards
// ---------------------------------------------------------------------------

fn extension(genesis: String, feature: &str) -> FeatureScheduleExtension {
    FeatureScheduleExtension {
        chain_id: "777".into(),
        network_id: "testnet".into(),
        genesis_hash: genesis,
        sequence: 0,
        entries: vec![ScheduleEntry::new(feature, 21_200 + MIN_RUNUP_BLOCKS)],
        version_steps: Vec::new(),
        min_protocol_version: 3,
        release_commit: "ab1b1a4930000000000000000000000000000000".into(),
    }
}

#[test]
fn retirement_is_schedulable_only_where_the_slot_exists() {
    install();
    let other = "cd".repeat(32);
    for f in BOND_ADMISSION_KEY_RETIREMENTS.iter().chain(STATION_REGISTRAR_KEY_RETIREMENTS) {
        assert!(f.schedulable(Some(&genesis_hex())), "{}", f.name());
        assert!(!f.schedulable(Some(&other)), "{}", f.name());
        assert!(!f.schedulable(None), "{}", f.name());
        let here = extension(genesis_hex(), f.name());
        assert_eq!(check_admission_policy(&here, Some(&genesis_hex())), Ok(()));
        sign_feature_schedule_extension_with_key(&here, &key(GOVERNANCE)).unwrap();
        let elsewhere = extension(other.clone(), f.name());
        assert_eq!(
            check_admission_policy(&elsewhere, Some(&other)),
            Err(ScheduleExtensionError::FeatureNotSchedulable { feature: f.name().into() })
        );
        assert!(sign_feature_schedule_extension_with_key(&elsewhere, &key(GOVERNANCE)).is_err());
    }
    // An index past the compiled slots is no feature at all.
    for name in ["bond_admission_key_retire_2", "station_registrar_key_retire_2"] {
        assert_eq!(ConsensusFeature::parse(name), None);
        assert_eq!(
            check_admission_policy(&extension(genesis_hex(), name), Some(&genesis_hex())),
            Err(ScheduleExtensionError::UnknownFeature { feature: name.into() })
        );
    }
}

#[test]
fn a_binary_with_the_slot_does_not_halt_at_a_retirement() {
    install();
    for f in BOND_ADMISSION_KEY_RETIREMENTS.iter().chain(STATION_REGISTRAR_KEY_RETIREMENTS) {
        let schedule = FeatureSchedule::from_entries(vec![(f.name().into(), R0)]).unwrap();
        assert_eq!(upgrade_required_in_feature_schedule(&schedule), None, "{}", f.name());
        assert!(feature_active(*f, R1));
    }
}
