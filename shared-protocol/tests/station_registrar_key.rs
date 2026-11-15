//! `station_registrar_key`: a pinned registrar key may register a NEW
//! phone-class station inside the compiled limits, and nothing else.
//!
//! This binary installs its own process-wide schedule
//! (`station_registration_nonce` at 1, `station_registrar_key` at `K` = 5),
//! the chain authority for a synthetic genesis, and two synthetic registrar
//! keys (online and spare) pinned for it. Heights below `K` are
//! pre-activation. The unscheduled binary is `station_registrar_key_unscheduled.rs`.
//!
//! Run from core4:
//!   cargo test -p wrfcoin-shared-protocol --test station_registrar_key

use std::sync::Arc;

use rand::SeedableRng;
use rand::rngs::StdRng;
use tokio::sync::RwLock;
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::station_owner::StationOwnerActionPayload;
use wrfcoin_shared_protocol::blockchain::transaction::{
    CommitPayload, ForecastArtifactManifest, RevealPayload, StationOwnerActionKind,
    StationRegistrationAuthority, StationRegistrationPayload, Transaction, TransactionInput,
    TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing;
use wrfcoin_shared_protocol::consensus::station_registrar::{
    MOBILE_STATION_SLOT, PHONE_HARDWARE_MODEL, PHONE_STATION_CEILING, RegistrarBlockSlot,
    RegistrarSelection, mobile_station_id, phone_station_count, registrar_keys_at,
    select_registrar_registrations, station_registrar_key_active,
};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, FeatureSchedule, install_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::domain_signing::is_authorized_oracle_governance;
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::governance::schedule_extension::{
    FeatureScheduleExtension, MIN_RUNUP_BLOCKS, ScheduleEntry, bond_admission_keys,
    check_admission_policy, install_schedule_extension_authority_for_tests,
    install_station_registrar_pin_for_tests, station_registrar_keys,
};
use wrfcoin_shared_protocol::mempool::pool::Mempool;
use wrfcoin_shared_protocol::station_registry::{
    ContributionProvenanceClass, DataQualityTier, StationLocation, WeatherStation,
};

const GOVERNANCE_ENV: &str = "WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS";
const NONCE_AT: u64 = 1;
const K: u64 = 5;
const GOVERNANCE: u8 = 0x60;
const REGISTRAR: u8 = 0x51;
const SPARE: u8 = 0x52;
const WRONG: u8 = 0x53;

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

fn install() {
    // Every test in this process sets the same value.
    std::env::set_var(GOVERNANCE_ENV, pk_hex(GOVERNANCE));
    install_feature_schedule(
        FeatureSchedule::from_entries(vec![
            ("station_registration_nonce".into(), NONCE_AT),
            ("station_owner_actions".into(), NONCE_AT),
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
    install_station_registrar_pin_for_tests(&genesis_hex(), vec![pk_hex(REGISTRAR), pk_hex(SPARE)]);
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

fn is_registrar(t: &Option<StateTransition>) -> bool {
    matches!(
        t,
        Some(StateTransition::ApplyStationRegistrarRegistration { .. })
    )
}

fn apply(state: &mut State, tx: &Transaction, height: u64) -> bool {
    match tx.station_registration_step(height) {
        Some(t) => {
            state
                .apply_transition(t)
                .expect("an extracted step never fails the block");
            true
        },
        None => false,
    }
}

fn phone_station(n: u32) -> WeatherStation {
    WeatherStation {
        station_id: format!("mob-prefill-{n}"),
        owner_address: format!("0x{n:040x}"),
        location: StationLocation {
            latitude: 0.0,
            longitude: 0.0,
            elevation_meters: None,
        },
        hardware_model: PHONE_HARDWARE_MODEL.into(),
        calibration_date: None,
        is_active: true,
        reputation_score: 0,
        data_quality_tier: DataQualityTier::Unverified,
        provenance_class: ContributionProvenanceClass::Real,
        ring_public_key: None,
    }
}

#[test]
fn registrar_is_inert_below_activation() {
    install();
    assert!(!station_registrar_key_active(K - 1));
    assert!(station_registrar_key_active(K));
    assert!(
        registrar_keys_at(K - 1).is_empty(),
        "no registrar key before activation"
    );
    assert_eq!(registrar_keys_at(K), vec![pk_hex(REGISTRAR), pk_hex(SPARE)]);
    let owner = key(0xA1);
    let tx = signed(phone(&owner), &owner, REGISTRAR);
    for h in [NONCE_AT, K - 1] {
        assert_eq!(
            tx.station_registration_authority_at_height(h),
            None,
            "height {h}"
        );
        assert!(tx.station_registration_step(h).is_none(), "height {h}");
        assert!(!tx.verify_station_registration_authorization_at_height(h));
    }
}

#[test]
fn registrar_can_activate_a_phone_within_limits() {
    install();
    let owner = key(0xA2);
    let tx = signed(phone(&owner), &owner, REGISTRAR);
    assert_eq!(
        tx.station_registration_authority_at_height(K),
        Some(StationRegistrationAuthority::Registrar)
    );
    let mut state = State::new();
    assert!(apply(&mut state, &tx, K));
    let id = mobile_station_id(&addr(&owner), 0).unwrap();
    let station = state.station_registry.get(&id).expect("registered");
    assert_eq!(station.owner_address, addr(&owner));
    assert_eq!(station.hardware_model, PHONE_HARDWARE_MODEL);
    assert_eq!(
        state.station_owner_ledger_view(&id).owner_nonce,
        1,
        "nonce advanced"
    );
    assert_eq!(phone_station_count(&state), 1);

    // The spare key works the same way.
    let other = key(0xA3);
    assert!(is_registrar(
        &signed(phone(&other), &other, SPARE).station_registration_step(K)
    ));

    // The same registration again: it still extracts, but cannot re-register.
    state.recalculate_state_root();
    let root = state.state_root;
    assert!(apply(&mut state, &tx, K + 1));
    state.recalculate_state_root();
    assert_eq!(
        state.state_root, root,
        "an existing station is never touched"
    );

    // A new owner-ledger nonce cannot be used to take over or move a station:
    // the registrar only signs nonce 0.
    let mut moved = phone(&owner);
    moved.expected_nonce = Some(1);
    assert!(
        signed(moved, &owner, REGISTRAR)
            .station_registration_step(K + 1)
            .is_none()
    );
}

#[test]
fn registrar_refuses_a_wrong_key() {
    install();
    let owner = key(0xA4);
    let tx = signed(phone(&owner), &owner, WRONG);
    assert_eq!(tx.station_registration_authority_at_height(K), None);
    assert!(tx.station_registration_step(K).is_none());
    // A registrar signature re-paired with another owner's proof fails too.
    let thief = key(0xA5);
    let mut p = phone(&owner);
    p.sign_ownership(&thief);
    p.sign(&key(REGISTRAR));
    assert!(
        tx_of("re-paired", &p)
            .station_registration_step(K)
            .is_none()
    );
}

#[test]
fn registrar_refuses_anything_outside_the_phone_shape() {
    install();
    let owner = key(0xA6);
    type Mutation = fn(&mut StationRegistrationPayload);
    let cases: [(&str, Mutation); 10] = [
        ("gold tier", |p| p.data_quality_tier = DataQualityTier::Gold),
        ("other hardware", |p| {
            p.hardware_model = "davis-vantage".into()
        }),
        ("synthetic", |p| {
            p.provenance_class = ContributionProvenanceClass::Synthetic
        }),
        ("slot 1 id", |p| {
            p.station_id = mobile_station_id(&p.owner_address, 1).unwrap()
        }),
        ("free id", |p| p.station_id = "ws-free-name".into()),
        ("elevation", |p| p.elevation_meters = Some(180.0)),
        ("off-grid latitude", |p| p.latitude = 46.78),
        ("off-grid longitude", |p| p.longitude = -92.15),
        ("unbound owner address", |p| {
            p.owner_address = "0x00112233445566778899aabbccddeeff00112233".into();
            p.station_id = mobile_station_id(&p.owner_address, 0).unwrap();
        }),
        ("nonce 1", |p| p.expected_nonce = Some(1)),
    ];
    for (name, mutate) in cases {
        let mut p = phone(&owner);
        mutate(&mut p);
        let tx = signed(p, &owner, REGISTRAR);
        assert!(
            tx.station_registration_step(K).is_none(),
            "{name} must be refused"
        );
    }
    // v1 (no nonce) is refused: the format rule already makes it inert from
    // `station_registration_nonce`, and the registrar never accepts it.
    let mut v1 = phone(&owner);
    v1.tx_kind = StationRegistrationPayload::TX_KIND.into();
    v1.expected_nonce = None;
    assert!(
        signed(v1, &owner, REGISTRAR)
            .station_registration_step(K)
            .is_none()
    );
    // Control: the unmutated payload passes.
    assert!(is_registrar(
        &signed(phone(&owner), &owner, REGISTRAR).station_registration_step(K)
    ));
}

/// `verify_station_registrar_authorization_with_domain` (the CLI signer's
/// self-check) says yes exactly when consensus applies the tx as a registrar
/// registration: same signature predicate, same pins, same rule function.
#[test]
fn offline_registrar_check_agrees_with_consensus() {
    use wrfcoin_shared_protocol::crypto::domain_signing::{signing_chain_id, signing_network_id};
    install();
    let (chain, network) = (signing_chain_id(), signing_network_id());
    let owner = key(0xA9);
    type Mutation = fn(&mut StationRegistrationPayload);
    let shapes: [(&str, Mutation); 6] = [
        ("control", |_| {}),
        ("gold tier", |p| p.data_quality_tier = DataQualityTier::Gold),
        ("other hardware", |p| {
            p.hardware_model = "davis-vantage".into()
        }),
        ("elevation", |p| p.elevation_meters = Some(180.0)),
        ("off-grid latitude", |p| p.latitude = 46.78),
        ("nonce 1", |p| p.expected_nonce = Some(1)),
    ];
    let mut accepted = 0;
    for (name, mutate) in shapes {
        for signer in [REGISTRAR, SPARE, WRONG, GOVERNANCE] {
            let mut p = phone(&owner);
            mutate(&mut p);
            let tx = signed(p, &owner, signer);
            let consensus = is_registrar(&tx.station_registration_step(K));
            let offline = tx.verify_station_registrar_authorization_with_domain(
                &chain,
                &network,
                &genesis_hex(),
            );
            assert_eq!(offline, consensus, "{name}, signer {signer:#x}");
            accepted += usize::from(offline);
            // A chain instance with no pin accepts nothing offline either.
            assert!(!tx.verify_station_registrar_authorization_with_domain(
                &chain,
                &network,
                &hex::encode([3u8; 32])
            ));
            // Signed for one chain, never valid for another.
            assert!(!tx.verify_station_registrar_authorization_with_domain(
                "other-chain",
                &network,
                &genesis_hex()
            ));
        }
    }
    assert_eq!(
        accepted, 2,
        "only the control, signed by the online key or the spare"
    );
}

#[test]
fn governance_path_is_unchanged_while_active() {
    install();
    let owner = key(0xA7);
    // Governance may still register anything it could before (a gold
    // station with a free id), as a v2 governance registration.
    let mut p = phone(&owner);
    p.station_id = "ws-gold".into();
    p.data_quality_tier = DataQualityTier::Gold;
    p.hardware_model = "davis-vantage".into();
    let tx = signed(p, &owner, GOVERNANCE);
    assert_eq!(
        tx.station_registration_authority_at_height(K),
        Some(StationRegistrationAuthority::Governance)
    );
    assert!(matches!(
        tx.station_registration_step(K),
        Some(StateTransition::ApplyStationRegistrationV2 { .. })
    ));
    // A governance-signed phone payload stays on the governance path.
    let phone_tx = signed(phone(&owner), &owner, GOVERNANCE);
    assert!(matches!(
        phone_tx.station_registration_step(K),
        Some(StateTransition::ApplyStationRegistrationV2 { .. })
    ));
}

#[test]
fn registrar_key_authorizes_nothing_else() {
    install();
    for seed in [REGISTRAR, SPARE] {
        let k = pk_hex(seed);
        assert!(
            !is_authorized_oracle_governance(&k),
            "not an oracle-governance key"
        );
        assert!(
            !wrfcoin_shared_protocol::governance::schedule_extension::schedule_extension_authority(
            )
            .unwrap()
            .allowlist()
            .contains(&k),
            "cannot sign schedules or upgrade notices"
        );
        assert!(
            !bond_admission_keys(&genesis_hex()).contains(&k),
            "not a bond admission key"
        );
    }
    // Pinned: schedulable here, refused on any other chain instance.
    assert_eq!(station_registrar_keys(&genesis_hex()).len(), 2);
    assert!(ConsensusFeature::StationRegistrarKey.schedulable(Some(&genesis_hex())));
    let other = "cd".repeat(32);
    assert!(!ConsensusFeature::StationRegistrarKey.schedulable(Some(&other)));
    assert!(!ConsensusFeature::StationRegistrarKey.schedulable(None));
    let extension = |genesis: &str| FeatureScheduleExtension {
        chain_id: "777".into(),
        network_id: "testnet".into(),
        genesis_hash: genesis.into(),
        sequence: 0,
        entries: vec![ScheduleEntry::new(
            "station_registrar_key",
            100 + MIN_RUNUP_BLOCKS,
        )],
        version_steps: vec![],
        min_protocol_version: 2,
        release_commit: "test".into(),
    };
    assert!(check_admission_policy(&extension(&genesis_hex()), Some(&genesis_hex())).is_ok());
    assert!(check_admission_policy(&extension(&other), Some(&other)).is_err());
}

#[test]
fn registrar_refuses_over_the_ceiling() {
    install();
    let mut state = State::new();
    for n in 0..(PHONE_STATION_CEILING as u32 - 1) {
        state.apply_station_registration(phone_station(n)).unwrap();
    }
    assert_eq!(phone_station_count(&state), PHONE_STATION_CEILING - 1);
    // The 128th phone station fits.
    let last = key(0xB0);
    assert!(apply(
        &mut state,
        &signed(phone(&last), &last, REGISTRAR),
        K
    ));
    assert_eq!(phone_station_count(&state), PHONE_STATION_CEILING);
    // The 129th is an inert no-op: extracted, but nothing moves.
    let over = key(0xB1);
    state.recalculate_state_root();
    let root = state.state_root;
    assert!(apply(
        &mut state,
        &signed(phone(&over), &over, REGISTRAR),
        K + 1
    ));
    state.recalculate_state_root();
    assert_eq!(state.state_root, root, "over the ceiling changes nothing");
    assert_eq!(phone_station_count(&state), PHONE_STATION_CEILING);
    let over_id = mobile_station_id(&addr(&over), 0).unwrap();
    assert!(state.station_registry.get(&over_id).is_none());
    assert_eq!(state.station_owner_ledger_view(&over_id).owner_nonce, 0);
    // Governance is not bound by the registrar's ceiling.
    let gov_owner = key(0xB2);
    assert!(apply(
        &mut state,
        &signed(phone(&gov_owner), &gov_owner, GOVERNANCE),
        K + 1
    ));
    assert_eq!(phone_station_count(&state), PHONE_STATION_CEILING + 1);
}

/// Apply an owner-signed station action (the real deactivation path) for
/// `owner`'s phone station at `height`.
fn owner_action(state: &mut State, owner: &PrivateKey, kind: StationOwnerActionKind, height: u64) {
    let id = mobile_station_id(&addr(owner), 0).unwrap();
    let nonce = state.station_owner_ledger_view(&id).owner_nonce;
    let identity = wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity::active();
    let mut action = StationOwnerActionPayload::new(
        id.clone(),
        addr(owner),
        PublicKey::from_private_key(owner).to_hex(),
        kind,
        nonce,
        height + 100,
    );
    action.sign_with_domain(&identity.chain_id, &identity.network_id, owner);
    let mut tx = tx_of(&format!("action-{id}-{nonce}"), &phone(owner));
    tx.data = Some(serde_json::to_string(&action).unwrap());
    tx.set_id();
    let transition = tx
        .station_owner_ledger_transition(height)
        .expect("an owner action extracts");
    state.apply_transition(transition).unwrap();
    assert_eq!(
        state.station_owner_ledger_view(&id).owner_nonce,
        nonce + 1,
        "the action applied"
    );
}

/// The ceiling counts phone stations that hold a slot: registry-active and
/// not suspended by the owner ledger (owner decision, 11-07). A real
/// owner-ledger Deactivate frees a slot, and so does a report-stolen freeze.
/// A deactivated or frozen `mob-` id is never re-registered through the
/// registrar.
#[test]
fn owner_deactivation_and_freeze_free_ceiling_slots_but_ids_cannot_be_re_registered() {
    install();
    let mut state = State::new();
    for n in 0..(PHONE_STATION_CEILING as u32 - 2) {
        state.apply_station_registration(phone_station(n)).unwrap();
    }
    let (deactivating, freezing) = (key(0xB3), key(0xB4));
    for owner in [&deactivating, &freezing] {
        assert!(apply(
            &mut state,
            &signed(phone(owner), owner, REGISTRAR),
            K
        ));
    }
    assert_eq!(phone_station_count(&state), PHONE_STATION_CEILING);
    let (w1, w2) = (key(0xB5), key(0xB6));
    let w1_id = mobile_station_id(&addr(&w1), 0).unwrap();
    let w2_id = mobile_station_id(&addr(&w2), 0).unwrap();
    assert!(apply(
        &mut state,
        &signed(phone(&w1), &w1, REGISTRAR),
        K + 1
    ));
    assert!(
        state.station_registry.get(&w1_id).is_none(),
        "full: refused"
    );

    // Owner self-deactivation frees one slot; the registry record stays.
    owner_action(
        &mut state,
        &deactivating,
        StationOwnerActionKind::Deactivate,
        K + 2,
    );
    let deactivated_id = mobile_station_id(&addr(&deactivating), 0).unwrap();
    assert!(
        state
            .station_registry
            .get(&deactivated_id)
            .unwrap()
            .is_active
    );
    assert_eq!(phone_station_count(&state), PHONE_STATION_CEILING - 1);
    assert!(apply(
        &mut state,
        &signed(phone(&w1), &w1, REGISTRAR),
        K + 2
    ));
    assert!(
        state.station_registry.get(&w1_id).is_some(),
        "the freed slot is taken"
    );
    assert_eq!(phone_station_count(&state), PHONE_STATION_CEILING);

    // A report-stolen freeze frees another.
    owner_action(
        &mut state,
        &freezing,
        StationOwnerActionKind::ReportStolen,
        K + 3,
    );
    assert_eq!(phone_station_count(&state), PHONE_STATION_CEILING - 1);
    assert!(apply(
        &mut state,
        &signed(phone(&w2), &w2, REGISTRAR),
        K + 3
    ));
    assert!(state.station_registry.get(&w2_id).is_some());

    // Neither id comes back through the registrar, even with a slot free and
    // a fresh spare-key signature.
    owner_action(&mut state, &w2, StationOwnerActionKind::Deactivate, K + 4);
    let frozen_id = mobile_station_id(&addr(&freezing), 0).unwrap();
    for (owner, id) in [(&deactivating, &deactivated_id), (&freezing, &frozen_id)] {
        let mut fresh = phone(owner);
        fresh.sign_ownership(owner);
        fresh.sign(&key(SPARE));
        state.recalculate_state_root();
        let root = state.state_root;
        assert!(apply(
            &mut state,
            &tx_of(&format!("again-{id}"), &fresh),
            K + 5
        ));
        state.recalculate_state_root();
        assert_eq!(
            state.state_root, root,
            "re-registering {id} changes nothing"
        );
    }

    // A Reactivate can take the count above the ceiling: harmless, the
    // ceiling only gates new registrar registrations.
    // Before: 127 hold a slot (128 minus w2, deactivated just above).
    assert_eq!(phone_station_count(&state), PHONE_STATION_CEILING - 1);
    owner_action(
        &mut state,
        &deactivating,
        StationOwnerActionKind::Reactivate,
        K + 6,
    );
    owner_action(&mut state, &w2, StationOwnerActionKind::Reactivate, K + 6);
    assert_eq!(phone_station_count(&state), PHONE_STATION_CEILING + 1);
    owner_action(
        &mut state,
        &freezing,
        StationOwnerActionKind::Unfreeze,
        K + 6,
    );
    assert_eq!(phone_station_count(&state), PHONE_STATION_CEILING + 2);
    // Over the ceiling, a new registrar registration is refused.
    let late = key(0xB7);
    assert!(apply(
        &mut state,
        &signed(phone(&late), &late, REGISTRAR),
        K + 7
    ));
    assert!(
        state
            .station_registry
            .get(&mobile_station_id(&addr(&late), 0).unwrap())
            .is_none()
    );
}

#[test]
fn registrar_registers_at_most_one_station_per_block() {
    install();
    let (a, b) = (key(0xC1), key(0xC2));
    let mut slot = RegistrarBlockSlot::new();
    let first = slot.admit(signed(phone(&a), &a, REGISTRAR).station_registration_step(K));
    let second = slot.admit(signed(phone(&b), &b, REGISTRAR).station_registration_step(K));
    assert!(is_registrar(&first));
    assert!(
        second.is_none(),
        "the second registrar registration in a block yields nothing"
    );
}

/// Producer selection (`create_next_block`): with two registrar registrations
/// in the pool, block 1 carries one and block 2 the other, and both apply. A
/// registration that can never apply is evicted and does not take the slot.
#[tokio::test]
async fn production_path_mines_one_registrar_registration_per_block_and_holds_the_rest() {
    install();
    let mut rng = StdRng::seed_from_u64(11);
    let miner = PrivateKey::generate(&mut rng);
    let miner_hex = format!(
        "0x{}",
        hex::encode(
            wrfcoin_shared_protocol::common::types::Address::from_public_key(
                &PublicKey::from_private_key(&miner)
            )
            .0
        )
    );
    let chain = Arc::new(RwLock::new(Blockchain::default()));
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    pop.declare_no_genesis_oracles();
    let offer = |tx: Transaction| {
        let chain = chain.clone();
        let mempool = mempool.clone();
        async move {
            let keys = chain.read().await.state.signing_keys.clone();
            mempool
                .write()
                .await
                .add_transaction_with_signing_keys(tx, Some(&keys))
                .expect("admitted to the mempool");
        }
    };
    while chain.read().await.height < K - 1 {
        pop.create_next_block(chain.clone(), mempool.clone(), &miner_hex)
            .await
            .unwrap();
    }
    let (a, b) = (key(0xD1), key(0xD2));
    let ids = [addr(&a), addr(&b)].map(|address| mobile_station_id(&address, 0).unwrap());
    let registered = |c: &Blockchain| {
        ids.iter()
            .filter(|id| c.state.station_registry.get(id).is_some())
            .count()
    };
    // Registrar registrations carried by the tip block.
    let in_tip = |c: &Blockchain| {
        let tip = c.blocks().values().last().unwrap();
        tip.transactions
            .iter()
            .filter(|tx| is_registrar(&tx.station_registration_step(tip.height)))
            .count()
    };
    let first = signed(phone(&a), &a, REGISTRAR);
    let second = signed(phone(&b), &b, REGISTRAR);
    offer(first.clone()).await;
    offer(second.clone()).await;
    pop.create_next_block(chain.clone(), mempool.clone(), &miner_hex)
        .await
        .unwrap();
    {
        let c = chain.read().await;
        assert_eq!(c.height, K);
        assert_eq!(in_tip(&c), 1, "block 1 carries one registrar registration");
        assert_eq!(registered(&c), 1, "and it applied");
    }
    let held = if mempool.read().await.contains(first.id.as_ref().unwrap()) {
        first.id.unwrap()
    } else {
        second.id.unwrap()
    };
    assert!(
        mempool.read().await.contains(&held),
        "the other stays in the mempool"
    );
    pop.create_next_block(chain.clone(), mempool.clone(), &miner_hex)
        .await
        .unwrap();
    {
        let c = chain.read().await;
        assert_eq!(c.height, K + 1);
        assert_eq!(in_tip(&c), 1, "block 2 carries the other");
        assert_eq!(registered(&c), 2, "both applied");
    }
    assert!(!mempool.read().await.contains(&held));

    // A registration that can never apply (its station is registered) does
    // not take the slot: it is evicted, and a live one mines in its block.
    let mut duplicate = phone(&a);
    duplicate.sign_ownership(&a);
    duplicate.sign(&key(SPARE));
    let duplicate = tx_of("duplicate-a", &duplicate);
    let c_key = key(0xD3);
    let live = signed(phone(&c_key), &c_key, REGISTRAR);
    let live_id = mobile_station_id(&addr(&c_key), 0).unwrap();
    offer(duplicate.clone()).await;
    offer(live).await;
    pop.create_next_block(chain.clone(), mempool.clone(), &miner_hex)
        .await
        .unwrap();
    {
        let c = chain.read().await;
        assert_eq!(c.height, K + 2);
        assert_eq!(in_tip(&c), 1);
        assert!(
            c.state.station_registry.get(&live_id).is_some(),
            "the live one applied"
        );
        let tip = c.blocks().values().last().unwrap();
        assert!(
            !tip.transactions.iter().any(|tx| tx.id == duplicate.id),
            "duplicate not mined"
        );
    }
    assert!(
        !mempool
            .read()
            .await
            .contains(duplicate.id.as_ref().unwrap()),
        "a registration that can never apply is evicted"
    );
}

/// Nothing but the registration step, its scheduling guards and the
/// registrar module reads the registrar table, so no other action can come to
/// trust a registrar key.
#[test]
fn only_the_registration_path_and_the_scheduling_guards_read_the_pins() {
    fn scan(dir: &std::path::Path, hits: &mut Vec<String>, root: &std::path::Path) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                scan(&path, hits, root);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let source = std::fs::read_to_string(&path).unwrap();
                if source.contains("station_registrar_keys(")
                    || source.contains("STATION_REGISTRAR_KEY_PINS")
                    || source.contains("registrar_keys_at(")
                {
                    hits.push(path.strip_prefix(root).unwrap().display().to_string());
                }
            }
        }
    }
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.parent().unwrap();
    let mut hits = Vec::new();
    for crate_src in [
        "shared-protocol/src",
        "native-chain/src",
        "tools/cli/WrfcoinCLI/src",
    ] {
        scan(&root.join(crate_src), &mut hits, root);
    }
    hits.sort();
    assert_eq!(
        hits,
        [
            "shared-protocol/src/blockchain/transaction.rs",
            "shared-protocol/src/consensus/station_registrar.rs",
            "shared-protocol/src/crypto/chain_binding.rs",
            "shared-protocol/src/governance/schedule_extension.rs",
        ]
    );
}

/// Selection: a ceiling refusal is held (it may clear), and a governance
/// registration earlier in the same candidate that fills the ceiling is seen.
#[test]
fn selection_holds_ceiling_refusals_and_replays_the_candidate_in_order() {
    install();
    let mut state = State::new();
    for n in 0..(PHONE_STATION_CEILING as u32 - 1) {
        state.apply_station_registration(phone_station(n)).unwrap();
    }
    let (r, g) = (key(0xE5), key(0xE6));
    let registrar_tx = signed(phone(&r), &r, REGISTRAR);
    let governance_tx = signed(phone(&g), &g, GOVERNANCE);
    // One slot left: the registrar registration alone is selected.
    assert_eq!(
        select_registrar_registrations(&state, K, std::slice::from_ref(&registrar_tx)),
        RegistrarSelection::default()
    );
    // A governance phone ahead of it fills the ceiling: held, not evicted.
    assert_eq!(
        select_registrar_registrations(&state, K, &[governance_tx.clone(), registrar_tx.clone()]),
        RegistrarSelection {
            defer: vec![1],
            evict: vec![]
        }
    );
    // Behind it, the registrar registration takes the last slot.
    assert_eq!(
        select_registrar_registrations(&state, K, &[registrar_tx.clone(), governance_tx]),
        RegistrarSelection::default()
    );
    // Two with room: the first is selected, the second waits; other
    // transactions are never touched.
    let (s1, s2) = (key(0xE7), key(0xE8));
    let roomy = State::new();
    assert_eq!(
        select_registrar_registrations(
            &roomy,
            K,
            &[
                signed(phone(&s1), &s1, REGISTRAR),
                signed(phone(&s2), &s2, REGISTRAR),
            ],
        ),
        RegistrarSelection {
            defer: vec![1],
            evict: vec![]
        }
    );
    // Below activation selection never touches anything.
    assert_eq!(
        select_registrar_registrations(&state, K - 1, &[registrar_tx]),
        RegistrarSelection::default()
    );
}

/// A commit + reveal pair for `station_id` (`kind` is "forecast" or
/// "contribution"), signed by a throwaway key, as the earn-loop tests build
/// them.
fn commit_and_reveal(station_id: &str, kind: &str, tag: &str) -> (Transaction, Transaction) {
    let data = serde_json::json!({
        "quality": 1.0, "volume": 1.0, "uniqueness": 1.0,
        "temperature_c": 21.5, "humidity_percent": 60.0, "pressure_hpa": 1014.0,
    });
    let timestamp = chrono::Utc::now().timestamp();
    let nonce = format!("nonce-{tag}");
    let commit_id = Transaction::compute_commit_id(&data, &nonce, station_id, timestamp, kind);
    let mut commit = CommitPayload {
        commit_id: commit_id.clone(),
        station_id: station_id.to_string(),
        timestamp,
        contribution_type: kind.to_string(),
        auth_pubkey: None,
        auth_signature: None,
    };
    commit.sign_authorization(&key(0x77));
    let reveal = RevealPayload {
        commit_id,
        station_id: station_id.to_string(),
        timestamp,
        contribution_type: kind.to_string(),
        data,
        nonce,
        data_cid: None,
        data_hash: None,
        storage_provider: None,
        provenance: None,
        // A forecast reveal must carry an artifact manifest.
        manifest: (kind == "forecast").then(|| ForecastArtifactManifest {
            schema_version: 1,
            model_id: "wrf-gfs-v3".to_string(),
            input_hash: "a".repeat(64),
            output_hash: "b".repeat(64),
            produced_at: timestamp,
            region: "us-east".to_string(),
            horizon_hours: 48,
            input_variables: vec!["pressure".to_string(), "temperature".to_string()],
            output_variables: vec!["temperature".to_string()],
        }),
        verification_receipt: None,
        compute_receipt: None,
        quality_tier: None,
    };
    let (commit_type, reveal_type) = if kind == "forecast" {
        (
            TransactionType::ForecastCommit,
            TransactionType::ForecastReveal,
        )
    } else {
        (
            TransactionType::ContributionCommit,
            TransactionType::ContributionReveal,
        )
    };
    let envelope = |tx_type: TransactionType, data: String, tag: String| {
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_string(&tag),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 1,
                address: format!("wrf-{tag}"),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.tx_type = tx_type;
        tx.data = Some(data);
        tx.fee = 1;
        tx.set_id();
        tx
    };
    (
        envelope(
            commit_type,
            serde_json::to_string(&commit).unwrap(),
            format!("commit-{tag}"),
        ),
        envelope(
            reveal_type,
            serde_json::to_string(&reveal).unwrap(),
            format!("reveal-{tag}"),
        ),
    )
}

/// Liveness: a deferred registrar registration plus same-block forecast and
/// contribution commit + reveal pairs naming the deferred station never make
/// the producer refuse its own block, and the next round makes progress.
#[tokio::test]
async fn deferred_registration_with_same_block_commits_and_reveals_never_stalls_production() {
    install();
    let mut rng = StdRng::seed_from_u64(13);
    let miner = PrivateKey::generate(&mut rng);
    let miner_hex = format!(
        "0x{}",
        hex::encode(
            wrfcoin_shared_protocol::common::types::Address::from_public_key(
                &PublicKey::from_private_key(&miner)
            )
            .0
        )
    );
    let chain = Arc::new(RwLock::new(Blockchain::default()));
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    pop.declare_no_genesis_oracles();
    while chain.read().await.height < K - 1 {
        pop.create_next_block(chain.clone(), mempool.clone(), &miner_hex)
            .await
            .unwrap();
    }
    let offer = |tx: Transaction| {
        let chain = chain.clone();
        let mempool = mempool.clone();
        async move {
            let keys = chain.read().await.state.signing_keys.clone();
            mempool
                .write()
                .await
                .add_transaction_with_signing_keys(tx, Some(&keys))
                .is_ok()
        }
    };
    let (a, b) = (key(0xF4), key(0xF5));
    let (reg_a, reg_b) = (
        signed(phone(&a), &a, REGISTRAR),
        signed(phone(&b), &b, REGISTRAR),
    );
    // Fixed outcome: both carry fee 0, and the mempool breaks fee ties by
    // ascending txid, so the registration with the smaller txid is selected
    // and the other one is deferred.
    let ((selected_key, selected_reg), (deferred_key, deferred_reg)) = if reg_a.id < reg_b.id {
        ((&a, reg_a), (&b, reg_b))
    } else {
        ((&b, reg_b), (&a, reg_a))
    };
    let selected_id = mobile_station_id(&addr(selected_key), 0).unwrap();
    let deferred_id = mobile_station_id(&addr(deferred_key), 0).unwrap();
    assert!(offer(selected_reg).await);
    assert!(offer(deferred_reg.clone()).await);
    // Forecast and contribution commit + reveal pairs for BOTH stations.
    let mut deferred_station_txs = Vec::new();
    let mut admitted = 0;
    for (station, tag) in [(&selected_id, "selected"), (&deferred_id, "deferred")] {
        for kind in ["forecast", "contribution"] {
            let (commit, reveal) = commit_and_reveal(station, kind, &format!("{kind}-{tag}"));
            if station == &deferred_id {
                deferred_station_txs.push(commit.clone());
                deferred_station_txs.push(reveal.clone());
            }
            admitted += usize::from(offer(commit).await);
            admitted += usize::from(offer(reveal).await);
        }
    }
    assert_eq!(
        admitted, 8,
        "all eight commits and reveals reach the mempool"
    );

    pop.create_next_block(chain.clone(), mempool.clone(), &miner_hex)
        .await
        .expect("a deferred registration never makes the producer refuse its block");
    {
        let c = chain.read().await;
        assert_eq!(c.height, K);
        assert!(
            c.state.station_registry.get(&selected_id).is_some(),
            "the selected one applied"
        );
        assert!(
            c.state.station_registry.get(&deferred_id).is_none(),
            "the other was deferred"
        );
        let mined: Vec<_> = c
            .blocks()
            .values()
            .last()
            .unwrap()
            .transactions
            .iter()
            .filter_map(|t| t.id)
            .collect();
        assert!(!mined.contains(deferred_reg.id.as_ref().unwrap()));
        let pool = mempool.read().await;
        assert!(
            pool.contains(deferred_reg.id.as_ref().unwrap()),
            "the deferred registration waits"
        );
        // The deferred station's commits and reveals: each mined in block K
        // or still pending, never lost.
        for tx in &deferred_station_txs {
            let id = tx.id.unwrap();
            assert!(
                mined.contains(&id) || pool.contains(&id),
                "{:?} for the deferred station was lost (neither mined nor pending)",
                tx.tx_type
            );
        }
    }
    pop.create_next_block(chain.clone(), mempool.clone(), &miner_hex)
        .await
        .expect("the next round makes progress");
    {
        let c = chain.read().await;
        assert_eq!(c.height, K + 1);
        assert!(
            c.state.station_registry.get(&deferred_id).is_some(),
            "the deferred registration applies next"
        );
    }
}
