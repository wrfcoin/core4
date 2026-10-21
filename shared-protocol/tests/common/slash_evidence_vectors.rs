#![allow(dead_code)]

use serde_json::{Value, json};
use wrfcoin_shared_protocol::blockchain::transaction::{
    Transaction, commit_authorization_message_with_domain,
};
use wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity;
use wrfcoin_shared_protocol::crypto::domain_signing::DomainSeparated;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::security::slash_evidence::{
    SlashEvidenceReport, SlashEvidenceReportPayload,
};

pub fn identity() -> ChainIdentity {
    ChainIdentity::new("777", "testnet")
}

pub fn reporter() -> PrivateKey {
    PrivateKey::from_bytes(&[9; 32]).unwrap()
}

pub fn payload() -> SlashEvidenceReportPayload {
    let owner = PrivateKey::from_bytes(&[7; 32]).unwrap();
    let data = json!({"temperature_c":20.0,"humidity_percent":250.0,"pressure_hpa":1013.0,"wind_speed_ms":1.0});
    let commit_id = Transaction::compute_commit_id(
        &data,
        "synthetic-nonce",
        "station-synthetic-1",
        1_700_000_000,
        "contribution",
    );
    let authorization = commit_authorization_message_with_domain(
        "777",
        "testnet",
        "station-synthetic-1",
        &commit_id,
        "contribution",
    );
    serde_json::from_value(json!({
        "version":1,"commit_id":commit_id,
        "signed_commit":{"commit_id":commit_id,"station_id":"station-synthetic-1","timestamp":1_700_000_000,"contribution_type":"contribution",
            "auth_pubkey":PublicKey::from_private_key(&owner).to_hex(),"auth_signature":owner.sign(&authorization).to_hex()},
        "reveal":{"commit_id":commit_id,"station_id":"station-synthetic-1","timestamp":1_700_000_000,"contribution_type":"contribution","data":data,"nonce":"synthetic-nonce"},
        "attestations":[],"context_hash":"44".repeat(32),
        "reporter_pubkey":PublicKey::from_private_key(&reporter()).to_hex(),"reporter_nonce":3,"fee":1,"reporter_signature":null
    })).unwrap()
}

pub fn vector() -> Value {
    let mut payload = payload();
    payload.sign_for(&reporter(), &identity()).unwrap();
    json!({
        "schema":"slash_evidence_report.v1","input":payload,
        "body_hex":hex::encode(SlashEvidenceReport::try_from(&payload).unwrap().canonical_body()),
        "preimage_hex":hex::encode(payload.signing_preimage_for(&identity()).unwrap()),
        "signature_hex":payload.reporter_signature,
        "evidence_id_hex":hex::encode(payload.evidence_id().unwrap()),
        "transaction_index":36,"prior_tail_index":35
    })
}

pub fn fixture_text() -> String {
    // JSON comments label only these public values from the synthetic signers.
    // They use the pre-commit hook's supported per-line false-positive pragma.
    let public_fields = [
        "body_hex",
        "evidence_id_hex",
        "preimage_hex",
        "signature_hex",
        "reporter_pubkey",
        "reporter_signature",
        "auth_pubkey",
        "auth_signature",
        "commit_id",
        "context_hash",
    ];
    serde_json::to_string_pretty(&vector())
        .unwrap()
        .lines()
        .map(|line| {
            if public_fields
                .iter()
                .any(|field| line.trim_start().starts_with(&format!("\"{field}\":")))
            {
                format!("{line} // pragma: allowlist secret - synthetic public signing vector\n")
            } else {
                format!("{line}\n")
            }
        })
        .collect()
}

pub fn parse_fixture(text: &str) -> Value {
    let json = text
        .lines()
        .map(|line| line.split(" // pragma: allowlist secret").next().unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    serde_json::from_str(&json).unwrap()
}

pub fn assert_pinned_vector() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/golden/slash_evidence_report_v1.jsonc");
    let pinned = parse_fixture(&std::fs::read_to_string(path).unwrap());
    assert_eq!(vector(), pinned, "canonical evidence bytes changed");
    let payload: SlashEvidenceReportPayload =
        serde_json::from_value(pinned["input"].clone()).unwrap();
    payload.verify_for(&identity()).unwrap();
    let mut tampered = payload.clone();
    tampered.reporter_nonce += 1;
    assert_ne!(
        tampered.signing_preimage_for(&identity()).unwrap(),
        payload.signing_preimage_for(&identity()).unwrap()
    );
    assert!(tampered.verify_for(&identity()).is_err());
}
