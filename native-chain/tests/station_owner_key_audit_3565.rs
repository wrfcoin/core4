//! The station owner key audit on a copied DB whose WAL tail crosses a feature
//! activation (review of #184): a checkpoint at height 2, `station_owner_consent`
//! active from 3, an owner TRANSFER replayed from the WAL at 4, and a
//! collector-signed commit at 5. A plain `load_state` (no schedule) fails on
//! the transfer; `run_audit` replays under block 1's schedule, verifies the
//! reconstruction and counts from the TIP state (the station has its new owner).
//! Own process: the feature schedule is process-wide.

use std::path::PathBuf;

use wrfcoin_native_chain::station_owner_audit::{AuditCounts, run_audit};
use wrfcoin_native_chain::storage::Storage;
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::station_owner::StationOwnerTransferPayload;
use wrfcoin_shared_protocol::blockchain::transaction::{
    CommitPayload, Transaction, TransactionInput, TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, GENESIS_ACTIVATION_SCHEDULE_KEY, try_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::hash::{Hash, hash_string};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::station_registry::{
    ContributionProvenanceClass, DataQualityTier, StationLocation, WeatherStation,
};

const ACTIVATION: u64 = 3;
const CHECKPOINT: u64 = 2;
const TRANSFER_AT: u64 = 4;
const COMMIT_AT: u64 = 5;
const TIP: u64 = 6;

fn key(seed: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[seed; 32]).unwrap()
}
fn addr(k: &PrivateKey) -> String {
    PublicKey::from_private_key(k).to_address()
}

fn station(id: &str, owner: &str) -> WeatherStation {
    WeatherStation {
        station_id: id.into(),
        owner_address: owner.into(),
        // 0.1-degree grid points, not real stations.
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

fn genesis_state(a: &PrivateKey, passkey_owner: &str) -> State {
    let mut state = State::new();
    state.register_weather_station(station("s-wallet", &addr(a))).unwrap();
    state.register_weather_station(station("s-passkey", passkey_owner)).unwrap();
    state.recalculate_state_root();
    state
}

fn tx(tx_type: TransactionType, tag: &str, data: String) -> Transaction {
    let mut tx = Transaction::new(
        vec![TransactionInput { txid: hash_string(&format!("in-{tag}")), vout: 0, signature: None, public_key: None, coinbase_data: None }],
        vec![TransactionOutput { amount: 1, address: format!("wrf-{tag}"), script: "OP_TRUE".into(), nft_data: None }],
    );
    tx.tx_type = tx_type;
    tx.data = Some(data);
    tx.set_id();
    tx
}

#[tokio::test]
async fn audit_replays_a_wal_tail_across_an_activation_and_counts_the_verified_tip() {
    assert!(try_feature_schedule().is_none(), "this file owns its process");
    let (a, b, collector) = (key(1), key(2), key(3));
    // A passkey-style owner: an address no ed25519 key on chain derives to.
    let passkey_owner = format!("wrf{}", "5a".repeat(20));
    let dir = tempfile::tempdir().unwrap();
    let path: PathBuf = dir.path().join("copy.redb");

    let mut transfer = StationOwnerTransferPayload::new("s-wallet", addr(&a), PublicKey::from_private_key(&a).to_hex(), addr(&b), PublicKey::from_private_key(&b).to_hex(), 0, 5_000);
    transfer.sign_as_owner_with_domain("c", "n", &a);
    transfer.countersign_with_domain("c", "n", &b);
    let transfer_tx = tx(TransactionType::StationRegistration, "transfer", serde_json::to_string(&transfer).unwrap());
    let mut commit = CommitPayload {
        commit_id: hash_string("c1").to_hex(),
        station_id: "s-passkey".into(),
        timestamp: 1,
        contribution_type: "weather".into(),
        auth_pubkey: None,
        auth_signature: None,
    };
    commit.sign_authorization(&collector);
    let commit_tx = tx(TransactionType::ContributionCommit, "commit", serde_json::to_string(&commit).unwrap());

    let transitions_at = |h: u64| -> Vec<StateTransition> {
        if h == TRANSFER_AT {
            vec![StateTransition::ApplyStationOwnerTransfer {
                station_id: "s-wallet".into(),
                from_owner_address: addr(&a),
                to_owner_address: addr(&b),
                transfer_nonce: 0,
                block_height: h,
            }]
        } else {
            Vec::new()
        }
    };
    let mut prev = Hash::default();
    let mut next_block = |h: u64| {
        let txs = match h {
            TRANSFER_AT => vec![transfer_tx.clone()],
            COMMIT_AT => vec![commit_tx.clone()],
            _ => vec![],
        };
        let mut block = Block::new(1, prev.clone(), txs, 1, h);
        if h == 1 {
            block.metadata.insert(
                GENESIS_ACTIVATION_SCHEDULE_KEY.to_string(),
                format!("0:v1,{ACTIVATION}:{}", ConsensusFeature::StationOwnerConsent.name()),
            );
        }
        let hash = block.calculate_hash();
        block.set_hash(hash.clone());
        prev = hash;
        block
    };
    {
        let mut storage = Storage::open(&path, CHECKPOINT).unwrap();
        storage.persist_genesis_state(&genesis_state(&a, &passkey_owner)).await.unwrap();
        let mut state = genesis_state(&a, &passkey_owner);
        for h in 1..=CHECKPOINT {
            state.apply_transitions_batch_at_height(h, transitions_at(h)).unwrap();
            state.apply_block_finalization(h);
            let checkpoint = storage.checkpoint_state_bytes(h, &state).unwrap();
            storage.persist_block(next_block(h), transitions_at(h), checkpoint).await.unwrap();
        }
    }
    {
        let mut storage = Storage::open(&path, 100).unwrap();
        for h in CHECKPOINT + 1..=TIP {
            storage.persist_block(next_block(h), transitions_at(h), None).await.unwrap();
        }
    }
    // The old way: replay with no schedule refuses the post-activation transfer.
    assert!(Storage::open(&path, 100).unwrap().load_state().is_err(), "plain load_state fails on the WAL transfer");

    let report = run_audit(&path, 2_000).expect("the audit replays under block 1's schedule");
    assert_eq!(report.tip, TIP);
    assert!(!report.commitment_compared, "a V1 copy commits no tip root; the report says so");
    let counts = report.counts;
    assert_eq!(
        counts,
        AuditCounts {
            stations: 2,
            active_stations: 2,
            // The tip owners: B (after the replayed transfer) and the passkey owner.
            owners: 2,
            owners_with_ed25519_key: 1,
            owners_passkey_or_unknown: 1,
            active_stations_of_passkey_or_unknown_owners: 1,
            stations_with_recent_commits: 1,
            stations_with_recent_non_owner_commits: 1,
            of_which_owner_cannot_sign: 1,
        }
    );
}
