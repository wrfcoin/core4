//! The lead rule (no 1 h lead; the 24 h lead's reference from a 48 h window)
//! changes nothing while `forecast_late_score` is not in force.
//!
//! Forecasts at every lead the old rule took (1, 6, 12 and 24 h) and one it
//! never took (3 h), for both scored variables, are committed and revealed
//! through the real `State` apply path with the feature unscheduled. The
//! state-root material and the snapshot export must be byte-identical to
//! the ones `origin/main` cd1a82e (before this rule) produced for the same
//! transitions; the pinned digests below were taken there.
//!
//! Own test binary: the feature schedule is process-wide, and this file must
//! never schedule it.

use sha3::{Digest, Sha3_256};
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::state_snapshot::export_state_entries;
use wrfcoin_shared_protocol::blockchain::transaction::{
    CommitPayload, ForecastArtifactManifest, RevealPayload, Transaction,
};
use wrfcoin_shared_protocol::crypto::chain_binding::forecast_late_score_active;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::station_registry::{
    ContributionProvenanceClass, DataQualityTier, StationLocation, WeatherStation,
};

const T0: i64 = 1_762_646_400;
const STATION: &str = "st-lead-rule-1";
const ROOT_TAG: &[u8] = b"wrfcoin_forecast_late_score_v1";

/// SHA3-256 of `State::state_root_material()` after the transitions below,
/// on origin/main cd1a82e.
const ROOT_MATERIAL_SHA3: &str = "242eef6038e230065f949b831f8748ff2478d926d638a58ddfdfc3c471e5b9d3";
/// SHA3-256 over the snapshot export (each key and value, length-prefixed),
/// on origin/main cd1a82e.
const SNAPSHOT_SHA3: &str = "449ae8cf12c747ed304495a4595f0e9840e8a6dff0ff88624ea0cb82ab69162b";

fn key(seed: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[seed; 32]).unwrap()
}

fn forecast(signer: &PrivateKey, data: serde_json::Value, lead_hours: u32, tag: &str) -> (CommitPayload, RevealPayload) {
    let nonce = format!("nonce-{tag}");
    let commit_id = Transaction::compute_commit_id(&data, &nonce, STATION, T0, "forecast");
    let mut commit = CommitPayload {
        commit_id: commit_id.clone(),
        station_id: STATION.to_string(),
        timestamp: T0,
        contribution_type: "forecast".to_string(),
        auth_pubkey: None,
        auth_signature: None,
    };
    commit.sign_authorization(signer);
    let reveal = RevealPayload {
        commit_id,
        station_id: STATION.to_string(),
        timestamp: T0,
        contribution_type: "forecast".to_string(),
        data,
        nonce,
        data_cid: None,
        data_hash: None,
        storage_provider: None,
        provenance: None,
        manifest: Some(ForecastArtifactManifest {
            schema_version: 1,
            model_id: "test-model".to_string(),
            input_hash: "11".repeat(32),
            output_hash: "22".repeat(32),
            produced_at: T0,
            region: STATION.to_string(),
            horizon_hours: lead_hours,
            input_variables: vec!["temp_c".to_string()],
            output_variables: vec!["temp_c".to_string()],
        }),
        verification_receipt: None,
        compute_receipt: None,
        quality_tier: None,
    };
    (commit, reveal)
}

fn run() -> State {
    let mut state = State::new();
    let owner = Address::from_public_key(&PublicKey::from_private_key(&key(0x01)));
    state
        .register_weather_station(WeatherStation {
            station_id: STATION.to_string(),
            owner_address: owner.to_string(),
            // A 0.1-degree grid point, not a real station.
            location: StationLocation {
                latitude: 45.0,
                longitude: -93.0,
                elevation_meters: Some(250.0),
            },
            hardware_model: "test-station".to_string(),
            calibration_date: None,
            is_active: true,
            reputation_score: 50,
            data_quality_tier: DataQualityTier::Gold,
            provenance_class: ContributionProvenanceClass::Real,
            ring_public_key: None,
        })
        .expect("station registers");
    let mut height = 100u64;
    let mut seed = 0x40u8;
    for lead in [1u32, 3, 6, 12, 24] {
        for data in [
            serde_json::json!({ "temp_c_centi": 2_000 + lead as i64 }),
            serde_json::json!({ "pressure_pa": 101_300 + lead as i64 }),
        ] {
            let tag = format!("l{lead}-{seed}");
            let (commit, reveal) = forecast(&key(seed), data, lead, &tag);
            state
                .apply_transition(StateTransition::RegisterCommit {
                    payload: commit,
                    block_height: height,
                })
                .expect("commit applies");
            state
                .apply_transition(StateTransition::ApplyReveal {
                    payload: reveal,
                    block_height: height + 1,
                    block_timestamp: T0 + 77,
                })
                .expect("reveal applies");
            height += 2;
            seed += 1;
        }
    }
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
fn inactive_lead_rule_leaves_root_and_snapshot_byte_identical() {
    for height in [0, 100, 2_010, u64::MAX] {
        assert!(!forecast_late_score_active(height), "never scheduled here");
    }
    let state = run();
    assert!(state.forecast_late_score.is_empty(), "nothing is written while inactive");
    let material = state.state_root_material();
    assert!(
        !material.windows(ROOT_TAG.len()).any(|w| w == ROOT_TAG),
        "no late-score section in the root"
    );
    let root = hex::encode(Sha3_256::digest(&material));
    let snapshot = snapshot_digest(&state);
    // Deterministic: a second run gives the same bytes.
    let again = run();
    assert_eq!(hex::encode(Sha3_256::digest(again.state_root_material())), root);
    assert_eq!(snapshot_digest(&again), snapshot);
    println!("root_material_sha3={root} snapshot_sha3={snapshot}");
    assert_eq!(root, ROOT_MATERIAL_SHA3, "root material differs from origin/main");
    assert_eq!(snapshot, SNAPSHOT_SHA3, "snapshot export differs from origin/main");
}
