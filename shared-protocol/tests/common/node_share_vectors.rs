#![allow(dead_code)]

use wrfcoin_shared_protocol::common::types::Address;

use crate::common::{identity, keypair};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion;
use wrfcoin_shared_protocol::crypto::domain_signing::{DomainSeparated, verify_domain_for};

#[path = "node_share_reference.rs"]
mod reference;

pub const FIXTURE: &str = "tests/fixtures/golden/node_share_contracts_v1.jsonc";

fn build_case<T: DomainSeparated + Serialize>(
    kind: &str,
    seed: u64,
    golden: &T,
    tampered: &T,
    note: &str,
) -> Value {
    let identity = identity();
    let (sk, pk) = keypair(seed);
    let body = reference::body(kind, &serde_json::to_value(golden).unwrap());
    let preimage = reference::preimage(kind, &body, &identity.chain_id, &identity.network_id);
    assert_eq!(
        golden.canonical_body(),
        body,
        "production body != ADR reference"
    );
    assert_eq!(
        golden.signing_preimage_for(&identity, SigningProtocolVersion::V2),
        preimage
    );
    assert_eq!(T::DOMAIN_TAG, reference::tag(kind).as_bytes());
    json!({
        "kind": kind, "tag": String::from_utf8_lossy(T::DOMAIN_TAG), "signer_seed": seed,
        "signer_public_key_hex": pk.to_hex(),
        "input": golden, "tampered_input": tampered, "tamper_note": note,
        "canonical_body_hex": hex::encode(&body),
        "preimage_v2_hex": hex::encode(&preimage), "signature_v2_hex": sk.sign(&preimage).to_hex(),
    })
}

pub fn vectors() -> Value {
    let mut cases = Vec::new();
    {
        use wrfcoin_shared_protocol::consensus::node_share_contracts::*;
        assert_eq!(
            BondedServiceClaim::receipt_hash(&reference::sample_receipt()).unwrap(),
            reference::receipt_hash(&reference::sample_receipt())
        );
        assert_eq!(
            BondedServiceClaim::manifest_hash(&reference::sample_manifest()).unwrap(),
            reference::manifest_hash(&reference::sample_manifest())
        );
        let g = NodeServiceTicket {
            context: NodeSettlementContext {
                genesis_hash: [1; 32],
                chain_id: identity().chain_id,
                network_id: identity().network_id,
                epoch: 3,
                roster_sequence: 7,
                roster_hash: [2; 32],
                epoch_start_parent: [3; 32],
            },
            server_id: [4; 32],
            consumer_id: [5; 32],
            ticket_id: [6; 32],
            signed_receipt_hash: reference::receipt_hash(&reference::sample_receipt()),
            signed_manifest_hash: reference::manifest_hash(&reference::sample_manifest()),
        };
        let mut t = g.clone();
        t.context.genesis_hash[0] ^= 1;
        cases.push(build_case(
            "NodeServiceTicket",
            900_141,
            &g,
            &t,
            "genesis hash bit flip",
        ));
        let g = NodeSettlementEndorsement {
            ticket_preimage_hash: [9; 32],
        };
        let mut t = g.clone();
        t.ticket_preimage_hash[0] ^= 1;
        cases.push(build_case(
            "NodeSettlementEndorsement",
            900_142,
            &g,
            &t,
            "ticket digest bit flip",
        ));
        let g = NodeBondAuthorization {
            genesis_hash: [1; 32],
            deployment_id: [2; 32],
            owner_account: Address::new([3; 20]),
            escrow_account: Address::new([4; 20]),
            payout_account: Address::new([5; 20]),
            server_key: [6; 32],
            consumer_key: [7; 32],
            service_alias: "snapshot-golden.example".into(),
            action: NodeBondAction::Deposit,
            amount_units: 250_000_000_000,
            expected_nonce: 11,
            effective_epoch: 12,
        };
        let mut t = g.clone();
        t.action = NodeBondAction::Withdraw;
        cases.push(build_case(
            "NodeBondAuthorization",
            900_143,
            &g,
            &t,
            "deposit -> withdraw",
        ));
        let g = NodeBondPossession {
            authorization_preimage_hash: [10; 32],
            role: NodeBondKeyRole::Server,
        };
        let mut t = g.clone();
        t.role = NodeBondKeyRole::Consumer;
        cases.push(build_case(
            "NodeBondPossession",
            900_144,
            &g,
            &t,
            "server -> consumer",
        ));
    }
    json!({"schema":"wrfcoin.node-share-contracts.v1/1", "chain_id":identity().chain_id, "network_id":identity().network_id, "cases":cases})
}

pub fn fixture_text() -> String {
    let public_fields = [
        "signer_public_key_hex",
        "canonical_body_hex",
        "preimage_v2_hex",
        "signature_v2_hex",
    ];
    serde_json::to_string_pretty(&vectors()).unwrap().lines().map(|line| {
        if public_fields.iter().any(|f| line.trim_start().starts_with(&format!("\"{f}\":"))) {
            format!("{line} // pragma: allowlist secret - deterministic synthetic public S1 vector\n")
        } else { format!("{line}\n") }
    }).collect()
}

fn check<T: DomainSeparated + DeserializeOwned>(case: &Value) {
    let (sk, pk) = keypair(case["signer_seed"].as_u64().unwrap());
    let golden: T = serde_json::from_value(case["input"].clone()).unwrap();
    let tampered: T = serde_json::from_value(case["tampered_input"].clone()).unwrap();
    let kind = case["kind"].as_str().unwrap();
    let body = reference::body(kind, &case["input"]);
    let message = reference::preimage(kind, &body, &identity().chain_id, &identity().network_id);
    assert_eq!(golden.canonical_body(), body);
    assert_eq!(
        golden.signing_preimage_for(&identity(), SigningProtocolVersion::V2),
        message
    );
    let signature = sk.sign(&message);
    assert_eq!(case["tag"], String::from_utf8_lossy(T::DOMAIN_TAG).as_ref());
    assert_eq!(case["signer_public_key_hex"], pk.to_hex());
    assert_eq!(
        case["canonical_body_hex"],
        hex::encode(golden.canonical_body())
    );
    assert_eq!(case["preimage_v2_hex"], hex::encode(&message));
    assert_eq!(case["signature_v2_hex"], signature.to_hex());
    assert!(verify_domain_for(
        &pk,
        &golden,
        &signature,
        &identity(),
        SigningProtocolVersion::V2
    ));
    assert!(!verify_domain_for(
        &pk,
        &tampered,
        &signature,
        &identity(),
        SigningProtocolVersion::V2
    ));
}

pub fn assert_pinned_vectors() {
    use wrfcoin_shared_protocol::consensus::node_share_contracts::*;
    let text =
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE))
            .unwrap();
    let json = text
        .lines()
        .map(|line| line.split(" // pragma: allowlist secret").next().unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    let fixture: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(vectors(), fixture, "S1 signed-contract golden drift");
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 4);
    for case in cases {
        match case["kind"].as_str().unwrap() {
            "NodeServiceTicket" => check::<NodeServiceTicket>(case),
            "NodeSettlementEndorsement" => check::<NodeSettlementEndorsement>(case),
            "NodeBondAuthorization" => check::<NodeBondAuthorization>(case),
            "NodeBondPossession" => check::<NodeBondPossession>(case),
            other => panic!("unaccounted S1 kind {other}"),
        }
    }
}
