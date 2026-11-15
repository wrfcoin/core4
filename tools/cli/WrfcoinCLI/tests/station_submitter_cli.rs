//! `wrfcoin station set-submitter` / `clear-submitter` end to end
//! (`station_submitter_key`, #3565 design 1): run the real binary and check
//! that each artifact is admitted by core4's node-side precheck and extracts
//! to the transition that approves (or clears) exactly the requested key.
//! Covers the `house` hook (`WRFCOIN_HOUSE_RELAY_PUBKEY`, a PUBLIC key).
//! One test: it installs this process's feature schedule.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{SubmitterClass, Transaction};
use wrfcoin_shared_protocol::crypto::chain_binding::{FeatureSchedule, install_feature_schedule};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::station_registry::{
    ContributionProvenanceClass, DataQualityTier, StationLocation, WeatherStation,
};

const CHAIN: &str = "wrfcoin-cli-submitter-test";
const NETWORK: &str = "testnet";
const STATION: &str = "ws-cli-submitter";
const HEIGHT: u64 = 1_000;
const NOW: i64 = 1_700_000_000;

fn key(seed: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[seed; 32]).expect("32-byte ed25519 seed")
}
fn pk_hex(k: &PrivateKey) -> String {
    PublicKey::from_private_key(k).to_hex()
}

fn wrfcoin(args: &[&str], house: Option<&str>) -> (bool, Value, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_wrfcoin"));
    cmd.arg("--output").arg("json").args(args).env_remove("WRFCOIN_HOUSE_RELAY_PUBKEY");
    if let Some(h) = house {
        cmd.env("WRFCOIN_HOUSE_RELAY_PUBKEY", h);
    }
    let out = cmd.output().expect("run wrfcoin");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let data = serde_json::from_slice::<Value>(&out.stdout).map(|v| v["data"].clone()).unwrap_or(Value::Null);
    (out.status.success(), data, text)
}

fn tx_of(artifact: &Path) -> Transaction {
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(artifact).unwrap()).unwrap();
    let tx: Transaction = serde_json::from_value(doc["transactions"][0].clone()).expect("a Transaction");
    assert_eq!(tx.id, Some(tx.calculate_hash()));
    assert!(tx.is_canonical_station_owner_envelope());
    tx
}

fn state_with_station(owner: &PrivateKey) -> State {
    let mut state = State::new();
    state
        .register_weather_station(WeatherStation {
            station_id: STATION.to_string(),
            owner_address: PublicKey::from_private_key(owner).to_address(),
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
        .unwrap();
    state
}

#[test]
fn set_and_clear_submitter_build_artifacts_the_node_admits() {
    install_feature_schedule(
        FeatureSchedule::from_entries(vec![
            ("station_owner_actions".to_string(), 1),
            ("station_submitter_key".to_string(), 1),
        ])
        .unwrap(),
    );
    let (owner, phone, house) = (key(0xA1), key(0xB2), key(0xC3));
    let dir = std::env::temp_dir().join(format!("wrfcoin-cli-submitter-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let owner_key: PathBuf = dir.join("owner.key");
    std::fs::write(&owner_key, owner.to_hex()).unwrap();
    let mut state = state_with_station(&owner);
    let exp = (HEIGHT + 500).to_string();
    let now = NOW.to_string();
    let base = |cmd: &str, nonce: u64, out: &Path| -> Vec<String> {
        [
            "station", cmd, "--station-id", STATION, "--owner-key", owner_key.to_str().unwrap(), "--nonce",
            &nonce.to_string(), "--expires-at-height", &exp, "--chain-id", CHAIN, "--network-id", NETWORK,
            "--submitted-at", &now, "--out", out.to_str().unwrap(),
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    };
    let approve = |state: &mut State, artifact: &Path, nonce: u64| -> Option<(String, SubmitterClass)> {
        let tx = tx_of(artifact);
        let step = state.precheck_station_owner_tx(&tx, HEIGHT, CHAIN, NETWORK).expect("the node admits the artifact");
        assert_eq!((step.kind, step.nonce), ("station_owner_action_v1", nonce));
        // The extractor binds the RUNTIME chain identity; this test signs for
        // CHAIN, so apply the transition the payload describes directly.
        let doc: Value = serde_json::from_str(tx.data.as_deref().unwrap()).unwrap();
        let submitter = doc["submitter_pubkey"].as_str().map(|k| {
            (k.to_string(), serde_json::from_value(doc["submitter_class"].clone()).unwrap())
        });
        state
            .apply_transition(StateTransition::ApplyStationSubmitterKey {
                station_id: STATION.into(),
                owner_address: PublicKey::from_private_key(&owner).to_address(),
                action_nonce: nonce,
                submitter: submitter.clone(),
                block_height: HEIGHT,
            })
            .unwrap();
        submitter
    };

    // A phone collect key, class device.
    let out = dir.join("set-phone.json");
    let mut args = base("set-submitter", 0, &out);
    args.extend(["--submitter-key".into(), pk_hex(&phone), "--class".into(), "device".into()]);
    let (ok, summary, text) = wrfcoin(&args.iter().map(String::as_str).collect::<Vec<_>>(), None);
    assert!(ok, "{text}");
    assert_eq!((summary["action"].as_str(), summary["submitter_class"].as_str()), (Some("set_submitter_key"), Some("device")));
    assert_eq!(approve(&mut state, &out, 0), Some((pk_hex(&phone), SubmitterClass::Device)));
    assert_eq!(state.station_submitter(STATION).unwrap().pubkey, pk_hex(&phone));

    // The house hook: refused while unset, relay class from the env key.
    let out = dir.join("set-house.json");
    let mut args = base("set-submitter", 1, &out);
    args.extend(["--submitter-key".into(), "house".into()]);
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    let (ok, _, text) = wrfcoin(&argv, None);
    assert!(!ok && text.contains("WRFCOIN_HOUSE_RELAY_PUBKEY is not set"), "{text}");
    let (ok, _, text) = wrfcoin(&argv, Some("not-a-key"));
    assert!(!ok && text.contains("not a lowercase ed25519"), "{text}");
    let (ok, summary, text) = wrfcoin(&argv, Some(&pk_hex(&house)));
    assert!(ok, "{text}");
    assert_eq!(summary["submitter_class"], "relay");
    assert_eq!(approve(&mut state, &out, 1), Some((pk_hex(&house), SubmitterClass::Relay)));
    let mut house_device = args.clone();
    house_device.extend(["--class".into(), "device".into()]);
    let (ok, _, text) = wrfcoin(&house_device.iter().map(String::as_str).collect::<Vec<_>>(), Some(&pk_hex(&house)));
    assert!(!ok && text.contains("always --class relay"), "{text}");

    // Bad explicit keys and a missing class are refused before signing.
    for (key_arg, class) in [(pk_hex(&phone).to_uppercase(), Some("device")), ("ab".repeat(31), Some("relay")), (pk_hex(&phone), None)] {
        let out = dir.join("bad.json");
        let mut args = base("set-submitter", 2, &out);
        args.extend(["--submitter-key".into(), key_arg]);
        if let Some(c) = class {
            args.extend(["--class".into(), c.into()]);
        }
        let (ok, _, text) = wrfcoin(&args.iter().map(String::as_str).collect::<Vec<_>>(), None);
        assert!(!ok, "{text}");
    }
    // The owner's own key is not a submitter key.
    let out = dir.join("self.json");
    let mut args = base("set-submitter", 2, &out);
    args.extend(["--submitter-key".into(), pk_hex(&owner), "--class".into(), "device".into()]);
    let (ok, _, text) = wrfcoin(&args.iter().map(String::as_str).collect::<Vec<_>>(), None);
    assert!(!ok && text.contains("owner key already signs"), "{text}");

    // Clear.
    let out = dir.join("clear.json");
    let args = base("clear-submitter", 2, &out);
    let (ok, summary, text) = wrfcoin(&args.iter().map(String::as_str).collect::<Vec<_>>(), None);
    assert!(ok, "{text}");
    assert!(summary["submitter_pubkey"].is_null());
    assert_eq!(approve(&mut state, &out, 2), None);
    assert!(state.station_submitter(STATION).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
