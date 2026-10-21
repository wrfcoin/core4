//! #3413 S0: frozen legacy compatibility before indexed outputs are added.
//! No feature registration, schedule, genesis, verifier or preimage change.
//! Only the OUTER transaction uses bincode; the payload boundary is JSON.
#[path = "common/privacy_legacy_3413.rs"]
mod fixture;

use curve25519_dalek::{constants::RISTRETTO_BASEPOINT_TABLE, scalar::Scalar};
use serde_json::Value;
use wrfcoin_privacy::ring_signatures::{KeyImage, MLSAGSignature, RingMember};
use wrfcoin_shared_protocol::{
    blockchain::{
        state::{State, StateTransition},
        transaction::{PrivacyLevel, confidential_transfer_authorization_message_with_domain},
    },
    crypto::chain_binding::{ConsensusFeature, FeatureSchedule, install_feature_schedule},
};

fn baseline() -> Value {
    // Comments mark public frozen vectors for the secret scanner. This exact
    // suffix does not occur in fixture strings; do not strip arbitrary URLs.
    let json = include_str!("fixtures/privacy_legacy_3413.jsonc")
        .lines()
        .map(|line| line.split(" // pragma: allowlist secret").next().unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    serde_json::from_str(&json).expect("frozen public S0 fixture")
}

fn gate_off() {
    let schedule = install_feature_schedule(FeatureSchedule::default());
    assert_eq!(schedule, &FeatureSchedule::default());
    for height in [0, 1, 100_000] {
        for feature in ConsensusFeature::ALL {
            assert!(!schedule.active(*feature, height));
        }
    }
}

#[test]
fn legacy_confidential_transfer_json_and_preimage_unchanged_gate_off_3413() {
    gate_off();
    let pin = baseline();
    let payload = fixture::payload();
    let json = serde_json::to_string(&payload).unwrap();
    assert_eq!(json, pin["payload_json"].as_str().unwrap());
    let decoded: wrfcoin_shared_protocol::blockchain::transaction::ConfidentialTransferPayload =
        serde_json::from_str(&json).unwrap();
    assert_eq!(payload, decoded);
    let preimage = confidential_transfer_authorization_message_with_domain(
        "wrfcoin-3413-s0",
        "testnet",
        &payload,
    );
    assert_eq!(
        preimage,
        hex::decode(pin["authorization_v2_hex"].as_str().unwrap()).unwrap()
    );
    let tx = fixture::transaction();
    assert_eq!(tx.data.as_deref(), Some(json.as_str()));
    assert_eq!(tx.parse_confidential_transfer_payload().unwrap(), payload);
    let outer = bincode::serialize(&tx).unwrap();
    assert_eq!(
        outer,
        hex::decode(pin["outer_transaction_hex"].as_str().unwrap()).unwrap()
    );
    let decoded_tx: wrfcoin_shared_protocol::blockchain::transaction::Transaction =
        bincode::deserialize(&outer).unwrap();
    assert_eq!(bincode::serialize(&decoded_tx).unwrap(), outer);
}

#[test]
fn legacy_confidential_transfer_root_parity_gate_off_3413() {
    gate_off();
    let pin = baseline();
    let mut direct = State::new();
    let mut from_wire = State::new();
    for height in 1..=2 {
        let mut payload = fixture::payload();
        if height == 2 {
            payload.output_commitments = vec!["77".repeat(32)];
            payload.key_images = vec!["88".repeat(32)];
        }
        let mut tx = fixture::transaction();
        tx.data = Some(serde_json::to_string(&payload).unwrap());
        tx.set_id();
        direct
            .apply_transition(StateTransition::ApplyConfidentialTransfer {
                payload,
                current_block: height,
            })
            .unwrap();
        from_wire
            .apply_transition(StateTransition::ApplyConfidentialTransfer {
                payload: tx.parse_confidential_transfer_payload().unwrap(),
                current_block: height,
            })
            .unwrap();
        assert_eq!(direct.state_root, from_wire.state_root);
        assert_eq!(
            direct.state_root.to_string(),
            pin["roots"][(height - 1) as usize].as_str().unwrap()
        );
        assert!(direct.feature_schedule_extensions.is_empty());
    }
    assert_eq!(direct.privacy_commitments.len(), 3);
    assert_eq!(direct.privacy_spent_key_images.len(), 2);
}

#[test]
fn legacy_confidential_transfer_verifier_retained_3413() {
    gate_off();
    // Public, deterministic test scalars; signature nonces remain random.
    let signer = Scalar::from(1u64).to_bytes();
    let decoys = (2..=8)
        .map(|seed| RingMember {
            public_key: (&Scalar::from(seed as u64) * RISTRETTO_BASEPOINT_TABLE)
                .compress()
                .to_bytes(),
        })
        .collect::<Vec<_>>();
    let mut payload = fixture::payload();
    payload.privacy_level = PrivacyLevel::High;
    payload.key_images = vec![hex::encode(KeyImage::generate(&signer).unwrap().to_bytes())];
    let message = payload.authorization_message();
    let signature = MLSAGSignature::sign(&message, &signer, &decoys).unwrap();
    payload.ring_signature = Some(hex::encode(serde_json::to_vec(&signature).unwrap()));
    payload.signed_message = Some(hex::encode(message));
    let mut tampered = payload.clone();
    tampered.output_commitments[0] = "99".repeat(32);
    let mut state = State::new();
    state
        .apply_transition(StateTransition::ApplyConfidentialTransfer {
            payload,
            current_block: 1,
        })
        .expect("legacy v2 ring signature must still verify");
    let mut rejected = State::new();
    let before = rejected.state_root;
    let error = rejected
        .apply_transition(StateTransition::ApplyConfidentialTransfer {
            payload: tampered,
            current_block: 1,
        })
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("SP_PRIVACY_SIGNED_MESSAGE_BINDING")
    );
    assert_eq!(rejected.state_root, before);
    assert!(rejected.privacy_commitments.is_empty());
    assert!(rejected.privacy_spent_key_images.is_empty());
}
