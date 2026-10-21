//! `wrfcoin station <owner-action>` end to end (#3345 / #3347): run the real
//! binary for every owner command and check that each artifact's transaction is
//! exactly what core4's node-side admission check
//! (`State::precheck_station_owner_tx_with_features`) ADMITS, against a state
//! that holds the station. One test: it sets the governance allowlist for this
//! process, so it owns it.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use wrfcoin_shared_protocol::blockchain::state::State;
use wrfcoin_shared_protocol::blockchain::transaction::{StationRegistrationPayload, Transaction};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::station_registry::{ContributionProvenanceClass, DataQualityTier};

const CHAIN: &str = "wrfcoin-cli-owner-test";
const NETWORK: &str = "testnet";
const STATION: &str = "ws-cli-owner";
const HEIGHT: u64 = 1_000;
const NOW: i64 = 1_700_000_000;

fn key(seed: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[seed; 32]).expect("32-byte ed25519 seed")
}
fn pk_hex(k: &PrivateKey) -> String {
    PublicKey::from_private_key(k).to_hex()
}
fn addr(k: &PrivateKey) -> String {
    PublicKey::from_private_key(k).to_address()
}

fn run(args: &[&str]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_wrfcoin"))
        .arg("--output")
        .arg("json")
        .args(args)
        .output()
        .expect("run wrfcoin");
    assert!(
        out.status.success(),
        "wrfcoin {args:?} failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let envelope: Value = serde_json::from_slice(&out.stdout).expect("JSON envelope");
    assert_eq!(envelope["ok"], true, "{envelope}");
    envelope["data"].clone()
}

fn run_expect_failure(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_wrfcoin"))
        .arg("--output")
        .arg("json")
        .args(args)
        .output()
        .expect("run wrfcoin");
    assert!(!out.status.success(), "wrfcoin {args:?} should have failed");
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn write_key(dir: &Path, name: &str, k: &PrivateKey) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, k.to_hex()).unwrap();
    p
}

fn state_with_station(owner: &PrivateKey, governance: &PrivateKey) -> State {
    let mut payload = StationRegistrationPayload::new(
        STATION,
        addr(owner),
        46.7867,
        -92.1005,
        "test-real-station",
        DataQualityTier::Gold,
        ContributionProvenanceClass::Real,
    );
    payload.sign_ownership(owner);
    payload.sign(governance);
    let mut state = State::new();
    state
        .apply_station_registration(Transaction::station_from_registration(&payload))
        .unwrap();
    state
}

fn tx_of(artifact: &Path) -> Transaction {
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(artifact).unwrap()).unwrap();
    assert_eq!(doc["version"], "station_owner_artifact_v1");
    let txs = doc["transactions"].as_array().expect("transactions");
    assert_eq!(txs.len(), 1);
    let tx: Transaction = serde_json::from_value(txs[0].clone()).expect("a Transaction");
    assert_eq!(tx.id, Some(tx.calculate_hash()), "the id recomputes");
    assert!(
        tx.is_canonical_station_owner_envelope(),
        "the CLI emits exactly the envelope the node builds in payload form"
    );
    tx
}

fn admitted(state: &State, artifact: &Path, kind: &str, nonce: u64) {
    let tx = tx_of(artifact);
    let step = state
        .precheck_station_owner_tx_with_features(&tx, HEIGHT, CHAIN, NETWORK, true, true)
        .unwrap_or_else(|r| panic!("{kind}: the node would refuse the CLI's artifact: {r:?}"));
    assert_eq!((step.kind, step.nonce, step.station_id.as_str()), (kind, nonce, STATION));
}

#[test]
fn every_owner_command_builds_an_artifact_the_node_admits() {
    let (a, b, gov) = (key(0xA1), key(0xB2), key(0x60));
    std::env::set_var("WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS", pk_hex(&gov));
    let dir = std::env::temp_dir().join(format!("wrfcoin-cli-owner-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (a_key, b_key, gov_key) = (
        write_key(&dir, "a.key", &a),
        write_key(&dir, "b.key", &b),
        write_key(&dir, "gov.key", &gov),
    );
    let state = state_with_station(&a, &gov);
    let exp = (HEIGHT + 500).to_string();
    let now = NOW.to_string();
    let s = |p: &Path| p.to_str().unwrap().to_string();
    // The tx timestamp is pinned at NOW, far from this machine's clock: the
    // node's 150 s freshness check is the SUBMIT route's, exercised elsewhere.

    // Owner actions: all four.
    for (cmd, nonce) in [("report-stolen", 0u64), ("unfreeze", 0), ("deactivate", 0), ("reactivate", 0)] {
        let out = dir.join(format!("{cmd}.json"));
        let args = [
            "station", cmd, "--station-id", STATION, "--owner-key", &s(&a_key), "--nonce",
            &nonce.to_string(), "--expires-at-height", &exp, "--chain-id", CHAIN, "--network-id",
            NETWORK, "--submitted-at", &now, "--out", &s(&out),
        ];
        let summary = run(&args);
        assert_eq!(summary["self_verified"], true);
        assert_eq!(summary["station_id"], STATION);
        let tx = tx_of(&out);
        // Unfreeze/reactivate are stateless-valid but stateful-refused on a fresh
        // station; the node says so with a named code, not silence.
        let verdict = state.precheck_station_owner_tx_with_features(&tx, HEIGHT, CHAIN, NETWORK, true, true);
        match cmd {
            "report-stolen" | "deactivate" => assert!(verdict.is_ok(), "{cmd}: {verdict:?}"),
            "unfreeze" => assert_eq!(verdict.unwrap_err().code(), "OWNER_TX_NOT_FROZEN"),
            "reactivate" => assert_eq!(verdict.unwrap_err().code(), "OWNER_TX_NOT_DEACTIVATED"),
            _ => unreachable!(),
        }
    }

    // Transfer: proposal by the current owner, acceptance by the new owner.
    let proposal = dir.join("proposal.json");
    run(&[
        "station", "transfer", "--station-id", STATION, "--owner-key", &s(&a_key), "--to-pubkey",
        &pk_hex(&b), "--nonce", "0", "--expires-at-height", &exp, "--chain-id", CHAIN,
        "--network-id", NETWORK, "--out", &s(&proposal),
    ]);
    let accepted = dir.join("transfer.json");
    run(&[
        "station", "accept", "--proposal", &s(&proposal), "--new-owner-key", &s(&b_key),
        "--submitted-at", &now, "--out", &s(&accepted),
    ]);
    admitted(&state, &accepted, "station_owner_transfer_v1", 0);
    // The wrong key must not countersign someone else's proposal.
    let wrong = write_key(&dir, "mallory.key", &key(0xE5));
    let err = run_expect_failure(&[
        "station", "accept", "--proposal", &s(&proposal), "--new-owner-key", &s(&wrong),
        "--submitted-at", &now, "--out", &s(&dir.join("nope.json")),
    ]);
    assert!(err.contains("to_owner_pubkey mismatch"), "{err}");

    // Recovery: acceptance by --new-owner-key, and by a detached acceptance.
    let filing_a = dir.join("file-a.json");
    run(&[
        "station", "recover-file", "--station-id", STATION, "--from-owner-address", &addr(&a),
        "--governance-key", &s(&gov_key), "--new-owner-key", &s(&b_key), "--nonce", "0",
        "--chain-id", CHAIN, "--network-id", NETWORK, "--submitted-at", &now, "--out", &s(&filing_a),
    ]);
    admitted(&state, &filing_a, "station_owner_recovery_file_v1", 0);
    let detached = run(&[
        "station", "recover-accept", "--station-id", STATION, "--from-owner-address", &addr(&a),
        "--new-owner-key", &s(&b_key), "--nonce", "0", "--chain-id", CHAIN, "--network-id", NETWORK,
    ]);
    let filing_b = dir.join("file-b.json");
    run(&[
        "station", "recover-file", "--station-id", STATION, "--from-owner-address", &addr(&a),
        "--governance-key", &s(&gov_key), "--to-pubkey", detached["to_pubkey"].as_str().unwrap(),
        "--to-owner-signature", detached["to_owner_signature"].as_str().unwrap(), "--nonce", "0",
        "--chain-id", CHAIN, "--network-id", NETWORK, "--submitted-at", &now, "--out", &s(&filing_b),
    ]);
    admitted(&state, &filing_b, "station_owner_recovery_file_v1", 0);
    // The phone exchange: the operator prints a request, the claimant's phone
    // (here: `recover-accept`, which emits the same text) replies, and
    // `recover-file --accept-text` consumes the reply.
    let request = run(&[
        "station", "recover-request", "--station-id", STATION, "--from-owner-address", &addr(&a),
        "--to-pubkey", &pk_hex(&b), "--nonce", "0", "--chain-id", CHAIN, "--network-id", NETWORK,
    ]);
    let request_text = request["request_text"].as_str().unwrap();
    assert!(request_text.starts_with("WRFOWN1.recovery."), "{request_text}");
    let accept_text = detached["accept_text"].as_str().expect("accept_text");
    assert!(accept_text.starts_with("WRFOWN1.recovery-accept."), "{accept_text}");
    let filing_c = dir.join("file-c.json");
    run(&[
        "station", "recover-file", "--station-id", STATION, "--from-owner-address", &addr(&a),
        "--governance-key", &s(&gov_key), "--accept-text", accept_text, "--nonce", "0",
        "--chain-id", CHAIN, "--network-id", NETWORK, "--submitted-at", &now, "--out", &s(&filing_c),
    ]);
    admitted(&state, &filing_c, "station_owner_recovery_file_v1", 0);
    // An accept text for a different recovery (nonce 0 vs 1) is refused, and so is a damaged one.
    let err = run_expect_failure(&[
        "station", "recover-file", "--station-id", STATION, "--from-owner-address", &addr(&a),
        "--governance-key", &s(&gov_key), "--accept-text", accept_text, "--nonce", "1",
        "--chain-id", CHAIN, "--network-id", NETWORK, "--submitted-at", &now,
        "--out", &s(&dir.join("mismatch.json")),
    ]);
    assert!(err.contains("different recovery"), "{err}");
    let err = run_expect_failure(&[
        "station", "recover-file", "--station-id", STATION, "--from-owner-address", &addr(&a),
        "--governance-key", &s(&gov_key), "--accept-text", &accept_text[..accept_text.len() - 3],
        "--nonce", "0", "--chain-id", CHAIN, "--network-id", NETWORK, "--submitted-at", &now,
        "--out", &s(&dir.join("damaged.json")),
    ]);
    assert!(err.contains("--accept-text refused"), "{err}");
    assert!(!dir.join("mismatch.json").exists() && !dir.join("damaged.json").exists());

    // A non-governance key cannot file: the CLI refuses before writing.
    let err = run_expect_failure(&[
        "station", "recover-file", "--station-id", STATION, "--from-owner-address", &addr(&a),
        "--governance-key", &s(&b_key), "--new-owner-key", &s(&b_key), "--nonce", "0",
        "--chain-id", CHAIN, "--network-id", NETWORK, "--submitted-at", &now,
        "--out", &s(&dir.join("bad-file.json")),
    ]);
    assert!(err.contains("NotGovernance"), "{err}");
    assert!(!dir.join("bad-file.json").exists(), "nothing written on refusal");

    // Cancel and execute need a pending recovery in state.
    let mut pending = state.clone();
    let filing_tx = tx_of(&filing_a);
    let p: wrfcoin_shared_protocol::blockchain::station_owner::StationOwnerRecoveryFilingPayload =
        serde_json::from_str(filing_tx.data.as_deref().unwrap()).unwrap();
    pending
        .apply_station_owner_recovery_filing(STATION, &p.from_owner_address, &p.to_owner_address, 0, HEIGHT - 10)
        .unwrap();
    let cancel = dir.join("cancel.json");
    run(&[
        "station", "recover-cancel", "--station-id", STATION, "--owner-key", &s(&a_key), "--nonce",
        "0", "--expires-at-height", &exp, "--chain-id", CHAIN, "--network-id", NETWORK,
        "--submitted-at", &now, "--out", &s(&cancel),
    ]);
    admitted(&pending, &cancel, "station_owner_recovery_cancel_v1", 0);
    let execute = dir.join("execute.json");
    run(&[
        "station", "recover-execute", "--station-id", STATION, "--nonce", "0", "--chain-id", CHAIN,
        "--network-id", NETWORK, "--submitted-at", &now, "--out", &s(&execute),
    ]);
    // Executable only once the notice window has elapsed.
    let tx = tx_of(&execute);
    let early = pending
        .precheck_station_owner_tx_with_features(&tx, HEIGHT, CHAIN, NETWORK, true, true)
        .unwrap_err();
    assert_eq!(early.code(), "OWNER_TX_NOTICE_WINDOW_NOT_ELAPSED");
    let late = pending
        .precheck_station_owner_tx_with_features(&tx, HEIGHT - 10 + 34_000, CHAIN, NETWORK, true, true)
        .expect("executable at filed + notice");
    assert_eq!(late.kind, "station_owner_recovery_execute_v1");

    // v2 registration: `build-registration --expected-nonce` signs the nonce into
    // the governance preimage; without it the artifact is v1.
    for (flag, nonce) in [(None, None), (Some("5"), Some(5u64))] {
        let out = dir.join(format!("reg-{}.json", flag.unwrap_or("v1")));
        let mut args = vec![
            "station", "build-registration", "--station-id", "ws-cli-reg", "--owner-address",
            "", "--owner-key", "", "--governance-key", "", "--chain-id", CHAIN, "--network-id",
            NETWORK, "--latitude", "46.7867", "--longitude", "-92.1005", "--submitted-at",
            "1700000000", "--out", "",
        ];
        let owner_addr = addr(&a);
        let (a_path, gov_path, out_path) = (s(&a_key), s(&gov_key), s(&out));
        args[5] = &owner_addr;
        args[7] = &a_path;
        args[9] = &gov_path;
        let last = args.len() - 1;
        args[last] = &out_path;
        if let Some(n) = flag {
            args.extend(["--expected-nonce", n]);
        }
        run(&args);
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
        let tx: Transaction = serde_json::from_value(doc["transactions"][0].clone()).unwrap();
        let payload = tx.parse_station_registration().expect("a registration payload");
        assert_eq!(payload.expected_nonce, nonce);
        assert_eq!(payload.is_v2(), nonce.is_some());
        assert!(
            tx.verify_station_registration_authorization_with_domain(CHAIN, NETWORK),
            "the CLI's registration (v1 or v2) verifies under the chain it was signed for"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}
