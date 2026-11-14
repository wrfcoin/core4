//! `station_submitter_key` changes nothing while it is not active, even with
//! `device_submission_limit` and the owner features active.
//!
//! Through the real `State` apply path and the one shared owner-step
//! extractor: owner-signed contribution commits under the limit, a commit
//! signed by another key (refused), a `set_submitter_key` owner action (inert),
//! then report-stolen, unfreeze, deactivate, reactivate and an owner transfer
//! (each would clear an approved key), and a commit by the new owner. The
//! state-root material and the snapshot export must be byte-identical to the
//! ones `origin/main` cd1a82e (before the feature) produces for the same
//! steps; the pinned digests below were taken there.
//!
//! Own test binary: the feature schedule is process-wide, and this file must
//! never schedule `station_submitter_key`.

use sha3::{Digest, Sha3_256};
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::state_snapshot::export_state_entries;
use wrfcoin_shared_protocol::blockchain::station_owner::{
    StationOwnerActionPayload, StationOwnerTransferPayload,
};
use wrfcoin_shared_protocol::blockchain::transaction::{
    CommitPayload, StationOwnerActionKind, Transaction, TransactionInput, TransactionOutput,
    TransactionType,
};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ChainIdentity, ConsensusFeature, FeatureSchedule, feature_active, install_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::station_registry::{
    ContributionProvenanceClass, DataQualityTier, StationLocation, WeatherStation,
};

const T0: i64 = 1_762_646_400;
const STATION: &str = "st-submitter-inactive-1";

/// SHA3-256 of `State::state_root_material()` after the steps below, on
/// origin/main 6ade084.
const ROOT_MATERIAL_SHA3: &str = "26ec7aca41d988c97870c3924b85f04fd80c4e69eed02a33b86d07ef796efb66"; // pragma: allowlist secret -- state digest, not a secret
/// SHA3-256 over the snapshot export (each key and value, length-prefixed), on
/// origin/main 6ade084.
const SNAPSHOT_SHA3: &str = "5fdf36ccbbcea0fe243c15a56f85287302c5903130f3946d5ddc527fb0c57f0d"; // pragma: allowlist secret -- state digest, not a secret

fn key(seed: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[seed; 32]).unwrap()
}

fn addr_of(k: &PrivateKey) -> String {
    PublicKey::from_private_key(k).to_address()
}

fn pk_of(k: &PrivateKey) -> String {
    PublicKey::from_private_key(k).to_hex()
}

fn chain() -> (String, String) {
    let id = ChainIdentity::active();
    (id.chain_id.clone(), id.network_id.clone())
}

fn ledger_tx(tag: &str, data: String) -> Transaction {
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(&format!("input-{tag}")),
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
    tx.tx_type = TransactionType::StationRegistration;
    tx.data = Some(data);
    tx.set_id();
    tx
}

/// Extract through the shared constructor, then apply; whether a transition
/// was extracted.
fn step(state: &mut State, tag: &str, data: String, height: u64) -> bool {
    match ledger_tx(tag, data).station_owner_ledger_transition(height) {
        Some(t) => {
            state.apply_transition(t).expect("an extracted owner step never fails");
            true
        },
        None => false,
    }
}

fn action(owner: &PrivateKey, kind: StationOwnerActionKind, nonce: u64) -> String {
    let (c, n) = chain();
    let mut p = StationOwnerActionPayload::new(STATION, addr_of(owner), pk_of(owner), kind, nonce, 5_000);
    p.sign_with_domain(&c, &n, owner);
    serde_json::to_string(&p).unwrap()
}

/// A `set_submitter_key` action as a phone or the CLI would send it, written
/// out as JSON so this file builds on a binary that has no such action.
fn set_submitter_json(owner: &PrivateKey, submitter: &PrivateKey, nonce: u64) -> String {
    let (c, n) = chain();
    let mut p = StationOwnerActionPayload::new(
        STATION,
        addr_of(owner),
        pk_of(owner),
        StationOwnerActionKind::ReportStolen,
        nonce,
        5_000,
    );
    p.sign_with_domain(&c, &n, owner);
    let mut v = serde_json::to_value(&p).unwrap();
    v["action"] = serde_json::json!("set_submitter_key");
    v["submitter_pubkey"] = serde_json::json!(pk_of(submitter));
    v["submitter_class"] = serde_json::json!("device");
    v.to_string()
}

fn commit(signer: &PrivateKey, tag: &str) -> CommitPayload {
    let data = serde_json::json!({ "temperature_c": 12.5, "tag": tag });
    let nonce = format!("nonce-{tag}");
    let commit_id = Transaction::compute_commit_id(&data, &nonce, STATION, T0, "weather");
    let mut payload = CommitPayload {
        commit_id,
        station_id: STATION.to_string(),
        timestamp: T0,
        contribution_type: "weather".to_string(),
        auth_pubkey: None,
        auth_signature: None,
    };
    payload.sign_authorization(signer);
    payload
}

fn run() -> State {
    install_feature_schedule(
        FeatureSchedule::from_entries(vec![
            ("station_owner_consent".to_string(), 1),
            ("station_owner_actions".to_string(), 1),
            ("device_submission_limit".to_string(), 1),
        ])
        .unwrap(),
    );
    let owner = key(0x11);
    let collect = key(0x22);
    let next_owner = key(0x33);
    let mut state = State::new();
    state
        .register_weather_station(WeatherStation {
            station_id: STATION.to_string(),
            owner_address: addr_of(&owner),
            // A 0.1-degree grid point, not a real station.
            location: StationLocation { latitude: 45.0, longitude: -93.0, elevation_meters: Some(250.0) },
            hardware_model: "test-station".to_string(),
            calibration_date: None,
            is_active: true,
            reputation_score: 50,
            data_quality_tier: DataQualityTier::Gold,
            provenance_class: ContributionProvenanceClass::Real,
            ring_public_key: None,
        })
        .expect("station registers");

    let mut h = 10u64;
    // Owner-signed commit: applies under the limit.
    state
        .apply_transition(StateTransition::RegisterCommit { payload: commit(&owner, "owner-1"), block_height: h })
        .expect("owner-signed commit applies");
    h += 1;
    // Collect-key commit: refused (no approved key exists while inactive).
    assert!(
        state
            .apply_transition(StateTransition::RegisterCommit { payload: commit(&collect, "collect-1"), block_height: h })
            .is_err(),
        "a commit not signed by the owner key is refused"
    );
    h += 1;
    // set_submitter_key: inert (no transition).
    assert!(!step(&mut state, "set-submitter", set_submitter_json(&owner, &collect, 0), h), "set_submitter_key is inert");
    h += 1;
    // The owner steps that clear an approved key; the nonce was not consumed above.
    for (nonce, kind) in [
        StationOwnerActionKind::ReportStolen,
        StationOwnerActionKind::Unfreeze,
        StationOwnerActionKind::Deactivate,
        StationOwnerActionKind::Reactivate,
    ]
    .into_iter()
    .enumerate()
    {
        assert!(step(&mut state, &format!("action-{nonce}"), action(&owner, kind, nonce as u64), h));
        h += 1;
    }
    let (c, n) = chain();
    let mut transfer = StationOwnerTransferPayload::new(
        STATION,
        addr_of(&owner),
        pk_of(&owner),
        addr_of(&next_owner),
        pk_of(&next_owner),
        4,
        5_000,
    );
    transfer.sign_as_owner_with_domain(&c, &n, &owner);
    transfer.countersign_with_domain(&c, &n, &next_owner);
    assert!(step(&mut state, "transfer", serde_json::to_string(&transfer).unwrap(), h));
    h += 1;
    state
        .apply_transition(StateTransition::RegisterCommit { payload: commit(&next_owner, "owner-2"), block_height: h })
        .expect("the new owner's commit applies");
    state
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
fn inactive_submitter_key_leaves_root_and_snapshot_byte_identical() {
    let state = run();
    for height in [0, 10, 100, u64::MAX] {
        assert!(feature_active(ConsensusFeature::DeviceSubmissionLimit, height.max(1)));
    }
    assert_eq!(
        ConsensusFeature::parse("station_submitter_key").map(|f| feature_active(f, 100)),
        ConsensusFeature::parse("station_submitter_key").map(|_| false),
        "never scheduled here"
    );
    let root = hex::encode(Sha3_256::digest(state.state_root_material()));
    let snapshot = snapshot_digest(&state);
    println!("root_material_sha3={root} snapshot_sha3={snapshot}");
    assert_eq!(root, ROOT_MATERIAL_SHA3, "root material differs from origin/main");
    assert_eq!(snapshot, SNAPSHOT_SHA3, "snapshot export differs from origin/main");
}
