//! With every flag of this stack OFF (`late_data_pay`, `station_submitter_key`,
//! `device_submission_limit`) and only the existing owner features on, the
//! stack changes nothing, byte for byte.
//!
//! Through the real `State` apply path and the one shared owner-step
//! extractor: contribution commits by the owner and by another key, a
//! `set_submitter_key` owner action (inert), the four older owner actions
//! carrying ODD extra JSON (an unknown `submitter_class` string, a numeric
//! `submitter_pubkey`, the new field names duplicated), which must still parse
//! and apply exactly as before (unknown fields were always ignored), a
//! governance re-registration that changes the owner, an owner transfer and a
//! commit by the new owner. The state-root material and the snapshot export
//! must be byte-identical to the ones `origin/main` (before the stack)
//! produces for the same steps; the pinned digests below were taken there.
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
const ROOT_MATERIAL_SHA3: &str = "0452b9008a7bd9b8b1736d286e3a3fe8b1fd8f4bd1965a1deb41637a1cfbf0f5"; // pragma: allowlist secret -- state digest, not a secret
/// SHA3-256 over the snapshot export (each key and value, length-prefixed), on
/// origin/main 6ade084.
const SNAPSHOT_SHA3: &str = "5ea77be8d5e80e8be4342203332b098bc683c6745021aa7f7e2b77aa54bf38fc"; // pragma: allowlist secret -- state digest, not a secret

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

/// A legacy action whose JSON also carries odd values under the new field
/// names (variant 0: unknown class string, 1: numeric key, 2: both names
/// twice). On a binary without the feature these are unknown fields.
fn odd_action(owner: &PrivateKey, kind: StationOwnerActionKind, nonce: u64, variant: usize) -> String {
    let clean = action(owner, kind, nonce);
    let body = clean.trim_end_matches('}');
    match variant {
        0 => format!(r#"{body},"submitter_class":"future_class"}}"#),
        1 => format!(r#"{body},"submitter_pubkey":17}}"#),
        _ => format!(
            r#"{body},"submitter_pubkey":"zz","submitter_class":"device","submitter_pubkey":[1],"submitter_class":null}}"#
        ),
    }
}

fn reregistration(new_owner: &PrivateKey) -> WeatherStation {
    WeatherStation { owner_address: addr_of(new_owner), ..station(&key(0x11)) }
}

fn station(owner: &PrivateKey) -> WeatherStation {
    WeatherStation {
        station_id: STATION.to_string(),
        owner_address: addr_of(owner),
        // A 0.1-degree grid point, not a real station.
        location: StationLocation { latitude: 45.0, longitude: -93.0, elevation_meters: Some(250.0) },
        hardware_model: "test-station".to_string(),
        calibration_date: None,
        is_active: true,
        reputation_score: 50,
        data_quality_tier: DataQualityTier::Gold,
        provenance_class: ContributionProvenanceClass::Real,
        ring_public_key: None,
    }
}

fn run() -> State {
    install_feature_schedule(
        FeatureSchedule::from_entries(vec![
            ("station_owner_consent".to_string(), 1),
            ("station_owner_actions".to_string(), 1),
        ])
        .unwrap(),
    );
    let owner = key(0x11);
    let collect = key(0x22);
    let gov_owner = key(0x44);
    let next_owner = key(0x33);
    let mut state = State::new();
    state.register_weather_station(station(&owner)).expect("station registers");

    let mut h = 10u64;
    // Contribution commits: the limit is off, so any signing key applies.
    for (signer, tag) in [(&owner, "owner-1"), (&collect, "collect-1")] {
        state
            .apply_transition(StateTransition::RegisterCommit { payload: commit(signer, tag), block_height: h })
            .expect("commit applies");
        h += 1;
    }
    // set_submitter_key: inert (no transition).
    assert!(!step(&mut state, "set-submitter", set_submitter_json(&owner, &collect, 0), h), "set_submitter_key is inert");
    h += 1;
    // The four older actions with odd extra JSON: each parses and applies.
    let mut nonce = 0u64;
    for variant in 0..3 {
        for kind in [
            StationOwnerActionKind::ReportStolen,
            StationOwnerActionKind::Unfreeze,
            StationOwnerActionKind::Deactivate,
            StationOwnerActionKind::Reactivate,
        ] {
            let tx = ledger_tx(&format!("odd-{variant}-{nonce}"), odd_action(&owner, kind, nonce, variant));
            let t = tx.station_owner_ledger_transition(h).unwrap_or_else(|| panic!("{kind:?} variant {variant} extracts"));
            assert!(matches!(t, StateTransition::ApplyStationOwnerAction { action, .. } if action == kind));
            state.apply_transition(t).expect("applies");
            nonce += 1;
            h += 1;
        }
    }
    assert_eq!(state.station_owner_ledger.get(STATION).map(|e| e.owner_nonce), Some(nonce), "all twelve applied");
    // A governance re-registration that changes the owner (direct apply).
    state.apply_station_registration(reregistration(&gov_owner)).expect("re-registration applies");
    h += 1;
    // An owner transfer from the re-registered owner, then a commit.
    let (c, n) = chain();
    let mut transfer = StationOwnerTransferPayload::new(
        STATION,
        addr_of(&gov_owner),
        pk_of(&gov_owner),
        addr_of(&next_owner),
        pk_of(&next_owner),
        nonce,
        5_000,
    );
    transfer.sign_as_owner_with_domain(&c, &n, &gov_owner);
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
fn every_stack_flag_off_leaves_root_and_snapshot_byte_identical() {
    let state = run();
    for name in ["late_data_pay", "station_submitter_key", "device_submission_limit"] {
        assert!(
            ConsensusFeature::parse(name).is_none_or(|f| !feature_active(f, 100)),
            "{name} is never scheduled here"
        );
    }
    let root = hex::encode(Sha3_256::digest(state.state_root_material()));
    let snapshot = snapshot_digest(&state);
    println!("root_material_sha3={root} snapshot_sha3={snapshot}");
    assert_eq!(root, ROOT_MATERIAL_SHA3, "root material differs from origin/main");
    assert_eq!(snapshot, SNAPSHOT_SHA3, "snapshot export differs from origin/main");
}
