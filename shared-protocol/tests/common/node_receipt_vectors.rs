#![allow(dead_code)]
use serde_json::{Value, json};
use wrfcoin_shared_protocol::{
    common::types::Address,
    consensus::node_receipt_registry::NodeReceiptSubmissionAuthorization,
    crypto::{
        chain_binding::SigningProtocolVersion,
        domain_signing::{DomainSeparated, verify_domain_for},
        signature::Signature,
    },
};
const FILE: &str = "tests/fixtures/golden/node_receipt_submission_v1.jsonc";
fn bytes(out: &mut Vec<u8>, value: &[u8]) {
    out.extend_from_slice(&(value.len() as u64).to_le_bytes());
    out.extend_from_slice(value);
}
fn reference(s: &NodeReceiptSubmissionAuthorization) -> Vec<u8> {
    let mut body = vec![];
    bytes(&mut body, &s.genesis_hash);
    bytes(&mut body, &s.sender.0);
    body.extend_from_slice(&s.nonce.to_le_bytes());
    bytes(&mut body, &s.batch_hash);
    let identity = crate::common::identity();
    let mut frame = vec![];
    bytes(&mut frame, b"wrfcoin.node_receipt_submission.v1");
    bytes(&mut frame, identity.chain_id.as_bytes());
    bytes(&mut frame, identity.network_id.as_bytes());
    frame.extend_from_slice(&2u32.to_le_bytes());
    bytes(&mut frame, &body);
    frame
}
pub fn check_pins() {
    let subject = NodeReceiptSubmissionAuthorization {
        genesis_hash: [1; 32],
        sender: Address::new([2; 20]),
        nonce: 7,
        batch_hash: [3; 32],
    };
    let (sk, pk) = crate::common::keypair(900_145);
    let identity = crate::common::identity();
    let expected = reference(&subject);
    assert_eq!(
        expected,
        subject.signing_preimage_for(&identity, SigningProtocolVersion::V2)
    );
    let vector = json!({"kind":"NodeReceiptSubmissionAuthorization","input":subject,"preimage_hex":hex::encode(&expected),"public_key_hex":pk.to_hex(),"signature_hex":sk.sign(&expected).to_hex()});
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FILE);
    if std::env::var("WRFCOIN_REGEN_NODE_RECEIPT_VECTORS").as_deref() == Ok("1") {
        let text=serde_json::to_string_pretty(&vector).unwrap().lines().map(|line| {
            if ["preimage_hex","public_key_hex","signature_hex"].iter().any(|field| line.trim_start().starts_with(&format!("\"{field}\":"))) { format!("{line} // pragma: allowlist secret - deterministic synthetic public S3 vector\n") } else { format!("{line}\n") }
        }).collect::<String>();
        std::fs::write(&path, text).unwrap();
    }
    let text = std::fs::read_to_string(path).unwrap();
    let parsed: Value = serde_json::from_str(
        &text
            .lines()
            .map(|line| line.split(" // pragma:").next().unwrap())
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    assert_eq!(vector, parsed);
    let sig = Signature::from_hex(parsed["signature_hex"].as_str().unwrap()).unwrap();
    assert!(verify_domain_for(
        &pk,
        &subject,
        &sig,
        &identity,
        SigningProtocolVersion::V2
    ));
    for field in 0..4 {
        let mut tampered = subject.clone();
        match field {
            0 => tampered.genesis_hash[0] ^= 1,
            1 => tampered.sender = Address::new([4; 20]),
            2 => tampered.nonce += 1,
            _ => tampered.batch_hash[0] ^= 1,
        }
        assert_ne!(reference(&tampered), expected);
        assert!(!verify_domain_for(
            &pk,
            &tampered,
            &sig,
            &identity,
            SigningProtocolVersion::V2
        ));
    }
}
