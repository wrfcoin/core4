//! The station owner key audit on a V2 copy whose WAL carries an APPLIED
//! schedule extension (review of #184): block 1 pins protocol V2 and enables
//! `schedule_extension` at 2; block 3 applies an extension that schedules
//! `station_owner_consent`; an owner transfer is replayed after that
//! activation. Every block commits its post-state root (V2), so the audit's
//! tip-commitment check is real: the honest copy counts the tip (new owner),
//! a copy whose tip commitment was tampered with is refused. A plain
//! `load_state` (no schedule, no overlay) fails on the transfer.
//! Own process: protocol version, schedule and authority are process-wide.

use std::path::Path;

use rand::SeedableRng;
use rand::rngs::StdRng;
use wrfcoin_native_chain::station_owner_audit::run_audit;
use wrfcoin_native_chain::storage::Storage;
use wrfcoin_shared_protocol::blockchain::block::{Block, STATE_ROOT_METADATA_KEY};
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::station_owner::StationOwnerTransferPayload;
use wrfcoin_shared_protocol::blockchain::transaction::{Transaction, TransactionInput, TransactionOutput, TransactionType};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, GENESIS_ACTIVATION_SCHEDULE_KEY, GENESIS_PROTOCOL_VERSION_KEY,
    SigningProtocolVersion, install_activation_table, install_feature_schedule,
    refresh_feature_schedule_extensions, resolve_activation_schedule, resolve_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::hash::{Hash, hash_string};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::governance::schedule_extension::{
    FeatureScheduleExtension, MIN_RUNUP_BLOCKS, SCHEDULE_EXTENSION_FEATURE, ScheduleEntry,
    SignedFeatureScheduleExtension, install_schedule_extension_authority_for_tests,
    sign_feature_schedule_extension_with_key,
};
use wrfcoin_shared_protocol::station_registry::{
    ContributionProvenanceClass, DataQualityTier, StationLocation, WeatherStation,
};

const GENESIS: &str = "abababababababababababababababababababababababababababababababab";
const H_ENABLE: u64 = 2;
const CARRY: u64 = 3;
const CONSENT_AT: u64 = CARRY + MIN_RUNUP_BLOCKS;
const TRANSFER_AT: u64 = CONSENT_AT + 1;
const TIP: u64 = TRANSFER_AT + 1;

fn key(seed: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[seed; 32]).unwrap()
}
fn addr(k: &PrivateKey) -> String {
    PublicKey::from_private_key(k).to_address()
}
fn governance() -> PrivateKey {
    PrivateKey::generate(&mut StdRng::seed_from_u64(3565))
}
fn schedule_raw() -> String {
    format!("0:v2,{H_ENABLE}:{SCHEDULE_EXTENSION_FEATURE}")
}

/// What the node's boot gate installs from block 1, plus the authority (first install wins).
fn install_gate() {
    let raw = schedule_raw();
    let pairs = vec![(GENESIS_PROTOCOL_VERSION_KEY, "2"), (GENESIS_ACTIVATION_SCHEDULE_KEY, raw.as_str())];
    let table = resolve_activation_schedule(pairs.clone(), SigningProtocolVersion::V2).expect("table");
    let features = resolve_feature_schedule(pairs, &table).expect("features");
    SigningProtocolVersion::install_active(SigningProtocolVersion::V2);
    install_activation_table(table);
    install_feature_schedule(features);
    install_schedule_extension_authority_for_tests("777", "testnet", GENESIS, vec![
        PublicKey::from_private_key(&governance()).to_hex().to_ascii_lowercase(),
    ]);
}

fn consent_extension() -> SignedFeatureScheduleExtension {
    let extension = FeatureScheduleExtension {
        chain_id: "777".into(),
        network_id: "testnet".into(),
        genesis_hash: GENESIS.into(),
        sequence: 0,
        entries: vec![ScheduleEntry::new(ConsensusFeature::StationOwnerConsent.name(), CONSENT_AT)],
        version_steps: Vec::new(),
        min_protocol_version: 3,
        release_commit: "3565".into(),
    };
    let signatures = vec![sign_feature_schedule_extension_with_key(&extension, &governance()).expect("signs")];
    SignedFeatureScheduleExtension { extension, signatures }
}

fn station(owner: &str) -> WeatherStation {
    WeatherStation {
        station_id: "s-1".into(),
        owner_address: owner.into(),
        // A 0.1-degree grid point, not a real station.
        location: StationLocation { latitude: 45.0, longitude: -93.0, elevation_meters: None },
        hardware_model: "fixture".into(),
        calibration_date: None,
        is_active: true,
        reputation_score: 50,
        data_quality_tier: DataQualityTier::Gold,
        provenance_class: ContributionProvenanceClass::Real,
        ring_public_key: None,
    }
}

fn genesis_state(a: &PrivateKey) -> State {
    let mut state = State::new();
    state.register_weather_station(station(&addr(a))).unwrap();
    state.recalculate_state_root();
    state
}

/// Persist the chain as the node does (apply, finalize, commit the root,
/// refresh the effective schedule). `tamper_tip` writes a wrong tip commitment.
async fn persist(path: &Path, tamper_tip: bool) {
    let (a, b) = (key(1), key(2));
    let mut transfer = StationOwnerTransferPayload::new("s-1", addr(&a), PublicKey::from_private_key(&a).to_hex(), addr(&b), PublicKey::from_private_key(&b).to_hex(), 0, 50_000);
    transfer.sign_as_owner_with_domain("777", "testnet", &a);
    transfer.countersign_with_domain("777", "testnet", &b);
    let mut transfer_tx = Transaction::new(
        vec![TransactionInput { txid: hash_string("in-t"), vout: 0, signature: None, public_key: None, coinbase_data: None }],
        vec![TransactionOutput { amount: 1, address: "wrf-t".into(), script: "OP_TRUE".into(), nft_data: None }],
    );
    transfer_tx.tx_type = TransactionType::StationRegistration;
    transfer_tx.data = Some(serde_json::to_string(&transfer).unwrap());
    transfer_tx.set_id();

    let mut storage = Storage::open(path, u64::MAX).expect("open");
    let genesis = genesis_state(&a);
    storage.persist_genesis_state(&genesis).await.expect("anchor");
    let mut state = genesis;
    refresh_feature_schedule_extensions(Vec::new());
    state.refresh_effective_feature_schedule();
    let mut prev = Hash::default();
    for h in 1..=TIP {
        let transitions = match h {
            CARRY => vec![StateTransition::ApplyFeatureScheduleExtension { extension: consent_extension(), block_height: h }],
            TRANSFER_AT => vec![StateTransition::ApplyStationOwnerTransfer {
                station_id: "s-1".into(),
                from_owner_address: addr(&a),
                to_owner_address: addr(&b),
                transfer_nonce: 0,
                block_height: h,
            }],
            _ => Vec::new(),
        };
        let txs = if h == TRANSFER_AT { vec![transfer_tx.clone()] } else { vec![] };
        let mut block = Block::new(1, prev.clone(), txs, 1, h);
        if h == 1 {
            block.metadata.insert(GENESIS_PROTOCOL_VERSION_KEY.to_string(), "2".to_string());
            block.metadata.insert(GENESIS_ACTIVATION_SCHEDULE_KEY.to_string(), schedule_raw());
        }
        state
            .apply_transitions_batch_at_height(h, transitions.clone().into_iter())
            .unwrap_or_else(|e| panic!("producer applies {h}: {e}"));
        state.apply_block_finalization(h);
        let committed = if tamper_tip && h == TIP { Hash::default().to_string() } else { state.state_root.to_string() };
        block.metadata.insert(STATE_ROOT_METADATA_KEY.to_string(), committed);
        let hash = block.calculate_hash();
        block.set_hash(hash.clone());
        prev = hash;
        storage.persist_block(block, transitions, None).await.expect("persist");
        state.refresh_effective_feature_schedule();
    }
    assert_eq!(state.station_registry.get("s-1").unwrap().owner_address, addr(&b), "fixture: the transfer applied");
}

#[tokio::test]
async fn v2_copy_with_an_applied_extension_counts_the_verified_tip_and_refuses_a_bad_commitment() {
    install_gate();
    let dir = tempfile::tempdir().unwrap();
    let honest = dir.path().join("honest.redb");
    let tampered = dir.path().join("tampered.redb");
    persist(&honest, false).await;
    persist(&tampered, true).await;

    // A fresh view: no overlay. Plain load_state replays the transfer before
    // the extension's activation is known and refuses it.
    refresh_feature_schedule_extensions(Vec::new());
    assert!(Storage::open(&honest, u64::MAX).unwrap().load_state().is_err(), "plain load_state fails without the overlay");

    refresh_feature_schedule_extensions(Vec::new());
    let report = run_audit(&honest, 2_000).expect("the honest V2 copy is audited");
    assert_eq!(report.tip, TIP);
    assert!(report.commitment_compared, "V2: the tip commitment was compared");
    let counts = report.counts;
    assert_eq!((counts.stations, counts.owners), (1, 1));
    // The tip owner B showed its key in the transfer: it can sign.
    assert_eq!((counts.owners_with_ed25519_key, counts.owners_passkey_or_unknown), (1, 0));

    refresh_feature_schedule_extensions(Vec::new());
    let err = run_audit(&tampered, 2_000).expect_err("a wrong tip commitment is refused");
    assert!(err.contains("tip commitment"), "{err}");
}
