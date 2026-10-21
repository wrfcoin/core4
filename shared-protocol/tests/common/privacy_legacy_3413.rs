//! Deterministic, synthetic S0 input; no deployment identity, genesis or keys.
use wrfcoin_shared_protocol::blockchain::transaction::{
    ConfidentialTransferPayload, PrivacyLevel, Transaction, TransactionType,
};

pub fn payload() -> ConfidentialTransferPayload {
    ConfidentialTransferPayload {
        output_commitments: vec!["11".repeat(32), "22".repeat(32)],
        range_proof: Some("33".repeat(32)),
        stealth_data: Some("44".repeat(32)),
        ring_signature: None,
        key_images: vec!["55".repeat(32)],
        encrypted_payload: Some("66".repeat(32)),
        privacy_level: PrivacyLevel::Standard,
        selective_disclosures: Vec::new(),
        signed_message: None,
    }
}

pub fn transaction() -> Transaction {
    let mut tx = Transaction::new(Vec::new(), Vec::new());
    tx.tx_type = TransactionType::ConfidentialTransfer;
    tx.timestamp = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
    tx.data = Some(serde_json::to_string(&payload()).unwrap());
    tx.set_id();
    tx
}
