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
use wrfcoin_shared_protocol::blockchain::transaction::{
    StationRegistrationAuthority, StationRegistrationPayload, Transaction, TransactionInput,
    TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing;
use wrfcoin_shared_protocol::consensus::station_registrar::{
    MOBILE_STATION_SLOT, PHONE_HARDWARE_MODEL, PHONE_STATION_CEILING, RegistrarBlockSlot,
    RegistrarSelection, select_registrar_registrations,
    mobile_station_id, phone_station_count, registrar_keys_at, station_registrar_key_active,
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
    assert_eq!(phone_station_count(&state.station_registry), 1);

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
    assert_eq!(
        phone_station_count(&state.station_registry),
        PHONE_STATION_CEILING - 1
    );
    // The 128th phone station fits.
    let last = key(0xB0);
    assert!(apply(
        &mut state,
        &signed(phone(&last), &last, REGISTRAR),
        K
    ));
    assert_eq!(
        phone_station_count(&state.station_registry),
        PHONE_STATION_CEILING
    );
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
    assert_eq!(
        phone_station_count(&state.station_registry),
        PHONE_STATION_CEILING
    );
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
    assert_eq!(
        phone_station_count(&state.station_registry),
        PHONE_STATION_CEILING + 1
    );
}

/// The ceiling counts ACTIVE phone stations only (owner decision, 11-07):
/// deactivating one frees a slot, but a deactivated `mob-` id is never
/// re-registered through the registrar.
#[test]
fn a_deactivated_station_frees_a_ceiling_slot_but_cannot_be_re_registered() {
    install();
    let mut state = State::new();
    for n in 0..(PHONE_STATION_CEILING as u32 - 1) {
        state.apply_station_registration(phone_station(n)).unwrap();
    }
    let last = key(0xB3);
    let last_tx = signed(phone(&last), &last, REGISTRAR);
    assert!(apply(&mut state, &last_tx, K));
    assert_eq!(phone_station_count(&state.station_registry), PHONE_STATION_CEILING);
    let waiting = key(0xB4);
    let waiting_tx = signed(phone(&waiting), &waiting, REGISTRAR);
    let waiting_id = mobile_station_id(&addr(&waiting), 0).unwrap();
    assert!(apply(&mut state, &waiting_tx, K + 1));
    assert!(state.station_registry.get(&waiting_id).is_none(), "full: refused");

    // An inactive phone station holds no slot.
    let last_id = mobile_station_id(&addr(&last), 0).unwrap();
    assert!(state.deactivate_weather_station(&last_id));
    assert_eq!(phone_station_count(&state.station_registry), PHONE_STATION_CEILING - 1);
    assert!(apply(&mut state, &waiting_tx, K + 2));
    assert!(state.station_registry.get(&waiting_id).is_some(), "the freed slot is taken");
    assert_eq!(phone_station_count(&state.station_registry), PHONE_STATION_CEILING);

    // The deactivated id stays deactivated: the registrar cannot bring it back,
    // even with a slot free.
    assert!(state.deactivate_weather_station(&waiting_id));
    state.recalculate_state_root();
    let root = state.state_root;
    assert!(apply(&mut state, &last_tx, K + 3));
    state.recalculate_state_root();
    assert_eq!(state.state_root, root, "re-registering a deactivated id changes nothing");
    assert!(!state.station_registry.get(&last_id).unwrap().is_active);
    // Even after a fresh signature over a new envelope.
    let mut fresh = phone(&last);
    fresh.sign_ownership(&last);
    fresh.sign(&key(SPARE));
    assert!(apply(&mut state, &tx_of("fresh-last", &fresh), K + 3));
    assert!(!state.station_registry.get(&last_id).unwrap().is_active);
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
    assert!(mempool.read().await.contains(&held), "the other stays in the mempool");
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
        assert!(c.state.station_registry.get(&live_id).is_some(), "the live one applied");
        let tip = c.blocks().values().last().unwrap();
        assert!(!tip.transactions.iter().any(|tx| tx.id == duplicate.id), "duplicate not mined");
    }
    assert!(
        !mempool.read().await.contains(duplicate.id.as_ref().unwrap()),
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
    for crate_src in ["shared-protocol/src", "native-chain/src", "tools/cli/WrfcoinCLI/src"] {
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
        RegistrarSelection { defer: vec![1], evict: vec![] }
    );
    // Behind it, the registrar registration takes the last slot.
    assert_eq!(
        select_registrar_registrations(&state, K, &[registrar_tx.clone(), governance_tx]),
        RegistrarSelection::default()
    );
    // Below activation selection never touches anything.
    assert_eq!(
        select_registrar_registrations(&state, K - 1, &[registrar_tx]),
        RegistrarSelection::default()
    );
}
